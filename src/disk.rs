// Pre-flight disk space check (PRD v2 §3.1).
//
// Estimates the peak disk footprint of a sort and compares it against the
// available space on every involved volume BEFORE any work starts:
//   input (already on disk) + temp chunks (~1.1x input, incl. multi-pass
//   intermediate slack) + output — temp dir and output dir are checked
//   separately when they live on different drives.
use std::path::Path;

/// Estimated peak disk need for a sort of `input_size` bytes.
/// Temp usage ~1.1x input (chunks are newline/binary rewritten with per-record
/// overhead); multi-pass merging reuses freed space as batches are consumed,
/// so a modest 1.15x factor covers the intermediate-pass slack.
pub fn estimate_need_bytes(input_size: u64) -> u64 {
    input_size + (input_size as f64 * 1.15) as u64
}

fn available_space_bytes(path: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // Find the deepest mount point that is a prefix of the path.
    let mut best: Option<(usize, u64)> = None;
    for d in disks.list() {
        let mp = d.mount_point();
        if canonical.starts_with(mp) {
            let score = mp.as_os_str().len();
            if best.map(|(s, _)| score > s).unwrap_or(true) {
                best = Some((score, d.available_space()));
            }
        }
    }
    best.map(|(_, avail)| avail)
}

/// Human-readable size: <1MB in KB, <1GB in MB, else GB.
pub fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b < 1024.0 * 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", b / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Volume name for error messages ("C:" on Windows, mount point elsewhere).
fn volume_of(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let disks = sysinfo::Disks::new_with_refreshed_list();
    // Collect (len, mount) owned so no temporaries dangle.
    let mut mounts: Vec<(usize, String)> = disks
        .list()
        .iter()
        .filter(|d| canonical.starts_with(d.mount_point()))
        .map(|d| (d.mount_point().as_os_str().len(), d.mount_point().display().to_string()))
        .collect();
    mounts.sort_by_key(|(len, _)| *len);
    match mounts.last() {
        Some((_, mp)) => mp.clone(),
        None => path.display().to_string(),
    }
}

pub struct PreflightError {
    pub target: String,
    pub need_bytes: u64,
    pub avail_bytes: u64,
    pub suggestion: String,
}

impl PreflightError {
    pub fn message(&self) -> String {
        format!(
            "ruang disk tidak cukup di {} — kebutuhan ~{}, tersedia {}.\nSaran: {}",
            self.target,
            human_size(self.need_bytes),
            human_size(self.avail_bytes),
            self.suggestion
        )
    }
}

/// Checks the temp dir and (if on a different volume) the output dir.
/// `temp_need` is the estimated temp-chunk footprint; `output_need` is the
/// final output size (~input_size, or 0 when output shares the temp volume and
/// temp_need already dominates the peak).
pub fn preflight_check(
    input_size: u64,
    temp_dir: &Path,
    output_dir: &Path,
) -> Result<(), PreflightError> {
    let temp_need = estimate_need_bytes(input_size);
    let output_need = input_size;

    // Check the output volume first when separate from temp: its share is the
    // final output size; the temp volume's share is the chunk footprint.
    let same_volume = volume_of(temp_dir) == volume_of(output_dir);
    if same_volume {
        let need = temp_need + output_need;
        let avail = available_space_bytes(output_dir)
            .unwrap_or_else(|| available_space_bytes(temp_dir).unwrap_or(u64::MAX));
        if avail < need {
            return Err(PreflightError {
                target: volume_of(output_dir),
                need_bytes: need,
                avail_bytes: avail,
                suggestion: "kosongkan ruang disk, atau arahkan temp file ke drive lain dengan --temp-dir <path>".to_string(),
            });
        }
        return Ok(());
    }

    // Different volumes: each is checked against its own share.
    let temp_avail = available_space_bytes(temp_dir).unwrap_or(u64::MAX);
    if temp_avail < temp_need {
        return Err(PreflightError {
            target: volume_of(temp_dir),
            need_bytes: temp_need,
            avail_bytes: temp_avail,
            suggestion: "pilih --temp-dir di drive dengan ruang cukup, atau kosongkan drive ini".to_string(),
        });
    }
    let out_avail = available_space_bytes(output_dir).unwrap_or(u64::MAX);
    if out_avail < output_need {
        return Err(PreflightError {
            target: volume_of(output_dir),
            need_bytes: output_need,
            avail_bytes: out_avail,
            suggestion: "kosongkan ruang di drive output, atau tulis output ke drive lain".to_string(),
        });
    }
    Ok(())
}

/// Run-id candidates for --resume listing: "run-1234-5678" style names.
#[allow(dead_code)]
pub fn fmt_bytes_hint(need: u64, avail: u64) -> String {
    format!("need ~{} vs {} available", human_size(need), human_size(avail))
}
