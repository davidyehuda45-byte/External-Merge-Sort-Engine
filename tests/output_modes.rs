// Tests for output modes: --json summary, --quiet suppression, and the live
// --dashboard HTTP endpoint serving / and /metrics while a sort runs.
use std::fs;
use std::io::{BufWriter, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_mergesort")
}

fn gen_bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_gen")
}

fn tmp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(Path::new(&format!("mergesort-modes-{}", std::process::id())));
    let _ = fs::create_dir_all(d.clone());
    d
}

fn case_dir(name: &str) -> PathBuf {
    let d = tmp_dir().join(Path::new(name));
    let _ = fs::create_dir_all(d.clone());
    d
}

/// Minimal JSON sanity: balanced braces, "key": "value" pair presence checks.
fn assert_json_field(json: &str, key: &str) {
    let pat = format!("\"{}\"", key);
    assert!(json.contains(&pat), "JSON missing key {}:\n{}", key, json);
}

fn make_numeric_input(dir: &Path, records: u64) -> (PathBuf, PathBuf) {
    let input = dir.join(Path::new("in.bin"));
    let output = dir.join(Path::new("out.bin"));
    let f = fs::File::create(input.clone()).unwrap();
    let mut w = BufWriter::new(f);
    let mut x: u64 = 0xC0FFEE;
    for _ in 0..records {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        w.write_all(&x.to_le_bytes()).unwrap();
    }
    w.flush().unwrap();
    (input, output)
}

#[test]
fn json_summary_is_well_formed() {
    let dir = case_dir("json");
    let (input, output) = make_numeric_input(dir.clone().as_path(), 200_000);
    let out = Command::new(bin_path())
        .args([
            "--input", input.display().to_string().as_str(),
            "--output", output.display().to_string().as_str(),
            "--max-memory", "16MB",
            "--threads", "2",
            "--json",
            "--verify",
        ])
        .output()
        .expect("run failed");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    // Balanced braces + no banner text in JSON mode.
    assert!(stdout.trim_start().starts_with('{'), "not JSON: {}", stdout);
    assert_eq!(stdout.matches('{').count(), stdout.matches('}').count(), "unbalanced JSON");
    assert!(!stdout.contains("SORTIR SELESAI"), "text banner leaked into --json mode");
    for key in [
        "\"status\"", "\"input\"", "\"output\"", "\"mode\"", "\"input_bytes\"",
        "\"records\"", "\"timing\"", "\"peak_memory_bytes\"", "\"io_bytes_read\"", "\"verified\"",
    ] {
        assert_json_field(&stdout, key.trim_matches('"'));
    }
    assert!(stdout.contains("\"verified\": true"), "verify flag not surfaced: {}", stdout);
}

#[test]
fn quiet_mode_prints_summary_only() {
    let dir = case_dir("quiet");
    let (input, output) = make_numeric_input(dir.clone().as_path(), 100_000);
    let out = Command::new(bin_path())
        .args([
            "--input", input.display().to_string().as_str(),
            "--output", output.display().to_string().as_str(),
            "--max-memory", "16MB",
            "--threads", "2",
            "--quiet",
        ])
        .output()
        .expect("run failed");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(!stdout.contains("EXTERNAL MERGE SORT"), "banner leaked in quiet mode");
    assert!(stdout.contains("SORTIR SELESAI"), "final summary missing in quiet mode:\n{}", stdout);
    assert!(stdout.contains("Total Waktu"), "summary stats missing");
}

#[test]
fn dashboard_serves_metrics_during_run() {
    let dir = case_dir("dashboard");
    // Deterministic port: find a free one by binding and releasing.
    let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port: u16 = probe.local_addr().unwrap().port();
    drop(probe);

    let (input, output) = make_numeric_input(dir.clone().as_path(), 3_000_000);
    let mut child = Command::new(bin_path())
        .args([
            "--input", input.display().to_string().as_str(),
            "--output", output.display().to_string().as_str(),
            "--max-memory", "8MB",
            "--threads", "2",
            "--json",
            "--dashboard",
            &port.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("run failed");

    // Poll /metrics until the engine reports progress or the run ends.
    let mut saw_live = false;
    let mut last_body = String::new();
    for _ in 0..200 {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            let req = format!("GET /metrics HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n");
            if stream.write_all(req.as_bytes()).is_ok() {
                let mut buf = Vec::new();
                let mut s = stream;
                let _ = std::io::Read::read_to_end(&mut s, &mut buf);
                let body = String::from_utf8_lossy(&buf).into_owned();
                // The HTTP response must be JSON metrics (not the HTML page).
                if body.contains("\"phase\"") {
                    last_body = body;
                    saw_live = true;
                    if last_body.contains("\"percent\":100") || last_body.contains("\"finished\":true") {
                        break;
                    }
                }
            }
        } else if child.try_wait().map(|s| s.is_some()).unwrap_or(false) {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let _ = child.wait();
    assert!(saw_live, "dashboard /metrics never returned JSON; last body: {}", last_body);
    assert!(
        last_body.contains("\"bytes_total\"") && last_body.contains("\"memory_bytes\""),
        "metrics payload incomplete: {}",
        last_body
    );
}

#[test]
fn gen_bin_smoke() {
    // gen binary used by docs/examples still works end-to-end.
    let dir = case_dir("gen");
    let p = dir.join(Path::new("g.txt"));
    let out = Command::new(gen_bin_path())
        .args([p.display().to_string().as_str(), "1000", "string", "3"])
        .output()
        .expect("gen failed");
    assert!(out.status.success());
    let n = fs::read_to_string(p.clone().as_path()).unwrap().lines().count();
    assert_eq!(n, 1000);
}
