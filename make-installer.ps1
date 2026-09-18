# MergeSort Pro — sale-ready packager (Windows).
# Builds release, packs dist/MergeSortPro-<version>/ + .zip for delivery.
# NOTE: $ErrorActionPreference stays "Continue" (the default) on purpose —
# "Stop" would turn cargo's normal stderr progress output ("Compiling...",
# "Finished...") into a terminating error and abort this script every time,
# even on a successful build. Steps that must succeed check $LASTEXITCODE
# or pass -ErrorAction Stop explicitly instead.
$ver = "2.0.1"
$dist = "dist/MergeSortPro-$ver"
Write-Host "=== MergeSort Pro packager v$ver ===" -ForegroundColor Cyan
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
Remove-Item -Recurse -Force "dist" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path "$dist/uploads" -ErrorAction Stop | Out-Null
Copy-Item "target/release/mergesort.exe" "$dist/mergesort.exe" -ErrorAction Stop
Copy-Item "target/release/gen.exe" "$dist/mergesort-gen.exe" -ErrorAction SilentlyContinue
Copy-Item "README.md" "$dist/README.md" -ErrorAction Stop
Copy-Item "LICENSE" "$dist/LICENSE.txt" -ErrorAction Stop
Copy-Item "CHANGELOG.md" "$dist/CHANGELOG.md" -ErrorAction SilentlyContinue
@"
@echo off
title MergeSort Pro Console
echo ============================================
echo  MergeSort Pro v$ver - Web Console
echo ============================================
echo  Membuka console di browser...
start http://127.0.0.1:8080/
mergesort.exe --gui 127.0.0.1:8080
pause
"@ | Set-Content "$dist/Start-GUI.bat" -Encoding ASCII
@"
@echo off
mergesort.exe --help
echo.
echo Contoh cepat:
echo   mergesort.exe --input uploads\demo.txt --output uploads\sorted.txt --max-memory 512MB --mode string --verify
pause
"@ | Set-Content "$dist/Help.bat" -Encoding ASCII
Set-Content "$dist/uploads/.keep" "" -Encoding ASCII
$zip = "dist/MergeSortPro-$ver-windows-x64.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path "$dist/*" -DestinationPath $zip -ErrorAction Stop
Write-Host ""
Write-Host "OK: $zip" -ForegroundColor Green
Write-Host "Isi: mergesort.exe, mergesort-gen.exe, Start-GUI.bat, README, LICENSE" -ForegroundColor Gray
Write-Host "Cara jual: kirim ZIP ini ke pembeli (tidak ada license-key check di binary saat ini)" -ForegroundColor Yellow
