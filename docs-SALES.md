# Sales Kit — External Merge Sort Engine

## 30-second pitch
"Sort file yang lebih besar dari RAM. 2GB di laptop 8GB, 1TB di server. CSV per kolom, resume kalau mati lampu, dashboard live, output JSON buat CI. Ada GUI buat staf non-teknis."

## Demo script (5 menit, bikin klien ngangguk)
1. `Start-GUI.bat` → Generate demo 10k → Urutkan sekarang → Download (30 detik, tanpa CLI).
2. CLI power: `--dry-run` dulu (estimasi gratis), lalu sort 200k rows + `--verify` + `--json`.
3. Matikan paksa (Ctrl-C) di tengah jalan → `--resume list` → `--resume` → "tidak ada kerja yang hilang".
4. Tunjukkan `--reverse --unique --header --limit 1000` (Top-N) — fitur yang GNU sort pun ribet untuk file raksasa.
5. Tutup: "audit tiap run ke `--log-file audit.jsonl`."

## Pricing suggestion (custom dev model — jasa)
- Personal/Tim: custom build sesuai kebutuhan
- Enterprise: SLA + custom (S3, gzip, GUI branding, installer MSI)

## Objection handling
- "Kenapa tidak pakai GNU sort?" → GNU sort tidak punya resume, dashboard, JSON, Top-N streaming, pre-flight disk check, dan makan RAM tak terbatas.
- "Kenapa tidak Python pandas?" → pandas butuh RAM > file. Ini bounded `--max-memory`.
- "Data sensitif?" → 100% offline, single binary, temp dibersihkan otomatis, atomic output (tidak pernah setengah-jadi).

## What's inside
- Formats: numeric binary u64, string, CSV multi-key (s/d 4 kolom), JSONL, Excel, header, reverse, unique, limit.
- Safety: exit codes 0-6, pre-flight disk, `.part` + rename, `--verify` order + row-count.
- Ops: `--gui`, `--api`, `--watch`, `--dashboard`, `--json`, `--log-file`, `--temp-dir`, `--resume`, `--merge`, `--split-by`, Docker.
