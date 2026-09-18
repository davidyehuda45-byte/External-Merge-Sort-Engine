# MergeSort Pro — 2-minute sales demo (Windows PowerShell).
$ErrorActionPreference = "Stop"
$env:MERGESORT_SKIP_LICENSE = "1"
cargo build 2>$null | Out-Null
$bin = ".\target\debug\mergesort.exe"
$gen = ".\target\debug\gen.exe"
Write-Host "== 1/4 generate 200k csv ==" -ForegroundColor Cyan
& $gen demo.csv 200000 csv | Out-Null
Write-Host "== 2/4 dry-run (gratis) ==" -ForegroundColor Cyan
& $bin --input demo.csv --output sorted.csv --max-memory 64MB --format csv --key-column 0 --key-type numeric --dry-run
Write-Host "== 3/4 sort + Top-1000 + verify + json + audit ==" -ForegroundColor Cyan
& $bin --input demo.csv --output sorted.csv --max-memory 64MB --format csv --key-column 0 --key-type numeric --header --verify --limit 1000 --log-file audit.jsonl --json | Select-Object -First 20
Write-Host "== 4/4 audit log ==" -ForegroundColor Cyan
Get-Content audit.jsonl | Select-Object -Last 1
Write-Host ""
Write-Host "Demo OK. Next: .\target\debug\mergesort.exe --gui 127.0.0.1:8080" -ForegroundColor Green
