// Extended coverage: CSV, reverse, unique, dedupe-by, limit, header,
// preview, check, dry-run, merge, split-by, verify-fail, json summary.
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static CTR: AtomicU64 = AtomicU64::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mergesort")
}

fn case_dir(name: &str) -> PathBuf {
    let n = CTR.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!(
        "msort-ext-{}-{}-{}",
        std::process::id(),
        n,
        name
    ));
    let _ = fs::create_dir_all(&d);
    d
}

struct R {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> R {
    let o = Command::new(bin()).args(args).output().expect("run mergesort");
    R {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

fn wlines(p: &Path, lines: &[&str]) {
    let f = fs::File::create(p).unwrap();
    let mut w = BufWriter::new(f);
    for l in lines {
        w.write_all(l.as_bytes()).unwrap();
        w.write_all(b"\n").unwrap();
    }
    w.flush().unwrap();
}

fn rlines(p: &Path) -> Vec<String> {
    let d = fs::read(p).unwrap();
    String::from_utf8(d).unwrap().lines().map(|s| s.to_string()).collect()
}

fn s(a: &[String]) -> Vec<&str> {
    a.iter().map(|x| x.as_str()).collect()
}

fn base_args(inp: &Path, out: &Path, mem: &str) -> Vec<String> {
    vec![
        "--input".into(),
        inp.display().to_string(),
        "--output".into(),
        out.display().to_string(),
        "--max-memory".into(),
        mem.into(),
    ]
}

#[test]
fn csv_numeric_key0_header() {
    let d = case_dir("csvnum");
    let (inp, out) = (d.join("in.csv"), d.join("out.csv"));
    wlines(&inp, &["id,name", "30,b", "5,a", "20,c"]);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend(["--format".into(), "csv".into(), "--key-column".into(), "0".into(), "--key-type".into(), "numeric".into(), "--header".into(), "--verify".into()]);
    let r = run(&s(&a));
    assert!(r.code == 0, "code={} out={} err={}", r.code, r.stdout, r.stderr);
    assert_eq!(rlines(&out), vec!["id,name", "5,a", "20,c", "30,b"]);
}

#[test]
fn csv_multikey_string() {
    let d = case_dir("csvmulti");
    let (inp, out) = (d.join("in.csv"), d.join("out.csv"));
    wlines(&inp, &["b,2", "a,2", "a,1"]);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend(["--format".into(), "csv".into(), "--multi-key".into(), "0,1".into(), "--verify".into()]);
    // data tanpa header: jangan pakai --header agar baris pertama ikut disort.
    let r = run(&s(&a));
    assert!(r.code == 0, "code={} err={}", r.code, r.stderr);
    assert_eq!(rlines(&out), vec!["a,1", "a,2", "b,2"]);
}

#[test]
fn reverse_desc() {
    let d = case_dir("rev");
    let (inp, out) = (d.join("in.txt"), d.join("out.txt"));
    wlines(&inp, &["a", "c", "b"]);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend(["--mode".into(), "string".into(), "--reverse".into(), "--verify".into()]);
    let r = run(&s(&a));
    assert_eq!(r.code, 0, "err={}", r.stderr);
    assert_eq!(rlines(&out), vec!["c", "b", "a"]);
}

#[test]
fn unique_dedup() {
    let d = case_dir("uniq");
    let (inp, out) = (d.join("in.txt"), d.join("out.txt"));
    wlines(&inp, &["b", "a", "b", "a", "c"]);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend(["--mode".into(), "string".into(), "--unique".into(), "--verify".into()]);
    let r = run(&s(&a));
    assert_eq!(r.code, 0, "err={}", r.stderr);
    assert_eq!(rlines(&out), vec!["a", "b", "c"]);
}

#[test]
fn dedupe_by_first_last() {
    for (keep, want) in [("first", vec!["1,a", "2,b"]), ("last", vec!["1,z", "2,b"])] {
        let d = case_dir(&format!("dedupe{keep}"));
        let (inp, out) = (d.join("in.csv"), d.join("out.csv"));
        wlines(&inp, &["1,a", "1,z", "2,b"]);
        let mut a = base_args(&inp, &out, "8MB");
        a.extend([
            "--format".into(), "csv".into(), "--key-column".into(), "0".into(),
            "--dedupe-by".into(), "0".into(), "--dedupe-keep".into(), keep.into(),
        ]);
        let r = run(&s(&a));
        assert_eq!(r.code, 0, "keep={keep} err={}", r.stderr);
        assert_eq!(rlines(&out), want, "keep={keep}");
    }
}

#[test]
fn limit_topn() {
    let d = case_dir("limit");
    let (inp, out) = (d.join("in.txt"), d.join("out.txt"));
    wlines(&inp, &["d", "b", "a", "c"]);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend(["--mode".into(), "string".into(), "--limit".into(), "2".into()]);
    let r = run(&s(&a));
    assert_eq!(r.code, 0, "err={}", r.stderr);
    assert_eq!(rlines(&out), vec!["a", "b"]);
}

#[test]
fn preview_no_commit() {
    let d = case_dir("preview");
    let inp = d.join("in.txt");
    wlines(&inp, &["c", "a", "b"]);
    let r = run(&[
        "--input", &inp.display().to_string(),
        "--max-memory", "8MB",
        "--mode", "string",
        "--preview", "2",
    ]);
    assert_eq!(r.code, 0, "err={}", r.stderr);
    assert!(r.stdout.contains('a'), "stdout={}", r.stdout);
}

#[test]
fn check_ok_and_bad() {
    let d = case_dir("check");
    let good = d.join("good.csv");
    wlines(&good, &["a,b", "1,2"]);
    let r = run(&["--input", &good.display().to_string(), "--check"]);
    assert_eq!(r.code, 0, "err={}", r.stderr);
    let bad = d.join("bad.bin");
    fs::write(&bad, b"abc").unwrap(); // bukan kelipatan 8 untuk numeric
    let r2 = run(&["--input", &bad.display().to_string(), "--check", "--mode", "numeric"]);
    assert!(r2.code == 5, "code={} err={}", r2.code, r2.stderr);
}

#[test]
fn dry_run_estimates() {
    let d = case_dir("dry");
    let inp = d.join("in.txt");
    wlines(&inp, &["b", "a"]);
    let r = run(&[
        "--input", &inp.display().to_string(),
        "--output", &d.join("o.txt").display().to_string(),
        "--max-memory", "8MB",
        "--mode", "string",
        "--dry-run",
    ]);
    assert_eq!(r.code, 0, "err={}", r.stderr);
    assert!(r.stdout.contains("Estimasi") || r.stdout.contains("estimasi") || r.stdout.contains("temp"), "stdout={}", r.stdout);
}

#[test]
fn merge_presorted() {
    let d = case_dir("merge");
    let (f1, f2, out) = (d.join("a.txt"), d.join("b.txt"), d.join("o.txt"));
    wlines(&f1, &["a", "c"]);
    wlines(&f2, &["b", "d"]);
    let r = run(&[
        "--merge", &f1.display().to_string(), &f2.display().to_string(),
        "--output", &out.display().to_string(),
        "--max-memory", "8MB",
        "--mode", "string",
    ]);
    assert_eq!(r.code, 0, "err={} out={}", r.stderr, r.stdout);
    assert_eq!(rlines(&out), vec!["a", "b", "c", "d"]);
}

#[test]
fn split_by_two() {
    let d = case_dir("split");
    let (inp, out) = (d.join("in.txt"), d.join("o.txt"));
    wlines(&inp, &["d", "b", "a", "c"]);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend(["--mode".into(), "string".into(), "--split-by".into(), "2".into()]);
    let r = run(&s(&a));
    assert_eq!(r.code, 0, "err={}", r.stderr);
    let stem = out.with_extension("");
    let p1 = d.join(format!("{}-001.txt", stem.file_name().unwrap().to_string_lossy()));
    let p2 = d.join(format!("{}-002.txt", stem.file_name().unwrap().to_string_lossy()));
    // Nama part mengikuti pola out-001.ext di dir yang sama.
    let mut found = vec![];
    for e in fs::read_dir(&d).unwrap().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.contains("-001") || n.contains("-002") {
            found.push(n);
        }
    }
    assert!(found.len() >= 2, "parts missing: {:?} (p1={:?} p2={:?})", found, p1, p2);
}

#[test]
fn verify_fails_on_unsorted_merge() {
    let d = case_dir("vfail");
    let (f1, out) = (d.join("a.txt"), d.join("o.txt"));
    wlines(&f1, &["b", "a"]); // tidak sorted
    let r = run(&[
        "--merge", &f1.display().to_string(),
        "--output", &out.display().to_string(),
        "--max-memory", "8MB",
        "--mode", "string",
        "--verify",
    ]);
    assert!(r.code == 6, "code={} err={}", r.code, r.stderr);
}

#[test]
fn json_summary_verified() {
    let d = case_dir("json");
    let (inp, out) = (d.join("in.txt"), d.join("out.txt"));
    wlines(&inp, &["b", "a"]);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend(["--mode".into(), "string".into(), "--verify".into(), "--json".into()]);
    let r = run(&s(&a));
    assert_eq!(r.code, 0, "err={}", r.stderr);
    assert!(r.stdout.contains("\"verified\""), "stdout={}", r.stdout);
}

#[test]
fn input_eq_output_rejected() {
    let d = case_dir("same");
    let inp = d.join("in.txt");
    wlines(&inp, &["b", "a"]);
    let r = run(&[
        "--input", &inp.display().to_string(),
        "--output", &inp.display().to_string(),
        "--max-memory", "8MB",
        "--mode", "string",
    ]);
    assert_eq!(r.code, 2, "code={} err={}", r.code, r.stderr);
}

#[test]
fn ignore_case_string() {
    let d = case_dir("icase");
    let (inp, out) = (d.join("in.txt"), d.join("out.txt"));
    wlines(&inp, &["b", "A", "c"]);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend(["--mode".into(), "string".into(), "--ignore-case".into(), "--verify".into()]);
    let r = run(&s(&a));
    assert_eq!(r.code, 0, "err={}", r.stderr);
    // A (lower a) sorts first; original case preserved.
    assert_eq!(rlines(&out), vec!["A", "b", "c"]);
}

#[test]
fn key_dir_mixed() {
    let d = case_dir("keydir");
    let (inp, out) = (d.join("in.csv"), d.join("out.csv"));
    wlines(&inp, &["a,2", "a,1", "b,1"]);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend([
        "--format".into(), "csv".into(),
        "--multi-key".into(), "0,1".into(),
        "--key-dir".into(), "asc,desc".into(),
        "--verify".into(),
    ]);
    let r = run(&s(&a));
    assert_eq!(r.code, 0, "err={}", r.stderr);
    assert_eq!(rlines(&out), vec!["a,2", "a,1", "b,1"]);
}

#[test]
fn key_dir_needs_matching_count() {
    let d = case_dir("keydirbad");
    let (inp, out) = (d.join("in.csv"), d.join("out.csv"));
    wlines(&inp, &["a,2", "b,1"]);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend([
        "--format".into(), "csv".into(),
        "--multi-key".into(), "0,1".into(),
        "--key-dir".into(), "asc".into(),
    ]);
    let r = run(&s(&a));
    assert_eq!(r.code, 2, "code={} err={}", r.code, r.stderr);
}

#[test]
fn nulls_last() {
    let d = case_dir("nulls");
    let (inp, out) = (d.join("in.csv"), d.join("out.csv"));
    wlines(&inp, &["b,2", ",1", "a,3"]);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend([
        "--format".into(), "csv".into(),
        "--key-column".into(), "0".into(),
        "--nulls".into(), "last".into(),
        "--verify".into(),
    ]);
    let r = run(&s(&a));
    assert_eq!(r.code, 0, "err={}", r.stderr);
    assert_eq!(rlines(&out), vec!["a,3", "b,2", ",1"]);
}

#[test]
fn multiline_csv() {
    let d = case_dir("ml");
    let (inp, out) = (d.join("in.csv"), d.join("out.csv"));
    // Quoted newline stays inside one record; sort by numeric col 0.
    fs::write(&inp, "id,note\n2,\"bar\nbaz\"\n1,aaa\n").unwrap();
    let mut a = base_args(&inp, &out, "8MB");
    a.extend([
        "--format".into(), "csv".into(),
        "--key-column".into(), "0".into(),
        "--key-type".into(), "numeric".into(),
        "--header".into(), "--verify".into(),
    ]);
    let r = run(&s(&a));
    assert_eq!(r.code, 0, "err={} out={}", r.stderr, r.stdout);
    let raw = fs::read(&out).unwrap();
    assert_eq!(raw, b"id,note\n1,aaa\n2,\"bar\nbaz\"\n".to_vec());
}

#[test]
fn split_by_size() {
    let d = case_dir("sbs");
    let (inp, out) = (d.join("in.txt"), d.join("o.txt"));
    let rows: Vec<String> = (0..200).map(|i| format!("row{:04}", 200 - i)).collect();
    let rws: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
    wlines(&inp, &rws);
    let mut a = base_args(&inp, &out, "8MB");
    a.extend(["--mode".into(), "string".into(), "--split-by-size".into(), "1KB".into(), "--force".into()]);
    let r = run(&s(&a));
    assert_eq!(r.code, 0, "err={}", r.stderr);
    let mut n = 0;
    for e in fs::read_dir(&d).unwrap().flatten() {
        let nm = e.file_name().to_string_lossy().into_owned();
        if nm.starts_with("o-") {
            n += 1;
        }
    }
    assert!(n >= 2, "expected >=2 parts, found {}", n);
}

#[test]
fn backup_creates_bak() {
    let d = case_dir("bak");
    let (inp, out) = (d.join("in.txt"), d.join("out.txt"));
    wlines(&inp, &["b", "a"]);
    fs::write(&out, "OLD\n").unwrap();
    let mut a = base_args(&inp, &out, "8MB");
    a.extend(["--mode".into(), "string".into(), "--backup".into()]);
    let r = run(&s(&a));
    assert_eq!(r.code, 0, "err={}", r.stderr);
    let bak = PathBuf::from(format!("{}.bak", out.display()));
    assert_eq!(fs::read(&bak).unwrap(), b"OLD\n".to_vec());
    assert_eq!(rlines(&out), vec!["a", "b"]);
}

#[test]
fn auto_memory_default() {
    // Tanpa --max-memory: pakai default 60% RAM, tetap jalan + verify OK.
    let d = case_dir("automem");
    let (inp, out) = (d.join("in.txt"), d.join("out.txt"));
    wlines(&inp, &["b", "a"]);
    let r = run(&[
        "--input", &inp.display().to_string(),
        "--output", &out.display().to_string(),
        "--mode", "string",
        "--verify",
    ]);
    assert_eq!(r.code, 0, "err={}", r.stderr);
    assert_eq!(rlines(&out), vec!["a", "b"]);
}
