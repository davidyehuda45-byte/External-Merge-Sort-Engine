// inspect.rs — file intelligence for MergeSort Pro v1.2+
// Auto-detect delimiter/header/encoding, --check validation, smart --dry-run.
// Pure std (no new deps) so --check works even offline.
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf8Bom,
    Latin1,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

impl TextEncoding {
    pub fn parse(s: &str) -> Option<TextEncoding> {
        match s.trim().to_lowercase().replace('_', "-").as_str() {
            "utf-8" | "utf8" => Some(TextEncoding::Utf8),
            "utf-8-bom" | "utf8-bom" => Some(TextEncoding::Utf8Bom),
            "latin-1" | "latin1" | "iso-8859-1" | "windows-1252" => Some(TextEncoding::Latin1),
            "utf-16le" | "utf16le" | "utf-16-le" => Some(TextEncoding::Utf16Le),
            "utf-16be" | "utf16be" | "utf-16-be" => Some(TextEncoding::Utf16Be),
            "utf-32le" | "utf32le" | "utf-32-le" => Some(TextEncoding::Utf32Le),
            "utf-32be" | "utf32be" | "utf-32-be" => Some(TextEncoding::Utf32Be),
            _ => None,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            TextEncoding::Utf8 => "UTF-8",
            TextEncoding::Utf8Bom => "UTF-8-BOM",
            TextEncoding::Latin1 => "Latin-1",
            TextEncoding::Utf16Le => "UTF-16LE",
            TextEncoding::Utf16Be => "UTF-16BE",
            TextEncoding::Utf32Le => "UTF-32LE",
            TextEncoding::Utf32Be => "UTF-32BE",
        }
    }
}

/// BOM sniff: returns (encoding, bom_len). Defaults to UTF-8 no BOM.
/// Checks 4-byte UTF-32 patterns BEFORE the 2-byte UTF-16 ones: `FF FE`
/// alone is UTF-16LE, but `FF FE 00 00` is UTF-32LE (and `00 00 FE FF` is
/// UTF-32BE) — testing the short pattern first would misclassify every
/// UTF-32 file.
pub fn sniff_bom(path: &Path) -> (TextEncoding, usize) {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return (TextEncoding::Utf8, 0),
    };
    let mut buf = [0u8; 4];
    let n = f.read(&mut buf).unwrap_or(0);
    if n >= 3 && buf[0] == 0xEF && buf[1] == 0xBB && buf[2] == 0xBF {
        (TextEncoding::Utf8Bom, 3)
    } else if n >= 4 && buf[0] == 0xFF && buf[1] == 0xFE && buf[2] == 0x00 && buf[3] == 0x00 {
        (TextEncoding::Utf32Le, 4)
    } else if n >= 4 && buf[0] == 0x00 && buf[1] == 0x00 && buf[2] == 0xFE && buf[3] == 0xFF {
        (TextEncoding::Utf32Be, 4)
    } else if n >= 2 && buf[0] == 0xFF && buf[1] == 0xFE {
        (TextEncoding::Utf16Le, 2)
    } else if n >= 2 && buf[0] == 0xFE && buf[1] == 0xFF {
        (TextEncoding::Utf16Be, 2)
    } else {
        (TextEncoding::Utf8, 0)
    }
}

/// Decode raw bytes to UTF-8 String per encoding. Lossy but never panics.
pub fn decode_to_utf8(raw: &[u8], enc: &TextEncoding) -> String {
    match enc {
        TextEncoding::Utf8 | TextEncoding::Utf8Bom => {
            String::from_utf8_lossy(raw).into_owned()
        }
        TextEncoding::Latin1 => raw.iter().map(|&b| b as char).collect(),
        TextEncoding::Utf16Le => {
            let mut u16s = Vec::with_capacity(raw.len() / 2);
            let mut i = 0;
            while i + 1 < raw.len() {
                u16s.push(u16::from_le_bytes([raw[i], raw[i + 1]]));
                i += 2;
            }
            String::from_utf16_lossy(&u16s)
        }
        TextEncoding::Utf16Be => {
            let mut u16s = Vec::with_capacity(raw.len() / 2);
            let mut i = 0;
            while i + 1 < raw.len() {
                u16s.push(u16::from_be_bytes([raw[i], raw[i + 1]]));
                i += 2;
            }
            String::from_utf16_lossy(&u16s)
        }
        TextEncoding::Utf32Le => {
            // Trailing 1-3 partial bytes (bounded-sample cut) are ignored.
            raw.as_chunks::<4>().0
                .iter()
                .map(|c| char::from_u32(u32::from_le_bytes([c[0], c[1], c[2], c[3]])).unwrap_or('\u{FFFD}'))
                .collect()
        }
        TextEncoding::Utf32Be => {
            raw.as_chunks::<4>().0
                .iter()
                .map(|c| char::from_u32(u32::from_be_bytes([c[0], c[1], c[2], c[3]])).unwrap_or('\u{FFFD}'))
                .collect()
        }
    }
}

/// Encode UTF-8 string to target encoding bytes.
pub fn encode_from_utf8(s: &str, enc: &TextEncoding, with_bom: bool) -> Vec<u8> {
    match enc {
        TextEncoding::Utf8 => s.as_bytes().to_vec(),
        TextEncoding::Utf8Bom => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(s.as_bytes());
            v
        }
        TextEncoding::Latin1 => s.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect(),
        TextEncoding::Utf16Le => {
            let mut v = Vec::new();
            if with_bom {
                v.extend_from_slice(&[0xFF, 0xFE]);
            }
            for u in s.encode_utf16() {
                v.extend_from_slice(&u.to_le_bytes());
            }
            v
        }
        TextEncoding::Utf16Be => {
            let mut v = Vec::new();
            if with_bom {
                v.extend_from_slice(&[0xFE, 0xFF]);
            }
            for u in s.encode_utf16() {
                v.extend_from_slice(&u.to_be_bytes());
            }
            v
        }
        TextEncoding::Utf32Le => {
            let mut v = Vec::new();
            if with_bom {
                v.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x00]);
            }
            for c in s.chars() {
                v.extend_from_slice(&(c as u32).to_le_bytes());
            }
            v
        }
        TextEncoding::Utf32Be => {
            let mut v = Vec::new();
            if with_bom {
                v.extend_from_slice(&[0x00, 0x00, 0xFE, 0xFF]);
            }
            for c in s.chars() {
                v.extend_from_slice(&(c as u32).to_be_bytes());
            }
            v
        }
    }
}

/// Read first `max_lines` decoded lines from a BOUNDED head sample (max 256KB
/// after the BOM). The old implementation `fs::read` the whole file, so
/// `--check`/`--dry-run` on a 100GB input needed 100GB RAM; now memory is
/// O(256KB). Returns fewer than `max_lines` when the head sample is short —
/// the tail sample in `check_file` covers the file end separately.
pub fn read_sample_lines(path: &Path, enc: &TextEncoding, bom_len: usize, max_lines: usize) -> Vec<String> {
    const MAX_SAMPLE_BYTES: u64 = 256 * 1024;
    let f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut r = BufReader::new(f);
    if bom_len > 0 && r.seek(SeekFrom::Start(bom_len as u64)).is_err() {
        return Vec::new();
    }
    let mut raw = Vec::new();
    // `take` bounds the read: at most 256KB are ever pulled from disk here.
    if r.take(MAX_SAMPLE_BYTES).read_to_end(&mut raw).is_err() {
        return Vec::new();
    }
    // Decode needs whole code units: a 256KB cut can end mid-unit. Trim the
    // trailing partial unit (1 byte for UTF-16, 1-3 for UTF-32) so the
    // decoder never sees a torn unit.
    match enc {
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            if raw.len() % 2 == 1 {
                raw.pop();
            }
        }
        TextEncoding::Utf32Le | TextEncoding::Utf32Be => {
            let rem = raw.len() % 4;
            raw.truncate(raw.len() - rem);
        }
        _ => {}
    }
    let text = decode_to_utf8(&raw, enc);
    text.lines().take(max_lines).map(|l| l.to_string()).collect()
}

/// Candidate delimiters for auto-detect: , ; : TAB |
/// (`:` covers time-series / `key:value` logs; it only wins when it splits
/// consistently — see scoring below.)
pub const DELIM_CANDIDATES: &[u8] = b",;:\t|";

/// Counts `delim` occurrences OUTSIDE double-quoted spans (simple quoted
/// validation: `"` toggles in/out, `""` inside quotes is an escaped quote and
/// does not toggle). A naive `split` would count delimiters inside `"a,b"`
/// as column separators and mis-detect the delimiter.
fn count_unquoted(line: &str, delim: char) -> usize {
    let mut count = 0usize;
    let mut in_q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if in_q && chars.peek() == Some(&'"') {
                chars.next(); // escaped quote: stay inside
            } else {
                in_q = !in_q;
            }
        } else if c == delim && !in_q {
            count += 1;
        }
    }
    count
}

/// True when the line's quotes are balanced (even number of unescaped `"`).
/// Unbalanced lines (torn records, stray quotes) are excluded from delimiter
/// scoring so one corrupt line can't elect the wrong delimiter.
fn balanced_quotes(line: &str) -> bool {
    let mut in_q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if in_q && chars.peek() == Some(&'"') {
                chars.next();
            } else {
                in_q = !in_q;
            }
        }
    }
    !in_q
}

/// Auto-detect delimiter: most consistent column count over sample lines.
/// Column counts use quote-aware splitting; quoted validation excludes
/// unbalanced lines. Returns (delimiter, avg_cols). Falls back to b','.
pub fn detect_delimiter(lines: &[String]) -> (u8, usize) {
    let usable: Vec<&str> = lines
        .iter()
        .filter(|l| !l.trim().is_empty() && balanced_quotes(l))
        .take(100)
        .map(|s| s.as_str())
        .collect();
    if usable.is_empty() {
        return (b',', 1);
    }
    let mut best = (b',', 0usize, 0usize); // delim, consistency_score, cols
    for &d in DELIM_CANDIDATES {
        let ch = d as char;
        let mut counts: Vec<usize> = usable.iter().map(|l| count_unquoted(l, ch) + 1).collect();
        if counts.is_empty() {
            continue;
        }
        counts.sort_unstable();
        let median = counts[counts.len() / 2];
        let consistent = counts.iter().filter(|&&c| c == median).count();
        // Prefer higher consistency, then more columns (a real delimiter splits).
        let score = consistent * 100 + median.min(20);
        let best_score = best.1 * 100 + best.2.min(20);
        if score > best_score && median > 1 {
            best = (d, consistent, median);
        }
    }
    if best.2 <= 1 {
        (b',', 1)
    } else {
        (best.0, best.2)
    }
}

/// Quote-aware field split (double-quote spans hide the delimiter;
/// `""` is an escaped quote). Byte-scan is char-boundary safe because the
/// delimiter and `"` are ASCII and never appear inside multi-byte sequences.
fn split_quoted(line: &str, delim: u8) -> Vec<&str> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut in_q = false;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'"' {
            if in_q && i + 1 < b.len() && b[i + 1] == b'"' {
                i += 2;
                continue;
            }
            in_q = !in_q;
        } else if b[i] == delim && !in_q {
            out.push(&line[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    out.push(&line[start..]);
    out
}

/// Heuristic header detect, scoped to the actual key column so unrelated
/// text columns (e.g. a city/name field next to a numeric id) can't cause a
/// false positive. `delimiter` is the auto-detected (or user) delimiter —
/// field extraction splits on THAT byte quote-aware, never a hardcoded set
/// (the old hardcoded `[',', ';', '\t', '|']` mislocated the key column for
/// `:`-delimited or TAB data). Pass 0 for single-column (string) mode: the
/// whole line is the key field. Signals:
///  - numeric key: line 0's key field is non-numeric while sampled data rows'
///    key field is consistently numeric.
///  - non-numeric key: line 0's key field matches a common header word.
pub fn detect_header(lines: &[String], key_numeric: bool, key_col: usize, delimiter: u8) -> bool {
    if lines.len() < 2 {
        return false;
    }
    let field_at = |line: &str, idx: usize| -> String {
        if delimiter == 0 {
            return line.trim().to_string();
        }
        split_quoted(line.trim(), delimiter).get(idx).unwrap_or(&"").trim().to_string()
    };
    let f0 = field_at(&lines[0], key_col);
    if f0.is_empty() {
        return false;
    }
    if key_numeric {
        // The key field itself being numeric on line 0 means it's a data row,
        // no matter what other columns contain.
        if f0.parse::<f64>().is_ok() {
            return false;
        }
        let mut numeric_rows = 0;
        let mut sampled = 0;
        for l in lines.iter().skip(1).take(20) {
            let f = field_at(l, key_col);
            if f.is_empty() {
                continue;
            }
            sampled += 1;
            if f.parse::<f64>().is_ok() {
                numeric_rows += 1;
            }
        }
        return sampled >= 3 && numeric_rows == sampled;
    }
    // Non-numeric key: header-like words (id, name, nama, tanggal, total...)
    // in the key column specifically.
    let low = f0.to_lowercase();
    for kw in ["id", "nama", "name", "tanggal", "date", "total", "header", "kolom", "column", "email", "kota", "city"] {
        if low.contains(kw) {
            return true;
        }
    }
    false
}

pub struct CheckReport {
    pub file: String,
    pub size_bytes: u64,
    pub rows_sampled: usize,
    pub est_rows: u64,
    pub delimiter: u8,
    pub delimiter_consistent: bool,
    pub header: Option<String>,
    pub columns: usize,
    pub col_names: Vec<String>,
    #[allow(dead_code)]
    pub key_col_ok: bool,
    pub key_col_msg: String,
    pub bad_rows: usize,
    pub encoding: String,
    pub ready: bool,
    pub notes: Vec<String>,
}

/// Full --check: sample head+tail, validate columns/encoding/keys.
#[allow(clippy::too_many_arguments)]
pub fn check_file(
    path: &Path,
    fmt_kind: &str,
    key_column: Option<usize>,
    key_numeric: bool,
    encoding_opt: Option<&TextEncoding>,
) -> Result<CheckReport, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("cannot stat {}: {}", path.display(), e))?;
    let size = meta.len();
    let (bom_enc, bom_len) = sniff_bom(path);
    let enc = encoding_opt.cloned().unwrap_or(bom_enc);
    let head = read_sample_lines(path, &enc, bom_len, 1000);

    // Tail sample for consistency (last ~64KB decoded).
    let tail_lines: Vec<String> = {
        let f = File::open(path).map_err(|e| format!("cannot open: {}", e))?;
        let mut r = BufReader::new(f);
        let seek = (size as i64 - 65536).max(0);
        r.seek(SeekFrom::Start(seek as u64)).map_err(|e| e.to_string())?;
        let mut tail_raw = Vec::new();
        r.read_to_end(&mut tail_raw).map_err(|e| e.to_string())?;
        let t = decode_to_utf8(&tail_raw, &enc);
        let mut v: Vec<String> = t.lines().map(|l| l.to_string()).collect();
        if seek > 0 && !v.is_empty() {
            v.remove(0); // first tail line may be partial
        }
        v
    };

    let mut notes = Vec::new();
    if fmt_kind == "numeric" {
        return Ok(CheckReport {
            file: path.display().to_string(),
            size_bytes: size,
            rows_sampled: head.len(),
            est_rows: size / 8,
            delimiter: 0,
            delimiter_consistent: true,
            header: None,
            columns: 1,
            col_names: vec![],
            key_col_ok: size % 8 == 0,
            key_col_msg: if size % 8 == 0 { "size % 8 == 0 OK".into() } else { format!("size {} not multiple of 8", size) },
            bad_rows: 0,
            encoding: "binary-u64le".into(),
            ready: size % 8 == 0,
            notes: if size % 8 == 0 { vec![] } else { vec!["trailing partial record".into()] },
        });
    }

    // Text modes: delimiter + header + column consistency.
    let (delim, cols) = if fmt_kind == "csv" {
        let (d, c) = detect_delimiter(&head);
        (d, c)
    } else {
        (0, 1)
    };
    // Consistency: what fraction of sampled lines have `cols` columns?
    let mut checked = 0usize;
    let mut bad = 0usize;
    let all_sample: Vec<&String> = head.iter().chain(tail_lines.iter()).collect();
    if fmt_kind == "csv" {
        let ch = delim as char;
        for l in all_sample.iter().take(2000) {
            if l.trim().is_empty() {
                continue;
            }
            checked += 1;
            if l.split(ch).count() != cols {
                bad += 1;
            }
        }
    }
    let consistent = checked == 0 || (bad * 100 / checked) < 5;
    if !consistent {
        notes.push(format!("kolom tidak konsisten di {}/{} sampel", bad, checked));
    }

    let has_header = detect_header(&head, key_numeric, key_column.unwrap_or(0), delim);
    let header_line = if has_header { head.first().cloned() } else { None };
    let col_names: Vec<String> = if fmt_kind == "csv" {
        if let Some(h) = &header_line {
            h.split(delim as char).map(|s| s.trim().to_string()).collect()
        } else {
            (0..cols).map(|i| format!("col{}", i)).collect()
        }
    } else {
        vec![]
    };

    // Key column validation over sample.
    let (key_ok, key_msg) = match (fmt_kind, key_column) {
        ("csv", Some(k)) => {
            if k >= cols {
                (false, format!("key column {} >= detected {} cols", k, cols))
            } else if key_numeric {
                let mut okc = 0;
                let mut badc = 0;
                let start = if has_header { 1 } else { 0 };
                for l in head.iter().skip(start).take(500) {
                    if l.trim().is_empty() {
                        continue;
                    }
                    let f: Vec<&str> = l.split(delim as char).collect();
                    match f.get(k).map(|s| s.trim().parse::<f64>().is_ok()).unwrap_or(false) {
                        true => okc += 1,
                        false => badc += 1,
                    }
                }
                if badc == 0 && okc > 0 {
                    (true, format!("kolom {} numerik ({} sampel valid)", k, okc))
                } else {
                    (false, format!("kolom {} bukan numerik ({}/{} valid)", k, okc, okc + badc))
                }
            } else {
                (true, format!("kolom {} string OK", k))
            }
        }
        _ => (true, "no key check".into()),
    };
    if !key_ok {
        notes.push(key_msg.clone());
    }

    // Row estimate: size / avg line len.
    let avg_len = {
        let tot: usize = head.iter().take(200).map(|l| l.len() + 1).sum();
        let n = head.iter().take(200).filter(|l| !l.is_empty()).count().max(1);
        (tot / n).max(1) as u64
    };
    // A detected header row is metadata, not data: subtract it so --dry-run
    // counts don't overshoot by one.
    let est_rows = if has_header { (size / avg_len).saturating_sub(1) } else { size / avg_len };
    let ready = consistent && key_ok;

    Ok(CheckReport {
        file: path.display().to_string(),
        size_bytes: size,
        rows_sampled: head.len().min(1000) + tail_lines.len().min(200),
        est_rows,
        delimiter: delim,
        delimiter_consistent: consistent,
        header: header_line,
        columns: cols,
        col_names,
        key_col_ok: key_ok,
        key_col_msg: key_msg,
        bad_rows: bad,
        encoding: enc.name().to_string(),
        ready,
        notes,
    })
}

/// Normalize input to UTF-8 temp file when encoding_in is not UTF-8.
/// Returns (path_to_use, Option<temp_to_delete>).
/// Streaming: reads + writes in 64KB chunks so a multi-GB Latin-1/UTF-16
/// input never sits fully in RAM (the old `fs::read` did). Chunk edges that
/// split a code unit hold the partial tail as `carry` for the next round;
/// only the final flush decodes a possibly-torn tail (lossy, best effort).
pub fn normalize_input_to_utf8(input: &Path, enc: &TextEncoding, bom_len: usize) -> std::io::Result<(PathBuf, Option<PathBuf>)> {
    match enc {
        TextEncoding::Utf8 => Ok((input.to_path_buf(), None)),
        _ => {
            const CHUNK: usize = 64 * 1024;
            let f = File::open(input)?;
            let mut r = BufReader::with_capacity(CHUNK, f);
            if bom_len > 0 {
                r.seek(SeekFrom::Start(bom_len as u64))?;
            }
            let tmp = input.with_extension("utf8norm.tmp");
            let out = File::create(&tmp)?;
            let mut w = BufWriter::with_capacity(CHUNK, out);
            let mut buf = vec![0u8; CHUNK];
            let mut carry: Vec<u8> = Vec::new();
            loop {
                let mut n = 0usize;
                while n < buf.len() {
                    match r.read(&mut buf[n..])? {
                        0 => break,
                        k => n += k,
                    }
                }
                let eof = n == 0;
                if eof && carry.is_empty() {
                    break;
                }
                // Assemble carry + new bytes, then split off a decodable
                // prefix; the torn tail (if any) becomes the next carry.
                let mut unit = std::mem::take(&mut carry);
                unit.extend_from_slice(&buf[..n]);
                let complete = if eof {
                    unit.len()
                } else {
                    match enc {
                        TextEncoding::Utf16Le | TextEncoding::Utf16Be => unit.len() - unit.len() % 2,
                        TextEncoding::Utf32Le | TextEncoding::Utf32Be => unit.len() - unit.len() % 4,
                        TextEncoding::Utf8Bom => valid_utf8_prefix_len(&unit),
                        TextEncoding::Latin1 => unit.len(), // byte-wise decode: no torn units
                        TextEncoding::Utf8 => unit.len(),
                    }
                };
                carry = unit[complete..].to_vec();
                let text = decode_to_utf8(&unit[..complete], enc);
                w.write_all(text.as_bytes())?;
                if eof {
                    // Flush the torn tail best-effort (lossy) so no byte is
                    // silently dropped at the file end.
                    if !carry.is_empty() {
                        let tail = decode_to_utf8(&carry, enc);
                        w.write_all(tail.as_bytes())?;
                        carry.clear();
                    }
                    break;
                }
            }
            w.flush()?;
            Ok((tmp.clone(), Some(tmp)))
        }
    }
}

/// Length of the largest valid-UTF-8 prefix (never splits a multi-byte char
/// across 64KB chunk edges). Uses the error's `valid_up_to` when available.
fn valid_utf8_prefix_len(buf: &[u8]) -> usize {
    match std::str::from_utf8(buf) {
        Ok(_) => buf.len(),
        Err(e) => {
            let mut cut = e.valid_up_to();
            // Back off past any incomplete trailing sequence (valid_up_to
            // already points at the error; error_len None means truncated).
            if e.error_len().is_none() {
                while cut > 0 && !buf[..cut].is_empty() && std::str::from_utf8(&buf[..cut]).is_err() {
                    cut -= 1;
                }
            }
            cut
        }
    }
}

/// Convert sorted UTF-8 output file to target encoding (BOM written for UTF-16/UTF-32 + utf8-bom).
/// Streaming: the old version `fs::read` the whole output (doubling peak RAM
/// on convert); now it streams 64KB UTF-8 chunks through a `.part` staging
/// file and renames atomically, so a crash never leaves a half-converted
/// output in place.
pub fn convert_output_encoding(path: &Path, enc: &TextEncoding) -> std::io::Result<()> {
    match enc {
        TextEncoding::Utf8 => Ok(()),
        _ => {
            const CHUNK: usize = 64 * 1024;
            let with_bom = matches!(enc, TextEncoding::Utf16Le | TextEncoding::Utf16Be | TextEncoding::Utf8Bom | TextEncoding::Utf32Le | TextEncoding::Utf32Be);
            let f = File::open(path)?;
            let mut r = BufReader::with_capacity(CHUNK, f);
            let part = path.with_extension("conv.tmp");
            let out = File::create(&part)?;
            let mut w = BufWriter::with_capacity(CHUNK, out);
            // BOM first so even an empty output stays detectable.
            if with_bom {
                let bom: &[u8] = match enc {
                    TextEncoding::Utf8Bom => &[0xEF, 0xBB, 0xBF],
                    TextEncoding::Utf16Le => &[0xFF, 0xFE],
                    TextEncoding::Utf16Be => &[0xFE, 0xFF],
                    TextEncoding::Utf32Le => &[0xFF, 0xFE, 0x00, 0x00],
                    TextEncoding::Utf32Be => &[0x00, 0x00, 0xFE, 0xFF],
                    _ => &[],
                };
                w.write_all(bom)?;
            }
            let mut buf = vec![0u8; CHUNK];
            let mut carry: Vec<u8> = Vec::new();
            loop {
                let mut n = 0usize;
                while n < buf.len() {
                    match r.read(&mut buf[n..])? {
                        0 => break,
                        k => n += k,
                    }
                }
                let eof = n == 0;
                if eof && carry.is_empty() {
                    break;
                }
                let mut unit = std::mem::take(&mut carry);
                unit.extend_from_slice(&buf[..n]);
                let complete = if eof { unit.len() } else { valid_utf8_prefix_len(&unit) };
                carry = unit[complete..].to_vec();
                let text = String::from_utf8_lossy(&unit[..complete]).into_owned();
                let encoded = encode_from_utf8(&text, enc, false);
                w.write_all(&encoded)?;
                if eof {
                    if !carry.is_empty() {
                        let tail = String::from_utf8_lossy(&carry).into_owned();
                        let encoded = encode_from_utf8(&tail, enc, false);
                        w.write_all(&encoded)?;
                        carry.clear();
                    }
                    break;
                }
            }
            w.flush()?;
            drop(w);
            std::fs::rename(&part, path)?;
            Ok(())
        }
    }
}

fn delim_name(d: u8) -> String {
    if d == b'\t' {
        "'\\t' (TAB)".to_string()
    } else {
        format!("'{}'", (d as char))
    }
}

pub fn print_check_report(r: &CheckReport, fmt_kind: &str) {
    println!("=== Pemeriksaan File ===");
    println!("File        : {}", r.file);
    println!("Ukuran      : {} ({} bytes)", human(r.size_bytes), r.size_bytes);
    println!("Est. baris  : {}", fmt_n(r.est_rows));
    if fmt_kind == "csv" {
        println!("Delimiter   : {} ({})", delim_name(r.delimiter), if r.delimiter_consistent { "konsisten" } else { "TIDAK konsisten" });
        match &r.header {
            Some(h) => println!("Header      : ya -> {:?}", truncate(h, 80)),
            None => println!("Header      : tidak terdeteksi"),
        }
        println!("Kolom       : {} ({})", r.columns, r.col_names.join(", "));
    }
    println!("Key check   : {}", r.key_col_msg);
    println!("Baris rusak : {} (sampel {} baris)", r.bad_rows, r.rows_sampled);
    println!("Encoding    : {}", r.encoding);
    for n in &r.notes {
        println!("Catatan     : {}", n);
    }
    if r.ready {
        println!("Status      : SIAP DI-SORT");
    } else {
        println!("Status      : ADA MASALAH (lihat catatan)");
    }
    println!("========================");
}

pub fn human(b: u64) -> String {
    crate::disk::human_size(b)
}
/// Thousands-grouped counter for reports (`1,234,567`).
/// The separator is a plain comma — locale-neutral and unambiguous in logs/
/// JSON-adjacent output. (An earlier revision used `.`, the Indonesian
/// thousands separator, which reads as a decimal point to most tooling and
/// humans outside id-ID; `,` matches the widespread programmer convention
/// alongside Rust's `_` digit separator.)
pub fn fmt_n(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}
/// Char-boundary-safe head truncation for report previews. Byte-slicing
/// `&s[..n]` panics on multi-byte UTF-8 (e.g. CJK/emoji in headers); instead
/// walk `char_indices` and cut after the last whole char within `n` bytes.
fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut cut = 0usize;
    for (i, c) in s.char_indices() {
        if i + c.len_utf8() > n {
            break;
        }
        cut = i + c.len_utf8();
    }
    format!("{}...", &s[..cut])
}
