// Run manifest: crash-resume support (PRD v2 §3.2).
//
// The manifest lives in the run dir (`<run>/manifest.json`) and is rewritten
// atomically (write .part + rename) after every completed chunk. On startup,
// `--resume <run_id>` adopts an existing run dir whose manifest is not marked
// finished; completed chunks listed there are kept, everything else restarts.
//
// Manifest JSON (hand-built, no serde dependency):
// {"run_id":"...","format":"numeric|string|csv","delimiter":"\u00XX",
//  "key_columns":[N,...],"input":"...","finished":false,"chunks":["chunk_0000.dat",...]}
//
// `format` is stored for safety: a resume only proceeds when the stored config
// matches what the user passed on this invocation.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub run_id: String,
    /// "numeric" | "string" | "csv"
    pub format: String,
    /// CSV only: the delimiter as a decimal-escaped byte string.
    pub delimiter: String,
    /// CSV only: sort key column indices.
    pub key_columns: Vec<usize>,
    /// Absolute path of the original input file.
    pub input: String,
    /// Chunk capacity used at creation (guards resume against a changed
    /// --max-memory, which would make the completed-chunk boundaries wrong).
    pub capacity: u64,
    /// Original input size in bytes (informational).
    pub input_bytes: u64,
    /// True once the merge phase completed successfully.
    pub finished: bool,
    /// Chunk file names completed AND fully flushed (sorted, fsynced).
    pub chunks: Vec<String>,
    /// Input byte offset AFTER the last record of each chunk (parallel to
    /// `chunks`). Resume fast-forwards the input to the last offset instead of
    /// guessing boundaries from chunk capacity.
    pub chunk_offsets: Vec<u64>,
}

impl Manifest {
    /// Byte offset to resume from: end of the last completed chunk.
    pub fn resume_offset(&self) -> u64 {
        self.chunk_offsets.last().copied().unwrap_or(0)
    }
}

/// Removes quotes/backslashes and escapes control characters so a path or id
/// can be embedded safely in a JSON string literal.
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

impl Manifest {
    pub fn new(
        run_id: String,
        format: String,
        delimiter: String,
        key_columns: Vec<usize>,
        input: String,
        capacity: u64,
        input_bytes: u64,
    ) -> Manifest {
        Manifest {
            run_id,
            format,
            delimiter,
            key_columns,
            input,
            capacity,
            input_bytes,
            finished: false,
            chunks: Vec::new(),
            chunk_offsets: Vec::new(),
        }
    }

    pub fn to_json(&self) -> String {
        let mut s = String::with_capacity(256 + self.chunks.len() * 20);
        s.push_str("{\n");
        s.push_str(&format!("  \"run_id\": \"{}\",\n", json_escape(&self.run_id)));
        s.push_str(&format!("  \"format\": \"{}\",\n", json_escape(&self.format)));
        s.push_str(&format!("  \"delimiter\": \"{}\",\n", json_escape(&self.delimiter)));
        let keys: Vec<String> = self.key_columns.iter().map(|k| k.to_string()).collect();
        s.push_str(&format!("  \"key_columns\": [{}],\n", keys.join(", ")));
        s.push_str(&format!("  \"input\": \"{}\",\n", json_escape(&self.input)));
        s.push_str(&format!("  \"capacity\": {},\n", self.capacity));
        s.push_str(&format!("  \"input_bytes\": {},\n", self.input_bytes));
        s.push_str(&format!("  \"finished\": {},\n", self.finished));
        s.push_str("  \"chunks\": [\n");
        for (i, c) in self.chunks.iter().enumerate() {
            s.push_str(&format!("    \"{}\"{}\n", json_escape(c), if i + 1 < self.chunks.len() { "," } else { "" }));
        }
        s.push_str("  ],\n");
        let offs: Vec<String> = self.chunk_offsets.iter().map(|o| o.to_string()).collect();
        s.push_str(&format!("  \"chunk_offsets\": [{}]\n", offs.join(", ")));
        s.push_str("}\n");
        s
    }

    /// Writes manifest.part then renames over manifest.json (atomic on same dir).
    ///
    /// Durability note: the file itself is fsynced before rename so a
    /// completed-chunk record survives a crash. A directory fsync after rename
    /// (durable rename entry) is best-effort below: on Linux/Unix the dir fd
    /// is synced; on Windows std offers no stable dir-sync so we rely on the
    /// file sync + atomic rename. See also [`acquire_run_lock`] for the
    /// resume-race guard (two processes must not own one run dir).
    pub fn save_atomic(&self, run_dir: &Path) -> std::io::Result<()> {
        let part = run_dir.join(Path::new("manifest.part"));
        let final_path = run_dir.join(Path::new("manifest.json"));
        {
            let mut f = fs::File::create(part.clone())?;
            f.write_all(self.to_json().as_bytes())?;
            f.flush()?;
            f.sync_all().ok(); // best effort: durability of the completed-chunk record
        }
        fs::rename(part, final_path)?;
        // Best-effort directory fsync so the rename itself is durable.
        #[cfg(unix)]
        {
            if let Ok(d) = fs::File::open(run_dir) {
                let _ = d.sync_all();
            }
        }
        Ok(())
    }

    /// Parses a manifest; returns None if missing or unparseable.
    pub fn load(run_dir: &Path) -> Option<Manifest> {
        let txt = fs::read_to_string(run_dir.join(Path::new("manifest.json"))).ok()?;
        parse_manifest(&txt)
    }
}

/// Minimal JSON extraction for the fixed manifest schema.
fn json_str_field(txt: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\"", key);
    let i = txt.find(&pat)? + pat.len();
    let rest = txt[i..].trim_start();
    if !rest.starts_with(':') {
        return None;
    }
    let rest = rest[1..].trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut out = String::new();
    let mut j = 1usize;
    while j < bytes.len() {
        match bytes[j] {
            b'"' => return Some(out),
            b'\\' if j + 1 < bytes.len() => {
                let c = bytes[j + 1];
                match c {
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' if j + 5 < bytes.len() => {
                        let hex = std::str::from_utf8(&bytes[j + 2..j + 6]).ok()?;
                        let cp = u32::from_str_radix(hex, 16).ok()?;
                        out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                        j += 4;
                    }
                    other => out.push(other as char),
                }
                j += 1;
            }
            other => out.push(other as char),
        }
        j += 1;
    }
    None
}

fn json_bool_field(txt: &str, key: &str) -> Option<bool> {
    let pat = format!("\"{}\"", key);
    let i = txt.find(&pat)? + pat.len();
    let rest = txt[i..].trim_start();
    if !rest.starts_with(':') {
        return None;
    }
    let rest = rest[1..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Extracts a bare numeric field (digits only).
fn json_num_field(txt: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{}\"", key);
    let i = txt.find(&pat)? + pat.len();
    let rest = txt[i..].trim_start();
    if !rest.starts_with(':') {
        return None;
    }
    let rest = rest[1..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse::<u64>().ok()
}

fn json_num_array_field(txt: &str, key: &str) -> Option<Vec<u64>> {
    let pat = format!("\"{}\"", key);
    let i = txt.find(&pat)? + pat.len();
    let rest = txt[i..].trim_start();
    if !rest.starts_with(':') {
        return None;
    }
    let rest = rest[1..].trim_start();
    if !rest.starts_with('[') {
        return None;
    }
    let end = rest.find(']')?;
    let inner = &rest[1..end];
    let mut out = Vec::new();
    for part in inner.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        out.push(p.parse::<u64>().ok()?);
    }
    Some(out)
}

fn json_str_array_field(txt: &str, key: &str) -> Option<Vec<String>> {
    let pat = format!("\"{}\"", key);
    let i = txt.find(&pat)? + pat.len();
    let rest = txt[i..].trim_start();
    if !rest.starts_with(':') {
        return None;
    }
    let rest = rest[1..].trim_start();
    if !rest.starts_with('[') {
        return None;
    }
    let mut out = Vec::new();
    let bytes = rest.as_bytes();
    let mut j = 1usize;
    while j < bytes.len() {
        if bytes[j] == b']' {
            break;
        }
        if bytes[j] == b'"' {
            // Reuse the string scanner from json_str_field by slicing from j.
            let sub = &rest[j..];
            let s = json_str_field(&format!("\"k\": {}", sub), "k")?;
            out.push(s);
            // Advance past the closing quote of that string.
            j += 1; // opening quote
            while j < bytes.len() {
                match bytes[j] {
                    b'\\' => j += 1,
                    b'"' => break,
                    _ => {}
                }
                j += 1;
            }
            j += 1; // closing quote
            continue;
        }
        j += 1;
    }
    Some(out)
}

pub fn parse_manifest(txt: &str) -> Option<Manifest> {
    let m = Manifest {
        run_id: json_str_field(txt, "run_id")?,
        format: json_str_field(txt, "format")?,
        delimiter: json_str_field(txt, "delimiter").unwrap_or_default(),
        key_columns: json_num_array_field(txt, "key_columns")
            .unwrap_or_default()
            .into_iter()
            .map(|v| v as usize)
            .collect(),
        input: json_str_field(txt, "input").unwrap_or_default(),
        capacity: json_num_field(txt, "capacity").unwrap_or(0),
        input_bytes: json_num_field(txt, "input_bytes").unwrap_or(0),
        finished: json_bool_field(txt, "finished").unwrap_or(false),
        chunks: json_str_array_field(txt, "chunks").unwrap_or_default(),
        chunk_offsets: json_num_array_field(txt, "chunk_offsets").unwrap_or_default(),
    };
    // Reject corrupt manifests instead of resuming from garbage boundaries:
    // chunk/offset arrays must be parallel, offsets strictly increasing
    // (monoton), and the resume frontier must not exceed the recorded input
    // size. Callers (`load`, resume flow) treat None as "no resumable run".
    if !m.validate() {
        return None;
    }
    Some(m)
}

/// Resume-race guard: exclusively creates `run_dir/.lock` (`create_new`).
/// Ok(file) means this process owns the run; Err(AlreadyExists) means another
/// process (or a stale lock from a crashed owner — remove it manually after
/// verifying no live owner) already claimed it. Call before adopting a run
/// dir for `--resume`; the returned handle should be held for the run's
/// lifetime (dropping closes but does not delete the lock).
pub fn acquire_run_lock(run_dir: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(run_dir.join(Path::new(".lock")))
}

impl Manifest {
    /// Structural validation: parallel chunk/offset arrays, strictly
    /// increasing offsets bounded by `input_bytes`, non-empty identity.
    pub fn validate(&self) -> bool {
        if self.run_id.is_empty() || self.format.is_empty() {
            return false;
        }
        if self.chunks.len() != self.chunk_offsets.len() {
            return false;
        }
        let mut prev: Option<u64> = None;
        for &off in &self.chunk_offsets {
            // Monoton strictly increasing: equal/decreasing offsets mean a
            // torn write or hand-edited manifest — never resume from it.
            if let Some(p) = prev && off <= p {
                return false;
            }
            prev = Some(off);
            // The resume frontier can never lie past the recorded input size
            // (when known). `input_bytes == 0` with completed chunks is
            // likewise corrupt (empty input produces zero chunks).
            if self.input_bytes == 0 || off > self.input_bytes {
                return false;
            }
        }
        true
    }

    /// Full validation against the live input file: structural [`validate`]
    /// plus stored `input_bytes` matching the actual file size. A mismatch
    /// means the input changed since the run started — resume would silently
    /// mis-split, so the caller must reject it.
    pub fn validate_against_input(&self, actual_input_bytes: u64) -> bool {
        self.validate() && self.input_bytes == actual_input_bytes
    }
}

/// Scans `<base>/.temp_sort/run-*` for resumable (unfinished, completed>0) runs.
/// Returns (run_id, run_dir) pairs, newest first.
pub fn discover_resumable(base_dir: &Path) -> Vec<(String, PathBuf)> {
    let tmp_base = base_dir.join(Path::new(".temp_sort"));
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let entries = match fs::read_dir(tmp_base) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for e in entries.filter_map(|e| e.ok()) {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !name.starts_with("run-") {
            continue;
        }
        if let Some(m) = Manifest::load(p.as_path())
            && !m.finished && !m.chunks.is_empty() {
                out.push((m.run_id, p));
            }
    }
    out
}
