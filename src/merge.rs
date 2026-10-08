// K-way merge phase: BinaryHeap over per-file MergeReaders, multi-pass when
// the chunk count exceeds the open-file budget, byte accounting for metrics.
// v2: CSV formats (key-column comparison) and resume-aware temp retention.
use crate::chunk::{extract_csv_keys, extract_json_key, CsvKey, SortFormat, SortOpts};
use crate::manifest::Manifest;
use crate::progress::RunState;
use crate::temp_manager::remove_file_quiet;
use std::cell::RefCell;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

thread_local! {
    static MERGE_DESC: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    static MERGE_NULLS_LAST: RefCell<bool> = const { RefCell::new(false) };
}

/// Set per-run merge comparator (arah per-key + nulls-last).
/// Heap (BinaryHeap) butuh Ord statis; opsi dinamis disalurkan via
/// thread-local di atas yang diset di awal tiap merge_batch
/// (merge heap single-threaded).
fn set_merge_cmp(desc: &[bool], nulls_last: bool) {
    MERGE_DESC.with(|d| *d.borrow_mut() = desc.to_vec());
    MERGE_NULLS_LAST.with(|n| *n.borrow_mut() = nulls_last);
}

#[derive(PartialEq, Eq, Debug)]
enum MergeValue {
    Num(u64),
    S(String),
    /// CSV: comparison key (possibly multi-column) + source record id.
    /// The record bytes are re-read from the source chunk at emit time.
    Csv(Vec<CsvKey>),
}

impl PartialOrd for MergeValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MergeValue {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (MergeValue::Csv(a), MergeValue::Csv(b)) => {
                let nulls_last = MERGE_NULLS_LAST.with(|n| *n.borrow());
                MERGE_DESC.with(|d| SortOpts::cmp_keys(a, b, &d.borrow(), nulls_last))
            }
            _ => merge_value_ord(self, other),
        }
    }
}

/// Natural order untuk non-Csv (Num/S) — arah global via heap (Reverse wrapper),
/// bukan di sini. Dipisah agar Ord manual di atas tetap jelas.
fn merge_value_ord(a: &MergeValue, b: &MergeValue) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (MergeValue::Num(x), MergeValue::Num(y)) => x.cmp(y),
        (MergeValue::S(x), MergeValue::S(y)) => x.cmp(y),
        (MergeValue::Csv(x), MergeValue::Csv(y)) => x.cmp(y),
        // Urutan varian tetap: Num < S < Csv (tak pernah campur dalam satu run).
        (MergeValue::Num(_), _) => Ordering::Less,
        (_, MergeValue::Num(_)) => Ordering::Greater,
        (MergeValue::S(_), _) => Ordering::Less,
        (_, MergeValue::S(_)) => Ordering::Greater,
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
struct HeapItem {
    value: MergeValue,
    source: usize,
}

/// Buffered reader over one temp chunk, yielding values one at a time.
/// Numeric mode serves u64s from a refillable byte block; string mode uses
/// read_until into a reusable line buffer. No per-record allocation.
struct MergeReader {
    inner: BufReader<File>,
    fmt: SortFormat,
    opts: SortOpts,
    rec_buf: Vec<u8>,
    num_block: Vec<u8>,
    num_pos: usize,
    eof: bool,
}

const NUM_BLOCK_BYTES: usize = 64 * 1024;

impl MergeReader {
    #[allow(dead_code)]
    fn open(path: &Path, fmt: SortFormat, buf_size: usize) -> std::io::Result<MergeReader> {
        Self::open_skip_header(path, fmt, SortOpts::default(), buf_size, false)
    }

    /// Opens a chunk with optional first-line skip (used by --merge for
    /// header rows in files[1..] without copying data).
    fn open_skip_header(path: &Path, fmt: SortFormat, opts: SortOpts, buf_size: usize, skip_first: bool) -> std::io::Result<MergeReader> {
        let f = File::open(path)?;
        let mut inner = BufReader::with_capacity(buf_size, f);
        if skip_first {
            let mut junk = Vec::new();
            let _ = inner.read_until(b'\n', &mut junk);
        }
        Ok(MergeReader {
            inner,
            fmt,
            opts,
            rec_buf: Vec::with_capacity(256),
            num_block: Vec::new(),
            num_pos: 0,
            eof: false,
        })
    }

    fn refill_num_block(&mut self) -> std::io::Result<bool> {
        // NOTE: this std's read(&mut Vec) fills up to the Vec's current len,
        // so resize first and truncate to the bytes actually read.
        self.num_block.clear();
        self.num_block.resize(NUM_BLOCK_BYTES, 0);
        let mut filled = 0usize;
        while filled < NUM_BLOCK_BYTES {
            match self.inner.read(&mut self.num_block[filled..])? {
                0 => break,
                n => filled += n,
            }
        }
        self.num_block.truncate(filled);
        self.num_pos = 0;
        Ok(filled > 0)
    }

    /// Reads one line into `self.rec_buf` (without terminators); empty result at EOF.
    /// Returns Err on read failure.
    /// CSV multiline: physical lines digabung dengan `\n` sampai quotes balance,
    /// aturan sama dengan split phase (`chunk::quotes_balanced`).
    fn read_line_into_rec(&mut self) -> std::io::Result<usize> {
        self.rec_buf.clear();
        let n = self.inner.read_until(b'\n', &mut self.rec_buf)?;
        if n == 0 {
            return Ok(0);
        }
        let mut end = self.rec_buf.len();
        if self.rec_buf.last() == Some(&b'\n') {
            end -= 1;
        }
        if end > 0 && self.rec_buf[end - 1] == b'\r' {
            end -= 1;
        }
        self.rec_buf.truncate(end);
        if matches!(self.fmt, SortFormat::Csv { .. }) {
            while !crate::chunk::quotes_balanced(&self.rec_buf) {
                let mut extra = Vec::new();
                let m = self.inner.read_until(b'\n', &mut extra)?;
                if m == 0 {
                    break;
                }
                let mut eend = extra.len();
                if extra.last() == Some(&b'\n') {
                    eend -= 1;
                }
                if eend > 0 && extra[eend - 1] == b'\r' {
                    eend -= 1;
                }
                self.rec_buf.push(b'\n');
                self.rec_buf.extend_from_slice(&extra[..eend]);
            }
            return Ok(self.rec_buf.len());
        }
        Ok(end)
    }

    fn next(&mut self) -> std::io::Result<Option<MergeValue>> {
        // Borrowed dispatch: the old code cloned `self.fmt` on every record
        // (a Vec clone per row for Csv/Jsonl key lists). `kind()` returns a
        // 'static str so its borrow ends immediately; each arm re-borrows
        // `&self.fmt` only after the mutable `read_line_into_rec` call,
        // keeping the borrow checker happy with zero per-record allocation.
        match self.fmt.kind() {
            "numeric" => {
                if self.num_pos + 8 > self.num_block.len()
                    && (self.eof || !self.refill_num_block()?) {
                        self.eof = true;
                        return Ok(None);
                    }
                if self.num_pos + 8 > self.num_block.len() {
                    // Partial record at EOF => corrupted temp file.
                    return Err(std::io::Error::other(format!(
                        "corrupted numeric chunk: trailing {} bytes is not a full 8-byte record",
                        self.num_block.len() - self.num_pos
                    )));
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&self.num_block[self.num_pos..self.num_pos + 8]);
                self.num_pos += 8;
                Ok(Some(MergeValue::Num(u64::from_le_bytes(b))))
            }
            "string" => {
                let n = self.read_line_into_rec()?;
                if n == 0 && self.rec_buf.is_empty() {
                    return Ok(None);
                }
                let s = String::from_utf8_lossy(&self.rec_buf).into_owned();
                Ok(Some(MergeValue::S(if self.opts.ignore_case { s.to_lowercase() } else { s })))
            }
            "csv" => {
                let n = self.read_line_into_rec()?;
                if n == 0 && self.rec_buf.is_empty() {
                    return Ok(None);
                }
                let (delimiter, keys, key_numeric) = match &self.fmt {
                    SortFormat::Csv { delimiter, keys, key_numeric } => (*delimiter, keys, *key_numeric),
                    _ => unreachable!(),
                };
                let k = extract_csv_keys(&self.rec_buf, delimiter, keys, key_numeric, self.opts.ignore_case)
                    .map_err(|m| std::io::Error::new(std::io::ErrorKind::InvalidData, m))?;
                Ok(Some(MergeValue::Csv(k)))
            }
            _ => {
                // jsonl
                let n = self.read_line_into_rec()?;
                if n == 0 && self.rec_buf.is_empty() {
                    return Ok(None);
                }
                let (field, key_numeric) = match &self.fmt {
                    SortFormat::Jsonl { field, key_numeric } => (field, *key_numeric),
                    _ => unreachable!(),
                };
                let k = extract_json_key(&self.rec_buf, field, key_numeric, self.opts.ignore_case)
                    .map_err(|m| std::io::Error::new(std::io::ErrorKind::InvalidData, m))?;
                Ok(Some(MergeValue::Csv(k)))
            }
        }
    }
}

pub struct MergeStats {
    pub io_secs: f64,
    pub bytes_read: usize,
}

/// Merges sorted chunk files into `output`, running multi-pass when
/// `fan_in` < number of chunks. Intermediate files are deleted as consumed.
///
/// `keep_temps` (resume mode) preserves the original chunk files: the final
/// result is copied into place instead of renamed, so an interrupted merge can
/// be retried from the same chunks.
/// `dedupe_by` (`--dedupe-by`) collapses rows with equal key-column values on
/// the final pass only. keep-first uses a hash set (also collapses scattered
/// duplicates) — unbounded in key cardinality, warns past 1M keys, never
/// evicted (eviction would silently resurrect duplicates); keep-last keeps
/// the last row of each *consecutive* equal-key group — sort by the dedupe
/// column for exact keep-last semantics.
#[allow(clippy::too_many_arguments)]
pub fn merge_all(
    chunk_paths: Vec<PathBuf>,
    output: &Path,
    fmt: SortFormat,
    fan_in: usize,
    run_dir: &Path,
    reader_buf_size: usize,
    writer_buf_size: usize,
    keep_temps: bool,
    manifest: &mut Option<Manifest>,
    state: Option<&Arc<RunState>>,
    reverse: bool,
    unique: bool,
    dedupe_by: Option<Vec<usize>>,
    dedupe_keep_last: bool,
    opts: SortOpts,
) -> std::io::Result<MergeStats> {
    merge_all_inner(chunk_paths, output, fmt, fan_in, run_dir, reader_buf_size, writer_buf_size, keep_temps, manifest, state, reverse, unique, dedupe_by, dedupe_keep_last, &[], opts)
}

/// Public entry for --merge mode (pre-sorted files, no manifest/resume).
#[allow(clippy::too_many_arguments)]
pub fn merge_all_inner_public(
    chunk_paths: Vec<PathBuf>,
    output: &Path,
    fmt: SortFormat,
    fan_in: usize,
    run_dir: &Path,
    reader_buf_size: usize,
    writer_buf_size: usize,
    manifest: &mut Option<Manifest>,
    state: Option<&Arc<RunState>>,
    reverse: bool,
    unique: bool,
    dedupe_by: Option<Vec<usize>>,
    dedupe_keep_last: bool,
    skip_first: &[bool],
    opts: SortOpts,
) -> std::io::Result<MergeStats> {
    merge_all_inner(chunk_paths, output, fmt, fan_in, run_dir, reader_buf_size, writer_buf_size, false, manifest, state, reverse, unique, dedupe_by, dedupe_keep_last, skip_first, opts)
}

/// Inner merge with per-file header skips (used by --merge for headers in
/// files[1..]; `skip_first[i]` drops the first line of batch file i).
/// Intermediate merge files never carry headers, so skips apply to pass 0 only.
#[allow(clippy::too_many_arguments)]
fn merge_all_inner(
    chunk_paths: Vec<PathBuf>,
    output: &Path,
    fmt: SortFormat,
    fan_in: usize,
    run_dir: &Path,
    reader_buf_size: usize,
    writer_buf_size: usize,
    keep_temps: bool,
    manifest: &mut Option<Manifest>,
    state: Option<&Arc<RunState>>,
    reverse: bool,
    unique: bool,
    dedupe_by: Option<Vec<usize>>,
    dedupe_keep_last: bool,
    skip_first: &[bool],
    opts: SortOpts,
) -> std::io::Result<MergeStats> {
    let mut io_dur = Duration::ZERO;
    let mut bytes_read: usize = 0;

    let mut current: Vec<PathBuf> = chunk_paths;
    let fan_in = fan_in.max(2);

    // Total element estimate: record count from the split phase. Intermediate
    // passes re-merge the same elements, so work multiplies by ~passes; we
    // report element-merges done against that total.
    let est_total = state
        .map(|s| s.records.load(Ordering::Relaxed))
        .unwrap_or(0)
        .max(1);

    // Estimate merge passes: log_fan_in(chunk_count).
    let est_passes = est_merge_passes(current.len(), fan_in) as u64;
    let merge_work_total = est_total.saturating_mul(est_passes.max(1));

    // Estimated bytes moved per element (read + write) for live I/O reporting;
    // replaced by the exact total at the end.
    let total_chunk_bytes: u64 = current
        .iter()
        .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();
    let io_per_rec: u64 = match (&fmt, state) {
        (SortFormat::Numeric, _) => 16, // 8B read + 8B write
        (_, Some(_)) => {
            let rec = total_chunk_bytes / est_total;
            2 * rec + 2 // payload read + write, plus newline per direction
        }
        (_, None) => 0,
    };

    if let Some(s) = state {
        s.merge_total.store(merge_work_total, Ordering::Relaxed);
    }

    let mut pass: usize = 0;
    let needs_dedup_pass = unique || dedupe_by.is_some();
    // Single chunk + dedup needs a real pass instead of a fast rename.
    if current.len() == 1 && needs_dedup_pass {
        let t0 = Instant::now();
        let batch = current.clone();
        let skips = skip_first.to_vec();
        let bytes = merge_batch(&batch, output, &fmt, reader_buf_size, writer_buf_size, state, io_per_rec, reverse, true, dedupe_by.clone(), dedupe_keep_last, if skips.is_empty() { None } else { Some(skips) }, &opts)?;
        if !keep_temps {
            for p in &batch {
                remove_file_quiet(p);
            }
        }
        bytes_read += bytes;
        io_dur += t0.elapsed();
        if let Some(m) = manifest {
            m.finished = true;
            m.save_atomic(run_dir)?;
        }
        if let Some(s) = state {
            s.io_bytes.store(bytes_read as u64, Ordering::Relaxed);
            s.merge_done.store(s.merge_total.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        return Ok(MergeStats { io_secs: io_dur.as_secs_f64(), bytes_read });
    }
    loop {
        if current.len() <= 1 {
            // Single file left: move it into place (copy: output may be on another volume).
            if current.len() == 1 {
                let t0 = Instant::now();
                let src = &current[0];
                let src_len = std::fs::metadata(src).map(|m| m.len() as usize).unwrap_or(0);
                if keep_temps {
                    // Copy so the original chunk stays available for a re-run.
                    std::fs::copy(src, output)?;
                } else {
                    // Move into place (rename may fail across volumes; fall back to copy).
                    std::fs::rename(src, output)
                        .or_else(|_| std::fs::copy(src, output).map(|_| ()))?;
                    remove_file_quiet(src);
                }
                bytes_read += src_len;
                io_dur += t0.elapsed();
                if let Some(s) = state {
                    s.io_bytes.fetch_add(src_len as u64, Ordering::Relaxed);
                    s.merge_done.fetch_add(s.records.load(Ordering::Relaxed), Ordering::Relaxed);
                }
            }
            break;
        }

        let mut next: Vec<PathBuf> = Vec::new();
        let mut i = 0usize;
        let mut batch_idx = 0usize;
        while i < current.len() {
            let end = (i + fan_in).min(current.len());
            let batch = &current[i..end];
            let is_final_pass = end == current.len() && !keep_temps;

            let t0 = Instant::now();
            let bytes: usize = if is_final_pass && next.is_empty() {
                // Everything fits in one batch: write straight to the output.
                // Dedup only on the final pass (intermediates keep duplicates
                // so multi-pass counts stay correct).
                let single_shot = next.is_empty() && current.len() <= fan_in.max(2);
                let dedup_now = unique && single_shot;
                let by_now = if single_shot { dedupe_by.clone() } else { None };
                // Header skips only apply on pass 0 (original files).
                let skips = if pass == 0 && !skip_first.is_empty() {
                    Some(skip_first[i..end].to_vec())
                } else {
                    None
                };
                let bytes = merge_batch(batch, output, &fmt, reader_buf_size, writer_buf_size, state, io_per_rec, reverse, dedup_now, by_now, dedupe_keep_last, skips, &opts)?;
                if !keep_temps {
                    for p in batch {
                        remove_file_quiet(p);
                    }
                }
                bytes
            } else {
                let tmp = run_dir.join(Path::new(&format!("msort_merge_p{:02}_{:04}.dat", pass, batch_idx)));
                let skips = if pass == 0 && !skip_first.is_empty() {
                    Some(skip_first[i..end].to_vec())
                } else {
                    None
                };
                let bytes = merge_batch(batch, &tmp, &fmt, reader_buf_size, writer_buf_size, state, io_per_rec, reverse, false, None, false, skips, &opts)?;
                for p in batch {
                    // In keep_temps (resume) mode the original chunk_*.dat files
                    // must survive: the manifest still references them and a
                    // crashed merge must be retryable. Only derived
                    // intermediates (msort_merge_p*) may be reclaimed.
                    // NOTE: the `msort_merge_p` prefix is namespaced so a
                    // user-supplied --merge input named `merge_p*.dat` is
                    // never mistaken for an intermediate and deleted.
                    let is_intermediate = p
                        .file_name()
                        .map(|n| n.to_string_lossy().starts_with("msort_merge_p"))
                        .unwrap_or(false);
                    if !keep_temps || is_intermediate {
                        remove_file_quiet(p);
                    }
                }
                next.push(tmp);
                bytes
            };
            bytes_read += bytes;
            io_dur += t0.elapsed();
            if let Some(s) = state {
                s.merge_pass.store(pass as u64 + 1, Ordering::Relaxed);
            }

            i = end;
            batch_idx += 1;
        }
        current = next;
        pass += 1;
    }

    // Ensure the bars land at 100% with exact totals.
    if let Some(s) = state {
        s.io_bytes.store(bytes_read as u64, Ordering::Relaxed);
        s.merge_done.store(s.merge_total.load(Ordering::Relaxed), Ordering::Relaxed);
    }
    // Persist merge completion so a resume never re-adopts a finished run.
    if let Some(m) = manifest {
        m.finished = true;
        m.save_atomic(run_dir)?;
    }

    Ok(MergeStats { io_secs: io_dur.as_secs_f64(), bytes_read })
}

/// Number of passes needed to merge `files` files with fan-in `fan_in`.
fn est_merge_passes(files: usize, fan_in: usize) -> usize {
    if files <= 1 {
        return 0;
    }
    let mut passes = 0usize;
    let mut n = files;
    while n > 1 {
        n = n.div_ceil(fan_in);
        passes += 1;
    }
    passes
}

/// One K-way merge over `batch` (sorted files) into `dest`.
/// `reverse` selects a max-heap (descending); `dedup` skips identical
/// consecutive records (only enabled on the final pass for --unique);
/// `dedupe_by` collapses equal key-column values (final pass only);
/// `skips` drops the first line of selected batch files (--merge headers).
#[allow(clippy::too_many_arguments)]
fn merge_batch(
    batch: &[PathBuf],
    dest: &Path,
    fmt: &SortFormat,
    reader_buf_size: usize,
    writer_buf_size: usize,
    state: Option<&Arc<RunState>>,
    io_per_rec: u64,
    reverse: bool,
    dedup: bool,
    dedupe_by: Option<Vec<usize>>,
    dedupe_keep_last: bool,
    skips: Option<Vec<bool>>,
    opts: &SortOpts,
) -> std::io::Result<usize> {
    set_merge_cmp(&opts.key_desc, opts.nulls_last);
    let ignore_case = opts.ignore_case;
    let mut readers: Vec<MergeReader> = Vec::with_capacity(batch.len());
    let mut total_bytes: usize = 0;
    for (idx, p) in batch.iter().enumerate() {
        total_bytes += std::fs::metadata(p).map(|m| m.len() as usize).unwrap_or(0);
        let skip = skips.as_ref().and_then(|s| s.get(idx)).copied().unwrap_or(false);
        readers.push(MergeReader::open_skip_header(p, fmt.clone(), opts.clone(), reader_buf_size, skip)?);
    }

    let out_file = File::create(dest)?;
    let mut out = BufWriter::with_capacity(writer_buf_size, out_file);

    // Numeric mode: batch emitted values into a byte block to amortize
    // write_all calls (PRD §8.3: flush scheduled, not per row).
    let mut num_out: Vec<u8> = Vec::with_capacity(64 * 1024);
    // Flush progress to the shared state in bulk; an atomic add per record is
    // measurable at 100M records, one per 64K is not.
    let mut pending: u64 = 0;
    // --unique state: last emitted record (consecutive compare).
    let mut last_num: Option<u64> = None;
    let mut last_bytes: Vec<u8> = Vec::new();
    let mut has_last = false;
    // --dedupe-by state. keep-first: hash set (handles scattered dupes).
    // keep-last: consecutive-group buffering (sort by dedupe col for exactness).
    //
    // LIMIT NOTE: the keep-first `seen_keys` set holds one entry per distinct
    // key for the whole final pass — memory grows with key cardinality, not
    // input size. There is deliberately no LRU eviction: evicting a key would
    // re-emit a later scattered duplicate as "new" and corrupt results
    // silently. Instead we warn once past 1M keys so unexpected RSS growth is
    // attributable (mitigation: pre-aggregate, dedupe on fewer columns, or
    // add RAM). keep-last buffering is O(1) (one group) and unaffected.
    const DEDUPE_KEY_WARN: usize = 1_000_000;
    let mut seen_keys: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
    let mut dedupe_warned = false;
    let mut group_key: Vec<u8> = Vec::new();
    let mut group_has = false;
    let mut group_buf: Vec<u8> = Vec::new();
    let by_cols = dedupe_by.clone();
    let by_active = by_cols.is_some();

    // Dedupe key for the record currently buffered in readers[src] (csv only).
    // ignore_case: key di-lowercase agar --unique/--dedupe-by konsisten dengan sort.
    let key_of = |readers: &Vec<MergeReader>, src: usize, fmt: &SortFormat, cols: &Option<Vec<usize>>| -> Vec<u8> {
        let mut raw = match fmt {
            SortFormat::Csv { delimiter, .. } => match cols.as_ref() {
                Some(cs) => {
                    let rec = &readers[src].rec_buf;
                    let parts: Vec<&[u8]> = rec.split(|&b| b == *delimiter).collect();
                    let mut k = Vec::new();
                    for (i, c) in cs.iter().enumerate() {
                        if i > 0 {
                            k.push(0x1f);
                        }
                        if let Some(f) = parts.get(*c) {
                            k.extend_from_slice(f);
                        }
                    }
                    k
                }
                None => readers[src].rec_buf.clone(),
            },
            _ => readers[src].rec_buf.clone(),
        };
        if ignore_case {
            raw = String::from_utf8_lossy(&raw).to_lowercase().into_bytes();
        }
        raw
    };

    macro_rules! account {
        () => {{
            pending += 1;
            if pending >= 65536 {
                if let Some(s) = state {
                    s.merge_done.fetch_add(pending, Ordering::Relaxed);
                    s.io_bytes.fetch_add(pending.saturating_mul(io_per_rec), Ordering::Relaxed);
                }
                pending = 0;
            }
        }};
    }
    macro_rules! write_num {
        ($v:expr) => {{
            let le = ($v as u64).to_le_bytes();
            num_out.extend_from_slice(&le);
            if num_out.len() >= 64 * 1024 - 8 {
                out.write_all(&num_out)?;
                num_out.clear();
            }
        }};
    }

    // Emits one popped text record (rec still in readers[src].rec_buf).
    // `is_dup_unique` precomputed by caller for the whole-record check.
    macro_rules! emit_text {
        ($src:expr, $dup_unique:expr) => {{
            if $dup_unique {
                // pure --unique consecutive duplicate: skip everything
            } else if by_active {
                let k = key_of(&readers, $src, fmt, &by_cols);
                if dedupe_keep_last {
                    if group_has && k == group_key {
                        group_buf = readers[$src].rec_buf.clone();
                    } else {
                        if group_has {
                            out.write_all(&group_buf)?;
                            out.write_all(b"\n")?;
                        }
                        group_key = k;
                        group_buf = readers[$src].rec_buf.clone();
                        group_has = true;
                    }
                } else if seen_keys.contains(&k) {
                    // keep-first scattered duplicate: skip
                } else {
                    seen_keys.insert(k);
                    if !dedupe_warned && seen_keys.len() > DEDUPE_KEY_WARN {
                        dedupe_warned = true;
                        eprintln!(
                            "warning: --dedupe-by keep-first memegang {} kunci unik (tumbuh dengan kardinalitas; bukan LRU agar duplikat tersebar tidak muncul ulang)",
                            seen_keys.len()
                        );
                    }
                    out.write_all(&readers[$src].rec_buf)?;
                    out.write_all(b"\n")?;
                }
            } else {
                out.write_all(&readers[$src].rec_buf)?;
                out.write_all(b"\n")?;
            }
        }};
    }

    if reverse {
        let mut heap: BinaryHeap<HeapItem> = BinaryHeap::with_capacity(batch.len());
        for (id, r) in readers.iter_mut().enumerate() {
            if let Some(v) = r.next()? {
                heap.push(HeapItem { value: v, source: id });
            }
        }
        while let Some(item) = heap.pop() {
            account!();
            let src = item.source;
            match item.value {
                MergeValue::Num(v) => {
                    let dup = dedup && has_last && last_num == Some(v);
                    if !dup {
                        last_num = Some(v);
                        has_last = true;
                        write_num!(v);
                    }
                }
                MergeValue::S(_) | MergeValue::Csv(_) => {
                    let cur = readers[src].rec_buf.clone();
                    let dup = dedup && has_last && {
                        if ignore_case {
                            cur.eq_ignore_ascii_case(&last_bytes)
                        } else {
                            cur == last_bytes
                        }
                    };
                    last_bytes = cur;
                    has_last = true;
                    emit_text!(src, dup);
                }
            }
            if let Some(v) = readers[src].next()? {
                heap.push(HeapItem { value: v, source: src });
            }
        }
    } else {
        let mut heap: BinaryHeap<Reverse<HeapItem>> = BinaryHeap::with_capacity(batch.len());
        for (id, r) in readers.iter_mut().enumerate() {
            if let Some(v) = r.next()? {
                heap.push(Reverse(HeapItem { value: v, source: id }));
            }
        }
        while let Some(Reverse(item)) = heap.pop() {
            account!();
            let src = item.source;
            match item.value {
                MergeValue::Num(v) => {
                    let dup = dedup && has_last && last_num == Some(v);
                    if !dup {
                        last_num = Some(v);
                        has_last = true;
                        write_num!(v);
                    }
                }
                // Line-based modes emit the record still buffered in the source
                // reader — no per-record String allocation needed.
                MergeValue::S(_) | MergeValue::Csv(_) => {
                    let cur = readers[src].rec_buf.clone();
                    let dup = dedup && has_last && {
                        if ignore_case {
                            cur.eq_ignore_ascii_case(&last_bytes)
                        } else {
                            cur == last_bytes
                        }
                    };
                    last_bytes = cur;
                    has_last = true;
                    emit_text!(src, dup);
                }
            }
            if let Some(v) = readers[src].next()? {
                heap.push(Reverse(HeapItem { value: v, source: src }));
            }
        }
    }
    // Flush pending keep-last group + numeric block.
    if group_has {
        out.write_all(&group_buf)?;
        out.write_all(b"\n")?;
    }
    if !num_out.is_empty() {
        out.write_all(&num_out)?;
    }

    out.flush()?;
    drop(out);
    if let Some(s) = state {
        // Counting pops is the honest measure of work done (each pop emitted
        // one element); flush whatever is left from the bulk accounting.
        s.merge_done.fetch_add(pending, Ordering::Relaxed);
        s.io_bytes.fetch_add(pending.saturating_mul(io_per_rec), Ordering::Relaxed);
    }
    Ok(total_bytes)
}
