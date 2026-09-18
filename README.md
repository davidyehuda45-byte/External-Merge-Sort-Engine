# External Merge Sort Engine

**Sort files far larger than your available RAM — offline, fast, and safe.**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey)
![Made with Rust](https://img.shields.io/badge/made%20with-Rust-orange)

External Merge Sort Engine is a Rust-based, cross-platform tool for sorting files that don't fit in memory — think 1 TB files on an 8 GB laptop. It splits the file into memory-sized chunks, sorts each chunk, and merges them into a fully sorted output, all while staying within a memory budget you define.

Works with numeric data, plain text, CSV, JSONL, and Excel files. Use it as a CLI, a REST API, or a browser-based GUI for non-technical users.

---

## Table of Contents

- [Why Use It](#why-use-it)
- [Key Features](#key-features)
- [Quick Start](#quick-start)
  - [Web GUI](#web-gui)
  - [CLI](#cli)
- [Feature Guide](#feature-guide)
- [Full CLI Reference](#full-cli-reference)
- [Example Use Cases](#example-use-cases)
- [Performance](#performance)
- [Benchmark](#benchmark)
- [Exit Codes](#exit-codes)
- [Who It's For](#who-its-for)
- [Privacy](#privacy)
- [Platform Support](#platform-support)
- [License](#license)

---

## Why Use It

Sorting a file bigger than your RAM normally means either buying more memory or writing your own chunk-and-merge logic. This engine does that for you:

- You have a **1 TB file**
- Your machine only has **8 GB RAM**
- You sort it anyway with a fixed budget: `--max-memory 512MB`

The file is split into smaller sorted chunks, then merged into the final sorted output — no need to load everything into memory at once.

## Key Features

| Category | Capabilities |
|---|---|
| **Scale** | Sort files from a few MB up to multiple TB |
| **Memory control** | Hard cap on RAM usage via `--max-memory` |
| **Formats** | CSV, TSV, JSONL, Excel, plain text, binary |
| **Sorting** | Single or multi-column keys, numeric/string/date, ascending/descending |
| **Data cleanup** | Remove duplicates (whole row or by column) |
| **Filtering** | Keep only the top N results |
| **Safety** | Original file untouched, dry-run cost estimation, pre-flight disk checks |
| **Resilience** | Resume an interrupted job without re-processing finished chunks |
| **Verification** | Post-sort integrity check (`--verify`) |
| **Interfaces** | CLI, REST API, Web GUI, live dashboard |
| **Automation** | JSON output, audit logs, `--watch` for auto-processing new files |
| **Offline** | No internet connection or cloud upload required |

---

## Quick Start

### Web GUI

For users who prefer a graphical interface:

```bash
.\mergesort.exe --gui 127.0.0.1:8080
```

Or on Windows, double-click `Start-GUI.bat`, then open **http://127.0.0.1:8080/**.

From there you can: select a file → choose sort options → preview → sort → download the result → review past jobs and audit logs.

### CLI

**1. Build from source**

```bash
cargo build --release
```

**2. Generate test data (optional)**

```bash
.\target\release\gen.exe data.csv 100000 csv
```

**3. Estimate cost before running (recommended for large files)**

```bash
.\target\release\mergesort.exe --input data.csv --output sorted.csv \
  --max-memory 512MB --format csv --key-column 0 --key-type numeric --dry-run
```

This doesn't sort anything — it reports required disk space, memory usage, chunk count, and a recommended configuration.

**4. Sort a CSV file**

```bash
.\target\release\mergesort.exe --input data.csv --output sorted.csv \
  --max-memory 512MB --format csv --key-column 0 --key-type numeric --header --verify
```

**5. Sort text, dedupe, and keep only the top 1,000 results**

```bash
.\target\release\mergesort.exe --input data.txt --output sorted.txt \
  --max-memory 1GB --mode string --reverse --unique --limit 1000 --verify --log-file audit.jsonl
```

**6. Sort while watching progress on a dashboard**

```bash
.\target\release\mergesort.exe --input big.bin --output big.sorted.bin \
  --max-memory 1GB --json --dashboard 127.0.0.1:8080 --verify
```

---

## Feature Guide

<details>
<summary><b>Preview before saving</b> — <code>--preview N</code></summary>

See the first and last N results before committing to the final output.
</details>

<details>
<summary><b>Check a file before sorting</b> — <code>--check</code></summary>

Validates the file before the sort process starts.
</details>

<details>
<summary><b>Estimate before running</b> — <code>--dry-run</code></summary>

Detects file format, CSV delimiter, header, encoding, required disk space, and memory needs — without doing the actual sort.
</details>

<details>
<summary><b>Sort CSV files</b> — <code>--format csv --key-column N --key-type numeric</code></summary>

Sort by number, text, or date. Combine multiple columns with `--multi-key "2,3"`.
</details>

<details>
<summary><b>JSONL support</b> — <code>--format jsonl --key-field user.age --key-type numeric</code></summary>

Sort structured, nested JSON-lines data by a specific field.
</details>

<details>
<summary><b>Remove duplicates</b> — <code>--unique</code> / <code>--dedupe-by 2</code></summary>

Drop identical rows entirely, or deduplicate based on one specific column.
</details>

<details>
<summary><b>Top-N results</b> — <code>--limit 1000</code></summary>

Keep only the first N results instead of writing the full sorted file.
</details>

<details>
<summary><b>Resume after a crash</b> — <code>--resume list</code> / <code>--resume</code></summary>

List recoverable jobs, then continue one without redoing completed chunks.
</details>

<details>
<summary><b>Verify the result</b> — <code>--verify</code></summary>

Confirms the output is correctly sorted, has the right record count, and is readable.
</details>

<details>
<summary><b>Merge existing sorted files</b> — <code>--merge file1.csv file2.csv file3.csv --output monthly.csv</code></summary>

Combine already-sorted files into one without re-sorting everything.
</details>

<details>
<summary><b>Split large output</b> — <code>--split-by 5</code></summary>

Splits the result into multiple numbered files (`out-001.csv`, `out-002.csv`, ...).
</details>

<details>
<summary><b>Different output formats</b> — <code>--output-format csv|tsv|jsonl|sql</code></summary>

Includes SQL export via `--sql-table users`.
</details>

<details>
<summary><b>Excel support</b> — <code>--mode excel --sheet NAME</code></summary>

Sort large Excel files, targeting a specific sheet.
</details>

<details>
<summary><b>Compressed input</b> — <code>.gz</code> / <code>.zip</code></summary>

Processed directly, no manual extraction needed. `.zst` is currently rejected with guidance rather than failing silently.
</details>

<details>
<summary><b>Automatic file monitoring</b> — <code>--watch DIR</code></summary>

Automatically processes new files as they appear in a directory.
</details>

<details>
<summary><b>REST API</b> — <code>--api 127.0.0.1:8080</code></summary>

```
POST /api/sort
GET  /api/job/<id>
```

Lets other applications trigger and track sort jobs programmatically.
</details>

<details>
<summary><b>Web dashboard</b> — <code>--dashboard 127.0.0.1:8080</code></summary>

Live view of progress, phase, processing time, records processed, memory usage, and chunk/merge progress. Also exposes `GET /metrics` for CI/monitoring:

```json
{
  "phase": "merge",
  "elapsed_s": 12.4,
  "finished": false,
  "records": 1000000,
  "memory_bytes": 536870912
}
```

> ⚠️ For security, bind the dashboard to `127.0.0.1` unless authentication and network security are properly configured.
</details>

---

## Full CLI Reference

```text
--input <path> --output <path> --max-memory <size>
--mode numeric|string|jsonl|excel

--format csv
--key-column N
--multi-key "1,3"
--key-type numeric|string|date
--delimiter ,

--format jsonl
--key-field user.age

--header
--no-header

--reverse, -r
--unique, -u

--dedupe-by 2
--dedupe-keep first|last

--limit N

--preview N
--preview-json

--stats
--stats-json

--check
--dry-run

--encoding utf-8|latin-1|utf-16le|utf-16be
--encoding-in
--encoding-out

.gz/.zip
--sheet NAME

--merge F1 F2 ...
--output-format csv|tsv|jsonl|sql
--sql-table T

--split-by N
--temp-dir <path>

--resume [id|list]

--verify

--threads N
--max-open-files N

--quiet
--json
--log-file audit.jsonl

--dashboard [host:]PORT

--gui [host:]PORT
--brand NAME
--brand-color HEX

--api [host:]PORT
--api-token T

--interactive

--watch DIR
--output-dir D

--version
--help
```

---

## Example Use Cases

**Sort a large text file**

```bash
.\target\release\mergesort.exe --input words.txt --output words.sorted.txt \
  --max-memory 8MB --mode string --verify
```

**Sort CSV using multiple columns**

```bash
.\target\release\mergesort.exe --input data.csv --output sorted.csv \
  --max-memory 1GB --format csv --multi-key "2,3" --header --verify
```

**Continue an interrupted job**

```bash
.\target\release\mergesort.exe --input big.bin --output big.sorted.bin --max-memory 1GB --resume list
.\target\release\mergesort.exe --input big.bin --output big.sorted.bin --max-memory 1GB --resume
```

---

## Performance

The engine is designed for machines with limited RAM.

- Recommended starting point: `--max-memory` set to **50–70% of available RAM**
- Use an SSD for temp files (`--temp-dir`) for best throughput
- Keep enough free disk space for temp files + output
- Use `--threads` to take advantage of multiple CPU cores
- Always run `--dry-run` before very large jobs

## Benchmark

**Measured September 15, 2026** — 8-core CPU, SSD, Windows x64

| Metric | Result |
|---|---|
| Input size | 496 MB |
| Records | 65,000,000 |
| Memory limit | 512 MB |
| Peak memory used | 464.4 MB |
| Processing time | 24.64 s |
| Wall-clock time | 24.78 s |
| Throughput | 20.1 MB/s |
| Chunks | 2 |
| Merge passes | 1 |
| Verification | ✅ Passed |
| Exit code | 0 |

> A 5 GB benchmark was attempted but couldn't complete on the test machine due to insufficient free disk space (needed ≈16 GB peak: input + output + temp files). The engine's pre-flight disk check correctly stopped the run rather than failing partway through — this is the intended, safe behavior.

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `2` | Invalid command or arguments |
| `3` | Not enough resources / pre-flight check failed |
| `4` | File or disk error |
| `5` | Invalid data |
| `6` | Verification failed |

Designed to plug cleanly into automation and CI pipelines.

## Who It's For

| User | Recommended interface |
|---|---|
| Non-technical staff | Web GUI — select file → choose options → preview → sort → download |
| Developers | CLI or REST API |
| Data teams | CSV/JSONL sorting, Top-N, dedupe, split, merge, JSON monitoring |
| IT / Operations | `--verify`, `--dry-run`, `--check`, `--resume`, dashboard, audit logs, `--watch` |

## Privacy

100% offline. Files never leave your machine — no cloud upload, no external service required.

## Platform Support

Windows · Linux · macOS — distributed as a single binary, no runtime dependency.

## License

MIT — see [LICENSE](LICENSE).

Custom commercial builds available on request for: S3 integration, additional compression formats, custom GUI, custom branding, and enterprise deployments.
