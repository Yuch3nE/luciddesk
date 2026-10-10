param(
    [ValidateRange(1, 100)][int]$Repeat = 5,
    [switch]$LiveDesktop
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$logs = Join-Path $repo ('target/sync-validation/' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
New-Item -ItemType Directory -Path $logs -Force | Out-Null
$results = [System.Collections.Generic.List[object]]::new()
function Invoke-SyncTest([string]$Name, [string[]]$CargoArgs) {
    $log = Join-Path $logs ($Name + '.log')
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    & cargo @CargoArgs *> $log
    $code = $LASTEXITCODE
    $passed = $code -eq 0 -and (Select-String -LiteralPath $log -Pattern '^test result: ok\. 1 passed; 0 failed; 0 ignored;' -Quiet)
    $results.Add([pscustomobject]@{
        Name = $Name; ExitCode = $code; Passed = $passed; Seconds = $timer.Elapsed.TotalSeconds
        Arguments = $CargoArgs; Log = $log
    })
    $results | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $logs 'results.json') -Encoding utf8
    Write-Host "$Name : exit=$code ($([math]::Round($timer.Elapsed.TotalSeconds, 2))s)"
    if (-not $passed) {
        Get-Content -LiteralPath $log -Tail 40
        throw "Validation failed or did not execute exactly one test: $Name. Logs: $logs"
    }
}
Push-Location $repo
try {
    for ($round = 1; $round -le $Repeat; $round++) {
        Invoke-SyncTest "folder-watch-$round" @('test', '-p', 'luciddesk', '--bin', 'luciddesk', '--locked', '--offline', 'pane::folder::tests::folder_watch_tracks_children_and_recovers_after_missing_directory', '--', '--exact', '--test-threads=1')
    }
    if ($LiveDesktop) {
        # Read-only Explorer probe. Do not enable all ignored tests: some open
        # menus, change the clipboard or exercise the real desktop membership.
        Invoke-SyncTest 'live-desktop' @('test', '-p', 'luciddesk-shell', '--lib', '--locked', '--offline', 'native_layout::tests::background_revision_matches_full_snapshot', '--', '--ignored', '--exact', '--test-threads=1')
        Invoke-SyncTest 'live-hook' @('test', '-p', 'luciddesk-explorer', '--lib', '--locked', '--offline', 'filter::items::tests::live_filter_snapshot_reads_without_mutating_desktop', '--', '--ignored', '--exact', '--test-threads=1')
    }
    Write-Host "All requested checks passed. Logs: $logs"
} finally {
    Pop-Location
}
