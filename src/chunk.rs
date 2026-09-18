// Split + sort phase: adaptive chunk sizing, streaming read, parallel in-memory sort,
// binary (numeric) or newline-delimited (string/csv) temp chunk files.
// v2: CSV/delimited formats with multi-key sort, crash-resume (skips completed
// chunks recorded in the run manifest), and graceful data errors (no panics).
use crate::io_buffer::ReadAhead;
use crate::manifest::Manifest;
use crate::progress::RunState;
use crate::temp_manager;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use rayon::slice::ParallelSliceMut;

/// What we sort. Numeric and string are whole-line formats; Csv keeps the full
/// record and compares only the selected key columns; Jsonl keeps the raw line
/// and compares one nested field (dot notation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SortFormat {
    Numeric,
    String,
    Csv { delimiter: u8, keys: Vec<usize>, key_numeric: bool },
    Jsonl { field: Vec<String>, key_numeric: bool },
}

impl SortFormat {
    /// Stable name used in the manifest and summaries.
    pub fn kind(&self) -> &'static str {
        match self {
            SortFormat::Numeric => "numeric",
            SortFormat::String => "string",
            SortFormat::Csv { .. } => "csv",
            SortFormat::Jsonl { .. } => "jsonl",
        }
    }

    /// Legacy `--mode` parsing (numeric|string|jsonl|excel); CSV is configured separately.
    pub fn parse_mode(s: &str) -> Option<SortFormat> {
        match s {
            "numeric" => Some(SortFormat::Numeric),
            "string" => Some(SortFormat::String),
            // Bare jsonl without field: validated later (needs --key-field).
            "jsonl" => Some(SortFormat::Jsonl { field: Vec::new(), key_numeric: false }),
            // Excel = CSV semantics after xlsx->csv conversion (needs --key-column).
            "excel" => Some(SortFormat::Csv { delimiter: b',', keys: Vec::new(), key_numeric: false }),
            _ => None,
        }
    }
}

/// Extracts a nested JSON field (dot path) from one raw JSONL line.
/// Returns the key for ordering. Numbers compare numerically when key_numeric.
pub fn extract_json_key(
    rec: &[u8],
    field: &[String],
    key_numeric: bool,
) -> Result<Vec<CsvKey>, String> {
    let v: serde_json::Value = serde_json::from_slice(rec)
        .map_err(|e| format!("invalid JSON line: {}", e))?;
    let mut cur = &v;
    for part in field {
        cur = cur
            .get(part)
            .ok_or_else(|| format!("JSON field '{}' missing in line", field.join(".")))?;
    }
    if key_numeric {
        let n = if let Some(n) = cur.as_u64() {
            n
        } else if let Some(n) = cur.as_i64() {
            if n < 0 {
                return Err(format!("JSON field '{}' negative, u64 only", field.join(".")));
            }
            n as u64
        } else if let Some(s) = cur.as_str() {
            s.trim().parse::<u64>().map_err(|_| format!("JSON field '{}' is not numeric", field.join(".")))?
        } else {
            return Err(format!("JSON field '{}' is not numeric", field.join(".")));
        };
        Ok(vec![CsvKey::N(n)])
    } else {
        let s = if let Some(s) = cur.as_str() {
            s.to_string()
        } else if cur.is_number() || cur.is_boolean() {
            cur.to_string()
        } else if cur.is_null() {
            String::new()
        } else {
            // objects/arrays: canonical JSON string compare
            cur.to_string()
        };
        Ok(vec![CsvKey::S(s)])
    }
}

/// One sort key extracted from a CSV record. Numeric keys compare as u64
/// (semantic ordering: 9 < 10), string keys lexicographically.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone)]
pub enum CsvKey {
    N(u64),
    S(String),
}

/// Extracts the key columns from one raw CSV record.
/// `keys` must be sorted ascending (validated by the CLI layer).
pub fn extract_csv_keys(
    rec: &[u8],
    delimiter: u8,
    keys: &[usize],
    key_numeric: bool,
) -> Result<Vec<CsvKey>, String> {
    let mut out = Vec::with_capacity(keys.len());
    // Keys are validated ascending by the CLI layer, so one pass over the
    // record's fields collects them in order. splitn(limit) means the last
    // split takes the remainder — fine, because we stop at the largest key.
    let max_key = keys.iter().copied().max().unwrap_or(0);
    let mut fields = rec.splitn(max_key + 2, |&b| b == delimiter);
    let mut idx = 0usize;
    for &k in keys {
        while idx < k {
            if fields.next().is_none() {
                return Err(format!(
                    "CSV record has {} fields but key column {} was requested",
                    idx, k
                ));
            }
            idx += 1;
        }
        match fields.next() {
            Some(field) => {
                let f = std::str::from_utf8(field).unwrap_or("");
                if key_numeric {
                    match f.trim().parse::<u64>() {
                        Ok(v) => out.push(CsvKey::N(v)),
                        Err(_) => {
                            return Err(format!(
                                "CSV key column {} is not numeric: {:?}",
                                k, f
                            ))
                        }
                    }
                } else {
                    out.push(CsvKey::S(f.to_string()));
                }
                idx += 1;
            }
            None => {
                return Err(format!(
                    "CSV record has {} fields but key column {} was requested",
                    idx, k
                ))
            }
        }
    }
    Ok(out)
}

/// Per-element memory estimate used to derive chunk capacity.
pub fn elem_overhead(fmt: &SortFormat) -> usize {
    match fmt {
        SortFormat::Numeric => 8, // u64 payload
        SortFormat::String => std::mem::size_of::<String>() + 32, // 24B header + est. payload + slack
        SortFormat::Csv { .. } | SortFormat::Jsonl { .. } => {
            // raw record Vec (24B) + two key Strings (2x24B) + est. payload slack
            3 * std::mem::size_of::<Vec<u8>>() + 24
        }
    }
}

/// Computes how many elements fit in `max_memory` bytes after reserving margins.
///
/// Note: the budget governs the engine's own allocations. Process RSS as reported
/// by sysinfo also includes the OS/runtime baseline (binary, thread stacks),
/// which is why a baseline floor is subtracted here.
pub fn chunk_capacity(max_memory: usize, fmt: &SortFormat, threads: usize) -> usize {
    // Process baseline: binary + std runtime + rayon pool stacks (approximate).
    const PROCESS_BASELINE: usize = 8 * 1024 * 1024;
    // Reservations: writer buffer + read-ahead blocks + sampler + misc structures.
    let fixed_overhead: usize = 4 * 1024 * 1024;
    // Per-thread stack & allocator arenas margin (rough, 1MB per thread beyond baseline).
    let per_thread: usize = threads.saturating_sub(1).saturating_mul(1024 * 1024);
    // Fragmentation margin: 15%.
    let budget = max_memory
        .saturating_sub(PROCESS_BASELINE)
        .saturating_sub(fixed_overhead)
        .saturating_sub(per_thread);
    let budget = (budget as f64 * 0.85) as usize;
    let overhead = elem_overhead(fmt);
    (budget / overhead).max(1024)
}

pub struct SplitResult {
    pub chunk_paths: Vec<PathBuf>,
    pub total_lines: usize,
    pub read_secs: f64,
    pub sort_secs: f64,
}

/// Reads input in streaming blocks, splits into records, accumulates up to
/// `capacity` records, sorts in parallel, and writes chunks.
///
/// Resume support: when `skip_chunks > 0`, the first `skip_chunks` chunk
/// boundaries of the input are consumed (for progress/line accounting) but not
/// re-sorted; the already-sorted files recorded in the manifest are adopted.
/// `manifest`, if present, is updated atomically after every newly completed
/// chunk so a crash never loses more than one chunk of work.
#[allow(clippy::too_many_arguments)]
pub fn split_and_sort(
    input: &Path,
    run_dir: &Path,
    fmt: SortFormat,
    capacity: usize,
    writer_buf_size: usize,
    resume_from_offset: u64,
    manifest: &mut Option<Manifest>,
    state: Option<&Arc<RunState>>,
    reverse: bool,
    skip_first_line: bool,
) -> std::io::Result<SplitResult> {
    let mut ctx = SplitCtx {
        fmt,
        input: input.to_path_buf(),
        run_dir: run_dir.to_path_buf(),
        capacity,
        writer_buf_size,
        resume_from_offset,
        reverse,
        skip_first_line,
        lines_seen: 0,
        bytes_before_block: 0,
        manifest,
        state: state.cloned(),
        chunk_paths: Vec::new(),
        chunk_index: 0,
        cur_strings: Vec::new(),
        cur_nums: Vec::new(),
        cur_csv: Vec::new(),
        cur_bytes: 0,
        total_lines: 0,
        read_dur: Duration::ZERO,
        sort_dur: Duration::ZERO,
        write_dur: Duration::ZERO,
    };
    ctx.run()?;
    Ok(SplitResult {
        chunk_paths: ctx.chunk_paths,
        total_lines: ctx.total_lines,
        read_secs: ctx.read_dur.as_secs_f64(),
        sort_secs: ctx.sort_dur.as_secs_f64(),
    })
}

struct SplitCtx<'a> {
    fmt: SortFormat,
    input: PathBuf,
    run_dir: PathBuf,
    capacity: usize,
    writer_buf_size: usize,
    /// Input byte offset where live accumulation starts (resume). Everything
    /// before it was already written to adopted chunks in a previous run.
    resume_from_offset: u64,
    reverse: bool,
    skip_first_line: bool,
    lines_seen: u64,
    /// Bytes consumed from the input so far (block-granular). Also the exact
    /// end-offset recorded with each completed chunk.
    bytes_before_block: u64,
    manifest: &'a mut Option<Manifest>,
    state: Option<Arc<RunState>>,
    chunk_paths: Vec<PathBuf>,
    chunk_index: usize,
    cur_strings: Vec<String>,
    cur_nums: Vec<u64>,
    cur_csv: Vec<Vec<u8>>,
    cur_bytes: usize,
    total_lines: usize,
    read_dur: Duration,
    sort_dur: Duration,
    write_dur: Duration,
}

impl<'a> SplitCtx<'a> {
    /// Resume: adopt every completed chunk recorded in the manifest. Their
    /// sorted files stay in the run dir and precede the newly created chunks
    /// in `chunk_paths`, which is exactly the order the merge needs.
    fn adopt_manifest_chunks(&mut self) {
        if let Some(m) = self.manifest {
            for name in m.chunks.iter() {
                self.chunk_paths.push(self.run_dir.join(Path::new(name)));
            }
            self.chunk_index = m.chunks.len();
        }
        if let Some(s) = &self.state {
            s.chunks_done.store(self.chunk_paths.len() as u64, Ordering::Relaxed);
        }
    }

    fn run(&mut self) -> std::io::Result<()> {
        if self.resume_from_offset > 0 {
            self.adopt_manifest_chunks();
        }
        let mut reader = ReadAhead::open(&self.input, 256 * 1024, 4)?;
        let cap_bytes = self.capacity.saturating_mul(elem_overhead(&self.fmt));
        let mut carry: Vec<u8> = Vec::new();

        loop {
            let t0 = Instant::now();
            let block = match reader.next_block() {
                Some(Ok(b)) => b,
                Some(Err(e)) => return Err(e),
                None => break,
            };
            self.read_dur += t0.elapsed();

            if matches!(self.fmt, SortFormat::Numeric) {
                let data: Vec<u8> = if carry.is_empty() {
                    block
                } else {
                    let mut joined = std::mem::take(&mut carry);
                    joined.extend_from_slice(&block);
                    joined
                };
                let whole = data.len() - data.len() % 8;
                for (ri, chunk8) in data[..whole].as_chunks::<8>().0.iter().enumerate() {
                    // Records ending at or before the resume offset were
                    // already sorted into adopted chunks: count them, don't
                    // accumulate. (The offset is a multiple of 8 in numeric
                    // mode, so a record never straddles it.)
                    let rec_end = self.bytes_before_block + ((ri + 1) * 8) as u64;
                    if rec_end <= self.resume_from_offset {
                        continue;
                    }
                    let mut b = [0u8; 8];
                    b.copy_from_slice(chunk8);
                    self.cur_nums.push(u64::from_le_bytes(b));
                }
                self.total_lines += whole / 8;
                carry = data[whole..].to_vec();
                // Advance by fully-consumed bytes only: the trailing partial
                // record (carry) is re-prepended to the next block and must
                // not be counted twice.
                self.bytes_before_block += whole as u64;
                if let Some(s) = &self.state {
                    // Progress counts raw input bytes consumed (whole records
                    // only, so the denominator matches exactly).
                    s.input_bytes_read.fetch_add(whole as u64, Ordering::Relaxed);
                }
                reader.recycle(data);
            } else {
                // Line-based formats (string, csv).
                let data: Vec<u8> = if carry.is_empty() {
                    block
                } else {
                    let mut joined = std::mem::take(&mut carry);
                    joined.extend_from_slice(&block);
                    joined
                };
                let mut start = 0usize;
                let len = data.len();
                for i in 0..len {
                    if data[i] == b'\n' {
                        let mut line = &data[start..i];
                        if line.last() == Some(&b'\r') {
                            line = &line[..line.len() - 1];
                        }
                        // A line is in the fast-forward region when it ends at
                        // or before the resume offset (which always sits on a
                        // line boundary).
                        let line_end = self.bytes_before_block + (i as u64) + 1;
                        self.push_line(line, line_end <= self.resume_from_offset)?;
                        start = i + 1;
                    }
                }
                // Carry the trailing partial line.
                carry = data[start..].to_vec();
                // Advance only past complete lines; the partial tail is
                // re-prepended to the next block.
                self.bytes_before_block += start as u64;
                if let Some(s) = &self.state {
                    // Count consumed bytes including the partial-line carry so
                    // the bar reaches 100% exactly at EOF.
                    s.input_bytes_read.fetch_add(len as u64, Ordering::Relaxed);
                }
                reader.recycle(data);
            }

            // Flush a chunk when full (only while actually accumulating).
            let full = match self.fmt {
                SortFormat::Numeric => self.cur_nums.len() >= self.capacity,
                SortFormat::String => {
                    self.cur_strings.len() >= self.capacity || self.cur_bytes >= cap_bytes
                }
                SortFormat::Csv { .. } | SortFormat::Jsonl { .. } => {
                    self.cur_csv.len() >= self.capacity || self.cur_bytes >= cap_bytes
                }
            };
            if full {
                self.flush_chunk()?;
            }
        }

        // Trailing carry may hold the final partial record/line.
        if !carry.is_empty() {
            if matches!(self.fmt, SortFormat::Numeric) {
                if carry.len() != 8 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "numeric input has trailing {} bytes that are not a complete 8-byte record",
                            carry.len()
                        ),
                    ));
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&carry);
                // The record occupies [bytes_before_block, +8). Skip it only
                // when it lies entirely inside the fast-forward region.
                if self.bytes_before_block >= self.resume_from_offset {
                    self.cur_nums.push(u64::from_le_bytes(b));
                }
                self.total_lines += 1;
                if let Some(s) = &self.state {
                    s.input_bytes_read.fetch_add(8, Ordering::Relaxed);
                }
            } else {
                // The carry ends exactly at the current byte position; it is
                // fast-forwarded only when it lies wholly before the offset.
                let ff = self.bytes_before_block + carry.len() as u64 <= self.resume_from_offset;
                let mut line = &carry[..];
                if line.last() == Some(&b'\r') {
                    line = &line[..line.len() - 1];
                }
                self.push_line(line, ff)?;
                if let Some(s) = &self.state {
                    s.input_bytes_read.fetch_add(carry.len() as u64, Ordering::Relaxed);
                }
            }
        }

        // Flush the final (possibly partial) chunk.
        let nonempty = match self.fmt {
            SortFormat::Numeric => !self.cur_nums.is_empty(),
            SortFormat::String => !self.cur_strings.is_empty(),
            SortFormat::Csv { .. } | SortFormat::Jsonl { .. } => !self.cur_csv.is_empty(),
        };
        if nonempty {
            self.flush_chunk()?;
        }

        if let Some(s) = &self.state {
            s.records.store(self.total_lines as u64, Ordering::Relaxed);
            s.chunks_done.store(self.chunk_paths.len() as u64, Ordering::Relaxed);
        }
        Ok(())
    }

    fn push_line(&mut self, line: &[u8], fast_forward: bool) -> std::io::Result<()> {
        // --header: first non-empty line of the file is metadata, never a record.
        // Skipped on both fresh and resume runs (resume re-reads from byte 0).
        if self.skip_first_line && self.lines_seen == 0 {
            self.lines_seen += 1;
            return Ok(());
        }
        self.lines_seen += 1;
        // Blank lines are never records (consistent with v1 counting rules).
        if line.is_empty() && !matches!(self.fmt, SortFormat::Numeric) {
            return Ok(());
        }
        // Fast-forward region (resume): count only; the record already lives
        // in an adopted chunk from the previous run.
        if fast_forward {
            if !line.is_empty() || matches!(self.fmt, SortFormat::Numeric) {
                self.total_lines += 1;
            }
            return Ok(());
        }
        match &self.fmt {
            SortFormat::Numeric => {
                let text = std::str::from_utf8(line).ok();
                match text.and_then(|t| t.trim().parse::<u64>().ok()) {
                    Some(v) => {
                        self.cur_nums.push(v);
                        self.total_lines += 1;
                    }
                    None => {
                        // Tolerate blank lines; anything else is a data error.
                        let s = String::from_utf8_lossy(line);
                        if !s.trim().is_empty() {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("invalid numeric line: {}", s),
                            ));
                        }
                    }
                }
            }
            SortFormat::String => {
                let s = String::from_utf8_lossy(line).into_owned();
                self.cur_bytes += s.len() + std::mem::size_of::<String>();
                self.cur_strings.push(s);
                self.total_lines += 1;
            }
            SortFormat::Csv { delimiter, keys, key_numeric } => {
                let extracted = extract_csv_keys(line, *delimiter, keys, *key_numeric)
                    .map_err(|m| std::io::Error::new(std::io::ErrorKind::InvalidData, m))?;
                // Store the raw record; keys are re-extracted (and the record
                // sorted) at flush time so temp files stay plain newline text.
                self.cur_bytes += line.len() + 2 * std::mem::size_of::<String>() + 24;
                self.cur_csv.push(line.to_vec());
                let _ = extracted;
                self.total_lines += 1;
            }
            SortFormat::Jsonl { field, key_numeric } => {
                let extracted = extract_json_key(line, field, *key_numeric)
                    .map_err(|m| std::io::Error::new(std::io::ErrorKind::InvalidData, m))?;
                self.cur_bytes += line.len() + 2 * std::mem::size_of::<String>() + 24;
                self.cur_csv.push(line.to_vec());
                let _ = extracted;
                self.total_lines += 1;
            }
        }
        Ok(())
    }

    /// Sorts + writes the current accumulation as the next chunk and records
    /// it (name + exact input end-offset) in the manifest atomically.
    fn flush_chunk(&mut self) -> std::io::Result<()> {
        let t0 = Instant::now();
        let rev = self.reverse;
        match self.fmt {
            SortFormat::Numeric => {
                if rev {
                    self.cur_nums.par_sort_unstable_by(|a, b| b.cmp(a));
                } else {
                    self.cur_nums.par_sort_unstable();
                }
            }
            SortFormat::String => {
                if rev {
                    self.cur_strings.par_sort_unstable_by(|a, b| b.cmp(a));
                } else {
                    self.cur_strings.par_sort_unstable();
                }
            }
            SortFormat::Csv { .. } | SortFormat::Jsonl { .. } => {} // keys extracted per record below
        }
        // For CSV/JSONL we sort records by their extracted keys now; storing
        // sorted order avoids re-sorting during merge and keeps merge simple.
        if matches!(self.fmt, SortFormat::Csv { .. }) {
            let delimiter = match &self.fmt {
                SortFormat::Csv { delimiter, keys, key_numeric } => (*delimiter, keys.clone(), *key_numeric),
                _ => unreachable!(),
            };
            let mut rows: Vec<(Vec<CsvKey>, Vec<u8>)> = Vec::with_capacity(self.cur_csv.len());
            for rec in self.cur_csv.drain(..) {
                let k = extract_csv_keys(&rec, delimiter.0, &delimiter.1, delimiter.2)
                    .map_err(|m| std::io::Error::new(std::io::ErrorKind::InvalidData, m))?;
                rows.push((k, rec));
            }
            if rev {
                rows.sort_unstable_by(|a, b| b.0.cmp(&a.0));
            } else {
                rows.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            }
            self.cur_csv = rows.into_iter().map(|(_, r)| r).collect();
        }
        if matches!(self.fmt, SortFormat::Jsonl { .. }) {
            let (field, kn) = match &self.fmt {
                SortFormat::Jsonl { field, key_numeric } => (field.clone(), *key_numeric),
                _ => unreachable!(),
            };
            let mut rows: Vec<(Vec<CsvKey>, Vec<u8>)> = Vec::with_capacity(self.cur_csv.len());
            for rec in self.cur_csv.drain(..) {
                let k = extract_json_key(&rec, &field, kn)
                    .map_err(|m| std::io::Error::new(std::io::ErrorKind::InvalidData, m))?;
                rows.push((k, rec));
            }
            if rev {
                rows.sort_unstable_by(|a, b| b.0.cmp(&a.0));
            } else {
                rows.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            }
            self.cur_csv = rows.into_iter().map(|(_, r)| r).collect();
        }
        self.sort_dur += t0.elapsed();

        let t1 = Instant::now();
        let (path, file) = temp_manager::create_chunk_file(&self.run_dir, self.chunk_index)?;
        let mut w = std::io::BufWriter::with_capacity(self.writer_buf_size, file);
        match self.fmt {
            SortFormat::Numeric => {
                for v in self.cur_nums.iter() {
                    w.write_all(&v.to_le_bytes())?;
                }
                self.cur_nums.clear();
            }
            SortFormat::String => {
                for s in self.cur_strings.iter() {
                    w.write_all(s.as_bytes())?;
                    w.write_all(b"\n")?;
                }
                self.cur_strings.clear();
            }
            SortFormat::Csv { .. } | SortFormat::Jsonl { .. } => {
                for rec in self.cur_csv.iter() {
                    w.write_all(rec)?;
                    w.write_all(b"\n")?;
                }
                self.cur_csv.clear();
            }
        }
        w.flush()?;
        // Get the File back and fsync so a completed chunk survives a crash.
        let inner = w.into_inner().map_err(|e| std::io::Error::other(e.to_string()))?;
        inner.sync_all().ok();
        self.write_dur += t1.elapsed();

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.chunk_paths.push(path);
        self.chunk_index += 1;
        self.cur_bytes = 0;

        // Persist progress: crash after this point resumes without redoing work.
        if let Some(m) = self.manifest {
            m.chunks.push(file_name);
            m.chunk_offsets.push(self.bytes_before_block);
            m.save_atomic(&self.run_dir)?;
        }
        if let Some(s) = &self.state {
            s.chunks_done.store(self.chunk_paths.len() as u64, Ordering::Relaxed);
        }
        Ok(())
    }
}
