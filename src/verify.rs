// Post-sort verification: read the output back and confirm non-decreasing order.
// v2: CSV mode verifies by the selected key columns, not the whole line.
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use crate::chunk::{extract_csv_keys, extract_json_key, CsvKey, SortOpts};

#[allow(dead_code)]
pub fn verify(output: &Path, kind: &str) -> Result<usize, String> {
    verify_full(output, kind, false, false, &SortOpts::default())
}

/// Full verifier with commercial flags: `reverse` expects descending order,
/// `skip_header` ignores the first line (CSV/string header row).
/// Blank (empty) lines are never records and are skipped everywhere — the
/// same policy as the split phase and the CSV/JSONL verifiers below.
pub fn verify_full(output: &Path, kind: &str, reverse: bool, skip_header: bool, opts: &SortOpts) -> Result<usize, String> {
    let f = File::open(output).map_err(|e| format!("verify: cannot open output: {}", e))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, f);
    let mut count: usize = 0;
    match kind {
        "numeric" => {
            let mut prev: Option<u64> = None;
            let mut buf = [0u8; 8];
            loop {
                // Fill exactly one 8-byte record. A short `read` is normal
                // (BufReader slicing), so loop until 8B or clean EOF; 1-7
                // trailing bytes are corruption and MUST error, never break
                // silently (a torn write would otherwise verify as "sorted").
                let mut filled = 0usize;
                while filled < 8 {
                    match reader.read(&mut buf[filled..]) {
                        Ok(0) => break,
                        Ok(n) => filled += n,
                        Err(e) => return Err(format!("verify: read error: {}", e)),
                    }
                }
                if filled == 0 {
                    break; // clean EOF on a record boundary
                }
                if filled < 8 {
                    return Err(format!(
                        "verify FAILED: trailing {} bytes are not a complete 8-byte record ({} records OK)",
                        filled, count
                    ));
                }
                let v = u64::from_le_bytes(buf);
                if let Some(p) = prev {
                    let bad = if reverse { v > p } else { v < p };
                    if bad {
                        return Err(format!(
                            "verify FAILED: output[{}] = {} out of order vs output[{}] = {} (reverse={})",
                            count, v, count - 1, p, reverse
                        ));
                    }
                }
                prev = Some(v);
                count += 1;
            }
        }
        _ => {
            let mut prev: Option<String> = None;
            let mut line: Vec<u8> = Vec::new();
            let mut first = true;
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let mut end = line.len();
                        if line.last() == Some(&b'\n') {
                            end -= 1;
                        }
                        if end > 0 && line[end - 1] == b'\r' {
                            end -= 1;
                        }
                        if skip_header && first {
                            first = false;
                            continue;
                        }
                        first = false;
                        // Blank lines are never records (matches the split
                        // phase and the CSV/JSONL verifiers): skip without
                        // counting or comparing, so a stray "\n" can't fake
                        // an order break or inflate the record count.
                        if end == 0 {
                            continue;
                        }
                        let s = String::from_utf8_lossy(&line[..end]).into_owned();
                        let cur = if opts.ignore_case { s.to_lowercase() } else { s };
                        if let Some(p) = &prev {
                            let bad = if reverse { cur > *p } else { cur < *p };
                            if bad {
                                return Err(format!(
                                    "verify FAILED: output[{}] = {:?} out of order vs output[{}] = {:?}",
                                    count, cur, count - 1, p
                                ));
                            }
                        }
                        prev = Some(cur);
                        count += 1;
                    }
                    Err(e) => return Err(format!("verify: read error: {}", e)),
                }
            }
        }
    }
    Ok(count)
}

/// CSV verification: whole records are compared via their extracted keys
/// (numeric or string), so an unsorted record is reported with its line number.
///
/// Key-aware: like the merge phase, ordering is decided by
/// `extract_csv_keys` (the selected `keys` columns, numeric vs string) — NOT
/// by raw-line lexicographic order, which would disagree with the sort for
/// numeric keys ("10" < "9" lexically) or multi-column keys. Blank lines are
/// skipped (never records), mirroring the split phase.
#[allow(dead_code)]
pub fn verify_csv(
    output: &Path,
    delimiter: u8,
    keys: &[usize],
    key_numeric: bool,
) -> Result<usize, String> {
    verify_csv_full(output, delimiter, keys, key_numeric, false, false, &SortOpts::default())
}

pub fn verify_csv_full(
    output: &Path,
    delimiter: u8,
    keys: &[usize],
    key_numeric: bool,
    reverse: bool,
    skip_header: bool,
    opts: &SortOpts,
) -> Result<usize, String> {
    let f = File::open(output).map_err(|e| format!("verify: cannot open output: {}", e))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, f);
    let mut count: usize = 0;
    let mut physical: usize = 0;
    let mut prev: Option<Vec<CsvKey>> = None;
    let mut line: Vec<u8> = Vec::new();
    // Multiline CSV: gabung physical lines sampai quotes balance (aturan sama
    // dengan split + merge). Header = record yang mulai di physical line 1.
    let mut pending: Vec<u8> = Vec::new();
    let mut pending_from_first = false;
    // Satu titik verifikasi untuk semua record (single-line maupun gabungan).
    // Blank (kosong) bukan record: dilewati tanpa dihitung (sama dengan split).
    let check_rec = |rec: &[u8], prev: &mut Option<Vec<CsvKey>>, count: &mut usize| -> Result<(), String> {
        if rec.is_empty() {
            return Ok(());
        }
        let k = extract_csv_keys(rec, delimiter, keys, key_numeric, opts.ignore_case)
            .map_err(|m| format!("verify: record {}: {}", *count + 1, m))?;
        if let Some(p) = &*prev {
            let ord = SortOpts::cmp_keys(&k, p, &opts.key_desc, opts.nulls_last);
            let bad = if reverse { ord == std::cmp::Ordering::Greater } else { ord == std::cmp::Ordering::Less };
            if bad {
                return Err(format!(
                    "verify FAILED: CSV key at output line {} sorts before line {} (reverse={})",
                    *count + 1,
                    *count,
                    reverse
                ));
            }
        }
        *prev = Some(k);
        *count += 1;
        Ok(())
    };
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => {
                if !pending.is_empty() {
                    // EOF di tengah quote: verifikasi apa adanya (error jelas bila invalid).
                    // Bila pending mulai di baris 1 + skip_header: itu header, lewati.
                    if !(skip_header && pending_from_first) {
                        let rec = std::mem::take(&mut pending);
                        check_rec(&rec, &mut prev, &mut count)?;
                    }
                }
                break;
            }
            Ok(_) => {
                physical += 1;
                let mut end = line.len();
                if line.last() == Some(&b'\n') {
                    end -= 1;
                }
                if end > 0 && line[end - 1] == b'\r' {
                    end -= 1;
                }
                if pending.is_empty() {
                    if crate::chunk::quotes_balanced(&line[..end]) {
                        // Header = single-line record di physical line 1.
                        if skip_header && physical == 1 {
                            continue;
                        }
                        check_rec(&line[..end], &mut prev, &mut count)?;
                    } else {
                        pending.extend_from_slice(&line[..end]);
                        pending_from_first = physical == 1;
                    }
                } else {
                    pending.push(b'\n');
                    pending.extend_from_slice(&line[..end]);
                    if crate::chunk::quotes_balanced(&pending) {
                        let rec = std::mem::take(&mut pending);
                        let from_first = pending_from_first;
                        pending_from_first = false;
                        // Header = multiline record yang mulai di physical line 1.
                        if skip_header && from_first {
                            continue;
                        }
                        check_rec(&rec, &mut prev, &mut count)?;
                    }
                }
            }
            Err(e) => return Err(format!("verify: read error: {}", e)),
        }
    }
    Ok(count)
}

/// JSONL verification: raw lines compared via the nested key field.
///
/// Key-aware: ordering uses `extract_json_key` (the configured dot-path
/// field, numeric vs string) exactly as the chunk sort and k-way merge do —
/// raw-line comparison would mis-order numeric fields. Blank lines are
/// skipped (never records).
pub fn verify_jsonl_full(
    output: &Path,
    field: &[String],
    key_numeric: bool,
    reverse: bool,
    opts: &SortOpts,
) -> Result<usize, String> {
    let f = File::open(output).map_err(|e| format!("verify: cannot open output: {}", e))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, f);
    let mut count: usize = 0;
    let mut prev: Option<Vec<CsvKey>> = None;
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => {
                let mut end = line.len();
                if line.last() == Some(&b'\n') {
                    end -= 1;
                }
                if end > 0 && line[end - 1] == b'\r' {
                    end -= 1;
                }
                if end == 0 {
                    continue;
                }
                let k = extract_json_key(&line[..end], field, key_numeric, opts.ignore_case)
                    .map_err(|m| format!("verify: record {}: {}", count + 1, m))?;
                if let Some(p) = &prev {
                    let ord = SortOpts::cmp_keys(&k, p, &opts.key_desc, opts.nulls_last);
                    let bad = if reverse { ord == std::cmp::Ordering::Greater } else { ord == std::cmp::Ordering::Less };
                    if bad {
                        return Err(format!(
                            "verify FAILED: JSONL key at output line {} sorts before line {} (reverse={})",
                            count + 1,
                            count,
                            reverse
                        ));
                    }
                }
                prev = Some(k);
                count += 1;
            }
            Err(e) => return Err(format!("verify: read error: {}", e)),
        }
    }
    Ok(count)
}
