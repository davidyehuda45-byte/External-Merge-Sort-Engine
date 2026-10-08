# MergeSort Pro — 2-minute sales demo (Windows PowerShell).
$ErrorActionPreference = "Stop"
cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
$bin = ".\target\release\mergesort.exe"
$gen = ".\target\release\gen.exe"
Write-Host "== 1/4 generate 200k csv ==" -ForegroundColor Cyan
& $gen demo.csv 200000 csv
Write-Host "== 2/4 dry-run (gratis) ==" -ForegroundColor Cyan
& $bin --input demo.csv --output sorted.csv --max-memory 64MB --format csv --key-column 0 --key-type numeric --dry-run
Write-Host "== 3/4 sort + Top-1000 + verify + json + audit ==" -ForegroundColor Cyan
& $bin --input demo.csv --output sorted.csv --max-memory 64MB --format csv --key-column 0 --key-type numeric --header --verify --limit 1000 --log-file audit.jsonl --json --stats > demo-out.json
Get-Content demo-out.json -First 20
Write-Host "== 4/4 audit log ==" -ForegroundColor Cyan
Get-Content audit.jsonl | Select-Object -Last 1
Write-Host ""
Write-Host "Demo OK. Next: .\target\release\mergesort.exe --gui 127.0.0.1:8080" -ForegroundColor Green
Write-Host "Cleanup: Remove-Item demo.csv, sorted.csv, demo-out.json, audit.jsonl" -ForegroundColor Gray
Remove-Item -Force demo.csv, sorted.csv -ErrorAction SilentlyContinue
