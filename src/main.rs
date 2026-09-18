// External Merge Sort Engine: CLI + orchestration.
// v2: pre-flight disk check, --temp-dir, --format csv, --resume, atomic output
// replacement, human-readable errors with consistent exit codes.
mod chunk;
mod disk;
mod errors;
mod inspect;
mod io_buffer;
mod manifest;
mod merge;
mod metrics;
mod progress;
mod serve;
mod temp_manager;
mod verify;

use chunk::SortFormat;
use manifest::Manifest;
use metrics::MemorySampler;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[allow(dead_code)]
struct Config {
    input: PathBuf,
    output: PathBuf,
    max_memory: u64,
    fmt: SortFormat,
    max_open_files: Option<usize>,
    do_verify: bool,
    threads: Option<usize>,
    quiet: bool,
    json: bool,
    dashboard: Option<String>,
    temp_dir: Option<PathBuf>,
    /// None = fresh run; Some("list") = list; Some("") = auto-pick newest;
    /// Some(id) = resume that run.
    resume: Option<String>,
    reverse: bool,
    unique: bool,
    header: bool,
    header_explicit: bool,
    dry_run: bool,
    limit: Option<usize>,
    log_file: Option<PathBuf>,
    gui: Option<String>,
    // v1.2 Lapis-1
    preview: Option<usize>,
    preview_json: bool,
    stats: bool,
    stats_json: bool,
    check: bool,
    encoding: Option<String>,
    encoding_in: Option<String>,
    encoding_out: Option<String>,
    delimiter_explicit: bool,
    // v1.3 Lapis-2
    key_field: Option<String>,
    dedupe_by: Option<Vec<usize>>,
    dedupe_keep_last: bool,
    merge_inputs: Vec<PathBuf>,
    output_format: Option<String>,
    sql_table: Option<String>,
    split_by: Option<usize>,
    // v2.0 Lapis-3
    interactive: bool,
    watch: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    api: Option<String>,
    api_token: Option<String>,
    brand: Option<String>,
    brand_color: Option<String>,
    sheet: Option<String>,
}

fn usage() -> String {
    "usage: mergesort --input <path> --output <path> --max-memory <size> [options]
   or: mergesort --gui [host:]PORT   (web console, drag-drop, no CLI needed)

data format:
  --mode numeric|string|jsonl  line format (default: numeric)
  --format csv --key-column N  delimited data; sort by column N (0-based)
  --format jsonl --key-field F JSON Lines; sort by field F (dot nested, mis. \"user.age\")
  --delimiter <c>              field delimiter for csv (default ','; auto if omitted)
  --multi-key \"1,3\"            multi-column sort (secondary keys)
  --key-type numeric|string|date comparison type (date = ISO string order)
  --key-field F                jsonl key field path (wajib untuk jsonl)
  --dedupe-by C                drop dupes by CSV column(s) \"2\" / \"2,3\" (keep --dedupe-keep)
  --dedupe-keep first|last     which row to keep (default first)
  --merge F1 F2 ...            merge pre-sorted files (no re-sort) + --output
  --output-format F            convert on write: csv|tsv|jsonl|sql (+ --sql-table)
  --sql-table T                table name for --output-format sql
  --split-by N                 split output into N equal files (out-001.ext...)
  --sheet NAME                 xlsx sheet to read (default: first sheet)
  --header                     keep first line (CSV/string) as header, not sorted
  --no-header                  force no header (override auto-detect)
  --reverse, -r                sort descending (largest first)
  --unique, -u                 drop duplicate records (like `sort -u`)
  --limit N                    keep only first N records (Top-N, works with --reverse)
  --preview N                  show first/last N sorted rows, no output file commit
  --preview-json               same as --preview but JSON lines on stdout
  --stats                      execution report block (screenshot-able)
  --stats-json                 same report as JSON
  --check                      validate file only (no sort), exit 0=ready 5=data issue
  --encoding E                 utf-8|latin-1|utf-16le|utf-16be (default auto via BOM)
  --encoding-in E              input decoding override
  --encoding-out E             output encoding override
  .gz/.zip input/output transparan; .zst ditolak dengan pesan jelas

robustness:
  --temp-dir <path>            put temp chunk files on another drive
  --resume [run_id|list]       continue an interrupted run (no id = newest)
  --verify                     re-read output and check ordering

planning:
  --dry-run                    estimate disk/memory/chunks and exit (no sorting)

observability:
  --max-open-files N           merge fan-in limit (default: auto from ulimit)
  --threads N                  parallel sort threads (default: all cores)
  --quiet                      print only the final summary
  --json                       print the final summary as JSON (script-friendly)
  --log-file <path>            append JSON summary to a .jsonl audit log
  --dashboard [host:]PORT      serve a live metrics web page (port 0 = random)
  --gui [host:]PORT            web console for non-technical users (upload + sort + download)
  --api [host:]PORT            REST API mode (+ --api-token) for integrators
  --api-token T                bearer token for --api (default: none/localhost only)
  --brand NAME                 white-label GUI brand name
  --brand-color HEX            white-label accent color (mis. \"#0066cc\")
  --interactive                wizard tanya-jawab (tanpa hafal flag)
  --watch DIR                  daemon: auto-sort file baru di folder (+ --output-dir)
  --output-dir DIR             target folder untuk --watch
  --version, -V                print version and exit
  --help, -h                   print this help"
        .to_string()
}

/// Parses a size like "100MB", "1gb", "4096" into bytes.
fn parse_size(s: &str) -> Result<u64, String> {
    let t = s.trim().to_lowercase();
    let (num, mult): (&str, u64) = if let Some(x) = t.strip_suffix("gb") {
        (x, 1024 * 1024 * 1024)
    } else if let Some(x) = t.strip_suffix("mb") {
        (x, 1024 * 1024)
    } else if let Some(x) = t.strip_suffix("kb") {
        (x, 1024)
    } else if let Some(x) = t.strip_suffix("g") {
        (x, 1024 * 1024 * 1024)
    } else if let Some(x) = t.strip_suffix("m") {
        (x, 1024 * 1024)
    } else if let Some(x) = t.strip_suffix("k") {
        (x, 1024)
    } else {
        (t.as_str(), 1)
    };
    let n: f64 = num
        .trim()
        .parse()
        .map_err(|_| format!("ukuran tidak valid: '{}'", s))?;
    if n < 0.0 {
        return Err(format!("ukuran tidak valid: '{}'", s));
    }
    Ok((n * mult as f64) as u64)
}

fn parse_delimiter(s: &str) -> Result<u8, String> {
    match s {
        "\\t" | "tab" => Ok(b'\t'),
        "\\s" => Ok(b' '),
        other => {
            let b = other.as_bytes();
            if b.len() == 1 {
                Ok(b[0])
            } else {
                Err(format!("delimiter harus 1 byte ASCII, dapat '{}'", other))
            }
        }
    }
}

/// Consumes the CSV sub-flags (--key-column/--multi-key/--key-type) that
/// follow `--format csv`. Returns (delimiter, keys, key_numeric).
fn parse_csv_flags(args: &[String], i: &mut usize) -> Result<(u8, Vec<usize>, bool), String> {
    // `--format csv` optionally takes a delimiter value: `--format csv ';'`.
    // Otherwise the delimiter defaults to ',' (overridable via --delimiter).
    let mut delim: u8 = b',';
    if let Some(next) = args.get(*i + 1)
        && !next.starts_with("--") {
            delim = parse_delimiter(next)?;
            *i += 1;
        }
    let mut keys: Vec<usize> = Vec::new();
    let mut key_numeric = false;
    let mut j = *i + 1;
    while j < args.len() {
        match args[j].as_str() {
            "--key-column" => {
                let v = args
                    .get(j + 1)
                    .ok_or_else(|| "--key-column butuh nilai (index kolom)".to_string())?;
                keys.push(v.parse::<usize>().map_err(|_| format!("--key-column tidak valid: '{}'", v))?);
                j += 2;
            }
            "--multi-key" => {
                let v = args
                    .get(j + 1)
                    .ok_or_else(|| "--multi-key butuh nilai (mis. \"1,3\")".to_string())?;
                for p in v.split(',') {
                    keys.push(
                        p.trim()
                            .parse::<usize>()
                            .map_err(|_| format!("--multi-key tidak valid: '{}'", p))?,
                    );
                }
                j += 2;
            }
            "--key-type" => {
                let v = args
                    .get(j + 1)
                    .ok_or_else(|| "--key-type butuh nilai numeric|string|date".to_string())?;
                match v.as_str() {
                    "numeric" => key_numeric = true,
                    // date = ISO-8601 lexicographic == chronological: string compare
                    "string" | "date" => key_numeric = false,
                    other => return Err(format!("--key-type harus numeric|string|date, dapat '{}'", other)),
                }
                j += 2;
            }
            _ => break,
        }
    }
    *i = j - 1; // main loop's `i += 1` lands on the last consumed token
    if keys.is_empty() {
        return Err("--format csv membutuhkan --key-column N (atau --multi-key)".to_string());
    }
    keys.sort_unstable();
    keys.dedup();
    if keys.len() > 4 {
        return Err("maksimal 4 key column didukung".to_string());
    }
    Ok((delim, keys, key_numeric))
}

fn parse_args() -> Result<Config, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        println!("{}", usage());
        std::process::exit(errors::USAGE);
    }
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut max_memory: Option<u64> = None;
    let mut fmt = SortFormat::Numeric;
    let mut max_open_files: Option<usize> = None;
    let mut do_verify = false;
    let mut threads: Option<usize> = None;
    let mut quiet = false;
    let mut json = false;
    let mut dashboard: Option<String> = None;
    let mut temp_dir: Option<PathBuf> = None;
    let mut resume: Option<String> = None;
    let mut reverse = false;
    let mut unique = false;
    let mut header = false;
    let mut header_explicit = false;
    let mut dry_run = false;
    let mut limit: Option<usize> = None;
    let mut log_file: Option<PathBuf> = None;
    let mut gui: Option<String> = None;
    let mut preview: Option<usize> = None;
    let mut preview_json = false;
    let mut stats = false;
    let mut stats_json = false;
    let mut check = false;
    let mut encoding: Option<String> = None;
    let mut encoding_in: Option<String> = None;
    let mut encoding_out: Option<String> = None;
    let mut delimiter_explicit = false;
    // Lapis-2 flags
    let mut key_field: Option<String> = None;
    let mut dedupe_by: Option<String> = None;
    let mut dedupe_keep = String::from("first");
    let mut merge_inputs: Vec<String> = Vec::new();
    let mut output_format: Option<String> = None;
    let mut sql_table: Option<String> = None;
    let mut split_by: Option<usize> = None;
    // Lapis-3 flags
    let mut interactive = false;
    let mut watch: Option<String> = None;
    let mut output_dir: Option<String> = None;
    let mut api: Option<String> = None;
    let mut api_token: Option<String> = None;
    let mut brand: Option<String> = None;
    let mut brand_color: Option<String> = None;
    let mut sheet: Option<String> = None;

    let mut i = 0usize;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--version" | "-V" => {
                println!("mergesort {} (External Merge Sort Engine)", VERSION);
                std::process::exit(errors::OK);
            }
            "--input" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--input butuh nilai".to_string())?;
                input = Some(PathBuf::from(v));
            }
            "--output" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--output butuh nilai".to_string())?;
                output = Some(PathBuf::from(v));
            }
            "--max-memory" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--max-memory butuh nilai".to_string())?;
                max_memory = Some(parse_size(v)?);
            }
            "--mode" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--mode butuh nilai".to_string())?;
                fmt = SortFormat::parse_mode(v)
                    .ok_or_else(|| format!("--mode harus numeric|string|jsonl|excel, dapat '{}'", v))?;
            }
            // Standalone key flags (for --mode excel / --mode jsonl flows;
            // --format csv consumes its own via parse_csv_flags).
            "--key-column" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--key-column butuh nilai".to_string())?;
                let k: usize = v.parse().map_err(|_| format!("--key-column tidak valid: '{}'", v))?;
                match &mut fmt {
                    SortFormat::Csv { keys, .. } => {
                        keys.push(k);
                        keys.sort_unstable();
                        keys.dedup();
                        if keys.len() > 4 {
                            return Err("maksimal 4 key column didukung".to_string());
                        }
                    }
                    SortFormat::Jsonl { .. } => return Err("--key-column tidak berlaku dengan jsonl (pakai --key-field)".to_string()),
                    _ => return Err("--key-column hanya berlaku dengan --format csv / --mode excel".to_string()),
                }
            }
            "--multi-key" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--multi-key butuh nilai".to_string())?;
                match &mut fmt {
                    SortFormat::Csv { keys, .. } => {
                        for p in v.split(',') {
                            keys.push(p.trim().parse::<usize>().map_err(|_| format!("--multi-key tidak valid: '{}'", p))?);
                        }
                        keys.sort_unstable();
                        keys.dedup();
                        if keys.len() > 4 {
                            return Err("maksimal 4 key column didukung".to_string());
                        }
                    }
                    _ => return Err("--multi-key hanya berlaku dengan --format csv / --mode excel".to_string()),
                }
            }
            "--key-type" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--key-type butuh nilai".to_string())?;
                let num = match v.as_str() {
                    "numeric" => true,
                    "string" | "date" => false,
                    _ => return Err(format!("--key-type harus numeric|string|date, dapat '{}'", v)),
                };
                match &mut fmt {
                    SortFormat::Csv { key_numeric, .. } => *key_numeric = num,
                    SortFormat::Jsonl { key_numeric, .. } => *key_numeric = num,
                    _ => return Err("--key-type hanya berlaku dengan --format csv|jsonl".to_string()),
                }
            }
            "--format" => {
                i += 1;
                match args.get(i).map(|s| s.as_str()) {
                    Some("numeric") => fmt = SortFormat::Numeric,
                    Some("string") => fmt = SortFormat::String,
                    Some("excel") => {
                        fmt = SortFormat::Csv { delimiter: b',', keys: Vec::new(), key_numeric: false };
                    }
                    Some("jsonl") => {
                        fmt = SortFormat::Jsonl { field: Vec::new(), key_numeric: false };
                    }
                    Some("csv") => {
                        let (d, k, kn) = parse_csv_flags(&args, &mut i)?;
                        fmt = SortFormat::Csv { delimiter: d, keys: k, key_numeric: kn };
                    }
                    Some(other) => {
                        return Err(format!("--format harus numeric|string|csv|jsonl|excel, dapat '{}'", other))
                    }
                    None => return Err("--format butuh nilai".to_string()),
                }
            }
            "--delimiter" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--delimiter butuh nilai".to_string())?;
                delimiter_explicit = true;
                let d = parse_delimiter(v)?;
                // Set/patch the CSV variant's delimiter.
                fmt = match fmt {
                    SortFormat::Csv { keys, key_numeric, .. } => {
                        SortFormat::Csv { delimiter: d, keys, key_numeric }
                    }
                    other => other,
                };
            }
            "--max-open-files" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--max-open-files butuh nilai".to_string())?;
                max_open_files = Some(
                    v.parse::<usize>()
                        .map_err(|_| format!("--max-open-files tidak valid: '{}'", v))?,
                );
            }
            "--verify" => do_verify = true,
            "--reverse" | "-r" => reverse = true,
            "--unique" | "-u" => unique = true,
            "--header" => {
                header = true;
                header_explicit = true;
            }
            "--no-header" => {
                header = false;
                header_explicit = true;
            }
            "--dry-run" => dry_run = true,
            "--preview" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--preview butuh nilai N".to_string())?;
                let n: usize = v.parse().map_err(|_| format!("--preview tidak valid: '{}'", v))?;
                if n == 0 {
                    return Err("--preview harus >= 1".to_string());
                }
                preview = Some(n);
            }
            "--preview-json" => preview_json = true,
            "--stats" => stats = true,
            "--stats-json" => stats_json = true,
            "--check" => check = true,
            "--encoding" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--encoding butuh nilai".to_string())?;
                if inspect::TextEncoding::parse(v).is_none() {
                    return Err(format!("--encoding tidak dikenal: '{}' (utf-8|latin-1|utf-16le|utf-16be)", v));
                }
                encoding = Some(v.clone());
            }
            "--encoding-in" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--encoding-in butuh nilai".to_string())?;
                if inspect::TextEncoding::parse(v).is_none() {
                    return Err(format!("--encoding-in tidak dikenal: '{}'", v));
                }
                encoding_in = Some(v.clone());
            }
            "--encoding-out" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--encoding-out butuh nilai".to_string())?;
                if inspect::TextEncoding::parse(v).is_none() {
                    return Err(format!("--encoding-out tidak dikenal: '{}'", v));
                }
                encoding_out = Some(v.clone());
            }
            "--threads" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--threads butuh nilai".to_string())?;
                threads = Some(
                    v.parse::<usize>()
                        .map_err(|_| format!("--threads tidak valid: '{}'", v))?,
                );
            }
            "--quiet" | "-q" => quiet = true,
            "--json" => json = true,
            "--log-file" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--log-file butuh nilai".to_string())?;
                log_file = Some(PathBuf::from(v));
            }
            "--limit" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--limit butuh nilai".to_string())?;
                let n: usize = v.parse().map_err(|_| format!("--limit tidak valid: '{}'", v))?;
                if n == 0 {
                    return Err("--limit harus >= 1".to_string());
                }
                limit = Some(n);
            }
            "--gui" | "--serve" => {
                i += 1;
                // Optional value: `--gui 8080` or `--gui 127.0.0.1:8080`; bare = 127.0.0.1:8080
                match args.get(i) {
                    Some(v) if !v.starts_with("--") => gui = Some(v.clone()),
                    _ => {
                        gui = Some("127.0.0.1:8080".to_string());
                        continue;
                    }
                }
            }
            "--dashboard" => {
                i += 1;
                dashboard = Some(
                    args.get(i)
                        .ok_or_else(|| "--dashboard butuh nilai ([host:]PORT)".to_string())?
                        .clone(),
                );
            }
            "--temp-dir" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--temp-dir butuh nilai".to_string())?;
                temp_dir = Some(PathBuf::from(v));
            }
            "--key-field" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--key-field butuh nilai (mis. \"user.age\")".to_string())?;
                if v.trim().is_empty() {
                    return Err("--key-field tidak boleh kosong".to_string());
                }
                key_field = Some(v.clone());
            }
            "--dedupe-by" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--dedupe-by butuh nilai kolom (mis. \"2\" atau \"2,3\")".to_string())?;
                dedupe_by = Some(v.clone());
            }
            "--dedupe-keep" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--dedupe-keep butuh nilai first|last".to_string())?;
                match v.as_str() {
                    "first" | "last" => dedupe_keep = v.clone(),
                    _ => return Err(format!("--dedupe-keep harus first|last, dapat '{}'", v)),
                }
            }
            "--merge" => {
                // Consume following non-flag values as pre-sorted input files.
                loop {
                    match args.get(i + 1) {
                        Some(v) if !v.starts_with("--") => {
                            merge_inputs.push(v.clone());
                            i += 1;
                        }
                        _ => break,
                    }
                }
                if merge_inputs.is_empty() {
                    return Err("--merge butuh minimal 1 file".to_string());
                }
            }
            "--output-format" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--output-format butuh nilai csv|tsv|jsonl|sql".to_string())?;
                match v.as_str() {
                    "csv" | "tsv" | "jsonl" | "sql" => output_format = Some(v.clone()),
                    _ => return Err(format!("--output-format harus csv|tsv|jsonl|sql, dapat '{}'", v)),
                }
            }
            "--sql-table" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--sql-table butuh nilai nama tabel".to_string())?;
                sql_table = Some(v.clone());
            }
            "--split-by" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--split-by butuh nilai N".to_string())?;
                let n: usize = v.parse().map_err(|_| format!("--split-by tidak valid: '{}'", v))?;
                if n < 2 {
                    return Err("--split-by harus >= 2".to_string());
                }
                split_by = Some(n);
            }
            "--interactive" => interactive = true,
            "--watch" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--watch butuh nilai folder".to_string())?;
                watch = Some(v.clone());
            }
            "--output-dir" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--output-dir butuh nilai folder".to_string())?;
                output_dir = Some(v.clone());
            }
            "--api" => {
                i += 1;
                match args.get(i) {
                    Some(v) if !v.starts_with("--") => api = Some(v.clone()),
                    _ => {
                        api = Some("127.0.0.1:8080".to_string());
                        continue;
                    }
                }
            }
            "--api-token" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--api-token butuh nilai".to_string())?;
                api_token = Some(v.clone());
            }
            "--brand" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--brand butuh nilai".to_string())?;
                brand = Some(v.clone());
            }
            "--brand-color" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--brand-color butuh nilai hex".to_string())?;
                brand_color = Some(v.clone());
            }
            "--sheet" => {
                i += 1;
                let v = args.get(i).ok_or_else(|| "--sheet butuh nilai nama sheet".to_string())?;
                sheet = Some(v.clone());
            }
            "--resume" => {
                i += 1;
                // Optional value: `--resume list|run_id`; bare `--resume` = newest.
                match args.get(i) {
                    Some(v) if !v.starts_with("--") => resume = Some(v.clone()),
                    _ => {
                        resume = Some(String::new());
                        continue; // don't consume the next token
                    }
                }
            }
            "--help" | "-h" => {
                println!("{}", usage());
                std::process::exit(errors::OK);
            }
            other => return Err(format!("flag tidak dikenal: {}", other)),
        }
        i += 1;
    }

    // Alternate server modes don't need input/output/memory.
    if gui.is_some() || api.is_some() {
        return Ok(Config {
            input: input.unwrap_or_else(|| PathBuf::from("")),
            output: output.unwrap_or_else(|| PathBuf::from("")),
            max_memory: max_memory.unwrap_or(512 * 1024 * 1024),
            fmt,
            max_open_files,
            do_verify,
            threads,
            quiet: true,
            json: false,
            dashboard: None,
            temp_dir,
            resume: None,
            reverse,
            unique,
            header,
            header_explicit,
            dry_run: false,
            limit,
            log_file,
            gui,
            preview,
            preview_json,
            stats,
            stats_json,
            check,
            encoding,
            encoding_in,
            encoding_out,
            delimiter_explicit,
            key_field,
            dedupe_by: None,
            dedupe_keep_last: false,
            merge_inputs: Vec::new(),
            output_format,
            sql_table,
            split_by,
            interactive: false,
            watch: watch.map(PathBuf::from),
            output_dir: output_dir.map(PathBuf::from),
            api,
            api_token,
            brand,
            brand_color,
            sheet: None,
        });
    }
    // --interactive / --watch resolve their own inputs at runtime.
    if interactive || watch.is_some() {
        return Ok(Config {
            input: input.unwrap_or_else(|| PathBuf::from("")),
            output: output.unwrap_or_else(|| PathBuf::from("")),
            max_memory: max_memory.unwrap_or(512 * 1024 * 1024),
            fmt,
            max_open_files,
            do_verify,
            threads,
            quiet,
            json,
            dashboard,
            temp_dir,
            resume,
            reverse,
            unique,
            header,
            header_explicit,
            dry_run,
            limit,
            log_file,
            gui: None,
            preview,
            preview_json,
            stats,
            stats_json,
            check,
            encoding,
            encoding_in,
            encoding_out,
            delimiter_explicit,
            key_field,
            dedupe_by: None,
            dedupe_keep_last: false,
            merge_inputs: Vec::new(),
            output_format,
            sql_table,
            split_by,
            interactive,
            watch: watch.map(PathBuf::from),
            output_dir: output_dir.map(PathBuf::from),
            api: None,
            api_token,
            brand,
            brand_color,
            sheet: None,
        });
    }
    // --merge mode: inputs = pre-sorted files, needs --output + --max-memory.
    let is_merge = !merge_inputs.is_empty();
    if is_merge {
        if input.is_some() {
            return Err("--merge tidak bisa digabung dengan --input".to_string());
        }
        if output.is_none() || max_memory.is_none() {
            return Err("--merge butuh --output dan --max-memory".to_string());
        }
        if resume.is_some() {
            return Err("--merge tidak mendukung --resume".to_string());
        }
        if preview.is_some() || preview_json {
            return Err("--merge tidak bisa digabung dengan --preview".to_string());
        }
        if check {
            return Err("--merge tidak bisa digabung dengan --check".to_string());
        }
    }
    // --check needs only --input (+ format hints). --preview needs input+memory, output optional.
    let is_preview = preview.is_some() || preview_json;
    if check {
        if input.is_none() {
            return Err("flag wajib belum lengkap (butuh --input untuk --check)".to_string());
        }
        if max_memory.is_none() {
            max_memory = Some(512 * 1024 * 1024);
        }
        if output.is_none() {
            output = Some(PathBuf::from(""));
        }
    } else if is_preview {
        if input.is_none() || max_memory.is_none() {
            return Err("flag wajib belum lengkap (butuh --input, --max-memory untuk --preview; --output opsional)".to_string());
        }
        if output.is_none() {
            output = Some(PathBuf::from(""));
        }
    } else if !is_merge && (input.is_none() || output.is_none() || max_memory.is_none()) {
        return Err("flag wajib belum lengkap (butuh --input, --output, --max-memory) atau gunakan --gui/--check/--preview/--merge".to_string());
    }
    // Empty CSV keys (e.g. bare --mode excel without --key-column).
    if let SortFormat::Csv { keys, .. } = &fmt
        && keys.is_empty() {
            return Err("--format csv / --mode excel membutuhkan --key-column N (atau --multi-key)".to_string());
        }
    if header && matches!(fmt, SortFormat::Numeric) {
        return Err("--header hanya berlaku untuk --mode string atau --format csv/jsonl (numeric binary tidak punya header)".to_string());
    }
    // --mode/--format jsonl needs --key-field (dot path).
    if matches!(fmt, SortFormat::Jsonl { .. }) {
        let kf = key_field.clone().ok_or_else(|| "--mode jsonl membutuhkan --key-field \"a.b.c\"".to_string())?;
        let path: Vec<String> = kf.split('.').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if path.is_empty() {
            return Err("--key-field tidak boleh kosong".to_string());
        }
        let kn = match &fmt {
            SortFormat::Jsonl { key_numeric, .. } => *key_numeric,
            _ => false,
        };
        // --key-type after --format jsonl isn't consumed by parse_csv_flags; read it here.
        let mut kn2 = kn;
        for (idx, a) in args.iter().enumerate() {
            if a == "--key-type" {
                match args.get(idx + 1).map(|s| s.as_str()) {
                    Some("numeric") => kn2 = true,
                    Some("string") | Some("date") => kn2 = false,
                    Some(o) => return Err(format!("--key-type harus numeric|string|date, dapat '{}'", o)),
                    None => return Err("--key-type butuh nilai".to_string()),
                }
            }
        }
        fmt = SortFormat::Jsonl { field: path, key_numeric: kn2 };
        if key_field.is_some() && args.iter().any(|a| a == "--key-column" || a == "--multi-key") {
            return Err("--key-column/--multi-key tidak berlaku dengan jsonl (pakai --key-field)".to_string());
        }
    } else if key_field.is_some() {
        return Err("--key-field hanya berlaku dengan --mode jsonl".to_string());
    }
    // --dedupe-by: csv only, parse "2" / "2,3".
    let dedupe_cols: Option<Vec<usize>> = match dedupe_by.as_deref() {
        None => None,
        Some(s) => {
            if !matches!(fmt, SortFormat::Csv { .. }) {
                return Err("--dedupe-by hanya berlaku dengan --format csv".to_string());
            }
            let mut cols = Vec::new();
            for p in s.split(',') {
                cols.push(p.trim().parse::<usize>().map_err(|_| format!("--dedupe-by tidak valid: '{}'", p))?);
            }
            cols.sort_unstable();
            cols.dedup();
            if cols.is_empty() {
                return Err("--dedupe-by tidak boleh kosong".to_string());
            }
            Some(cols)
        }
    };
    if split_by.is_some() && is_preview {
        return Err("--split-by tidak bisa digabung dengan --preview".to_string());
    }
    if output_format.as_deref() == Some("sql") && !matches!(fmt, SortFormat::Csv { .. }) {
        return Err("--output-format sql butuh input --format csv (header jadi nama kolom)".to_string());
    }
    Ok(Config {
        input: input.unwrap_or_else(|| PathBuf::from("")),
        output: output.unwrap(),
        max_memory: max_memory.unwrap(),
        fmt,
        max_open_files,
        do_verify,
        threads,
        quiet: quiet || json,
        json,
        dashboard,
        temp_dir,
        resume,
        reverse,
        unique,
        header,
        header_explicit,
        dry_run,
        limit,
        log_file,
        gui: None,
        preview,
        preview_json,
        stats,
        stats_json,
        check,
        encoding,
        encoding_in,
        encoding_out,
        delimiter_explicit,
        key_field,
        dedupe_by: dedupe_cols,
        dedupe_keep_last: dedupe_keep == "last",
        merge_inputs: merge_inputs.into_iter().map(PathBuf::from).collect(),
        output_format,
        sql_table,
        split_by,
        interactive,
        watch: watch.map(PathBuf::from),
        output_dir: output_dir.map(PathBuf::from),
        api,
        api_token,
        brand,
        brand_color,
        sheet,
    })
}

/// Best-effort open-file limit detection. Falls back to a conservative default.
fn detect_max_open_files() -> usize {
    #[cfg(windows)]
    {
        // Windows CRT default is 512; allow a safe fan-in below it.
        256
    }
    #[cfg(not(windows))]
    {
        if let Ok(txt) = std::fs::read_to_string(Path::new("/proc/self/limits")) {
            for line in txt.lines() {
                if line.starts_with("Max open files") {
                    let parts = line.split_ascii_whitespace().collect::<Vec<_>>();
                    if let Some(v) = parts.get(3).and_then(|s| s.parse::<usize>().ok()) {
                        return v.saturating_sub(16).max(8);
                    }
                }
            }
        }
        128
    }
}

/// I/O buffer sizes scaled so that (fan_in + 1) buffers fit in ~25% of the
/// memory budget, clamped to a sane range.
fn io_buffer_sizes(max_memory: u64, fan_in: usize) -> (usize, usize) {
    const MIN: usize = 4 * 1024;
    const MAX: usize = 128 * 1024;
    let block_overhead = (64 * 1024) * (fan_in + 1);
    let io_budget = ((max_memory as usize) / 4)
        .saturating_sub(block_overhead)
        .max(64 * 1024);
    let per_stream = io_budget / (fan_in + 1);
    let sz = per_stream.clamp(MIN, MAX);
    (sz, sz)
}

/// Returns a staging path next to the final output: `<name>.part` in the same
/// directory (same volume => atomic rename at the end).
fn stage_output_part(output: &Path) -> Result<PathBuf, String> {
    let dir = match output.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    if !dir.is_dir() {
        return Err(format!(
            "direktori output tidak ada: {} (buat dulu atau pilih path lain)",
            dir.display()
        ));
    }
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    Ok(dir.join(Path::new(&format!(".{}.part", name))))
}

/// Durably flushes and atomically renames the staged output over the final path.
fn finalize_output(staged: &Path, final_path: &Path) -> std::io::Result<()> {
    if let Ok(f) = std::fs::File::open(staged) {
        f.sync_all().ok();
    }
    std::fs::rename(staged, final_path)
        .or_else(|_| std::fs::copy(staged, final_path).map(|_| ()))?;
    Ok(())
}

/// Reads the first line (header) as raw bytes, without the trailing newline.
fn read_header_line(input: &Path) -> Result<Vec<u8>, String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(input).map_err(|e| format!("cannot read header: {}", e))?;
    let mut r = BufReader::new(f);
    let mut line: Vec<u8> = Vec::new();
    r.read_until(b'\n', &mut line).map_err(|e| format!("cannot read header: {}", e))?;
    while line.last() == Some(&b'\n') || line.last() == Some(&b'\r') {
        line.pop();
    }
    if line.is_empty() {
        return Err("input empty: --header needs at least one header line".to_string());
    }
    Ok(line)
}

/// Prepends the header row in front of an already-sorted staged file.
/// Streams staged -> final-with-header so even 100GB files don't need extra RAM.
fn prepend_header_to_staged(staged: &Path, header: &[u8]) -> std::io::Result<()> {
    use std::io::{BufReader, BufWriter, Read, Write};
    let with_header = staged.with_extension("withheader.part");
    let fin = std::fs::File::open(staged)?;
    let mut rin = BufReader::with_capacity(64 * 1024, fin);
    let fout = std::fs::File::create(&with_header)?;
    let mut wout = BufWriter::with_capacity(64 * 1024, fout);
    wout.write_all(header)?;
    wout.write_all(b"\n")?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = rin.read(&mut buf)?;
        if n == 0 {
            break;
        }
        wout.write_all(&buf[..n])?;
    }
    wout.flush()?;
    drop(wout);
    std::fs::rename(&with_header, staged)?;
    Ok(())
}

/// Keeps only the first `n` records of an already-sorted staged file (Top-N).
/// Numeric (binary u64) truncates by bytes; text modes stream the first N data
/// lines (+ header when present). Heap-allocated buffers only.
fn truncate_to_limit(staged: &Path, fmt: &SortFormat, n: usize, has_header: bool) -> std::io::Result<()> {
    use std::io::{BufRead, BufReader, BufWriter, Write};
    if matches!(fmt, SortFormat::Numeric) {
        let meta = std::fs::metadata(staged)?;
        let keep = (n as u64 * 8).min(meta.len());
        let f = std::fs::OpenOptions::new().write(true).open(staged)?;
        f.set_len(keep)?;
        return Ok(());
    }
    let tmp = staged.with_extension("toplimit.part");
    let fin = std::fs::File::open(staged)?;
    let mut rin = BufReader::with_capacity(64 * 1024, fin);
    let fout = std::fs::File::create(&tmp)?;
    let mut wout = BufWriter::with_capacity(64 * 1024, fout);
    let mut kept: usize = 0;
    let mut first = true;
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        let r = rin.read_until(b'\n', &mut line)?;
        if r == 0 {
            break;
        }
        if has_header && first {
            wout.write_all(&line)?;
            first = false;
            continue;
        }
        first = false;
        if kept >= n {
            continue; // skip remainder (still drains input, cheap)
        }
        wout.write_all(&line)?;
        kept += 1;
    }
    wout.flush()?;
    drop(wout);
    std::fs::rename(&tmp, staged)?;
    Ok(())
}

/// Converts a sorted CSV output file to tsv|jsonl|sql|csv (streaming).
/// `sql_table` defaults to "data". JSONL/SQL need column names: from the
/// header row when present, else col0/col1/...
fn convert_output_format(
    output: &Path,
    fmt: &SortFormat,
    to: &str,
    has_header: bool,
    sql_table: Option<String>,
) -> Result<(), String> {
    use std::io::{BufRead, BufReader, BufWriter, Write};
    let (delim, _keys) = match fmt {
        SortFormat::Csv { delimiter, keys, .. } => (*delimiter, keys.clone()),
        _ => return Err("output-format convert butuh input --format csv".to_string()),
    };
    if to == "csv" && delim == b',' {
        return Ok(()); // no-op
    }
    let fin = std::fs::File::open(output).map_err(|e| e.to_string())?;
    let mut rin = BufReader::with_capacity(64 * 1024, fin);
    let tmp = output.with_extension("outfmt.part");
    let fout = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut wout = BufWriter::with_capacity(64 * 1024, fout);
    let table = sql_table.unwrap_or_else(|| "data".to_string());
    let mut col_names: Vec<String> = Vec::new();
    let mut first = true;
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        let r = rin.read_until(b'\n', &mut line).map_err(|e| e.to_string())?;
        if r == 0 {
            break;
        }
        let mut end = line.len();
        if line.last() == Some(&b'\n') {
            end -= 1;
        }
        if end > 0 && line[end - 1] == b'\r' {
            end -= 1;
        }
        let rec = &line[..end];
        if first && has_header {
            first = false;
            let h = String::from_utf8_lossy(rec).into_owned();
            col_names = h.split(delim as char).map(|s| s.trim().to_string()).collect();
            match to {
                "csv" | "tsv" => {
                    let out_line = if to == "tsv" { h.replace(delim as char, "\t") } else { h };
                    wout.write_all(out_line.as_bytes()).map_err(|e| e.to_string())?;
                    wout.write_all(b"\n").map_err(|e| e.to_string())?;
                }
                "jsonl" | "sql" => {} // header jadi nama kolom saja, tidak ditulis
                _ => {}
            }
            continue;
        }
        first = false;
        let s = String::from_utf8_lossy(rec).into_owned();
        let fields: Vec<&str> = s.split(delim as char).collect();
        if col_names.is_empty() {
            col_names = (0..fields.len()).map(|i| format!("col{}", i)).collect();
        }
        match to {
            "csv" => {
                wout.write_all(rec).map_err(|e| e.to_string())?;
                wout.write_all(b"\n").map_err(|e| e.to_string())?;
            }
            "tsv" => {
                wout.write_all(s.replace(delim as char, "\t").as_bytes()).map_err(|e| e.to_string())?;
                wout.write_all(b"\n").map_err(|e| e.to_string())?;
            }
            "jsonl" => {
                let mut obj = String::from("{");
                for (i, (c, f)) in col_names.iter().zip(fields.iter()).enumerate() {
                    if i > 0 {
                        obj.push(',');
                    }
                    obj.push('"');
                    obj.push_str(&c.replace('"', "\\\""));
                    obj.push_str("\":\"");
                    obj.push_str(&f.replace('\\', "\\\\").replace('"', "\\\""));
                    obj.push('"');
                }
                obj.push('}');
                wout.write_all(obj.as_bytes()).map_err(|e| e.to_string())?;
                wout.write_all(b"\n").map_err(|e| e.to_string())?;
            }
            "sql" => {
                let cols = col_names.iter().map(|c| format!("\"{}\"", c.replace('"', "\"\""))).collect::<Vec<_>>().join(", ");
                let vals = fields.iter().map(|f| format!("'{}'", f.replace('\'', "''"))).collect::<Vec<_>>().join(", ");
                let stmt = format!("INSERT INTO \"{}\" ({}) VALUES ({});\n", table.replace('"', "\"\""), cols, vals);
                wout.write_all(stmt.as_bytes()).map_err(|e| e.to_string())?;
            }
            _ => {}
        }
    }
    wout.flush().map_err(|e| e.to_string())?;
    drop(wout);
    std::fs::rename(&tmp, output).map_err(|e| e.to_string())?;
    Ok(())
}

/// Splits a final output file into N equal files (header repeated per part).
/// Returns created part paths: `<stem>-001.<ext>`, ... Numeric binary splits by records.
fn split_output_by_count(output: &Path, n: usize, has_header: bool) -> std::io::Result<Vec<PathBuf>> {
    use std::io::{BufRead, BufReader, BufWriter, Write};
    let stem = output.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "out".to_string());
    let ext = output.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let parent = output.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    // Count data rows first (header excluded).
    let total: usize = if output.extension().map(|e| e == "bin").unwrap_or(false) {
        (std::fs::metadata(output)?.len() / 8) as usize
    } else {
        let f = std::fs::File::open(output)?;
        let r = BufReader::with_capacity(64 * 1024, f);
        let mut c = 0usize;
        let mut first = true;
        for line in r.lines() {
            line?;
            if has_header && first {
                first = false;
                continue;
            }
            first = false;
            c += 1;
        }
        c
    };
    let per = total.div_ceil(n).max(1);
    let mut parts: Vec<PathBuf> = Vec::new();
    if output.extension().map(|e| e == "bin").unwrap_or(false) {
        let raw = std::fs::read(output)?;
        for i in 0..n {
            let s = (i * per * 8).min(raw.len());
            let e = ((i + 1) * per * 8).min(raw.len());
            if s >= e {
                break;
            }
            let p = parent.join(format!("{}-{:03}{}", stem, i + 1, ext));
            std::fs::write(&p, &raw[s..e])?;
            parts.push(p);
        }
        return Ok(parts);
    }
    let f = std::fs::File::open(output)?;
    let mut rin = BufReader::with_capacity(64 * 1024, f);
    let mut header_line: Vec<u8> = Vec::new();
    if has_header {
        rin.read_until(b'\n', &mut header_line)?;
    }
    let mut idx = 0usize;
    let mut in_part = 0usize;
    let mut wout: Option<BufWriter<std::fs::File>> = None;
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        let r = rin.read_until(b'\n', &mut line)?;
        if r == 0 {
            break;
        }
        if in_part == 0 {
            let p = parent.join(format!("{}-{:03}{}", stem, idx + 1, ext));
            let fout = std::fs::File::create(&p)?;
            let mut w = BufWriter::with_capacity(64 * 1024, fout);
            if has_header && !header_line.is_empty() {
                w.write_all(&header_line)?;
            }
            parts.push(p);
            wout = Some(w);
            idx += 1;
        }
        if let Some(w) = wout.as_mut() {
            w.write_all(&line)?;
        }
        in_part += 1;
        if in_part >= per && idx < n {
            if let Some(mut w) = wout.take() {
                w.flush()?;
            }
            in_part = 0;
        }
    }
    if let Some(mut w) = wout.take() {
        w.flush()?;
    }
    Ok(parts)
}

/// Compression + Excel filename helpers (Lapis-3: transparent .gz/.zip/.xlsx).
fn ext_is(path: &Path, ext: &str) -> bool {
    path.extension().map(|e| e.to_string_lossy().to_lowercase() == ext).unwrap_or(false)
}

/// Decompresses .gz/.zip input to a temp file (streaming). Returns
/// (path_to_use, Option<temp_to_delete>). .zst is rejected with guidance.
fn maybe_decompress_input(input: &Path) -> Result<(PathBuf, Option<PathBuf>), String> {
    use std::io::{BufReader, BufWriter, Read};
    if ext_is(input, "zst") || ext_is(input, "zstd") {
        return Err("file .zst belum didukung — kompres ulang ke .gz (gzip) lalu coba lagi".to_string());
    }
    if ext_is(input, "gz") {
        let f = std::fs::File::open(input).map_err(|e| format!("buka {}: {}", input.display(), e))?;
        let mut dec = flate2::read::GzDecoder::new(BufReader::with_capacity(64 * 1024, f));
        let tmp = input.with_extension("decompressed.tmp");
        let fout = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut wout = BufWriter::with_capacity(64 * 1024, fout);
        std::io::copy(&mut dec, &mut wout).map_err(|e| format!("gunzip gagal: {}", e))?;
        drop(wout);
        return Ok((tmp.clone(), Some(tmp)));
    }
    if ext_is(input, "zip") {
        let f = std::fs::File::open(input).map_err(|e| format!("buka {}: {}", input.display(), e))?;
        let mut zip = zip::ZipArchive::new(BufReader::new(f)).map_err(|e| format!("baca zip gagal: {}", e))?;
        if zip.is_empty() {
            return Err("zip kosong".to_string());
        }
        if zip.len() > 1 {
            eprintln!("peringatan: zip berisi {} file — hanya file pertama yang di-sort", zip.len());
        }
        let mut zf = zip.by_index(0).map_err(|e| format!("extract zip gagal: {}", e))?;
        let tmp = input.with_extension("decompressed.tmp");
        let fout = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut wout = BufWriter::with_capacity(64 * 1024, fout);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n: usize = zf.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            use std::io::Write;
            wout.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
        drop(wout);
        return Ok((tmp.clone(), Some(tmp)));
    }
    Ok((input.to_path_buf(), None))
}

/// Compresses a finished output file in place (.gz/.zip by extension).
fn maybe_compress_output(output: &Path) -> Result<(), String> {
    use std::io::{BufReader, BufWriter, Read, Write};
    if ext_is(output, "zst") || ext_is(output, "zstd") {
        return Err("output .zst belum didukung — pakai .gz".to_string());
    }
    if ext_is(output, "gz") {
        let f = std::fs::File::open(output).map_err(|e| e.to_string())?;
        let mut rin = BufReader::with_capacity(64 * 1024, f);
        let tmp = output.with_extension("compressing.tmp");
        let fout = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let wout = BufWriter::with_capacity(64 * 1024, fout);
        let mut enc = flate2::write::GzEncoder::new(wout, flate2::Compression::default());
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = rin.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            enc.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
        enc.finish().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, output).map_err(|e| e.to_string())?;
        return Ok(());
    }
    if ext_is(output, "zip") {
        let raw = std::fs::read(output).map_err(|e| e.to_string())?;
        let inner = output.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "data".to_string());
        let tmp = output.with_extension("compressing.tmp");
        let fout = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut zipw = zip::ZipWriter::new(fout);
        zipw.start_file(inner, zip::write::SimpleFileOptions::default()).map_err(|e| e.to_string())?;
        zipw.write_all(&raw).map_err(|e| e.to_string())?;
        zipw.finish().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, output).map_err(|e| e.to_string())?;
        return Ok(());
    }
    Ok(())
}

/// Reads an .xlsx sheet into CSV bytes (RFC-4180 minimal: quote on demand).
/// Empty cells become empty fields; dates/numbers use calamine display strings.
fn xlsx_sheet_to_csv(path: &Path, sheet_opt: Option<&str>) -> Result<Vec<u8>, String> {
    use calamine::{Reader, Xlsx};
    let mut wb: Xlsx<_> = calamine::open_workbook(path).map_err(|e| format!("buka xlsx gagal: {}", e))?;
    let names = wb.sheet_names();
    if names.is_empty() {
        return Err("xlsx tidak punya sheet".to_string());
    }
    let target = match sheet_opt {
        Some(s) => {
            if !names.iter().any(|n| n == s) {
                return Err(format!("sheet '{}' tidak ada (ada: {})", s, names.join(", ")));
            }
            s.to_string()
        }
        None => names[0].clone(),
    };
    let range = wb.worksheet_range(&target).map_err(|e| format!("baca sheet gagal: {}", e))?;
    let mut out: Vec<u8> = Vec::new();
    for row in range.rows() {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            let s = cell.to_string();
            if s.contains([',', '"', '\n', '\r']) {
                out.push(b'"');
                out.extend_from_slice(s.replace('"', "\"\"").as_bytes());
                out.push(b'"');
            } else {
                out.extend_from_slice(s.as_bytes());
            }
        }
        out.push(b'\n');
    }
    Ok(out)
}

/// Writes sorted CSV bytes to .xlsx (single sheet). Header row bolded.
fn csv_to_xlsx(csv: &[u8], path: &Path, sheet_name: &str) -> Result<(), String> {
    use rust_xlsxwriter::{Format, Workbook};
    let text = String::from_utf8_lossy(csv);
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name(sheet_name).map_err(|e| e.to_string())?;
    let bold = Format::new().set_bold();
    for (r, line) in text.lines().enumerate() {
        // Minimal CSV parse (handles quoted fields from our writer).
        let fields = split_csv_line(line);
        for (c, f) in fields.iter().enumerate() {
            // "007"-style zero-padded integers (zip codes, ids) must stay text
            // or Excel would silently drop the padding.
            let leading_zero_int = f.len() > 1 && f.starts_with('0') && !f.contains('.') && f.bytes().all(|b| b.is_ascii_digit());
            if r == 0 {
                ws.write_string_with_format(r as u32, c as u16, f, &bold).map_err(|e| e.to_string())?;
            } else if !leading_zero_int && let Ok(n) = f.parse::<f64>() {
                // Write as a real number so Excel can sum/sort/format it.
                ws.write_number(r as u32, c as u16, n).map_err(|e| e.to_string())?;
            } else {
                ws.write_string(r as u32, c as u16, f).map_err(|e| e.to_string())?;
            }
        }
    }
    wb.save(path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Minimal CSV line splitter (quotes + escaped "").
fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_q {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_q = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_q = true;
        } else if c == ',' {
            fields.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    fields.push(cur);
    fields
}

/// Counts data rows in a final output (header-aware for text).
fn count_output_rows(path: &Path, is_numeric: bool, has_header: bool) -> usize {
    use std::io::{BufRead, BufReader};
    if is_numeric {
        return (std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) / 8) as usize;
    }
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let r = BufReader::with_capacity(64 * 1024, f);
    let mut c = 0usize;
    let mut first = true;
    for line in r.lines().flatten() {
        if has_header && first {
            first = false;
            continue;
        }
        first = false;
        if line.is_empty() {
            continue;
        }
        c += 1;
    }
    c
}

/// --merge mode: K-way merge of pre-sorted files without re-sorting.
/// Headers: first file's header kept, rest skipped. Runs in current process.
fn run_merge_mode(cfg: &Config) {
    use std::time::Instant;
    let t0 = Instant::now();
    let files = &cfg.merge_inputs;
    for f in files {
        match std::fs::metadata(f) {
            Ok(m) if m.is_file() => {}
            _ => errors::fail(errors::PREFLIGHT, &format!("merge input bukan file: {}", f.display())),
        }
        let low = f.to_string_lossy().to_lowercase();
        if low.ends_with(".gz") || low.ends_with(".zip") || low.ends_with(".zst") || low.ends_with(".xlsx") {
            errors::fail(errors::USAGE, &format!("--merge tidak mendukung file terkompresi/xlsx (dekompres dulu): {}", f.display()));
        }
    }
    if matches!(cfg.fmt, SortFormat::Numeric) && cfg.header {
        errors::fail(errors::USAGE, "--merge numeric tidak memakai --header");
    }
    // Header bytes from first file (text modes + --header).
    let header_bytes: Option<Vec<u8>> = if cfg.header {
        match read_header_line(&files[0]) {
            Ok(h) => Some(h),
            Err(m) => errors::fail(errors::PREFLIGHT, &m),
        }
    } else {
        None
    };
    // All files' headers skipped at read time; first file's header re-added after.
    let skips: Vec<bool> = files.iter().map(|_| cfg.header).collect();
    let threads = cfg.threads.unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
    let _ = rayon::ThreadPoolBuilder::new().num_threads(threads).build_global();
    let max_open = cfg.max_open_files.unwrap_or_else(detect_max_open_files).max(2);
    let fan_in = max_open.saturating_sub(4).max(2);
    let (rb, wb) = io_buffer_sizes(cfg.max_memory, fan_in);
    // Temp run dir for intermediate passes (next to output).
    let run_dir = match temp_manager::create_run_dir(cfg.output.as_path(), cfg.temp_dir.as_deref()) {
        Ok(d) => d,
        Err(e) => errors::fail(errors::IO, &format!("tidak bisa membuat direktori temp: {}", errors::io_hint(&e))),
    };
    let staged = match stage_output_part(cfg.output.as_path()) {
        Ok(p) => p,
        Err(msg) => errors::fail(errors::PREFLIGHT, &msg),
    };
    temp_manager::register_staging(staged.clone());
    // Progress state: estimate rows for the bar.
    let est_rows: u64 = if matches!(cfg.fmt, SortFormat::Numeric) {
        files.iter().map(|f| std::fs::metadata(f).map(|m| m.len() / 8).unwrap_or(0)).sum()
    } else {
        files.iter().map(|f| {
            let sz = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
            sz / 64
        }).sum::<u64>().max(1)
    };
    progress::set_start();
    let state = progress::RunState::new(files.iter().map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0)).sum());
    state.records.store(est_rows, std::sync::atomic::Ordering::Relaxed);
    let mut bars = progress::Progress::new(state.clone(), !cfg.quiet);
    state.phase.store(progress::PHASE_MERGE, std::sync::atomic::Ordering::Relaxed);
    let sampler = MemorySampler::spawn(Duration::from_millis(50));
    let mut manifest_opt: Option<Manifest> = None;
    // First file's header is re-added after merge (others were skipped).
    let mres = merge::merge_all_inner_public(
        files.clone(),
        staged.as_path(),
        cfg.fmt.clone(),
        fan_in,
        run_dir.as_path(),
        rb,
        wb,
        &mut manifest_opt,
        Some(&state),
        cfg.reverse,
        cfg.unique,
        cfg.dedupe_by.clone(),
        cfg.dedupe_keep_last,
        &skips,
    );
    if let Err(e) = mres {
        bars.finish();
        let code = if e.kind() == std::io::ErrorKind::InvalidData { errors::DATA } else { errors::IO };
        errors::fail(code, &format!("merge gagal: {}", errors::io_hint(&e)));
    }
    if let Some(h) = header_bytes.as_ref()
        && let Err(e) = prepend_header_to_staged(staged.as_path(), h) {
            bars.finish();
            errors::fail(errors::IO, &format!("header gagal: {}", errors::io_hint(&e)));
        }
    if let Some(n) = cfg.limit
        && let Err(e) = truncate_to_limit(staged.as_path(), &cfg.fmt, n, cfg.header) {
            bars.finish();
            errors::fail(errors::IO, &format!("limit gagal: {}", errors::io_hint(&e)));
        }
    state.phase.store(progress::PHASE_DONE, std::sync::atomic::Ordering::Relaxed);
    if let Err(e) = finalize_output(staged.as_path(), cfg.output.as_path()) {
        bars.finish();
        errors::fail(errors::IO, &format!("output gagal: {}", errors::io_hint(&e)));
    }
    if let Some(of) = cfg.output_format.clone()
        && let Err(e) = convert_output_format(cfg.output.as_path(), &cfg.fmt, &of, cfg.header, cfg.sql_table.clone()) {
            bars.finish();
            errors::fail(errors::IO, &format!("output-format gagal: {}", e));
        }
    let mut verify_note = String::from("skipped");
    if cfg.do_verify && cfg.output_format.is_none() {
        let vres = match &cfg.fmt {
            SortFormat::Csv { delimiter, keys, key_numeric } => {
                verify::verify_csv_full(cfg.output.as_path(), *delimiter, keys, *key_numeric, cfg.reverse, cfg.header)
            }
            SortFormat::Jsonl { field, key_numeric } => {
                verify::verify_jsonl_full(cfg.output.as_path(), field, *key_numeric, cfg.reverse)
            }
            SortFormat::Numeric => verify::verify_full(cfg.output.as_path(), "numeric", cfg.reverse, false),
            SortFormat::String => verify::verify_full(cfg.output.as_path(), "string", cfg.reverse, cfg.header),
        };
        match vres {
            Ok(n) => verify_note = format!("OK ({} records)", n),
            Err(msg) => {
                bars.finish();
                errors::fail(errors::VERIFY, &msg);
            }
        }
    }
    if let Some(n) = cfg.split_by
        && let Err(e) = split_output_by_count(cfg.output.as_path(), n, cfg.header) {
            bars.finish();
            errors::fail(errors::IO, &format!("split-by gagal: {}", e));
        }
    temp_manager::cleanup_run_dir_tree(run_dir.as_path());
    let peak = sampler.finish();
    bars.finish();
    let secs = t0.elapsed().as_secs_f64();
    if cfg.json || cfg.stats_json {
        println!("{{\n  \"status\": \"ok\",\n  \"mode\": \"merge\",\n  \"inputs\": {},\n  \"output\": \"{}\",\n  \"total_s\": {:.3},\n  \"peak_memory_bytes\": {},\n  \"verified\": \"{}\"\n}}",
            files.len(), json_escape(&cfg.output.display().to_string()), secs, peak, json_escape(&verify_note));
    } else {
        println!();
        println!("=== MERGE SELESAI (tanpa re-sort) ===");
        println!("Inputs : {} files", files.len());
        println!("Output : {}", cfg.output.display());
        println!("Waktu  : {:.3} detik", secs);
        println!("Verify : {}", verify_note);
    }
    if cfg.stats || cfg.stats_json {
        let rows = count_output_rows(cfg.output.as_path(), matches!(cfg.fmt, SortFormat::Numeric), cfg.header);
        print_stats_block(
            files.iter().map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0)).sum(),
            rows,
            rows,
            secs,
            peak,
            cfg.max_memory,
            cfg.temp_dir.as_deref().unwrap_or(Path::new(".")),
            &verify_note,
            cfg.stats_json || cfg.json,
        );
    }
}

/// --interactive wizard: tanya-jawab tanpa hafal flag, lalu jalankan sort.
fn run_interactive(base: &Config) {
    use std::io::{BufRead, Write};
    fn ask(line: &mut String, r: &mut impl BufRead, w: &mut impl Write, q: &str, def: &str) -> String {
        if def.is_empty() {
            print!("{}: ", q);
        } else {
            print!("{} [{}]: ", q, def);
        }
        let _ = w.flush();
        line.clear();
        let _ = r.read_line(line);
        let t = line.trim().to_string();
        if t.is_empty() {
            def.to_string()
        } else {
            t
        }
    }
    let stdin = std::io::stdin();
    let mut r = std::io::BufReader::new(stdin.lock());
    let mut w = std::io::stdout();
    let mut line = String::new();
    println!("=== MergeSort Pro — Mode Interaktif ===");
    let input = ask(&mut line, &mut r, &mut w, "File input", "");
    if input.is_empty() {
        errors::fail(errors::USAGE, "input wajib diisi");
    }
    let fmt_s = ask(&mut line, &mut r, &mut w, "Format (numeric/string/csv/jsonl)", "csv");
    let mut cmd: Vec<String> = vec!["--input".into(), input, "--output".into()];
    let output = ask(&mut line, &mut r, &mut w, "File output", "sorted.out");
    cmd.push(output);
    cmd.push("--max-memory".into());
    cmd.push(ask(&mut line, &mut r, &mut w, "Max memory", "512MB"));
    match fmt_s.as_str() {
        "csv" => {
            cmd.push("--format".into());
            cmd.push("csv".into());
            cmd.push("--key-column".into());
            cmd.push(ask(&mut line, &mut r, &mut w, "Kolom kunci (0-based)", "0"));
            cmd.push("--key-type".into());
            cmd.push(ask(&mut line, &mut r, &mut w, "Tipe kunci (string/numeric/date)", "string"));
            let d = ask(&mut line, &mut r, &mut w, "Delimiter (kosong = auto)", "");
            if !d.is_empty() {
                cmd.push("--delimiter".into());
                cmd.push(d);
            }
        }
        "jsonl" => {
            cmd.push("--mode".into());
            cmd.push("jsonl".into());
            cmd.push("--key-field".into());
            cmd.push(ask(&mut line, &mut r, &mut w, "Field kunci (mis. user.age)", "id"));
            cmd.push("--key-type".into());
            cmd.push(ask(&mut line, &mut r, &mut w, "Tipe kunci (string/numeric/date)", "string"));
        }
        "numeric" | "string" => {
            cmd.push("--mode".into());
            cmd.push(fmt_s.clone());
        }
        _ => errors::fail(errors::USAGE, &format!("format '{}' tidak dikenal", fmt_s)),
    }
    if ask(&mut line, &mut r, &mut w, "Header? (y/n)", "n").to_lowercase().starts_with('y') {
        cmd.push("--header".into());
    }
    if ask(&mut line, &mut r, &mut w, "Reverse/descending? (y/n)", "n").to_lowercase().starts_with('y') {
        cmd.push("--reverse".into());
    }
    if ask(&mut line, &mut r, &mut w, "Unique/dedup? (y/n)", "n").to_lowercase().starts_with('y') {
        cmd.push("--unique".into());
    }
    let lim = ask(&mut line, &mut r, &mut w, "Limit Top-N (kosong = semua)", "");
    if !lim.is_empty() {
        cmd.push("--limit".into());
        cmd.push(lim);
    }
    cmd.push("--verify".into());
    if base.stats || ask(&mut line, &mut r, &mut w, "Tampilkan laporan stats? (y/n)", "y").to_lowercase().starts_with('y') {
        cmd.push("--stats".into());
    }
    println!("Menjalankan: mergesort {}", cmd.join(" "));
    if !ask(&mut line, &mut r, &mut w, "Jalankan? (y/n)", "y").to_lowercase().starts_with('y') {
        println!("Dibatalkan.");
        return;
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("mergesort"));
    let st = std::process::Command::new(exe).args(&cmd).status();
    match st {
        Ok(s) if s.success() => {}
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => errors::fail(errors::IO, &format!("gagal menjalankan sort: {}", e)),
    }
}

/// --watch daemon: polling folder tiap 2 detik, sort file baru sesuai aturan.
fn run_watch_mode(base: &Config, watch_dir: &Path) {
    use std::collections::HashSet;
    use std::time::Duration;
    if !watch_dir.is_dir() {
        errors::fail(errors::PREFLIGHT, &format!("watch folder bukan direktori: {}", watch_dir.display()));
    }
    let out_dir = base.output_dir.clone().unwrap_or_else(|| watch_dir.to_path_buf());
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        errors::fail(errors::PREFLIGHT, &format!("output-dir gagal: {}", e));
    }
    println!("=== MergeSort Pro — Watch Mode ===");
    println!("Masuk : {}", watch_dir.display());
    println!("Keluar: {}", out_dir.display());
    println!("Tekan Ctrl-C untuk berhenti. Log: audit.jsonl");
    let mut done: HashSet<String> = HashSet::new();
    // Seed: anggap file yang sudah ada sebagai selesai (hanya proses file baru).
    if let Ok(rd) = std::fs::read_dir(watch_dir) {
        for e in rd.flatten() {
            done.insert(e.path().display().to_string());
        }
    }
    println!("Menunggu file baru di {} ...", watch_dir.display());
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let rd = match std::fs::read_dir(watch_dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let key = p.display().to_string();
            if done.contains(&key) {
                continue;
            }
            // Tunggu file stabil (size sama 2x, jeda 1 detik) agar tidak sort file yang masih ditulis.
            let s1 = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            std::thread::sleep(Duration::from_secs(1));
            let s2 = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            if s1 != s2 {
                continue;
            }
            done.insert(key);
            let fname = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".to_string());
            let dest = out_dir.join(format!("sorted-{}", fname));
            println!("File baru: {} -> {}", p.display(), dest.display());
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("mergesort"));
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("--input").arg(&p).arg("--output").arg(&dest);
            cmd.arg("--max-memory").arg(format!("{}", base.max_memory));
            match &base.fmt {
                SortFormat::Csv { delimiter, keys, key_numeric } => {
                    cmd.arg("--format").arg("csv");
                    if let Some(k) = keys.first() {
                        cmd.arg("--key-column").arg(k.to_string());
                    }
                    cmd.arg("--key-type").arg(if *key_numeric { "numeric" } else { "string" });
                    cmd.arg("--delimiter").arg(((*delimiter) as char).to_string());
                }
                SortFormat::Jsonl { field, key_numeric } => {
                    cmd.arg("--mode").arg("jsonl").arg("--key-field").arg(field.join("."));
                    cmd.arg("--key-type").arg(if *key_numeric { "numeric" } else { "string" });
                }
                SortFormat::Numeric => {
                    cmd.arg("--mode").arg("numeric");
                }
                SortFormat::String => {
                    cmd.arg("--mode").arg("string");
                }
            }
            if base.reverse {
                cmd.arg("--reverse");
            }
            if base.unique {
                cmd.arg("--unique");
            }
            if base.header {
                cmd.arg("--header");
            }
            cmd.arg("--verify").arg("--log-file").arg("audit.jsonl");
            match cmd.status() {
                Ok(s) if s.success() => println!("OK: {}", dest.display()),
                _ => eprintln!("GAGAL: {} (lihat log di atas)", p.display()),
            }
        }
    }
}

/// Smart dry-run: estimasi baris/waktu/temp + rekomendasi + deteksi header/delimiter.
#[allow(clippy::too_many_arguments)]
fn smart_dry_run(
    input: &Path,
    input_len: u64,
    fmt: &SortFormat,
    capacity: usize,
    fan_in: usize,
    threads: usize,
    temp_dir: &Path,
    enc_in: &inspect::TextEncoding,
    bom_len: usize,
    reverse: bool,
    unique: bool,
    header_flag: bool,
    as_json: bool,
) {
    use std::collections::HashSet;
    // Row estimate from sample avg line length (numeric exact).
    let (est_rows, avg_len) = if matches!(fmt, SortFormat::Numeric) {
        (input_len / 8, 8u64)
    } else {
        let sample = inspect::read_sample_lines(input, enc_in, bom_len, 500);
        let tot: usize = sample.iter().take(200).map(|l| l.len() + 1).sum();
        let n = sample.iter().take(200).filter(|l| !l.is_empty()).count().max(1);
        let avg = (tot / n).max(1) as u64;
        (if avg > 0 { input_len / avg } else { 1 }, avg)
    };
    let est_chunks = est_rows.div_ceil(capacity as u64).max(1);
    // Temp estimate: factor by format (string/csv/jsonl rewrite overhead).
    let factor: f64 = match fmt {
        SortFormat::Numeric => 2.0,
        SortFormat::String => 5.0,
        SortFormat::Csv { .. } | SortFormat::Jsonl { .. } => 4.0,
    };
    let est_temp = (input_len as f64 * factor) as u64;
    // Time estimate: heuristic throughput ~120MB/min single-thread-ish scaled.
    let mb = input_len as f64 / (1024.0 * 1024.0);
    let est_secs = (mb / 120.0 * 60.0 / (threads as f64).sqrt()).max(2.0);
    // Temp space check.
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let canon = std::fs::canonicalize(temp_dir).unwrap_or_else(|_| temp_dir.to_path_buf());
    let mut best_avail: Option<u64> = None;
    let mut best_mp = String::new();
    for d in disks.list() {
        if canon.starts_with(d.mount_point()) {
            best_avail = Some(d.available_space());
            best_mp = d.mount_point().display().to_string();
        }
    }
    // Suggest alternative volume with most free space.
    let mut alt: Option<(String, u64)> = None;
    {
        let mut seen = HashSet::new();
        for d in sysinfo::Disks::new_with_refreshed_list().list() {
            let mp = d.mount_point().display().to_string();
            if seen.insert(mp.clone()) {
                let a = d.available_space();
                if best_avail.map(|b| a > b).unwrap_or(true) {
                    match &alt {
                        Some((_, aa)) if *aa >= a => {}
                        _ => alt = Some((mp, a)),
                    }
                }
            }
        }
    }
    let enough = best_avail.map(|a| a >= est_temp).unwrap_or(true);
    // Header/delimiter detect for text.
    let (auto_delim, auto_header) = if matches!(fmt, SortFormat::Csv { .. }) {
        let sample = inspect::read_sample_lines(input, enc_in, bom_len, 100);
        let (d, _) = inspect::detect_delimiter(&sample);
        let (key_num, key_col) = match fmt {
            SortFormat::Csv { key_numeric, keys, .. } => (*key_numeric, keys.first().copied().unwrap_or(0)),
            _ => (false, 0),
        };
        (Some(d), inspect::detect_header(&sample, key_num, key_col))
    } else if matches!(fmt, SortFormat::String) {
        let sample = inspect::read_sample_lines(input, enc_in, bom_len, 100);
        (None, inspect::detect_header(&sample, false, 0))
    } else {
        (None, false)
    };
    if as_json {
        println!(
            "{{\n  \"status\": \"dry-run\",\n  \"input_bytes\": {},\n  \"est_rows\": {},\n  \"avg_bytes_per_row\": {},\n  \"temp_need_bytes\": {},\n  \"temp_dir\": \"{}\",\n  \"temp_avail_bytes\": {},\n  \"temp_ok\": {},\n  \"est_secs\": {:.0},\n  \"chunks\": {{\"estimated\": {}, \"capacity_records\": {}}},\n  \"fan_in\": {},\n  \"threads\": {},\n  \"format\": \"{}\",\n  \"mode\": \"{}\",\n  \"header_detected\": {},\n  \"encoding\": \"{}\"\n}}",
            input_len,
            est_rows,
            avg_len,
            est_temp,
            json_escape(&temp_dir.display().to_string()),
            best_avail.unwrap_or(u64::MAX),
            enough,
            est_secs,
            est_chunks,
            capacity,
            fan_in,
            threads,
            fmt.kind(),
            fmt.kind(),
            auto_header,
            enc_in.name(),
        );
        return;
    }
    println!("=== Estimasi (dry-run) ===");
    println!("Baris           : {}", inspect::fmt_n(est_rows));
    println!("Ukuran input    : {} ({} bytes)", inspect::human(input_len), input_len);
    println!("Estimasi waktu  : ~{:.0} detik ({:.1} MB, {} threads)", est_secs, mb, threads);
    println!("Estimasi temp   : {}", inspect::human(est_temp));
    match best_avail {
        Some(a) => {
            println!(
                "Lokasi temp     : {} (sisa {}) {}",
                temp_dir.display(),
                inspect::human(a),
                if enough { "" } else { "KURANG" }
            );
            if !enough {
                if let Some((mp, av)) = alt {
                    println!("Saran           : --temp-dir {} (sisa {})", mp, inspect::human(av));
                }
                println!("                  atau --max-memory lebih besar (temp turun, RAM naik)");
            }
        }
        None => println!("Lokasi temp     : {}", temp_dir.display()),
    }
    if !best_mp.is_empty() {
        println!("Volume          : {}", best_mp);
    }
    if let Some(d) = auto_delim {
        println!("Delimiter       : '{}' (terdeteksi otomatis)", d as char);
    }
    println!(
        "Header terdeteksi: {} {}",
        if auto_header { "ya" } else { "tidak" },
        if header_flag { "(dipakai --header)" } else { "" }
    );
    println!("Encoding        : {}", enc_in.name());
    if reverse {
        println!("Order           : descending (--reverse)");
    }
    if unique {
        println!("Dedup           : aktif (--unique)");
    }
    println!("=========================");
}

/// Read first N + last N data lines from a sorted staged file (header-aware).
/// Returns (head, tail, total_data_rows). Numeric binary unsupported for preview text.
fn read_head_tail(staged: &Path, fmt: &SortFormat, n: usize, has_header: bool) -> std::io::Result<(Vec<String>, Vec<String>, usize)> {
    use std::io::{BufRead, BufReader};
    if matches!(fmt, SortFormat::Numeric) {
        let meta = std::fs::metadata(staged)?;
        let total = (meta.len() / 8) as usize;
        let f = std::fs::File::open(staged)?;
        let mut r = BufReader::with_capacity(64 * 1024, f);
        let take_head = n.min(total);
        let mut head = Vec::with_capacity(take_head);
        let mut buf = [0u8; 8];
        for _ in 0..take_head {
            use std::io::Read;
            r.read_exact(&mut buf)?;
            head.push(u64::from_le_bytes(buf).to_string());
        }
        // Tail: seek to last N records.
        let mut tail = Vec::new();
        if total > take_head {
            let f2 = std::fs::File::open(staged)?;
            let mut r2 = BufReader::with_capacity(64 * 1024, f2);
            use std::io::Seek;
            use std::io::SeekFrom;
            let tail_n = n.min(total - take_head);
            let off = ((total - tail_n) as u64) * 8;
            r2.seek(SeekFrom::Start(off))?;
            for _ in 0..tail_n {
                use std::io::Read;
                r2.read_exact(&mut buf)?;
                tail.push(u64::from_le_bytes(buf).to_string());
            }
        }
        return Ok((head, tail, total));
    }
    let f = std::fs::File::open(staged)?;
    let r = BufReader::with_capacity(64 * 1024, f);
    let mut head: Vec<String> = Vec::new();
    let mut ring: Vec<String> = Vec::with_capacity(n);
    let mut total = 0usize;
    let mut first = true;
    for line in r.lines() {
        let l = line?;
        if has_header && first {
            first = false;
            continue; // header not counted, shown separately? spec shows data rows; keep simple
        }
        first = false;
        total += 1;
        if head.len() < n {
            head.push(l.clone());
        }
        if ring.len() < n {
            ring.push(l);
        } else if n > 0 {
            ring.remove(0);
            ring.push(l);
        }
    }
    // Tail never overlaps head: head = first min(n,total),
    // tail = last min(n, total-n) (empty when total <= n).
    let tail: Vec<String> = if total <= n {
        Vec::new()
    } else if total <= 2 * n {
        ring.into_iter().skip(2 * n - total).collect()
    } else {
        ring
    };
    Ok((head, tail, total))
}

fn print_preview_text(cfg: &Config, total: usize, _n: usize, head: &[String], tail: &[String]) {
    println!("=== Preview ({} baris total, tanpa commit) ===", inspect::fmt_n(total as u64));
    println!("Input : {}", cfg.input.display());
    for (i, l) in head.iter().enumerate() {
        println!("{:>6} | {}", i + 1, truncate_preview(l, 200));
    }
    if total > head.len() + tail.len() {
        println!("  ... ({} baris disembunyikan) ...", inspect::fmt_n((total - head.len() - tail.len()) as u64));
    }
    let start = total.saturating_sub(tail.len());
    for (j, l) in tail.iter().enumerate() {
        println!("{:>6} | {}", start + j + 1, truncate_preview(l, 200));
    }
    println!("Gunakan --output + tanpa --preview untuk commit penuh.");
}

fn print_preview_json(cfg: &Config, total: usize, head: &[String], tail: &[String]) {
    let jarr = |v: &[String]| format!("[{}]", v.iter().map(|s| format!("\"{}\"", json_escape(s))).collect::<Vec<_>>().join(","));
    println!(
        "{{\"status\":\"preview\",\"input\":\"{}\",\"total_rows\":{},\"head\":{},\"tail\":{}}}",
        json_escape(&cfg.input.display().to_string()),
        total,
        jarr(head),
        jarr(tail)
    );
}

fn truncate_preview(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

/// Screenshot-able execution report (--stats / --stats-json).
#[allow(clippy::too_many_arguments)]
fn print_stats_block(
    input_len: u64,
    rows_in: usize,
    rows_out: usize,
    total_secs: f64,
    peak_mem: usize,
    max_memory: u64,
    temp_dir: &Path,
    verify_note: &str,
    as_json: bool,
) {
    let dupes = rows_in.saturating_sub(rows_out);
    let temp_used = disk::estimate_need_bytes(input_len);
    let mb = input_len as f64 / (1024.0 * 1024.0);
    let per_min = if total_secs > 0.0 { mb / (total_secs / 60.0) } else { 0.0 };
    if as_json {
        println!(
            "{{\n  \"status\": \"stats\",\n  \"rows_in\": {},\n  \"rows_out\": {},\n  \"dupes_removed\": {},\n  \"total_s\": {:.3},\n  \"peak_ram_bytes\": {},\n  \"mem_limit_bytes\": {},\n  \"temp_need_bytes\": {},\n  \"temp_dir\": \"{}\",\n  \"throughput_mb_per_min\": {:.1},\n  \"verify\": \"{}\",\n  \"exit_code\": 0\n}}",
            rows_in,
            rows_out,
            dupes,
            total_secs,
            peak_mem,
            max_memory,
            temp_used,
            json_escape(&temp_dir.display().to_string()),
            per_min,
            json_escape(verify_note),
        );
        return;
    }
    println!();
    println!("=== MergeSort Pro — Laporan Eksekusi ===");
    println!("Baris masuk     : {}", inspect::fmt_n(rows_in as u64));
    println!("Baris keluar    : {}", inspect::fmt_n(rows_out as u64));
    println!("Duplikat dibuang: {}", inspect::fmt_n(dupes as u64));
    println!("Waktu total     : {:.1} detik", total_secs);
    println!("Peak RAM        : {} (batas {})", inspect::human(peak_mem as u64), inspect::human(max_memory));
    println!("Temp disk       : ~{} di {}", inspect::human(temp_used), temp_dir.display());
    println!("Throughput      : {:.1} MB/menit", per_min);
    println!("Verify          : {}", verify_note);
    println!("Exit code       : 0");
    println!("========================================");
}

fn main() {
    let mut cfg = match parse_args() {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("error[{}]: {}", errors::USAGE, msg);
            println!("{}", usage());
            std::process::exit(errors::USAGE);
        }
    };

    if let Some(spec) = cfg.gui.clone() {
        let (bind, port) = match parse_dashboard_spec(&spec) {
            Some(bp) => bp,
            None => {
                ("127.0.0.1".to_string(), 8080)
            }
        };
        match serve::run_gui(&bind, port, cfg.brand.clone(), cfg.brand_color.clone()) {
            Ok(_) => std::process::exit(errors::OK),
            Err(e) => errors::fail(errors::PREFLIGHT, &format!("tidak bisa menjalankan GUI di {}:{}: {}", bind, port, e)),
        }
    }

    // --- Alternate runtime modes (no temp hook needed yet). ---
    if cfg.interactive {
        run_interactive(&cfg);
        std::process::exit(errors::OK);
    }
    if let Some(watch_dir) = cfg.watch.clone() {
        run_watch_mode(&cfg, &watch_dir);
        std::process::exit(errors::OK);
    }
    if let Some(api_spec) = cfg.api.clone() {
        let (bind, port) = match parse_dashboard_spec(&api_spec) {
            Some(bp) => bp,
            None => ("127.0.0.1".to_string(), 8080),
        };
        match serve::run_api(&bind, port, cfg.api_token.clone()) {
            Ok(_) => std::process::exit(errors::OK),
            Err(e) => errors::fail(errors::PREFLIGHT, &format!("tidak bisa menjalankan API di {}:{}: {}", bind, port, e)),
        }
    }
    if !cfg.merge_inputs.is_empty() {
        run_merge_mode(&cfg);
        std::process::exit(errors::OK);
    }

    temp_manager::install_panic_hook();

    // --- `--resume list` needs only the temp dir, not the input file. ---
    if cfg.resume.as_deref() == Some("list") {
        let parent: PathBuf = cfg
            .temp_dir
            .clone()
            .unwrap_or_else(|| {
                cfg.output
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."))
            });
        let runs = manifest::discover_resumable(parent.as_path());
        if runs.is_empty() {
            println!("tidak ada run yang bisa di-resume di {}", parent.display());
        } else {
            for (id, dir) in runs {
                let m = Manifest::load(dir.as_path());
                let (chunks, missing) = match &m {
                    Some(m) => (
                        m.chunks.len(),
                        m.chunks
                            .iter()
                            .filter(|c| !dir.join(Path::new(c)).is_file())
                            .count(),
                    ),
                    None => (0, 0),
                };
                let note = if chunks == 0 {
                    ""
                } else if missing > 0 {
                    "\tUNRESUMABLE (chunk sumber sudah terkonsumsi fase merge yang crash)"
                } else {
                    ""
                };
                println!("{}\tchunks={}\t{}{}", id, chunks, dir.display(), note);
            }
        }
        std::process::exit(errors::OK);
    }

    // --- Pre-flight: input readable, output dir writable, disk space. ---
    match std::fs::metadata(cfg.input.clone()) {
        Ok(m) if m.is_file() => {},
        Ok(_) => errors::fail(
            errors::PREFLIGHT,
            &format!("input bukan file biasa: {}", cfg.input.display()),
        ),
        Err(e) => errors::fail(
            errors::PREFLIGHT,
            &format!(
                "input tidak bisa dibaca: {} ({}). Periksa path dan hak akses.",
                cfg.input.display(),
                errors::io_hint(&e)
            ),
        ),
    };

    // --- Transparent decompress (.gz/.zip) + xlsx->csv (before sizing). ---
    // work_input is what the engine actually sorts; cfg.input stays canonical.
    let mut _extra_temps: Vec<PathBuf> = Vec::new();
    let mut work_input: PathBuf;
    match maybe_decompress_input(cfg.input.as_path()) {
        Ok((p, guard)) => {
            work_input = p;
            if let Some(t) = guard {
                _extra_temps.push(t);
            }
        }
        Err(m) => errors::fail(errors::PREFLIGHT, &m),
    }
    let input_is_xlsx = ext_is(cfg.input.as_path(), "xlsx");
    if input_is_xlsx {
        if !matches!(cfg.fmt, SortFormat::Csv { .. }) {
            errors::fail(errors::USAGE, "--input .xlsx butuh --format csv/--mode excel + --key-column N (atau --multi-key)");
        }
        match xlsx_sheet_to_csv(work_input.as_path(), cfg.sheet.as_deref()) {
            Ok(bytes) => {
                let tmp = cfg.input.with_extension("xlsxcsv.tmp");
                match std::fs::write(&tmp, &bytes) {
                    Ok(_) => {
                        work_input = tmp.clone();
                        _extra_temps.push(tmp);
                    }
                    Err(e) => errors::fail(errors::IO, &format!("tulis temp xlsx->csv gagal: {}", e)),
                }
            }
            Err(m) => errors::fail(errors::DATA, &m),
        }
    }
    let input_len = std::fs::metadata(work_input.as_path()).map(|m| m.len()).unwrap_or(0);

    // --- Encoding resolve (BOM auto unless overridden). ---
    let (bom_enc, bom_len) = inspect::sniff_bom(work_input.as_path());
    let enc_in: inspect::TextEncoding = cfg
        .encoding_in
        .as_deref()
        .or(cfg.encoding.as_deref())
        .and_then(inspect::TextEncoding::parse)
        .unwrap_or(bom_enc);
    let enc_out: inspect::TextEncoding = cfg
        .encoding_out
        .as_deref()
        .or(cfg.encoding.as_deref())
        .and_then(inspect::TextEncoding::parse)
        .unwrap_or(inspect::TextEncoding::Utf8);

    // --- --check: validate only, no sort, no output needed. ---
    if cfg.check {
        let key_col: Option<usize> = match &cfg.fmt {
            SortFormat::Csv { keys, .. } => keys.first().copied(),
            _ => None,
        };
        let key_num = match &cfg.fmt {
            SortFormat::Csv { key_numeric, .. } => *key_numeric,
            _ => false,
        };
        // Auto-detect info for the report even when user passed explicit flags.
        // work_input = post-decompress/xlsx conversion (engine truth).
        let mut rep = match inspect::check_file(work_input.as_path(), cfg.fmt.kind(), key_col, key_num, Some(&enc_in)) {
            Ok(r) => r,
            Err(m) => errors::fail(errors::PREFLIGHT, &m),
        };
        rep.file = cfg.input.display().to_string();
        if cfg.json || cfg.stats_json {
            println!(
                "{{\n  \"status\": \"{}\",\n  \"file\": \"{}\",\n  \"size_bytes\": {},\n  \"est_rows\": {},\n  \"delimiter\": \"{}\",\n  \"header\": {},\n  \"columns\": {},\n  \"bad_rows\": {},\n  \"encoding\": \"{}\",\n  \"key\": \"{}\"\n}}",
                if rep.ready { "ready" } else { "data-issue" },
                json_escape(&rep.file),
                rep.size_bytes,
                rep.est_rows,
                if rep.delimiter == 0 { "-".to_string() } else { (rep.delimiter as char).to_string() },
                match &rep.header {
                    Some(h) => format!("\"{}\"", json_escape(h)),
                    None => "null".to_string(),
                },
                rep.columns,
                rep.bad_rows,
                json_escape(&rep.encoding),
                json_escape(&rep.key_col_msg),
            );
        } else {
            inspect::print_check_report(&rep, cfg.fmt.kind());
        }
        std::process::exit(if rep.ready { errors::OK } else { errors::DATA });
    }

    // --- Auto-detect delimiter & header for CSV (unless explicit). ---
    if matches!(cfg.fmt, SortFormat::Csv { .. }) {
        let need_auto = !cfg.delimiter_explicit || !cfg.header_explicit;
        if need_auto {
            let sample = inspect::read_sample_lines(work_input.as_path(), &enc_in, bom_len, 100);
            if !sample.is_empty() {
                let (cur_delim, cur_keys, cur_num) = match &cfg.fmt {
                    SortFormat::Csv { delimiter, keys, key_numeric } => (*delimiter, keys.clone(), *key_numeric),
                    _ => unreachable!(),
                };
                let cur_key_col = cur_keys.first().copied().unwrap_or(0);
                if !cfg.delimiter_explicit {
                    let (auto_d, _) = inspect::detect_delimiter(&sample);
                    if auto_d != cur_delim {
                        cfg.fmt = SortFormat::Csv { delimiter: auto_d, keys: cur_keys.clone(), key_numeric: cur_num };
                        if !cfg.quiet {
                            eprintln!("auto-deteksi delimiter: '{}' (override dengan --delimiter)", auto_d as char);
                        }
                    }
                }
                if !cfg.header_explicit {
                    let auto_h = inspect::detect_header(&sample, cur_num, cur_key_col);
                    if auto_h && !cfg.header {
                        cfg.header = true;
                        if !cfg.quiet {
                            eprintln!("auto-deteksi header: ya (override dengan --no-header)");
                        }
                    }
                }
            }
        }
    }

    // Output staging not needed for preview-without-output.
    let is_preview = cfg.preview.is_some() || cfg.preview_json;
    let has_real_output = !cfg.output.as_os_str().is_empty();
    if has_real_output
        && let Err(msg) = stage_output_part(cfg.output.as_path()) {
            errors::fail(errors::PREFLIGHT, &msg);
        }
    let temp_dir_for_check: PathBuf = cfg
        .temp_dir
        .clone()
        .unwrap_or_else(|| {
            if has_real_output {
                cfg.output.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
            } else {
                PathBuf::from(".")
            }
        });
    let check_output_path: PathBuf = if has_real_output { cfg.output.clone() } else { temp_dir_for_check.clone() };
    if let Err(pe) = disk::preflight_check(input_len, temp_dir_for_check.as_path(), check_output_path.as_path()) {
        errors::fail(errors::PREFLIGHT, &pe.message());
    }

    let threads = cfg.threads.unwrap_or_else(|| {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
    });
    let _ = rayon::ThreadPoolBuilder::new().num_threads(threads).build_global();

    let capacity = chunk::chunk_capacity(cfg.max_memory as usize, &cfg.fmt, threads);
    let max_open = match cfg.max_open_files {
        Some(n) => n.max(2),
        None => detect_max_open_files(),
    };
    let fan_in = max_open.saturating_sub(4).max(2);
    let (reader_buf, writer_buf) = io_buffer_sizes(cfg.max_memory, fan_in);

    // --- --dry-run pintar: estimasi + rekomendasi actionable. ---
    if cfg.dry_run {
        smart_dry_run(
            work_input.as_path(),
            input_len,
            &cfg.fmt,
            capacity,
            fan_in,
            threads,
            temp_dir_for_check.as_path(),
            &enc_in,
            bom_len,
            cfg.reverse,
            cfg.unique,
            cfg.header,
            cfg.json,
        );
        std::process::exit(errors::OK);
    }

    // --- Encoding normalize: non-UTF8 input -> UTF-8 temp (engine stays UTF-8). ---
    let (_norm_path, _norm_guard): (PathBuf, Option<PathBuf>) = match inspect::normalize_input_to_utf8(work_input.as_path(), &enc_in, bom_len) {
        Ok(v) => v,
        Err(e) => errors::fail(errors::IO, &format!("encoding normalize gagal: {}", e)),
    };
    let sort_input: PathBuf = _norm_path.clone();

    // --- --header: capture the first line, sort the rest. ---
    let header_bytes: Option<Vec<u8>> = if cfg.header {
        match read_header_line(sort_input.as_path()) {
            Ok(h) => Some(h),
            Err(m) => errors::fail(errors::PREFLIGHT, &m),
        }
    } else {
        None
    };

    // --- Resume resolution. ---
    let resume_parent: PathBuf = temp_dir_for_check.clone();
    let mut manifest_opt: Option<Manifest>;
    let run_dir: PathBuf;
    let mut resumed_chunks: usize = 0;
    let mut resume_offset: u64 = 0;
    let mut resumed_label = String::from("-");

    match cfg.resume.clone() {
        Some(spec) => {
            // Find the run dir (id, "run-<id>", or newest when empty).
            let tmp_base = resume_parent.join(Path::new(".temp_sort"));
            let candidate = if spec.is_empty() {
                let found = manifest::discover_resumable(resume_parent.as_path());
                found.last().map(|(_, dir)| dir.clone())
            } else {
                let strip = spec.strip_prefix("run-").unwrap_or(spec.as_str());
                let direct = tmp_base.join(Path::new(&format!("run-{}", strip)));
                if direct.is_dir() {
                    Some(direct)
                } else {
                    let as_given = tmp_base.join(Path::new(&spec));
                    if as_given.is_dir() {
                        Some(as_given)
                    } else {
                        None
                    }
                }
            };
            let dir = match candidate {
                Some(d) => d,
                None => errors::fail(
                    errors::PREFLIGHT,
                    &format!(
                        "run resume '{}' tidak ditemukan di {}. Gunakan --resume list untuk melihat run yang tersimpan.",
                        spec,
                        tmp_base.display()
                    ),
                ),
            };
            let m = match Manifest::load(dir.as_path()) {
                Some(m) => m,
                None => errors::fail(
                    errors::PREFLIGHT,
                    &format!("manifest.json tidak bisa dibaca di {}", dir.display()),
                ),
            };
            if m.finished {
                errors::fail(
                    errors::PREFLIGHT,
                    &format!("run '{}' sudah selesai sebelumnya; tidak ada yang perlu di-resume", m.run_id),
                );
            }
            if m.format != cfg.fmt.kind() {
                errors::fail(
                    errors::PREFLIGHT,
                    &format!(
                        "format run '{}' adalah {} tetapi perintah ini memakai {}; resume harus memakai konfigurasi yang sama",
                        m.run_id, m.format, cfg.fmt.kind()
                    ),
                );
            }
            // The merge phase consumes chunk files as it goes; a crash during
            // merge leaves a manifest that lists chunks that no longer exist.
            // Resume can only honor a split-phase crash.
            let missing = m
                .chunks
                .iter()
                .filter(|c| !dir.join(Path::new(c)).is_file())
                .count();
            if missing > 0 {
                errors::fail(
                    errors::PREFLIGHT,
                    &format!(
                        "run '{}' tidak bisa di-resume: {} dari {} chunk sumber sudah terkonsumsi fase merge yang crash sebelumnya. Hapus direktori {} dan mulai run baru (output lama tidak pernah difinalisasi, jadi tidak ada yang korup).",
                        m.run_id,
                        missing,
                        m.chunks.len(),
                        dir.display()
                    ),
                );
            }
            // The recorded offsets are meaningless against a different input
            // file: refuse rather than silently produce a wrong sort. The
            // stored path is resolved relative to the output dir first (where
            // the original run typically executed), then compared canonically.
            let stored_input = PathBuf::from(&m.input);
            let out_parent = cfg
                .output
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            let mut same_input = false;
            for cand in [stored_input.clone(), out_parent.join(stored_input.clone())] {
                if let (Ok(a), Ok(c)) = (
                    std::fs::canonicalize(cfg.input.clone()),
                    std::fs::canonicalize(cand),
                )
                    && a == c {
                        same_input = true;
                        break;
                    }
            }
            if !same_input {
                same_input = m.input == cfg.input.display().to_string();
            }
            if !same_input {
                errors::fail(
                    errors::PREFLIGHT,
                    &format!(
                        "run '{}' dibuat dari input '{}' tetapi perintah ini memakai '{}'; resume harus memakai input yang sama",
                        m.run_id, m.input, cfg.input.display()
                    ),
                );
            }
            if m.capacity != 0 && m.capacity != capacity as u64 {
                errors::fail(
                    errors::PREFLIGHT,
                    &format!(
                        "run '{}' dibuat dengan --max-memory yang berbeda (chunk capacity {} vs {}); pakai nilai yang sama atau mulai run baru",
                        m.run_id, m.capacity, capacity
                    ),
                );
            }
            resumed_chunks = m.chunks.len();
            resume_offset = m.resume_offset();
            resumed_label = m.run_id.clone();
            temp_manager::set_preserve(true);
            temp_manager::adopt_run_dir(dir.clone());
            run_dir = dir;
            manifest_opt = Some(m);
        }
        None => {
            run_dir = match temp_manager::create_run_dir(cfg.output.as_path(), cfg.temp_dir.as_deref()) {
                Ok(d) => d,
                Err(e) => errors::fail(
                    errors::IO,
                    &format!("tidak bisa membuat direktori temp: {}", errors::io_hint(&e)),
                ),
            };
            let dir_name = run_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let (delim_field, keys_field) = match &cfg.fmt {
                SortFormat::Csv { delimiter, keys, .. } => (format!("u{}", delimiter), keys.clone()),
                // jsonl reuses key_columns slot for field-path length marker
                SortFormat::Jsonl { field, .. } => (format!("jsonl:{}", field.join(".")), vec![field.len()]),
                _ => (String::new(), Vec::new()),
            };
            manifest_opt = Some(Manifest::new(
                dir_name,
                cfg.fmt.kind().to_string(),
                delim_field,
                keys_field,
                cfg.input.display().to_string(),
                capacity as u64,
                input_len,
            ));
        }
    }
    if let Some(m) = &mut manifest_opt {
        m.capacity = capacity as u64;
        m.input_bytes = input_len;
    }

    let format_desc = match &cfg.fmt {
        SortFormat::Numeric => "numeric".to_string(),
        SortFormat::String => "string".to_string(),
        SortFormat::Csv { delimiter, keys, key_numeric } => format!(
            "csv (delimiter '{}', key col {}{})",
            if *delimiter == b'\t' {
                "\\t".to_string()
            } else {
                String::from_utf8_lossy(&[*delimiter]).into_owned()
            },
            keys.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(","),
            if *key_numeric { ", numeric" } else { "" }
        ),
        SortFormat::Jsonl { field, key_numeric } => format!(
            "jsonl (key field '{}'{})",
            field.join("."),
            if *key_numeric { ", numeric" } else { "" }
        ),
    };

    if !cfg.quiet {
        println!("=== EXTERNAL MERGE SORT v{} ===", VERSION);
        println!("Input:            {}", cfg.input.display());
        println!("Output:           {}", cfg.output.display());
        println!("Format:           {}", format_desc);
        let mut opts: Vec<String> = Vec::new();
        if cfg.reverse {
            opts.push("reverse".to_string());
        }
        if cfg.unique {
            opts.push("unique".to_string());
        }
        if cfg.header {
            opts.push("header".to_string());
        }
        if let Some(db) = cfg.dedupe_by.as_ref() {
            opts.push(format!("dedupe-by:{}({})", db.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","), if cfg.dedupe_keep_last { "last" } else { "first" }));
        }
        if let Some(of) = cfg.output_format.as_ref() {
            opts.push(format!("out:{}", of));
        }
        if !opts.is_empty() {
            println!("Options:          {}", opts.join(", "));
        }
        println!("Max memory:       {} MB", cfg.max_memory / (1024 * 1024));
        println!("Chunk capacity:   {} records", capacity);
        println!("Threads:          {}", threads);
        println!("Merge fan-in:     {}", fan_in);
        println!("Temp dir:         {}", run_dir.display());
        if resumed_chunks > 0 {
            println!("Resume:           {} ({} chunk dipakai ulang)", resumed_label, resumed_chunks);
        }
    }

    progress::set_start();
    let state = progress::RunState::new(input_len);
    // Resume: the split bar starts where the adopted chunks left off.
    // Split bar starts where the adopted chunks left off.
    state
        .input_bytes_read
        .store(resume_offset.min(input_len), std::sync::atomic::Ordering::Relaxed);
    let mut bars = progress::Progress::new(state.clone(), !cfg.quiet);

    if let Some(spec) = cfg.dashboard.clone() {
        let (bind, port) = match parse_dashboard_spec(spec.as_str()) {
            Some(bp) => bp,
            None => errors::fail(errors::USAGE, "nilai --dashboard tidak valid (harus [host:]PORT)"),
        };
        match progress::start_dashboard(bind.as_str(), port, state.clone()) {
            Ok(bound) => {
                eprintln!("dashboard: http://{}:{}/", bind, bound);
            }
            Err(e) => errors::fail(
                errors::PREFLIGHT,
                &format!("tidak bisa menjalankan dashboard di {}:{}: {}", bind, port, e),
            ),
        }
    }

    let t_start = Instant::now();
    let sampler = MemorySampler::spawn(Duration::from_millis(50));

    // --- Phase 1: split + sort (skips completed chunks when resuming) ---
    // Preview tanpa --output: stage di run_dir, tidak commit ke mana pun.
    let staged: PathBuf = if has_real_output {
        match stage_output_part(cfg.output.as_path()) {
            Ok(p) => p,
            Err(msg) => errors::fail(errors::PREFLIGHT, &msg),
        }
    } else {
        run_dir.join(Path::new(".preview.part"))
    };
    temp_manager::register_staging(staged.clone());

    state.phase.store(progress::PHASE_SPLIT, std::sync::atomic::Ordering::Relaxed);
    let split = match chunk::split_and_sort(
        sort_input.as_path(),
        run_dir.clone().as_path(),
        cfg.fmt.clone(),
        capacity,
        writer_buf,
        resume_offset,
        &mut manifest_opt,
        Some(&state),
        cfg.reverse,
        cfg.header,
    ) {
        Ok(s) => s,
        Err(e) => {
            bars.finish();
            // Fresh runs must not leak temp dirs on failure; resume runs keep
            // completed chunks for a retry (preserve semantics).
            if resumed_chunks == 0 {
                temp_manager::cleanup_run_dir_tree(run_dir.as_path());
            }
            let code = if e.kind() == std::io::ErrorKind::InvalidData { errors::DATA } else { errors::IO };
            errors::fail(code, &format!("fase split gagal: {}", errors::io_hint(&e)));
        }
    };

    let chunk_count = split.chunk_paths.len();
    state.phase.store(progress::PHASE_MERGE, std::sync::atomic::Ordering::Relaxed);

    // --- Phase 2: K-way merge (empty input => empty output) ---
    let merge_stats = if chunk_count == 0 {
        if let Err(e) = std::fs::File::create(staged.clone()) {
            bars.finish();
            errors::fail(errors::IO, &format!("tidak bisa membuat file output: {}", errors::io_hint(&e)));
        }
        merge::MergeStats { io_secs: 0.0, bytes_read: 0 }
    } else {
        match merge::merge_all(
            split.chunk_paths,
            staged.clone().as_path(),
            cfg.fmt.clone(),
            fan_in,
            run_dir.clone().as_path(),
            reader_buf,
            writer_buf,
            resumed_chunks > 0, // keep_temps when resuming: output must not consume chunks
            &mut manifest_opt,
            Some(&state),
            cfg.reverse,
            cfg.unique,
            cfg.dedupe_by.clone(),
            cfg.dedupe_keep_last,
        ) {
            Ok(s) => s,
            Err(e) => {
                bars.finish();
                if resumed_chunks == 0 {
                    temp_manager::cleanup_run_dir_tree(run_dir.as_path());
                }
                let code = if e.kind() == std::io::ErrorKind::InvalidData { errors::DATA } else { errors::IO };
                errors::fail(code, &format!("fase merge gagal: {}", errors::io_hint(&e)));
            }
        }
    };

    // --header: staged file currently holds sorted body only; prepend header.
    // Empty input (no chunks) still gets a header-only staged file.
    if let Some(h) = header_bytes.as_ref() {
        if chunk_count == 0 {
            if let Err(e) = std::fs::write(staged.as_path(), [h.as_slice(), b"\n".as_slice()].concat()) {
                bars.finish();
                errors::fail(errors::IO, &format!("tidak bisa menulis header: {}", errors::io_hint(&e)));
            }
        } else if let Err(e) = prepend_header_to_staged(staged.as_path(), h) {
            bars.finish();
            errors::fail(errors::IO, &format!("tidak bisa menulis header: {}", errors::io_hint(&e)));
        }
    }

    // --limit N (Top-N): keep header + first N sorted records.
    if let Some(n) = cfg.limit
        && let Err(e) = truncate_to_limit(staged.as_path(), &cfg.fmt, n, cfg.header) {
            bars.finish();
            errors::fail(errors::IO, &format!("tidak bisa menerapkan --limit: {}", errors::io_hint(&e)));
        }

    state.phase.store(progress::PHASE_DONE, std::sync::atomic::Ordering::Relaxed);

    // --- --preview: tampilkan head/tail staged, jangan commit, cleanup, exit. ---
    if is_preview {
        let n = cfg.preview.unwrap_or(20);
        bars.finish();
        // encoding-out tetap UTF-8 untuk preview stdout (readable terminal).
        match read_head_tail(staged.as_path(), &cfg.fmt, n, cfg.header) {
            Ok((head, tail, total)) => {
                if cfg.preview_json || cfg.json {
                    print_preview_json(&cfg, total, &head, &tail);
                } else {
                    print_preview_text(&cfg, total, n, &head, &tail);
                }
            }
            Err(e) => errors::fail(errors::IO, &format!("preview gagal: {}", errors::io_hint(&e))),
        }
        let _ = std::fs::remove_file(staged.as_path());
        if std::env::var("MERGESORT_KEEP_TEMP").as_deref() != Ok("1") {
            temp_manager::cleanup_run_dir_tree(run_dir.as_path());
        }
        if let Some(tmp) = _norm_guard {
            let _ = std::fs::remove_file(tmp);
        }
        for t in &_extra_temps {
            let _ = std::fs::remove_file(t);
        }
        let _ = sampler.finish();
        std::process::exit(errors::OK);
    }

    // --- Phase 3: verify (pre-commit when converting) + atomic replace ---
    let output_is_xlsx = ext_is(cfg.output.as_path(), "xlsx");
    let output_is_compressed = ext_is(cfg.output.as_path(), "gz") || ext_is(cfg.output.as_path(), "zip");
    if ext_is(cfg.output.as_path(), "zst") || ext_is(cfg.output.as_path(), "zstd") {
        bars.finish();
        errors::fail(errors::USAGE, "output .zst belum didukung — pakai .gz");
    }
    if output_is_xlsx && !matches!(cfg.fmt, SortFormat::Csv { .. }) {
        bars.finish();
        errors::fail(errors::USAGE, "--output .xlsx butuh --format csv / --mode excel");
    }
    if output_is_xlsx && cfg.output_format.is_some() {
        bars.finish();
        errors::fail(errors::USAGE, "--output-format tidak bisa digabung dengan output .xlsx");
    }
    if output_is_xlsx && !matches!(enc_out, inspect::TextEncoding::Utf8) {
        bars.finish();
        errors::fail(errors::USAGE, "--encoding-out tidak berlaku untuk output .xlsx");
    }
    if (output_is_xlsx || output_is_compressed) && cfg.split_by.is_some() {
        bars.finish();
        errors::fail(errors::USAGE, "--split-by tidak bisa digabung dengan output .xlsx/.gz/.zip");
    }
    let converting = cfg.output_format.is_some() || output_is_xlsx || output_is_compressed;
    let mut verify_ok: Option<bool> = None;
    let mut verify_note = String::from("skipped");
    let mut rows_out: usize = split.total_lines;
    if let Some(n) = cfg.limit {
        rows_out = rows_out.min(n);
    }
    // With --output-format the final file changes shape: verify the staged
    // (pre-convert) file BEFORE finalize+convert. Otherwise verify afterwards.
    if converting && cfg.do_verify {
        let vres = match &cfg.fmt {
            SortFormat::Csv { delimiter, keys, key_numeric } => {
                verify::verify_csv_full(staged.as_path(), *delimiter, keys, *key_numeric, cfg.reverse, cfg.header)
            }
            SortFormat::Jsonl { field, key_numeric } => {
                verify::verify_jsonl_full(staged.as_path(), field, *key_numeric, cfg.reverse)
            }
            SortFormat::Numeric => verify::verify_full(staged.as_path(), "numeric", cfg.reverse, false),
            SortFormat::String => verify::verify_full(staged.as_path(), "string", cfg.reverse, cfg.header),
        };
        match vres {
            Ok(n) => {
                let shrinks = cfg.unique || cfg.limit.is_some() || cfg.dedupe_by.is_some();
                if !(if shrinks { n <= split.total_lines } else { n == split.total_lines }) {
                    bars.finish();
                    errors::fail(errors::VERIFY, &format!("verify gagal: jumlah baris output ({}) != input ({})", n, split.total_lines));
                }
                verify_note = format!("OK ({} records)", n);
                verify_ok = Some(true);
                rows_out = n;
            }
            Err(msg) => {
                bars.finish();
                errors::fail(errors::VERIFY, &msg);
            }
        }
    }
    if let Err(e) = finalize_output(staged.as_path(), cfg.output.as_path()) {
        bars.finish();
        errors::fail(errors::IO, &format!("tidak bisa menuliskan output final: {}", errors::io_hint(&e)));
    }
    // --encoding-out: convert final output dari UTF-8 internal ke target.
    if !matches!(enc_out, inspect::TextEncoding::Utf8)
        && let Err(e) = inspect::convert_output_encoding(cfg.output.as_path(), &enc_out) {
            bars.finish();
            errors::fail(errors::IO, &format!("encoding-out gagal: {}", errors::io_hint(&e)));
        }
    // --output-format: streaming convert (csv -> tsv|jsonl|sql|csv).
    if let Some(of) = cfg.output_format.clone()
        && let Err(e) = convert_output_format(cfg.output.as_path(), &cfg.fmt, &of, cfg.header, cfg.sql_table.clone()) {
            bars.finish();
            errors::fail(errors::IO, &format!("output-format gagal: {}", e));
        }
    // .xlsx output: sorted CSV -> workbook (sheet "Sorted" / --sheet).
    if output_is_xlsx {
        let csv_bytes = match std::fs::read(cfg.output.as_path()) {
            Ok(b) => b,
            Err(e) => {
                bars.finish();
                errors::fail(errors::IO, &format!("baca output csv gagal: {}", errors::io_hint(&e)));
            }
        };
        let sheet_name = cfg.sheet.clone().unwrap_or_else(|| "Sorted".to_string());
        if let Err(m) = csv_to_xlsx(&csv_bytes, cfg.output.as_path(), &sheet_name) {
            bars.finish();
            errors::fail(errors::IO, &format!("tulis xlsx gagal: {}", m));
        }
    }
    // .gz/.zip output: compress in place (after verify-independent converts).
    if output_is_compressed
        && let Err(m) = maybe_compress_output(cfg.output.as_path()) {
            bars.finish();
            errors::fail(errors::IO, &format!("kompresi output gagal: {}", m));
        }
    // Verify BEFORE deleting temp evidence; MERGESORT_KEEP_TEMP=1 (debugging)
    // skips cleanup entirely.
    let keep_temp = std::env::var("MERGESORT_KEEP_TEMP").as_deref() == Ok("1");

    if !converting && cfg.do_verify {
        let vres = match &cfg.fmt {
            SortFormat::Csv { delimiter, keys, key_numeric } => {
                verify::verify_csv_full(cfg.output.as_path(), *delimiter, keys, *key_numeric, cfg.reverse, cfg.header)
            }
            SortFormat::Jsonl { field, key_numeric } => {
                verify::verify_jsonl_full(cfg.output.as_path(), field, *key_numeric, cfg.reverse)
            }
            SortFormat::Numeric => verify::verify_full(cfg.output.as_path(), "numeric", cfg.reverse, false),
            SortFormat::String => verify::verify_full(cfg.output.as_path(), "string", cfg.reverse, cfg.header),
        };
        match vres {
            Ok(n) => {
                // --unique/--limit/--dedupe-by shrink the record count; --header
                // is already excluded from both counts. Otherwise require exact.
                let shrinks = cfg.unique || cfg.limit.is_some() || cfg.dedupe_by.is_some();
                let count_ok = if shrinks { n <= split.total_lines } else { n == split.total_lines };
                if !count_ok {
                    bars.finish();
                    errors::fail(
                        errors::VERIFY,
                        &format!(
                            "verify gagal: jumlah baris output ({}) != input ({})",
                            n, split.total_lines
                        ),
                    );
                }
                verify_note = format!("OK ({} records)", n);
                verify_ok = Some(true);
                rows_out = n;
            }
            Err(msg) => {
                bars.finish();
                errors::fail(errors::VERIFY, &msg);
            }
        }
    }

    // --split-by N: break final output into N equal files (header repeated).
    let mut split_note = String::new();
    if let Some(n) = cfg.split_by {
        match split_output_by_count(cfg.output.as_path(), n, cfg.header) {
            Ok(files) => {
                split_note = format!(" -> {} files (split-by-{})", files.len(), n);
                if !cfg.quiet {
                    for f in &files {
                        println!("Split: {}", f.display());
                    }
                }
            }
            Err(e) => {
                bars.finish();
                errors::fail(errors::IO, &format!("split-by gagal: {}", e));
            }
        }
    }

    if !keep_temp {
        temp_manager::cleanup_run_dir_tree(run_dir.as_path());
    }

    let peak_mem = sampler.finish();
    let total_secs = t_start.elapsed().as_secs_f64();
    bars.finish();

    let summary = JsonSummary {
        input: cfg.input.display().to_string(),
        output: cfg.output.display().to_string(),
        format: cfg.fmt.kind().to_string(),
        input_bytes: input_len,
        chunk_count,
        chunk_capacity_records: capacity,
        records: split.total_lines,
        resumed_chunks,
        read_secs: split.read_secs,
        sort_secs: split.sort_secs,
        merge_secs: merge_stats.io_secs,
        total_secs,
        peak_mem_bytes: peak_mem,
        io_bytes_read: merge_stats.bytes_read,
        verify: verify_ok,
        reverse: cfg.reverse,
        unique: cfg.unique,
        header: cfg.header,
        limit: cfg.limit,
    };
    if cfg.json {
        print_json_summary(&summary);
    } else {
        println!();
        println!("=== SORTIR SELESAI ===");
        println!("Ukuran File Input:          {:.2} GB", metrics::gb(input_len as usize));
        println!(
            "Ukuran Chunk:               {:.2} MB ({} chunk)",
            capacity as f64 * chunk::elem_overhead(&cfg.fmt) as f64 / (1024.0 * 1024.0),
            chunk_count
        );
        println!("Total File Sementara:       {}", chunk_count);
        println!("Waktu Baca-Chunk:           {:.3} detik", split.read_secs);
        println!("Waktu Sortir per Chunk:     {:.3} detik", split.sort_secs);
        println!("Waktu Merge (K-way):        {:.3} detik", merge_stats.io_secs);
        println!("Total Waktu:                {:.3} detik", total_secs);
        println!("Puncak Memori (RSS):        {:.1} MB", metrics::mb(peak_mem));
        println!("I/O Total Baca:             {:.2} GB", metrics::gb(merge_stats.bytes_read));
        println!("Baris/Rekaman:              {}", split.total_lines);
        if cfg.reverse {
            println!("Urutan:                   descending (--reverse)");
        }
        if cfg.unique {
            println!("Dedup:                    aktif (--unique)");
        }
        if cfg.header {
            println!("Header:                   dipertahankan (baris 1)");
        }
        if let Some(n) = cfg.limit {
            println!("Limit:                    Top-{} (--limit)", n);
        }
        if let Some(db) = cfg.dedupe_by.as_ref() {
            println!("Dedupe-by:                col {} ({})", db.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","), if cfg.dedupe_keep_last { "keep-last" } else { "keep-first" });
        }
        if let Some(of) = cfg.output_format.as_ref() {
            println!("Output-format:            {}{}", of, split_note);
        } else if !split_note.is_empty() {
            println!("Split:                   {}", split_note.trim_start_matches(" -> "));
        }
        if resumed_chunks > 0 {
            println!("Resume:                     {} chunk dipakai ulang dari run {}", resumed_chunks, resumed_label);
        }
        println!("Verifikasi:                 {}", verify_note);
    }
    if cfg.stats || cfg.stats_json {
        print_stats_block(
            input_len,
            split.total_lines,
            rows_out,
            total_secs,
            peak_mem,
            cfg.max_memory,
            temp_dir_for_check.as_path(),
            &verify_note,
            cfg.stats_json || cfg.json,
        );
    }
    // encoding/compression/xlsx temp cleanup (best-effort).
    if let Some(tmp) = _norm_guard {
        let _ = std::fs::remove_file(tmp);
    }
    for t in &_extra_temps {
        let _ = std::fs::remove_file(t);
    }
    if let Some(log_path) = cfg.log_file.as_ref() {
        append_jsonl_log(log_path, &summary);
    }
}

struct JsonSummary {
    input: String,
    output: String,
    format: String,
    input_bytes: u64,
    chunk_count: usize,
    chunk_capacity_records: usize,
    records: usize,
    resumed_chunks: usize,
    read_secs: f64,
    sort_secs: f64,
    merge_secs: f64,
    total_secs: f64,
    peak_mem_bytes: usize,
    io_bytes_read: usize,
    verify: Option<bool>,
    reverse: bool,
    unique: bool,
    header: bool,
    limit: Option<usize>,
}

/// Appends a compact single-line JSON record to a .jsonl audit log.
/// Best-effort: never fails the sort when logging fails (warns to stderr).
fn append_jsonl_log(path: &Path, j: &JsonSummary) {
    use std::io::Write;
    let verify_json = match j.verify {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };
    let limit_json = match j.limit {
        Some(n) => n.to_string(),
        None => "null".to_string(),
    };
    let line = format!(
        "{{\"ts\":{},\"status\":\"ok\",\"version\":\"{}\",\"input\":\"{}\",\"output\":\"{}\",\"format\":\"{}\",\"mode\":\"{}\",\"input_bytes\":{},\"chunks\":{{\"count\":{},\"capacity_records\":{}}},\"records\":{},\"resumed_chunks\":{},\"options\":{{\"reverse\":{},\"unique\":{},\"header\":{},\"limit\":{}}},\"timing\":{{\"read_s\":{:.3},\"sort_s\":{:.3},\"merge_s\":{:.3},\"total_s\":{:.3}}},\"peak_memory_bytes\":{},\"io_bytes_read\":{},\"verified\":{}}}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        VERSION,
        json_escape(&j.input),
        json_escape(&j.output),
        json_escape(&j.format),
        json_escape(&j.format),
        j.input_bytes,
        j.chunk_count,
        j.chunk_capacity_records,
        j.records,
        j.resumed_chunks,
        j.reverse,
        j.unique,
        j.header,
        limit_json,
        j.read_secs,
        j.sort_secs,
        j.merge_secs,
        j.total_secs,
        j.peak_mem_bytes,
        j.io_bytes_read,
        verify_json
    );
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    match opts.open(path) {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{}", line) {
                eprintln!("warning: cannot write --log-file {}: {}", path.display(), e);
            }
        }
        Err(e) => eprintln!("warning: cannot write --log-file {}: {}", path.display(), e),
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn print_json_summary(j: &JsonSummary) {
    let verify_json = match j.verify {
        Some(true) => "true".to_string(),
        Some(false) => "false".to_string(),
        None => "null".to_string(),
    };
    let limit_json = match j.limit {
        Some(n) => n.to_string(),
        None => "null".to_string(),
    };
    println!(
        "{{\n\
         \"status\": \"ok\",\n\
         \"version\": \"{}\",\n\
         \"input\": \"{}\",\n\
         \"output\": \"{}\",\n\
         \"format\": \"{}\",\n\
         \"mode\": \"{}\",\n\
         \"input_bytes\": {},\n\
         \"chunks\": {{\"count\": {}, \"capacity_records\": {}}},\n\
         \"records\": {},\n\
         \"resumed_chunks\": {},\n\
         \"options\": {{\"reverse\": {}, \"unique\": {}, \"header\": {}, \"limit\": {}}},\n\
         \"timing\": {{\"read_s\": {:.3}, \"sort_s\": {:.3}, \"merge_s\": {:.3}, \"total_s\": {:.3}}},\n\
         \"peak_memory_bytes\": {},\n\
         \"io_bytes_read\": {},\n\
         \"verified\": {}\n\
         }}",
        VERSION,
        json_escape(j.input.as_str()),
        json_escape(j.output.as_str()),
        json_escape(j.format.as_str()),
        json_escape(j.format.as_str()),
        j.input_bytes,
        j.chunk_count,
        j.chunk_capacity_records,
        j.records,
        j.resumed_chunks,
        j.reverse,
        j.unique,
        j.header,
        limit_json,
        j.read_secs,
        j.sort_secs,
        j.merge_secs,
        j.total_secs,
        j.peak_mem_bytes,
        j.io_bytes_read,
        verify_json
    );
}

/// Parses `--dashboard` values: "PORT" binds 127.0.0.1, "host:PORT" binds host.
fn parse_dashboard_spec(spec: &str) -> Option<(String, u16)> {
    if let Some((host, port_str)) = spec.rsplit_once(':') {
        let port: u16 = port_str.parse().ok()?;
        Some((host.to_string(), port))
    } else {
        let port: u16 = spec.parse().ok()?;
        Some(("127.0.0.1".to_string(), port))
    }
}
