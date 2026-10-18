param(
    [string]$TargetDirectory = 'target/review-validation',
    [int]$Cycles = 10,
    [int]$FirstSampleMilliseconds = 63000,
    [switch]$Offline
)
$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '../..')
$oldCycles = $env:LUCIDDESK_MEMORY_CYCLES
$oldFirst = $env:LUCIDDESK_MEMORY_FIRST_MS
try {
    $common = @('--locked', '--target-dir', $TargetDirectory)
    if ($Offline) { $common += '--offline' }
    & cargo build -p luciddesk -p luciddesk-monitor --features luciddesk-monitor/test-diagnostics @common
    if ($LASTEXITCODE -ne 0) { throw 'Monitor process build failed.' }
    $artifacts = & cargo test -p luciddesk --bin luciddesk --no-run --message-format=json-render-diagnostics @common
    if ($LASTEXITCODE -ne 0) { throw 'Monitor process test build failed.' }
    $test = $artifacts | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object {
        $_.reason -eq 'compiler-artifact' -and $_.profile.test -and $_.target.name -eq 'luciddesk'
    } | Select-Object -Last 1
    $debug = Join-Path $TargetDirectory 'debug'
    $destinations = @((Join-Path $debug 'plugins'), (Join-Path (Split-Path $test.executable) 'plugins'))
    foreach ($destination in $destinations) {
        New-Item -ItemType Directory -Force $destination | Out-Null
        Copy-Item (Join-Path $debug 'luciddesk_monitor.dll') (Join-Path $destination 'luciddesk_monitor.dll') -Force
    }
    $env:LUCIDDESK_MEMORY_CYCLES = "$Cycles"
    $env:LUCIDDESK_MEMORY_FIRST_MS = "$FirstSampleMilliseconds"
    & $test.executable 'pane::monitor::process::tests::isolated_monitor_closes_process_and_keeps_host_sampling_empty' --ignored --exact --nocapture --test-threads=1 2>&1 |
        Tee-Object (Join-Path $TargetDirectory 'monitor-process-memory.log')
    if ($LASTEXITCODE -ne 0) { throw 'Monitor process lifecycle/memory validation failed.' }
} finally {
    $env:LUCIDDESK_MEMORY_CYCLES = $oldCycles
    $env:LUCIDDESK_MEMORY_FIRST_MS = $oldFirst
    # Leave ordinary plugin artifacts in the validation directories, without the diagnostic command.
    if ($destinations) {
        & cargo build -p luciddesk-monitor @common
        if ($LASTEXITCODE -eq 0) {
            foreach ($destination in $destinations) {
                Copy-Item (Join-Path $debug 'luciddesk_monitor.dll') (Join-Path $destination 'luciddesk_monitor.dll') -Force
            }
        }
    }
    Pop-Location
}
