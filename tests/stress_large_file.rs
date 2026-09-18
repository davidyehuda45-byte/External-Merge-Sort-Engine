// Stress tests: bigger inputs, memory-budget compliance, forced multi-pass,
// and temp cleanup after an abnormal exit (invalid numeric data => panic path).
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_mergesort")
}

fn gen_bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_gen")
}

fn tmp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(Path::new(&format!("mergesort-stress-{}", std::process::id())));
    let _ = fs::create_dir_all(d.clone());
    d
}

/// Unique per-test parent dir so `.temp_sort` checks aren't confused by
/// other tests' engine children running concurrently.
fn case_dir(name: &str) -> PathBuf {
    let d = tmp_dir().join(Path::new(name));
    let _ = fs::create_dir_all(d.clone());
    d
}

fn run(args: &[&str]) -> (std::process::ExitStatus, String, String) {
    let out = Command::new(bin_path()).args(args).output().expect("failed to run mergesort");
    (
        out.status,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn verify_sorted_numeric(path: &Path) -> usize {
    let data = fs::read(path).unwrap();
    assert_eq!(data.len() % 8, 0);
    let mut prev: Option<u64> = None;
    let mut n = 0usize;
    for c in data.chunks_exact(8) {
        let v = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
        if let Some(p) = prev {
            assert!(v >= p, "unsorted at index {}", n);
        }
        prev = Some(v);
        n += 1;
    }
    n
}

fn verify_sorted_lines(path: &Path) -> usize {
    let data = fs::read(path).unwrap();
    let s = String::from_utf8(data).unwrap();
    let mut prev: Option<&str> = None;
    let mut n = 0usize;
    for line in s.lines() {
        if let Some(p) = prev {
            assert!(line >= p, "unsorted at line {}", n);
        }
        prev = Some(line);
        n += 1;
    }
    n
}

#[test]
fn stress_numeric_5m_tiny_memory() {
    // 5M u64 = 40MB input; 8MB budget => ~10+ chunks, real multi-chunk merge.
    let dir = tmp_dir();
    let input = dir.join(Path::new("stress-num.bin"));
    let output = dir.join(Path::new("stress-num.sorted"));
    let gen_out = Command::new(gen_bin_path())
        .args([input.display().to_string().as_str(), "5000000", "numeric", "99"])
        .output()
        .expect("gen failed");
    assert!(gen_out.status.success(), "gen failed: {}", String::from_utf8_lossy(&gen_out.stderr));

    // 16MB budget: multi-chunk (40MB input) and above the ~10MB Windows RSS
    // floor (binary + runtime + stacks), so the PRD's 20% tolerance check is
    // meaningful. Budgets below the process floor can't be honored at RSS level.
    let (status, stdout, stderr) = run(&[
        "--input", input.display().to_string().as_str(),
        "--output", output.display().to_string().as_str(),
        "--max-memory", "16MB",
        "--threads", "2",
        "--verify",
    ]);
    assert!(status.success(), "sort failed: {}", stderr);
    let n = verify_sorted_numeric(output.clone().as_path());
    assert_eq!(n, 5_000_000);
    assert!(stdout.contains("Verifikasi:                 OK"), "summary missing verify OK:\n{}", stdout);
    // Peak memory reported should stay under budget + 20% tolerance (PRD DoD).
    let mut peak_checked = false;
    for line in stdout.lines() {
        if line.starts_with("Puncak Memori") {
            let mb: f64 = line
                .split(':')
                .nth(1)
                .unwrap_or("0")
                .trim()
                .trim_end_matches(" MB")
                .trim()
                .parse()
                .unwrap_or(f64::MAX);
            assert!(mb <= 16.0 * 1.2, "peak memory {} MB exceeds budget", mb);
            peak_checked = true;
        }
    }
    assert!(peak_checked, "summary did not report peak memory");
}

#[test]
fn stress_string_2m_forced_multipass() {
    // 2M strings with fan-in 3 => log_3 passes over intermediate merges.
    let dir = tmp_dir();
    let input = dir.join(Path::new("stress-str.txt"));
    let output = dir.join(Path::new("stress-str.sorted"));
    let gen_out = Command::new(gen_bin_path())
        .args([input.display().to_string().as_str(), "2000000", "string", "7"])
        .output()
        .expect("gen failed");
    assert!(gen_out.status.success(), "gen failed: {}", String::from_utf8_lossy(&gen_out.stderr));

    let (status, stdout, stderr) = run(&[
        "--input", input.display().to_string().as_str(),
        "--output", output.display().to_string().as_str(),
        "--max-memory", "8MB",
        "--mode", "string",
        "--max-open-files", "3",
        "--verify",
    ]);
    assert!(status.success(), "sort failed: {}", stderr);
    let n = verify_sorted_lines(output.clone().as_path());
    assert_eq!(n, 2_000_000);
    assert!(stdout.contains("Verifikasi:                 OK"), "verify missing:\n{}", stdout);
}

#[test]
fn panic_cleans_temp_dir() {
    // Invalid numeric data fails gracefully (exit 5, no panic since v1.x);
    // no temp run dirs may be left behind either way.
    let dir = case_dir("panic-cleanup");
    let input = dir.join(Path::new("bad-in.bin"));
    let output = dir.join(Path::new("bad-out.bin"));
    let f = fs::File::create(input.clone().as_path()).unwrap();
    let mut w = BufWriter::new(f);
    for i in 0u64..1_000_000 {
        w.write_all(&i.to_le_bytes()).unwrap();
    }
    w.flush().unwrap();
    // Corrupt: append a text line by truncating mid-record and adding junk.
    let mut raw = fs::read(input.clone().as_path()).unwrap();
    raw.truncate(raw.len() - 3);
    raw.extend_from_slice(b"not-a-number\n");
    fs::write(input.clone().as_path(), raw).unwrap();

    let (status, _stdout, stderr) = run(&[
        "--input", input.display().to_string().as_str(),
        "--output", output.display().to_string().as_str(),
        "--max-memory", "8MB",
    ]);
    assert!(!status.success(), "corrupt input should fail");
    assert!(
        stderr.contains("not a complete 8-byte record") || stderr.contains("invalid numeric line") || stderr.contains("fase split gagal"),
        "expected corrupt-input error msg, got: {}",
        stderr
    );

    let temp_base = dir.join(Path::new(".temp_sort"));
    if temp_base.exists() {
        let leftovers: Vec<PathBuf> = fs::read_dir(temp_base.clone().as_path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        assert!(leftovers.is_empty(), "temp run dirs left behind after panic: {:?}", leftovers);
    }
}

#[test]
fn stress_thread_count_override() {
    let dir = tmp_dir();
    let input = dir.join(Path::new("thr-in.bin"));
    let output = dir.join(Path::new("thr-out.bin"));
    let f = fs::File::create(input.clone().as_path()).unwrap();
    let mut w = BufWriter::new(f);
    let mut x: u64 = 12345;
    for _ in 0..100_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        w.write_all(&x.to_le_bytes()).unwrap();
    }
    w.flush().unwrap();
    let (status, _stdout, stderr) = run(&[
        "--input", input.display().to_string().as_str(),
        "--output", output.display().to_string().as_str(),
        "--max-memory", "4MB",
        "--threads", "2",
        "--verify",
    ]);
    assert!(status.success(), "sort failed: {}", stderr);
    let n = verify_sorted_numeric(output.clone().as_path());
    assert_eq!(n, 100_000);
}
