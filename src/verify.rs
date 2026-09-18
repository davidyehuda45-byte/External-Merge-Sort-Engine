// Post-sort verification: read the output back and confirm non-decreasing order.
// v2: CSV mode verifies by the selected key columns, not the whole line.
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use crate::chunk::{extract_csv_keys, extract_json_key, CsvKey};

#[allow(dead_code)]
pub fn verify(output: &Path, kind: &str) -> Result<usize, String> {
    verify_full(output, kind, false, false)
}

/// Full verifier with commercial flags: `reverse` expects descending order,
/// `skip_header` ignores the first line (CSV/string header row).
pub fn verify_full(output: &Path, kind: &str, reverse: bool, skip_header: bool) -> Result<usize, String> {
    let f = File::open(output).map_err(|e| format!("verify: cannot open output: {}", e))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, f);
    let mut count: usize = 0;
    match kind {
        "numeric" => {
            let mut prev: Option<u64> = None;
            let mut buf = [0u8; 8];
            loop {
                match reader.read_exact(&mut buf) {
                    Ok(_) => {
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
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(e) => return Err(format!("verify: read error: {}", e)),
                }
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
                        let s = String::from_utf8_lossy(&line[..end]).into_owned();
                        if let Some(p) = prev {
                            let bad = if reverse { s > p } else { s < p };
                            if bad {
                                return Err(format!(
                                    "verify FAILED: output[{}] = {:?} out of order vs output[{}] = {:?}",
                                    count, s, count - 1, p
                                ));
                            }
                        }
                        prev = Some(s);
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
#[allow(dead_code)]
pub fn verify_csv(
    output: &Path,
    delimiter: u8,
    keys: &[usize],
    key_numeric: bool,
) -> Result<usize, String> {
    verify_csv_full(output, delimiter, keys, key_numeric, false, false)
}

pub fn verify_csv_full(
    output: &Path,
    delimiter: u8,
    keys: &[usize],
    key_numeric: bool,
    reverse: bool,
    skip_header: bool,
) -> Result<usize, String> {
    let f = File::open(output).map_err(|e| format!("verify: cannot open output: {}", e))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, f);
    let mut count: usize = 0;
    let mut physical: usize = 0;
    let mut prev: Option<Vec<CsvKey>> = None;
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => {
                physical += 1;
                let mut end = line.len();
                if line.last() == Some(&b'\n') {
                    end -= 1;
                }
                if end > 0 && line[end - 1] == b'\r' {
                    end -= 1;
                }
                // Header row is metadata: skip key extraction, don't count.
                // Blank lines are never records (matches chunk counting rules).
                if skip_header && physical == 1 {
                    continue;
                }
                if end == 0 {
                    continue;
                }
                let k = extract_csv_keys(&line[..end], delimiter, keys, key_numeric)
                    .map_err(|m| format!("verify: record {}: {}", count + 1, m))?;
                if let Some(p) = &prev {
                    let bad = if reverse { k.as_slice() > p.as_slice() } else { k.as_slice() < p.as_slice() };
                    if bad {
                        return Err(format!(
                            "verify FAILED: CSV key at output line {} sorts before line {} (reverse={})",
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

/// JSONL verification: raw lines compared via the nested key field.
pub fn verify_jsonl_full(
    output: &Path,
    field: &[String],
    key_numeric: bool,
    reverse: bool,
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
                let k = extract_json_key(&line[..end], field, key_numeric)
                    .map_err(|m| format!("verify: record {}: {}", count + 1, m))?;
                if let Some(p) = &prev {
                    let bad = if reverse { k.as_slice() > p.as_slice() } else { k.as_slice() < p.as_slice() };
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
