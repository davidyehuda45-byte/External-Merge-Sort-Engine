# External Merge Sort Engine

Sort files **larger than RAM** with bounded memory. Numeric (binary u64), string, CSV/multi-key (per-key direction, case-insensitive, nulls-first/last, multiline quoted), and JSONL. Crash-resume, live dashboard (JSON + Prometheus), JSON output for CI. **Web GUI for non-technical staff (Cancel/Preview/Browse), Top-N + audit log with rotation.**

Built in Rust. Single binary. No runtime. Windows / Linux / macOS. 100% offline.

## Why buy this?

- Handles files larger than RAM (verified: 496 MB / 65M records on an 8GB laptop with `--max-memory 512MB`; architecture scales with disk + `--temp-dir`)
- CSV sort by column: `--format csv --key-column 2 --key-type numeric`, multi-key with per-key direction `--multi-key "0,1" --key-dir "asc,desc"`, `--ignore-case`, `--nulls last`, multiline quoted fields supported end-to-end
- Beginner-friendly: `--max-memory` optional (default 60% RAM), `--backup` (`.bak` before overwrite), `--split-by-size 100MB`, `--force` for non-interactive overwrite
- Production safety: pre-flight disk check, atomic output (`.part` + rename), `--verify`, exit codes 0-6
- Crash resume: `--resume list` / `--resume` (never redo completed chunks)
- Observability: progress bars, `--json` for scripts, `--dashboard 127.0.0.1:8080` live web UI + `/metrics` JSON + `/metrics-prom` Prometheus, `--log-file audit.jsonl` (10MB x5 rotation)
- Commercial flags: `--reverse`, `--unique`, `--header`, `--dry-run`
- GUI: Cancel/Preview/Browse files, job history; REST API: `POST /api/sort|cancel`, `GET /api/job/<id>|preview|files`

## Quickstart (GUI — buat staf non-teknis)

```powershell
.\mergesort.exe --gui 127.0.0.1:8080
# buka http://127.0.0.1:8080/ → Generate demo → Urutkan sekarang → Download
```

> `Start-GUI.bat` hanya tersedia di ZIP installer (`dist/MergeSortPro-*.zip`),
> bukan di repo. Dari repo, build dulu (`cargo build --release`) lalu jalankan
> `.\target\release\mergesort.exe --gui 127.0.0.1:8080`.

## Quickstart (CLI)

```powershell
# build
cargo build --release

# generate test data (100k rows)
.\target\release\gen.exe data.csv 100000 csv

# estimate first (free, no sorting)
.\target\release\mergesort.exe --input data.csv --output sorted.csv --max-memory 512MB --format csv --key-column 0 --key-type numeric --dry-run

# sort CSV by column 0 (numeric), keep header, verify
.\target\release\mergesort.exe --input data.csv --output sorted.csv --max-memory 512MB --format csv --key-column 0 --key-type numeric --header --verify

# descending + dedup + Top-1000 + audit log
.\target\release\mergesort.exe --input data.txt --output sorted.txt --max-memory 1GB --mode string --reverse --unique --limit 1000 --verify --log-file audit.jsonl

# live dashboard + JSON for CI
.\target\release\mergesort.exe --input big.bin --output big.sorted.bin --max-memory 1GB --json --dashboard 127.0.0.1:8080 --verify
```

## Full CLI (v2.0.1 — 20 fitur jualan)

```
--input <path> --output <path> [--max-memory <size>]   (default: 60% RAM, e.g. 512MB, 2GB)
--mode numeric|string|jsonl|excel
--format csv --key-column N [--multi-key "1,3"] [--key-type numeric|string|date] [--delimiter ,]
  [--key-dir "asc,desc"] [--ignore-case] [--nulls first|last]   (multiline quoted didukung)
--format jsonl --key-field user.age [--key-type numeric|string|date] [--ignore-case]
--header / --no-header   header keep / force-off (auto-detect if omitted)
--reverse, -r         descending
--unique, -u          drop identical rows (sort -u)
--dedupe-by 2 [--dedupe-keep first|last]   drop dupes by column
--limit N             Top-N only
--preview N [--preview-json]   head/tail preview, no commit (output optional)
--stats [--stats-json]         execution report block
--check               validate file only (exit 0=ready, 5=data issue)
--dry-run             smart estimate + recommendation (auto delimiter/header/encoding)
--encoding utf-8|latin-1|utf-16le|utf-16be (+ --encoding-in/--encoding-out)
.gz/.zip transparent; .zst rejected with guidance; .xlsx via --sheet NAME
--merge F1 F2 ...     merge pre-sorted files (no re-sort) + --output
--output-format csv|tsv|jsonl|sql [--sql-table T]
--split-by N | --split-by-size 100MB   split output (out-001.ext..., header diulang)
--backup              salin output lama ke <output>.bak sebelum timpa
--temp-dir <path>     temp chunks on another drive
--resume [id|list]    resume interrupted run
--verify              re-read output, check order + row count
--threads N           (default: all cores)
--max-open-files N    (default: auto)
--quiet, --json, --log-file audit.jsonl (rotasi 10MB x5), --dashboard [host:]PORT (/metrics + /metrics-prom)
--gui [host:]PORT [--brand NAME --brand-color HEX]   web console + presets + history + Cancel/Preview/Browse
--api [host:]PORT [--api-token T]   REST API (POST /api/sort|cancel, GET /api/job/<id>|preview|files)
--interactive         wizard tanya-jawab
--watch DIR [--output-dir D]        daemon auto-sort file baru
--version, --help
```

Bahasa jualan per fitur: "Lihat hasil dulu" (preview), "Laporan tiap proses" (stats),
"Tahu biaya sebelum jalan" (dry-run), "Pastikan file valid" (check),
"Tinggal kasih file" (auto-deteksi), "Excel tidak kacau" (encoding),
"Sort log JSON" (jsonl), "Bersihkan duplikat" (dedupe-by),
"Gabung harian jadi bulanan" (merge), "Langsung masuk DB" (output-format),
"Bagi data besar" (split-by), "Sekali setup selamanya klik" (preset),
"Excel 500MB? Bisa." (xlsx), "Set dan lupakan" (watch),
"Panggil dari aplikasi Anda" (API), "Software milik Anda" (white-label),
"Tidak perlu extract .gz/.zip (.zst ditolak jelas)" (kompresi), "Jalan di server Linux" (cross-platform).

Exit codes: `0` ok · `2` usage · `3` pre-flight · `4` I/O · `5` data · `6` verify.

## Examples

```powershell
# string sort, 8MB budget (forces multi-pass merge — good stress test)
.\target\release\mergesort.exe --input words.txt --output words.sorted.txt --max-memory 8MB --mode string --verify

# CSV multi-key: city (col 2) then name (col 3), string keys
.\target\release\mergesort.exe --input data.csv --output sorted.csv --max-memory 1GB --format csv --multi-key "2,3" --key-type string --header --verify

# CSV single numeric key with explicit type (consistent with quickstart)
.\target\release\mergesort.exe --input data.csv --output sorted.csv --max-memory 1GB --format csv --key-column 2 --key-type numeric --header --verify

# resume after crash / Ctrl-C
.\target\release\mergesort.exe --input big.bin --output big.sorted.bin --max-memory 1GB --resume list
.\target\release\mergesort.exe --input big.bin --output big.sorted.bin --max-memory 1GB --resume
```

## Dashboard

Open `http://127.0.0.1:8080/` during a run. `GET /metrics` returns JSON for Prometheus/CI:

```json
{"phase":"merge","elapsed_s":12.4,"finished":false,"chunk":{...},"merge":{...},"records":1000000,"memory_bytes":536870912}
```

`GET /metrics-prom` returns the same counters in Prometheus exposition format
(`mergesort_bytes_read`, `mergesort_records`, …) for scraping.

> Bind to `127.0.0.1` in production. No auth — do not expose to the public internet.

## Performance tips (for your sales demo)

- `--max-memory 50-70% RAM` is the sweet spot; `--threads = cores`
- Put `--temp-dir` on the fastest SSD, output on another disk if possible
- `--dry-run` first to size chunks/fan-in for the customer

## Benchmark (real measurement, 2026-09-15)

Dataset: binary numeric u64 (random 64-bit integers, no duplicates)
System: Windows x64, SSD, CPU 8-core
Config: `--max-memory 512MB --verify --mode numeric`

### 496 MB (65,000,000 records) — verified reference

| Metric | Value |
|---|---|
| Input size | 496 MB (520,000,000 bytes = 65,000,000 × u64) |
| Peak memory (RSS, JSON) | 486,948,864 bytes ≈ **464.4 MB** (90.7% dari 512MB budget) |
| Total time (internal) | **24.641 seconds** (read 0.77s + sort 1.63s + merge 11.37s) |
| Total time (wall-clock PS) | 24.78 seconds |
| Throughput | 1207.5 MB/min = **20.1 MB/s** (input MB) |
| Chunk count | **2 chunks** (kapasitas ~54.48M records/chunk @ 512MB) |
| Merge pass count | **1 pass** (2-way merge, single fan-in level) |
| Records read/written | 65,000,000 (row count match) |
| Verify result | **OK** (`verified: true`, full order + row-count re-check) |
| Exit code | 0 (OK) |

### 5 GB and larger — run the benchmark yourself (generic steps)

The table above is the verified reference (496 MB, `--max-memory 512MB`).
For a larger dataset, scale the same procedure — estimated peak disk usage
is roughly `input + output + temp chunks` (≈ 3× input size):

```powershell
# 1. Generate input (example: 5 GB = 640,000,000 × u64, binary numeric)
.\target\release\gen.exe bench\input5g.bin 640000000 numeric

# 2. Sort with verification + JSON stats (use --temp-dir on the fastest SSD)
.\target\release\mergesort.exe --input bench\input5g.bin --output bench\out.bin --max-memory 1GB --verify --json --stats --temp-dir bench\.temp

# 3. Record from JSON output: timing.total_s, peak_memory_bytes,
#    chunks.count, merge passes, throughput_mb_per_min
# 4. Cleanup after done
Remove-Item -Recurse -Force bench
```

> Run `--dry-run` first to check disk/memory estimates before a large sort.
> If pre-flight reports insufficient disk, free space or point `--temp-dir`
> to a drive with enough free space.

## Troubleshooting (exit codes)

| Code | Meaning | Typical cause / fix |
|---|---|---|
| 0 | OK | Success. With `--verify`, output order + row count re-checked. |
| 2 | Usage error | Bad flags / missing required args. Run `--help`. |
| 3 | Pre-flight failure | Unreadable input, unwritable output, or insufficient disk. Free space or use `--temp-dir` on another drive. |
| 4 | I/O error mid-run | Disk full or permission lost during run. Check disk space and file permissions. |
| 5 | Data error | Malformed input for the selected format (e.g. bad CSV column, truncated binary). Run `--check` / `--dry-run` to validate first. |
| 6 | Verify failure | `--verify` found output unsorted or row-count mismatch. Do not use the output; re-run or inspect temp/input. |

Common checks:
- `--check` validates the file only (exit 0 = ready, 5 = data issue).
- `--dry-run` estimates chunks, temp disk, and time without sorting.
- Keep `--max-memory` at 50–70% of RAM; put `--temp-dir` on the fastest SSD.

## Development & deployment

```powershell
cargo build --locked            # debug build
cargo build --release --locked  # release build (single binary, static CRT on Windows)
cargo clippy --locked --all-targets -- -D warnings   # must be clean
cargo test --locked --test correctness -- --skip prop # 12 tests
cargo test --locked --test extended                   # 22 tests (all commercial flags)
cargo test --locked --test output_modes -- json_summary_is_well_formed quiet_mode_prints_summary_only gen_bin_smoke
```

- Toolchain: Rust 1.88+ (`edition = "2024"`, see `rust-version` in `Cargo.toml`).
- GUI/API smoke: `mergesort --gui 127.0.0.1:18089` → `GET /api/health`, `/api/files?dir=.`, `/api/preview?path=…`.
- Installer: `make-installer.ps1` → `dist/MergeSortPro-*-windows-x64.zip` (+ `Uninstall.bat`, `*.sha256`). Signing/MSI roadmap: `SIGNING.md`, `deploy/MergeSort.wxs`.
- Daemon: `deploy/mergesort.service` (systemd) or `deploy/install-watch-task.ps1` (Windows logon task).
- Full changelog: `CHANGELOG.md` (Unreleased section lists the latest flags).

## License

MIT — see LICENSE. Custom builds (S3, gzip, GUI installer) available on request.
