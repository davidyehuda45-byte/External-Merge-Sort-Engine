# MergeSort Pro — register --watch as a Windows scheduled task (no admin service needed).
# Run: powershell -ExecutionPolicy Bypass -File install-watch-task.ps1
$ErrorActionPreference = "Stop"
$exe = Join-Path $PSScriptRoot "mergesort.exe"
if (-not (Test-Path $exe)) { throw "mergesort.exe not found next to this script" }
$inbox = "$env:USERPROFILE\mergesort-inbox"
$out = "$env:USERPROFILE\mergesort-sorted"
New-Item -ItemType Directory $inbox -ErrorAction SilentlyContinue | Out-Null
New-Item -ItemType Directory $out -ErrorAction SilentlyContinue | Out-Null
$action = New-ScheduledTaskAction -Execute $exe -Argument "--watch `"$inbox`" --output-dir `"$out`" --max-memory 1GB --verify"
$trigger = New-ScheduledTaskTrigger -AtLogOn
$task = New-ScheduledTask -Action $action -Trigger $trigger -Description "MergeSort Pro watch daemon"
Register-ScheduledTask -TaskName "MergeSortProWatch" -InputObject $task -Force | Out-Null
Write-Host "OK: watch task registered (inbox=$inbox out=$out). Unregister: Unregister-ScheduledTask -TaskName MergeSortProWatch"
