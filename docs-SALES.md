# Sales Kit — External Merge Sort Engine

## 30-second pitch
"Sort file yang lebih besar dari RAM. Teruji 496 MB di laptop 8 GB; arsitektur mendukung hingga 1 TB+ di server dengan disk dan `--temp-dir` yang cukup. CSV per kolom, resume kalau mati lampu, dashboard live, output JSON buat CI. Ada GUI buat staf non-teknis."

## Demo script (5 menit, bikin klien ngangguk)
1. `Start-GUI.bat` (dari ZIP installer) → Generate demo 10k → Urutkan sekarang → Download (30 detik, tanpa CLI).
2. CLI power: `--dry-run` dulu (estimasi gratis), lalu sort 200k rows + `--verify` + `--json`.
3. Matikan paksa (Ctrl-C) di tengah jalan → `--resume list` → `--resume` → "tidak ada kerja yang hilang".
4. Tunjukkan `--reverse --unique --header --limit 1000` (Top-N) — fitur yang GNU sort pun ribet untuk file raksasa.
5. Tutup: "audit tiap run ke `--log-file audit.jsonl`."

## Pricing suggestion (contoh range, final hubungi kami)
- Personal: contoh Rp500rb sekali bayar (build standar + installer ZIP).
- Tim: contoh Rp2jt (preset, white-label dasar, dukungan email 30 hari).
- Enterprise: SLA + custom (S3, gzip, GUI branding, installer MSI) — harga custom, hubungi kami.
> Angka di atas contoh awal untuk diskusi — harga final tergantung scope. Jika tidak ingin angka, cukup tulis "hubungi kami untuk penawaran".

## Objection handling
- "Kenapa tidak pakai GNU sort?" → GNU sort tidak punya resume, dashboard, JSON, Top-N streaming, pre-flight disk check, dan makan RAM tak terbatas.
- "Kenapa tidak Python pandas?" → pandas butuh RAM > file. Ini bounded `--max-memory`.
- "Data sensitif?" → 100% offline, single binary, temp dibersihkan otomatis, atomic output (tidak pernah setengah-jadi).
- "Apakah .zip/.gz perlu extract manual?" → Tidak untuk `.gz`/`.zip` (transparan, streaming); `.zst` ditolak dengan pesan jelas — convert dulu ke `.gz`/`.zip`/plain.

## What's inside
- Formats: numeric binary u64, string, CSV multi-key (maks 4 kolom, contoh `--multi-key "0,2" --key-type numeric`), JSONL, Excel, header, reverse, unique, limit.
- Kompresi: `.gz`/`.zip` transparan; `.zst` ditolak dengan panduan.
- Safety: exit codes 0-6 (+ `--force` untuk timpa, tolak `input==output`), pre-flight disk, `.part` + rename, `--verify` order + row-count.
- Ops: `--gui` (Cancel + Preview + Browse server), `--api` (`/api/cancel`, `/api/preview`, `/api/files`, token), `--watch`, `--dashboard`, `--json`, `--log-file`, `--temp-dir`, `--resume`, `--merge`, `--split-by`, Docker.

## FAQ
- 1GB berapa lama? Acuan SSD ~120MB/mnt single-thread; `--dry-run` beri estimasi + `--temp-dir` di SSD tercepat.
- RAM berapa? `--max-memory 50-70% RAM`; 496MB teruji di 512MB budget (RSS 464MB).
- Mati lampu? `--resume list` / `--resume`; GUI: job tertinggal bisa rerun via Riwayat; job running bisa Cancel.
- Data sensitif? 100% offline, jail path cwd/uploads/temp, token API, audit JSONL, `Uninstall.bat` bersihkan `%APPDATA%\MergeSort`.
- Garansi? Lihat LICENSE MIT + penawaran SLA Enterprise.

## Perbandingan
| | MergeSort Pro | GNU sort | pandas |
|---|---|---|---|
| >RAM bounded | ya (`--max-memory`) | tidak (RAM tak terbatas) | tidak (butuh RAM>file) |
| Resume | ya | tidak | tidak |
| Dashboard/GUI/Cancel/Preview | ya | tidak | notebook saja |
| JSON CI + exit 0-6 | ya | tidak | manual |
| Top-N streaming (`--limit`) | ya | ribet file raksasa | OOM |
| Pre-flight disk | ya | tidak | tidak |
