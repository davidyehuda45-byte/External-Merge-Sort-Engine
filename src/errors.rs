// Consistent exit codes for scripting/CI integration (PRD v2 §3.5).
//
// 0  success
// 2  usage error (bad flags)
// 3  pre-flight failure (disk space, unreadable input, unwritable output)
// 4  I/O error during the run (disk full, permission lost mid-run)
// 5  data error (malformed input for the selected format)
// 6  verification failure (--verify found the output unsorted)
pub const OK: i32 = 0;
// 1  unexpected/internal error (reserved)
#[allow(dead_code)]
pub const INTERNAL: i32 = 1;
pub const USAGE: i32 = 2;
pub const PREFLIGHT: i32 = 3;
pub const IO: i32 = 4;
pub const DATA: i32 = 5;
pub const VERIFY: i32 = 6;

/// Prints a human-readable error prefixed with the exit code (machine-greppable)
/// and terminates the process with that code.
pub fn fail(code: i32, msg: &str) -> ! {
    eprintln!("error[{}]: {}", code, msg);
    std::process::exit(code);
}

/// Formats an I/O error with a hint for the most common painful case.
pub fn io_hint(e: &std::io::Error) -> String {
    let s = e.to_string();
    match e.kind() {
        std::io::ErrorKind::StorageFull => format!(
            "{} (disk penuh: kosongkan ruang atau arahkan temp ke drive lain dengan --temp-dir)", s
        ),
        std::io::ErrorKind::PermissionDenied => format!(
            "{} (izin ditolak: periksa hak akses file/direktori, atau jalankan tanpa opsi yang butuh akses khusus)", s
        ),
        _ => s,
    }
}
