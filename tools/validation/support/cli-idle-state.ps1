# Wait for prior saves and automatic backup work before measuring read-only calls.
# ReadState returns { snapshot: serialized disk state; busy: boolean }.
function Wait-CliIdleSnapshot {
    param(
        [Parameter(Mandatory)][scriptblock]$ReadState,
        [Parameter(Mandatory)][scriptblock]$RecordState,
        [double]$QuietSeconds = 12,
        [double]$TimeoutSeconds = 60,
        [scriptblock]$Clock,
        [scriptblock]$Pause = { Start-Sleep -Milliseconds 250 }
    )
    $timer = [Diagnostics.Stopwatch]::StartNew()
    if (-not $Clock) { $Clock = { $timer.Elapsed.TotalSeconds }.GetNewClosure() }
    $started = & $Clock
    $stableSince = $null
    $previous = $null
    do {
        if ((& $Clock) - $started -ge $TimeoutSeconds) { throw 'Data directory did not become idle before the deadline' }
        $state = & $ReadState
        $now = & $Clock
        if ($state.busy -isnot [bool] -or $state.snapshot -isnot [string]) { throw 'Invalid idle observation' }
        if ($state.busy -or $null -eq $previous -or $state.snapshot -cne $previous) {
            $stableSince = if ($state.busy) { $null } else { $now }
            & $RecordState $state ($now - $started)
        } elseif ($null -eq $stableSince) {
            $stableSince = $now
        }
        $previous = $state.snapshot
        if ($now - $started -ge $TimeoutSeconds) { throw 'Data directory did not become idle before the deadline' }
        if ($null -ne $stableSince -and $now - $stableSince -ge $QuietSeconds) { return $state.snapshot }
        & $Pause
    } while ($true)
}
