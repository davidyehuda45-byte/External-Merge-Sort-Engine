# Sales Kit — External Merge Sort Engine

## 30-Second Pitch

> **"Sort files that are larger than your available RAM. Handle 2GB files on an 8GB laptop, and 1TB+ files on servers. Sort CSV files by column, resume interrupted jobs, monitor progress with a live dashboard, and export JSON for CI. There is also a simple GUI for non-technical staff."**

---

## 5-Minute Demo

### 1. Start with the GUI

Run:

```text
Start-GUI.bat
```

Then:

```text
Generate demo 10k
→ Sort Now
→ Download
```

No command line required.

### 2. Show the Developer Workflow

Run `--dry-run` first to get a free estimate.

Then sort 200k rows using:

```text
--verify --json
```

Show the processing information and verification result.

### 3. Test Crash Recovery

Stop the process with `Ctrl-C` while it is running.

Then run:

```text
--resume list
```

and:

```text
--resume
```

The engine continues from the completed work instead of starting everything from the beginning.

### 4. Show Advanced Sorting

Demonstrate:

```text
--reverse --unique --header --limit 1000
```

This can sort in reverse order, remove duplicates, preserve headers, and return only the first 1,000 results.

### 5. Finish with the Audit Log

Every run can be recorded using:

```text
--log-file audit.jsonl
```

This gives you a machine-readable history of sorting jobs.

---

# Pricing

The engine is offered using a **custom development/service model**.

### Personal / Team

Custom builds based on your requirements.

### Enterprise

Custom solutions can include:

* SLA
* S3 integration
* Gzip support
* Custom GUI branding
* MSI installer
* Other enterprise-specific requirements

---

# Common Questions

### "Why not just use GNU sort?"

GNU `sort` is a powerful tool, but this engine adds features designed for larger and more controlled workflows, including:

* Resume support
* Live dashboard
* JSON output
* Top-N processing
* Pre-flight disk-space checks
* Controlled memory usage

### "Why not use Python pandas?"

For datasets larger than available RAM, pandas may require significant memory.

External Merge Sort Engine uses a configurable memory limit:

```text
--max-memory
```

This allows the sorting process to operate within a defined memory budget.

### "What about sensitive data?"

The engine is designed to run **100% offline**.

* Data stays on your machine or server.
* Distributed as a single binary.
* Temporary files are cleaned up automatically.
* Output is written safely using `.part` files and atomic rename.
* `--verify` can check the final result.

No cloud upload is required.

---

# What's Included

## Data Formats

* Binary numeric `u64`
* String
* CSV
* CSV multi-key sorting, up to 4 columns
* JSONL
* Excel
* Header support
* Reverse sorting
* Duplicate removal
* Top-N results

## Safety

* Exit codes `0-6`
* Pre-flight disk-space checks
* `.part` temporary output + atomic rename
* `--verify`
* Order verification
* Row-count verification

## Operations

* `--gui`
* `--api`
* `--watch`
* `--dashboard`
* `--json`
* `--log-file`
* `--temp-dir`
* `--resume`
* `--merge`
* `--split-by`
* Docker support
