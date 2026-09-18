# External Merge Sort Engine

Sort files **larger than RAM** with bounded memory. Numeric (binary u64), string, and CSV/multi-key. Crash-resume, live dashboard, JSON output for CI. **Web GUI for non-technical staff, Top-N + audit log.**

Built in Rust. Single binary. No runtime. Windows / Linux / macOS. 100% offline.

## Why buy this?

- Handles 2GB → 1TB+ files on an 8GB laptop (`--max-memory 512MB`)
- CSV sort by column: `--format csv --key-column 2 --key-type numeric`
- Production safety: pre-flight disk check, atomic output (`.part` + rename), `--verify`, exit codes 0-6
- Crash resume: `--resume list` / `--resume` (never redo completed chunks)
- Observability: progress bars, `--json` for scripts, `--dashboard 127.0.0.1:8080` live web UI + `/metrics`
- Commercial flags: `--reverse`, `--unique`, `--header`, `--dry-run`

## Quickstart (GUI — buat staf non-teknis)

```powershell
.\mergesort.exe --gui 127.0.0.1:8080
# atau double-click Start-GUI.bat dari ZIP installer
# buka http://127.0.0.1:8080/ → Generate demo → Urutkan sekarang → Download
```

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
--input <path> --output <path> --max-memory <size>   (required, e.g. 512MB, 2GB)
--mode numeric|string|jsonl|excel
--format csv --key-column N [--multi-key "1,3"] [--key-type numeric|string|date] [--delimiter ,]
--format jsonl --key-field user.age [--key-type numeric|string|date]
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
--split-by N          split output into N files (out-001.ext...)
--temp-dir <path>     temp chunks on another drive
--resume [id|list]    resume interrupted run
--verify              re-read output, check order + row count
--threads N           (default: all cores)
--max-open-files N    (default: auto)
--quiet, --json, --log-file audit.jsonl, --dashboard [host:]PORT
--gui [host:]PORT [--brand NAME --brand-color HEX]   web console + presets + history
--api [host:]PORT [--api-token T]   REST API (POST /api/sort, GET /api/job/<id>)
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
"Tidak perlu extract" (kompresi), "Jalan di server Linux" (cross-platform).

Exit codes: `0` ok · `2` usage · `3` pre-flight · `4` I/O · `5` data · `6` verify.

## Examples

```powershell
# string sort, 8MB budget (forces multi-pass merge — good stress test)
.\target\release\mergesort.exe --input words.txt --output words.sorted.txt --max-memory 8MB --mode string --verify

# CSV multi-key: city (col 2) then name (col 3)
.\target\release\mergesort.exe --input data.csv --output sorted.csv --max-memory 1GB --format csv --multi-key "2,3" --header --verify

# resume after crash / Ctrl-C
.\target\release\mergesort.exe --input big.bin --output big.sorted.bin --max-memory 1GB --resume list
.\target\release\mergesort.exe --input big.bin --output big.sorted.bin --max-memory 1GB --resume
```

## Dashboard

Open `http://127.0.0.1:8080/` during a run. `GET /metrics` returns JSON for Prometheus/CI:

```json
{"phase":"merge","elapsed_s":12.4,"finished":false,"chunk":{...},"merge":{...},"records":1000000,"memory_bytes":536870912}
```

> Bind to `127.0.0.1` in production. No auth — do not expose to the public internet.

## Performance tips (for your sales demo)

- `--max-memory 50-70% RAM` is the sweet spot; `--threads = cores`
- Put `--temp-dir` on the fastest SSD, output on another disk if possible
- `--dry-run` first to size chunks/fan-in for the customer

## Benchmark (real measurement, 2026-09-15)

Dataset: binary numeric u64 (random 64-bit integers, no duplicates)
System: Windows x64, SSD, CPU 8-core
Config: `--max-memory 512MB --verify --mode numeric`

### 496 MB (65,000,000 records) — ran successfully (disk C: free terbatas, lihat catatan di bawah)

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

### 5 GB Benchmark — Tidak Bisa Dijalankan Karena Disk C: Tidak Cukup

Rencana awal: 5 GB input (640M u64) + 5 GB output + 5-6 GB temp chunks = ~16 GB peak disk usage.
Actual kondisi saat ini:
- Free C: **hanya ~7.15 GB di awal** (terpakai target/debug 2.3GB, target/release 800MB, test artifacts, pagefile)
- Drive E: free 15.46 GB **tapi TRAE sandbox tidak mengizinkan write ke luar working directory C:**
- Preflight disk check Engine berjalan BENAR: menolak sort 1.4GB input dengan pesan `kebutuhan ~4.40 GB, tersedia 3.44 GB`.

**Untuk menjalankan 5 GB benchmark nyata di mesin ini:**
```powershell
# Di PowerShell LUAR sandbox (atau izinkan write E: di Settings > Permission)
$bench = 'E:\bench5g'
mkdir $bench
.\target\release\gen.exe $bench\input5g.bin 640000000 numeric        # 5120 MB = 640M u64
.\target\release\mergesort.exe --input $bench\input5g.bin --output $bench\out.bin --max-memory 1GB --verify --json --stats --temp-dir "$bench\.temp"
# Catat nilai dari JSON output: timing.total_s, peak_memory_bytes, chunks.count, merge passes (dari fan-in log), throughput_mb_per_min
Remove-Item -Recurse -Force $bench   # cleanup setelah selesai
```

## License

MIT — see LICENSE. Custom builds (S3, gzip, GUI installer) available on request.
