# External Merge Sort Engine

> Sort very large files without needing a huge amount of RAM.

External Merge Sort Engine is a fast, offline file-sorting tool built with Rust. It can sort files that are much larger than your computer's available memory.

It supports **numeric data, text, CSV, JSONL, and Excel files**, with both a command-line interface and a web-based GUI for non-technical users.

**Works on Windows, Linux, and macOS. No runtime or internet connection required.**

---

## Why Use It?

Large files can be difficult to sort because your computer may not have enough RAM.

For example:

* You have a **1 TB file**
* Your laptop only has **8 GB RAM**
* You can still sort the file using a limited memory budget such as `--max-memory 512MB`

The engine splits the file into smaller parts, sorts them separately, and combines them into the final sorted file.

### Key Features

* **Sort huge files** — Designed for files from GBs to TBs.
* **Low memory usage** — Set exactly how much RAM the engine can use.
* **CSV sorting** — Sort by a specific column.
* **Multiple sorting rules** — Sort using multiple columns.
* **Remove duplicates** — Remove identical rows or duplicates based on a column.
* **Top-N results** — Keep only the first N results.
* **Preview results** — See what the result will look like before saving it.
* **Safe file handling** — The original file is protected while sorting.
* **Resume after interruption** — Continue an interrupted job without starting everything again.
* **Verify results** — Check that the final file is correctly sorted.
* **Live dashboard** — Watch the sorting process from your browser.
* **JSON output** — Easy to integrate with scripts and CI systems.
* **Web GUI** — Simple interface for people who do not use command-line tools.
* **Offline** — Your data stays on your computer.
* **Cross-platform** — Windows, Linux, and macOS.

---

## Quick Start: Web GUI

Don't like command-line tools? Humans have invented buttons for a reason.

Start the web interface:

```powershell
.\mergesort.exe --gui 127.0.0.1:8080
```

Or double-click:

```text
Start-GUI.bat
```

Then open:

```text
http://127.0.0.1:8080/
```

The GUI lets non-technical users:

1. Select a file
2. Choose what to sort
3. Preview the result
4. Start sorting
5. Monitor progress
6. Download the result
7. View previous jobs and audit logs

---

## Quick Start: CLI

### Build

```powershell
cargo build --release
```

### Generate test data

```powershell
.\target\release\gen.exe data.csv 100000 csv
```

This creates a CSV file containing 100,000 test rows.

### Check the estimated cost first

```powershell
.\target\release\mergesort.exe --input data.csv --output sorted.csv --max-memory 512MB --format csv --key-column 0 --key-type numeric --dry-run
```

This does **not** sort the file.

It estimates:

* Required disk space
* Memory usage
* Number of chunks
* Expected processing setup
* Recommended configuration

### Sort a CSV file

```powershell
.\target\release\mergesort.exe --input data.csv --output sorted.csv --max-memory 512MB --format csv --key-column 0 --key-type numeric --header --verify
```

### Sort text, remove duplicates, and keep only 1,000 results

```powershell
.\target\release\mergesort.exe --input data.txt --output sorted.txt --max-memory 1GB --mode string --reverse --unique --limit 1000 --verify --log-file audit.jsonl
```

### Sort while monitoring the process

```powershell
.\target\release\mergesort.exe --input big.bin --output big.sorted.bin --max-memory 1GB --json --dashboard 127.0.0.1:8080 --verify
```

---

# Features

## Preview Before Saving

Use:

```text
--preview N
```

See the first and last results before committing the final output.

**Simple explanation:**

> "See the result first."

---

## Check a File Before Sorting

Use:

```text
--check
```

The engine checks whether the file is valid before starting the sorting process.

**Simple explanation:**

> "Make sure the file is ready."

---

## Estimate Before Running

Use:

```text
--dry-run
```

The engine estimates the resources required before doing the actual work.

It can detect things such as:

* File format
* CSV delimiter
* Header
* Encoding
* Required disk space
* Memory requirements

**Simple explanation:**

> "Know the cost before you start."

---

## Sort CSV Files

Example:

```text
--format csv --key-column 2 --key-type numeric
```

You can sort CSV files by:

* Number
* Text
* Date
* Multiple columns

Example:

```text
--multi-key "2,3"
```

**Simple explanation:**

> "Sort your spreadsheet data the way you need."

---

## JSONL Support

Sort JSONL files using a specific field:

```text
--format jsonl --key-field user.age --key-type numeric
```

**Simple explanation:**

> "Sort structured application data."

---

## Remove Duplicates

Remove identical rows:

```text
--unique
```

Or remove duplicates based on a specific column:

```text
--dedupe-by 2
```

**Simple explanation:**

> "Clean up duplicate data."

---

## Top-N Results

Only keep a specific number of results:

```text
--limit 1000
```

Useful when you only need the first 1,000 results instead of the entire sorted file.

**Simple explanation:**

> "Get only the results you need."

---

## Resume After a Crash

If the process is interrupted, you can check available jobs:

```text
--resume list
```

Then continue the previous job:

```text
--resume
```

Completed chunks do not need to be processed again.

**Simple explanation:**

> "Continue where you left off."

---

## Verify the Result

Use:

```text
--verify
```

The engine checks the final file to make sure:

* Data is correctly sorted
* The number of records is correct
* The output can be read successfully

**Simple explanation:**

> "Make sure the result is correct."

---

## Merge Existing Sorted Files

Already have multiple sorted files?

Combine them without sorting everything again:

```text
--merge file1.csv file2.csv file3.csv --output monthly.csv
```

**Simple explanation:**

> "Combine daily files into one larger file."

---

## Split Large Output

Split the result into multiple files:

```text
--split-by 5
```

This creates files such as:

```text
out-001.csv
out-002.csv
out-003.csv
out-004.csv
out-005.csv
```

**Simple explanation:**

> "Split large data into smaller files."

---

## Different Output Formats

Supported output formats include:

```text
csv
tsv
jsonl
sql
```

SQL output:

```text
--output-format sql --sql-table users
```

**Simple explanation:**

> "Send the sorted data where you need it."

---

## Excel Support

Excel files can be processed using:

```text
--mode excel
```

You can select a specific sheet:

```text
--sheet NAME
```

**Simple explanation:**

> "Have a large Excel file? It can handle it."

---

## Compressed Files

The engine can work with:

```text
.gz
.zip
```

without manually extracting them first.

`.zst` is currently rejected with guidance instead of silently failing.

**Simple explanation:**

> "No need to extract supported compressed files first."

---

## Automatic File Monitoring

Use:

```text
--watch DIR
```

The engine can automatically process new files that appear in a directory.

**Simple explanation:**

> "Set it once and let it handle new files automatically."

---

## REST API

Use:

```text
--api 127.0.0.1:8080
```

Applications can communicate with the sorting engine through an API.

Example endpoints:

```text
POST /api/sort
GET  /api/job/<id>
```

**Simple explanation:**

> "Connect it to your own application."

---

## Web Dashboard

Start the dashboard:

```text
--dashboard 127.0.0.1:8080
```

Open:

```text
http://127.0.0.1:8080/
```

The dashboard shows information such as:

* Current progress
* Current phase
* Processing time
* Records processed
* Memory usage
* Merge progress
* Chunk progress

It also provides:

```text
GET /metrics
```

for JSON monitoring and CI systems.

Example:

```json
{
  "phase": "merge",
  "elapsed_s": 12.4,
  "finished": false,
  "records": 1000000,
  "memory_bytes": 536870912
}
```

> For security, bind the dashboard to `127.0.0.1` unless you have configured proper authentication and network security.

---

# Full CLI

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

# Example Use Cases

### Sort a large text file

```powershell
.\target\release\mergesort.exe --input words.txt --output words.sorted.txt --max-memory 8MB --mode string --verify
```

### Sort CSV using multiple columns

```powershell
.\target\release\mergesort.exe --input data.csv --output sorted.csv --max-memory 1GB --format csv --multi-key "2,3" --header --verify
```

### Continue an interrupted job

```powershell
.\target\release\mergesort.exe --input big.bin --output big.sorted.bin --max-memory 1GB --resume list
```

Then:

```powershell
.\target\release\mergesort.exe --input big.bin --output big.sorted.bin --max-memory 1GB --resume
```

---

# Performance

The engine is designed for computers with limited RAM.

Recommended starting point:

```text
--max-memory 50-70% of available RAM
```

For better performance:

* Use an SSD for temporary files
* Use `--temp-dir` on a fast drive
* Keep enough free disk space for temporary files and output
* Use multiple CPU threads when appropriate
* Run `--dry-run` before very large jobs

---

# Benchmark

**Real measurement: September 15, 2026**

### 496 MB File

Test data:

* File size: **496 MB**
* Records: **65,000,000**
* Data type: Binary `u64`
* Memory limit: **512 MB**
* CPU: 8-core
* Storage: SSD
* Operating system: Windows x64

Results:

| Metric          |        Result |
| --------------- | ------------: |
| Input size      |        496 MB |
| Records         |    65,000,000 |
| Peak memory     |      464.4 MB |
| Processing time | 24.64 seconds |
| Wall-clock time | 24.78 seconds |
| Throughput      |     20.1 MB/s |
| Chunks          |             2 |
| Merge passes    |             1 |
| Verification    |        Passed |
| Exit code       |             0 |

The complete input was sorted successfully and the final output passed the verification step.

---

# 5 GB Benchmark

A 5 GB benchmark could not be completed on the test machine because there was not enough free disk space.

The planned test required approximately:

```text
5 GB input
+ 5 GB output
+ 5-6 GB temporary files
-------------------------
≈ 16 GB peak disk usage
```

The engine's disk-space check correctly stopped a test when there was not enough available space.

This is intentional: **the engine checks disk requirements before starting large jobs instead of discovering the problem halfway through.**

---

# Exit Codes

```text
0 = Success
2 = Invalid command or arguments
3 = Not enough resources / pre-flight failure
4 = File or disk error
5 = Invalid data
6 = Verification failed
```

This makes the engine easier to use in automation and CI pipelines.

---

# Built for Different Users

### Non-technical staff

Use the **Web GUI**.

```text
Select file → Choose options → Preview → Sort → Download
```

### Developers

Use the **CLI** or **REST API**.

### Data teams

Use:

* CSV / JSONL sorting
* Top-N
* Deduplication
* Split output
* Merge files
* JSON monitoring

### IT / Operations

Use:

* `--verify`
* `--dry-run`
* `--check`
* `--resume`
* Dashboard
* JSON output
* Audit logs
* API
* Watch mode

---

# Privacy

The engine is **100% offline**.

Your files do not need to be uploaded to a cloud service.

Processing happens directly on your computer or server.

---

# Platform Support

* Windows
* Linux
* macOS

The application is distributed as a **single binary** and does not require a separate runtime.

---

# License

MIT License.

Custom commercial builds are available for:

* S3 integration
* Additional compression formats
* Custom GUI
* Custom branding
* Enterprise deployments

See `LICENSE` for the full license.
