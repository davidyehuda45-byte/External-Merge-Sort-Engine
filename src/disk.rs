// Pre-flight disk space check (PRD v2 §3.1).
//
// Estimates the peak disk footprint of a sort and compares it against the
// available space on every involved volume BEFORE any work starts:
//   temp chunks (~1.15x input, incl. multi-pass intermediate slack) — the
//   input already lives on disk so it is not double-counted; the output dir
//   is checked separately (needs ~input_size) only when it lives on a
//   different volume.
use std::path::Path;

/// Estimated peak disk need for a sort of `input_size` bytes.
/// Temp usage ~1.15x input (chunks are newline/binary rewritten with per-record
/// overhead); multi-pass merging reuses freed space as batches are consumed,
/// so a modest 1.15x factor covers the intermediate-pass slack.
/// Total peak on a shared volume ≈ input (already on disk) + 1.15x temp
/// churn; the check below therefore requires only `temp_need` when temp and
/// output share a volume.
pub fn estimate_need_bytes(input_size: u64) -> u64 {
    input_size + (input_size as f64 * 1.15) as u64
}

/// Best-effort canonical path: canonicalize when the path exists, otherwise
/// absolutize a relative path against the CWD so prefix matching still works.
fn canonical_of(path: &Path) -> std::path::PathBuf {
    if let Ok(c) = std::fs::canonicalize(path) {
        return c;
    }
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path.to_path_buf(),
    }
}

/// True when `mount` is a path-prefix of `canonical`.
/// On Windows the comparison is case-insensitive (C: vs c:) and `/` vs `\`
/// are normalized; elsewhere a plain `starts_with` on components suffices.
fn is_mount_prefix(canonical: &Path, mount: &Path) -> bool {
    #[cfg(windows)]
    {
        let c = canonical.to_string_lossy().replace('/', "\\").to_lowercase();
        let mut m = mount.to_string_lossy().replace('/', "\\").to_lowercase();
        // A bare drive root like `c:` should match `c:\...`.
        if m.len() == 2 && m.ends_with(':') {
            m.push('\\');
        }
        if c == m {
            return true;
        }
        if c.starts_with(&m) {
            // Require a separator boundary so `C:\data2` doesn't match `C:\data`.
            if m.ends_with('\\') {
                return true;
            }
            return c[m.len()..].starts_with('\\');
        }
        false
    }
    #[cfg(not(windows))]
    {
        canonical.starts_with(mount)
    }
}

fn available_space_on(canonical: &Path, disks: &sysinfo::Disks) -> Option<u64> {
    // Deepest mount point that prefixes the path wins.
    let mut best: Option<(usize, u64)> = None;
    for d in disks.list() {
        let mp = d.mount_point();
        if is_mount_prefix(canonical, mp) {
            let score = mp.as_os_str().len();
            if best.map(|(s, _)| score > s).unwrap_or(true) {
                best = Some((score, d.available_space()));
            }
        }
    }
    best.map(|(_, avail)| avail)
}

/// Volume name for error messages ("C:" on Windows, mount point elsewhere).
fn volume_of(canonical: &Path, disks: &sysinfo::Disks) -> String {
    let mut mounts: Vec<(usize, String)> = disks
        .list()
        .iter()
        .filter(|d| is_mount_prefix(canonical, d.mount_point()))
        .map(|d| (d.mount_point().as_os_str().len(), d.mount_point().display().to_string()))
        .collect();
    mounts.sort_by_key(|(len, _)| *len);
    match mounts.last() {
        Some((_, mp)) => mp.clone(),
        None => canonical.display().to_string(),
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

/// Human-readable size: <1KB in B, <1MB in KB, <1GB in MB, else GB.
pub fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if b < 1024.0 * 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", b / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Checks the temp dir and (if on a different volume) the output dir.
/// `temp_need` is the estimated temp-chunk footprint (~1.15x input + input
/// rewrite headroom); `output_need` is the final output size (~input_size).
/// Same volume: only `temp_need` is required — the input is already on disk,
/// so demanding temp+output would over-estimate ~3.15x and reject healthy
/// disks. Different volumes: temp checked vs `temp_need`, output vs
/// `output_need`.
///
/// The disk list is refreshed exactly once per check and shared by all
/// lookups. When free space cannot be determined (`None`), the check is
/// skipped with a warning rather than failing open with u64::MAX or
/// failing closed with a bogus error.
pub fn preflight_check(
    input_size: u64,
    temp_dir: &Path,
    output_dir: &Path,
) -> Result<(), PreflightError> {
    let temp_need = estimate_need_bytes(input_size);
    let output_need = input_size;

    // Single refresh shared by every lookup below.
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let temp_canon = canonical_of(temp_dir);
    let out_canon = canonical_of(output_dir);

    let same_volume = volume_of(&temp_canon, &disks) == volume_of(&out_canon, &disks);
    if same_volume {
        // Input already occupies `input_size` on this volume; peak extra is
        // the temp churn, not temp + a second full output copy.
        let need = temp_need;
        let avail = match available_space_on(&out_canon, &disks)
            .or_else(|| available_space_on(&temp_canon, &disks))
        {
            Some(a) => a,
            None => {
                eprintln!(
                    "warning: tidak bisa membaca ruang disk untuk {} — lewati preflight check",
                    volume_of(&out_canon, &disks)
                );
                return Ok(());
            }
        };
        if avail < need {
            return Err(PreflightError {
                target: volume_of(&out_canon, &disks),
                need_bytes: need,
                avail_bytes: avail,
                suggestion: "kosongkan ruang disk, atau arahkan temp file ke drive lain dengan --temp-dir <path>".to_string(),
            });
        }
        return Ok(());
    }

    // Different volumes: each is checked against its own share.
    let temp_avail = match available_space_on(&temp_canon, &disks) {
        Some(a) => a,
        None => {
            eprintln!(
                "warning: tidak bisa membaca ruang disk untuk {} — lewati preflight check temp",
                volume_of(&temp_canon, &disks)
            );
            return Ok(());
        }
    };
    if temp_avail < temp_need {
        return Err(PreflightError {
            target: volume_of(&temp_canon, &disks),
            need_bytes: temp_need,
            avail_bytes: temp_avail,
            suggestion: "pilih --temp-dir di drive dengan ruang cukup, atau kosongkan drive ini".to_string(),
        });
    }
    let out_avail = match available_space_on(&out_canon, &disks) {
        Some(a) => a,
        None => {
            eprintln!(
                "warning: tidak bisa membaca ruang disk untuk {} — lewati preflight check output",
                volume_of(&out_canon, &disks)
            );
            return Ok(());
        }
    };
    if out_avail < output_need {
        return Err(PreflightError {
            target: volume_of(&out_canon, &disks),
            need_bytes: output_need,
            avail_bytes: out_avail,
            suggestion: "kosongkan ruang di drive output, atau tulis output ke drive lain".to_string(),
        });
    }
    Ok(())
}
