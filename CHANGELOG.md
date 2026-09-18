# Changelog

## 2.0.1 — Sale-readiness hardening pass
Fixed correctness/packaging bugs found during a pre-sale audit (all verified
with real repro, not just code review):
- **Header auto-detect false positive**: a numeric-key CSV/string file with
  no header could have its first data row misdetected as a header (letters
  anywhere on the line + numeric-looking later rows triggered it), silently
  dropping that row and corrupting the "header" line of the output. Detection
  is now scoped to the actual key column.
- **`--mode excel` wrote numbers as text**: sorted `.xlsx` output put every
  cell as a string even when the code detected a numeric value, so Excel
  users lost SUM()/numeric sort/filter on their own numbers. Now writes real
  numeric cells (zero-padded ids/zips still stay text on purpose).
- **GUI job status showed a bare `"}"`**: `/api/jobs` "detail" field for a
  finished job was always literally `}` due to grabbing the last line of a
  multi-line `--json` summary. Now shows e.g. `selesai: 200000 baris,
  verify=true`.
- **`make-installer.ps1` couldn't finish a single run**: `$ErrorActionPreference
  = "Stop"` turned cargo's normal build progress on stderr into a fatal
  error, aborting the packager every time, even on success. Also removed a
  reference to a `--gen-license` flag that doesn't exist in the binary.
- **Dockerfile didn't build, and if it had, wouldn't run**: base image
  predated Rust edition 2024 and several pinned dependency MSRVs; separately,
  build/runtime stage glibc versions didn't match, so the compiled binary
  would have failed at startup even after a successful build.
- Replaced one `panic!()` on a corrupted temp chunk (disk corruption
  mid-merge) with a normal exit-code error, so it can't surface as a raw
  Rust backtrace to a customer.
- Clippy cleanup pass (let-chains, dead branches); no behavior change.

## 2.0.0 — Full 20 fitur (Lapis 1+2+3)
Lapis 1: `--preview N` (+`--preview-json`, output opsional, tanpa commit),
`--stats`/`--stats-json` (laporan eksekusi screenshot-able), dry-run pintar
(estimasi baris/waktu/temp + saran `--temp-dir`, deteksi header/delimiter/encoding),
`--check` (validasi CSV/delimiter/header/kolom/encoding, exit 0/5),
auto-deteksi delimiter+header (`--no-header` override), `--encoding`
(utf-8|latin-1|utf-16le|utf-16be + `--encoding-in/out`, BOM auto).
Lapis 2: `--mode jsonl --key-field a.b.c` (nested, numeric|string|date),
`--dedupe-by C [--dedupe-keep first|last]`, `--merge F1 F2...` (pre-sorted,
header-aware), `--output-format csv|tsv|jsonl|sql [--sql-table]`,
`--split-by N` (header diulang per part), GUI preset (localStorage +
export/import) + riwayat server (100 job + Jalankan Lagi + hapus).
Lapis 3: `--mode excel`/`--sheet` + I/O `.xlsx` (calamine/rust_xlsxwriter),
`--watch DIR [--output-dir]` daemon polling + stabilitas-size + audit.jsonl,
`--api [host:]PORT [--api-token]` (health, job, job-result download),
`--interactive` wizard, GUI white-label (`--brand/--brand-color`),
kompresi transparan `.gz/.zip` (streaming, `.zst` ditolak jelas),
cross-platform (`build.sh` linux/mac-intel/mac-arm + CI 3-OS).

## 1.1.0 — GUI + enterprise
- New: `--gui [host:]PORT` web console (drag-drop upload, form, jobs live, download, demo generator, no CLI needed)
- New: `--limit N` Top-N (header-aware, binary-truncate fast path)
- New: `--log-file audit.jsonl` (compact JSON per run, buat audit enterprise)
- Packaging: `make-installer.ps1` → ZIP siap kirim + `Start-GUI.bat`, `INSTALL.md`, `docs-SALES.md`, `demo-sales.ps1`
- README: GUI-first quickstart

## 1.0.0 — Pro release (sale-ready)
- New: `--reverse` / `-r` (descending), `--unique` / `-u` (dedup like `sort -u`)
- New: `--header` (keep CSV/string header row on top, excluded from sort + verify)
- New: `--dry-run` (disk/memory/chunk estimate, JSON-compatible, sorts nothing)
- New: `--version` / `-V`, polished `--help`
- Dashboard rebuilt as branded "MergeSort Pro" UI (offline, no CDN, `/metrics` JSON kept)
- JSON summary: adds `version`, `mode` alias (compat), `options.{reverse,unique,header}`
- Windows: sane open-file default (256) instead of Linux-only `/proc` fallback
- Docs: README, LICENSE (MIT), Dockerfile
