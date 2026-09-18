// Correctness integration tests: run the mergesort binary on generated data
// and check outputs, edge cases, multi-pass merging, and temp cleanup.
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_mergesort")
}

fn tmp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(Path::new(&format!("mergesort-test-{}", std::process::id())));
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

struct RunResult {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> RunResult {
    let out = Command::new(bin_path()).args(args).output().expect("failed to run mergesort");
    RunResult {
        status: out.status,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn write_numeric(path: &Path, values: &[u64]) {
    let f = fs::File::create(path).unwrap();
    let mut w = BufWriter::new(f);
    for v in values {
        w.write_all(&v.to_le_bytes()).unwrap();
    }
    w.flush().unwrap();
}

fn read_numeric(path: &Path) -> Vec<u64> {
    let data = fs::read(path).unwrap();
    assert_eq!(data.len() % 8, 0, "output length not a multiple of 8");
    data.chunks_exact(8)
        .map(|c| u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect()
}

fn write_lines(path: &Path, lines: &[&str]) {
    let f = fs::File::create(path).unwrap();
    let mut w = BufWriter::new(f);
    for l in lines {
        w.write_all(l.as_bytes()).unwrap();
        w.write_all(b"\n").unwrap();
    }
    w.flush().unwrap();
}

fn read_lines(path: &Path) -> Vec<String> {
    let data = fs::read(path).unwrap();
    let s = String::from_utf8(data).unwrap();
    s.lines().map(|l| l.to_string()).collect()
}

fn numeric_case(name: &str, values: &[u64], extra_args: &[&str]) {
    let dir = tmp_dir();
    let input = dir.join(Path::new(&format!("{}-in.bin", name)));
    let output = dir.join(Path::new(&format!("{}-out.bin", name)));
    write_numeric(input.clone().as_path(), values);
    let mut args: Vec<String> = vec![
        "--input".to_string(),
        input.display().to_string(),
        "--output".to_string(),
        output.display().to_string(),
        "--max-memory".to_string(),
        "8MB".to_string(),
        "--verify".to_string(),
    ];
    for a in extra_args {
        args.push(a.to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let r = run(arg_refs.as_slice());
    assert!(r.status.success(), "{}: run failed\nstderr: {}\nstdout: {}", name, r.stderr, r.stdout);
    let got = read_numeric(output.clone().as_path());
    let mut want = values.to_vec();
    want.sort_unstable();
    assert_eq!(got, want, "{}: output mismatch", name);
}

fn string_case(name: &str, lines: &[&str], extra_args: &[&str]) {
    let dir = tmp_dir();
    let input = dir.join(Path::new(&format!("{}-in.txt", name)));
    let output = dir.join(Path::new(&format!("{}-out.txt", name)));
    write_lines(input.clone().as_path(), lines);
    let mut args: Vec<String> = vec![
        "--input".to_string(),
        input.display().to_string(),
        "--output".to_string(),
        output.display().to_string(),
        "--max-memory".to_string(),
        "8MB".to_string(),
        "--mode".to_string(),
        "string".to_string(),
        "--verify".to_string(),
    ];
    for a in extra_args {
        args.push(a.to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let r = run(arg_refs.as_slice());
    assert!(r.status.success(), "{}: run failed\nstderr: {}\nstdout: {}", name, r.stderr, r.stdout);
    let got = read_lines(output.clone().as_path());
    let mut want: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    want.sort();
    assert_eq!(got, want, "{}: output mismatch", name);
}

#[test]
fn numeric_basic() {
    numeric_case("basic", &[5, 3, 9, 1, 7, 2, 8, 6, 4, 0], &[]);
}

#[test]
fn numeric_duplicates_and_extremes() {
    numeric_case("extremes", &[u64::MAX, 0, u64::MAX, 0, 42, 42, u64::MAX - 1, 1], &[]);
}

#[test]
fn numeric_single_value() {
    numeric_case("single", &[12345], &[]);
}

#[test]
fn numeric_multipass_merge() {
    // Tiny memory + tiny fan-in forces several chunks and a multi-pass merge.
    let mut v: Vec<u64> = Vec::new();
    let mut x: u64 = 0x243F6A8885A308D3;
    for _ in 0..50_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.push(x);
    }
    numeric_case("multipass", v.as_slice(), &["--max-open-files", "2"]);
}

#[test]
fn string_basic() {
    string_case("strbasic", &["banana", "apple", "cherry", "apple", "date"], &[]);
}

#[test]
fn string_unicode_and_empty() {
    string_case(
        "struni",
        &["héllo", "wörld", "日本語", "aaa", "zzz", "Ápple"],
        &[],
    );
}

#[test]
fn string_multipass_merge() {
    let mut words: Vec<String> = Vec::new();
    let mut x: u64 = 0xDEADBEEFCAFEBABE;
    for _ in 0..20_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let len = (x % 15 + 1) as usize;
        let mut s = String::new();
        let mut y = x;
        for _ in 0..len {
            y = y.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            s.push((b'a' + ((y >> 33) % 26) as u8) as char);
        }
        words.push(s);
    }
    let refs: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
    string_case("strmultipass", refs.as_slice(), &["--max-open-files", "2"]);
}

#[test]
fn no_trailing_newline() {
    // Last line has no trailing \n; must still be sorted in.
    let dir = tmp_dir();
    let input = dir.join(Path::new("nonl-in.txt"));
    let output = dir.join(Path::new("nonl-out.txt"));
    fs::write(input.clone().as_path(), b"pear\napple\norange").unwrap();
    let r = run(&[
        "--input", input.display().to_string().as_str(),
        "--output", output.display().to_string().as_str(),
        "--max-memory", "8MB",
        "--mode", "string",
        "--verify",
    ]);
    assert!(r.status.success(), "run failed\nstderr: {}", r.stderr);
    let got = read_lines(output.clone().as_path());
    assert_eq!(got, vec!["apple", "orange", "pear"]);
}

#[test]
fn empty_input_produces_empty_output() {
    let dir = case_dir("empty-check");
    let input = dir.join(Path::new("empty.bin"));
    let output = dir.join(Path::new("empty-out.bin"));
    fs::write(input.clone().as_path(), b"").unwrap();
    let r = run(&[
        "--input", input.display().to_string().as_str(),
        "--output", output.display().to_string().as_str(),
        "--max-memory", "8MB",
    ]);
    assert!(r.status.success(), "empty input should succeed: {}", r.stderr);
    assert_eq!(fs::read(output.clone().as_path()).unwrap(), Vec::<u8>::new(), "output should be empty");
}

#[test]
fn crlf_lines_sorted() {
    let dir = tmp_dir();
    let input = dir.join(Path::new("crlf-in.txt"));
    let output = dir.join(Path::new("crlf-out.txt"));
    fs::write(input.clone().as_path(), b"pear\r\napple\r\norange\r\n").unwrap();
    let r = run(&[
        "--input", input.display().to_string().as_str(),
        "--output", output.display().to_string().as_str(),
        "--max-memory", "8MB",
        "--mode", "string",
        "--verify",
    ]);
    assert!(r.status.success(), "run failed\nstderr: {}", r.stderr);
    let got = read_lines(output.clone().as_path());
    // \r is stripped by the engine; expect clean lines.
    assert_eq!(got, vec!["apple", "orange", "pear"]);
}

#[test]
fn temp_dir_cleaned_after_success() {
    let dir = case_dir("cleanup-check");
    let input = dir.join(Path::new("clean-in.bin"));
    let output = dir.join(Path::new("clean-out.bin"));
    write_numeric(input.clone().as_path(), &[3, 1, 2]);
    let r = run(&[
        "--input", input.display().to_string().as_str(),
        "--output", output.display().to_string().as_str(),
        "--max-memory", "8MB",
    ]);
    assert!(r.status.success(), "run failed: {}", r.stderr);
    let temp_base = dir.join(Path::new(".temp_sort"));
    // .temp_sort may exist but must contain no run dirs.
    if temp_base.exists() {
        let leftovers: Vec<PathBuf> = fs::read_dir(temp_base.clone().as_path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        assert!(leftovers.is_empty(), "temp run dirs left behind: {:?}", leftovers);
    }
}

#[test]
fn verify_detects_unsorted() {
    // Strategy: trigger --verify failure via merge mode with a deliberately
    // corrupted pre-sorted input. The merge heap assumes every --merge input
    // is already sorted; if one is not, the merged stream is not monotonic and
    // --verify must catch it with exit code 6 (VERIFY_FAIL).
    let dir = case_dir("verify_detects_unsorted");
    let a = dir.join("a.txt");
    // a.txt claims to be pre-sorted ascending but is actually descending:
    // zebra -> apple -> mango  (next item "apple" < "zebra" => not monotonic).
    fs::write(a.as_path(), b"zebra\napple\nmango\n").unwrap();
    let b = dir.join("b.txt");
    // b.txt is genuinely pre-sorted (so only a.txt corrupts the output):
    fs::write(b.as_path(), b"banana\nkiwi\nyellow\n").unwrap();
    let out = dir.join("merged.txt");
    let r = run(&[
        "--merge", a.display().to_string().as_str(),
                  b.display().to_string().as_str(),
        "--output", out.display().to_string().as_str(),
        "--max-memory", "8MB",
        "--mode", "string",
        "--verify",
    ]);
    // 1. Basic assert: run must NOT succeed.
    assert!(!r.status.success(),
        "--verify seharusnya GAGAL (exit != 0) karena merge input a.txt tidak pre-sorted => merged tidak monoton, tapi exit=0. stderr: {}", r.stderr);
    // 2. Exact exit code: engine convention 6 = VERIFY_FAIL (errors.rs).
    if let Some(code) = r.status.code() {
        assert_eq!(code, 6,
            "expected exit code 6 (VERIFY_FAIL) karena output tidak terurut, actual exit={}. stderr: {}",
            code, r.stderr);
    }
    // 3. Sanity: merged file harus benar-benar ditulis (engine tidak mati sebelum write).
    assert!(out.exists(),
        "engine seharusnya menulis output merged.txt sebelum --verify berjalan, tapi file tidak ada.");
}

// ---- Property-based tests (proptest) ----

proptest::proptest! {
    #[test]
    fn prop_numeric_sorted(v in proptest::collection::vec(0u64..u64::MAX, 0..30_000)) {
        let dir = tmp_dir();
        let input = dir.join(Path::new("prop-in.bin"));
        let output = dir.join(Path::new("prop-out.bin"));
        write_numeric(input.clone().as_path(), &v);
        let in_str = input.display().to_string();
        let out_str = output.display().to_string();
        let r = run(&["--input", in_str.as_str(), "--output", out_str.as_str(), "--max-memory", "4MB"]);
        assert!(r.status.success(), "run failed: {}", r.stderr);
        let got = read_numeric(output.clone().as_path());
        let mut want = v.clone();
        want.sort_unstable();
        assert_eq!(got, want);
    }

    #[test]
    fn prop_string_sorted(words in proptest::collection::vec("[a-z]{1,20}", 0..15_000)) {
        let dir = tmp_dir();
        let input = dir.join(Path::new("props-in.txt"));
        let output = dir.join(Path::new("props-out.txt"));
        let refs: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
        write_lines(input.clone().as_path(), refs.as_slice());
        let in_str = input.display().to_string();
        let out_str = output.display().to_string();
        let r = run(&["--input", in_str.as_str(), "--output", out_str.as_str(), "--max-memory", "4MB", "--mode", "string"]);
        assert!(r.status.success(), "run failed: {}", r.stderr);
        let got = read_lines(output.clone().as_path());
        let mut want = words.clone();
        want.sort();
        assert_eq!(got, want);
    }
}
