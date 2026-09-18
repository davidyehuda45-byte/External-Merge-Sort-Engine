// Temp directory management: unique per-run dir, cleanup on success/panic.
// v2: `--temp-dir` override (run dirs live under <temp_dir>/.temp_sort/),
// resume discovery, and staging cleanup.
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static RUN_DIR: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
// Set when resuming: the panic hook must NOT delete the run dir, otherwise a
// retry would lose all completed chunks.
static PRESERVE: OnceLock<bool> = OnceLock::new();
// Staging paths (.part files) removed on any exit path.
static STAGING: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();

fn run_slot() -> &'static Mutex<Option<PathBuf>> {
    RUN_DIR.get_or_init(|| Mutex::new(None))
}

fn staging_slot() -> &'static Mutex<Vec<PathBuf>> {
    STAGING.get_or_init(|| Mutex::new(Vec::new()))
}

/// Marks the current run dir as preserved across panics (resume mode).
pub fn set_preserve(on: bool) {
    let _ = PRESERVE.set(on);
}

/// Registers a panic hook that removes the run's temp dir before reporting.
/// Call once at program start, before creating the temp dir.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        if !PRESERVE.get().copied().unwrap_or(false) {
            cleanup_run_dir();
        }
        cleanup_staging();
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        let loc = match info.location() {
            Some(l) => format!(" at {}:{}", l.file(), l.line()),
            None => String::new(),
        };
        eprintln!("PANIC{}: {}", loc, msg);
    }));
}

/// The `.temp_sort` base: `--temp-dir <p>/.temp_sort` when overridden,
/// otherwise `<parent_of_output>/.temp_sort` (v1 behavior).
pub fn temp_base(output_path: &Path, temp_dir_override: Option<&Path>) -> PathBuf {
    match temp_dir_override {
        Some(td) => td.join(Path::new(".temp_sort")),
        None => {
            let parent = match output_path.parent() {
                Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
                _ => PathBuf::from("."),
            };
            parent.join(Path::new(".temp_sort"))
        }
    }
}

/// Creates `<base>/.temp_sort/run-<pid>-<nanos>/` and remembers it for cleanup.
pub fn create_run_dir(output_path: &Path, temp_dir_override: Option<&Path>) -> std::io::Result<PathBuf> {
    let base = temp_base(output_path, temp_dir_override);
    fs::create_dir_all(base.clone())?;
    let unique = format!("run-{}-{}", std::process::id(), nanos_now());
    let dir = base.join(Path::new(&unique));
    fs::create_dir_all(dir.clone())?;

    {
        let mut guard = run_slot().lock().unwrap();
        // If an old run dir lingers in the slot (shouldn't happen), remove it first.
        if let Some(old) = guard.take() {
            let _ = fs::remove_dir_all(old);
        }
        *guard = Some(dir.clone());
    }
    Ok(dir)
}

/// Adopts an existing run dir for resume (registered for cleanup, not deleted).
pub fn adopt_run_dir(dir: PathBuf) {
    let mut guard = run_slot().lock().unwrap();
    if let Some(old) = guard.take()
        && old != dir {
            let _ = fs::remove_dir_all(old);
        }
    *guard = Some(dir);
}

/// Removes the run's temp dir (idempotent; safe from panic hook).
pub fn cleanup_run_dir() {
    let taken = {
        let mut guard = match run_slot().lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.take()
    };
    if let Some(dir) = taken {
        let _ = fs::remove_dir_all(dir);
    }
}

/// Registers a staging file (.part) for best-effort removal on every exit path.
pub fn register_staging(path: PathBuf) {
    staging_slot().lock().unwrap().push(path);
}

/// Removes all registered staging files (idempotent; safe from panic hook).
pub fn cleanup_staging() {
    let mut guard = match staging_slot().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    for p in guard.drain(..) {
        let _ = fs::remove_file(p);
    }
}

/// Removes the run's entire directory tree after a fully successful run.
pub fn cleanup_run_dir_tree(run_dir: &Path) {
    let _ = fs::remove_dir_all(run_dir);
    let slot_ok = run_slot().lock().map(|mut g| {
        if g.as_deref() == Some(run_dir) {
            *g = None;
        }
        true
    });
    let _ = slot_ok;
}

/// Best-effort delete of a single chunk file (used during multi-pass merges).
pub fn remove_file_quiet(path: &Path) {
    let _ = fs::remove_file(path);
}

/// Cheap unique-ish nanosecond counter for dir names.
fn nanos_now() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos(),
        Err(_) => 0,
    }
}

/// Convenience: create a chunk file handle with its path.
pub fn create_chunk_file(run_dir: &Path, index: usize) -> std::io::Result<(PathBuf, File)> {
    let name = format!("chunk_{:04}.dat", index);
    let path = run_dir.join(Path::new(&name));
    let f = File::create(path.clone())?;
    Ok((path, f))
}
