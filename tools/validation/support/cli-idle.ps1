# Run through test-cli-plan.ps1 -IdleSeconds N; measures the owned GUI and data directory.
$ErrorActionPreference = 'Stop'
# Prior mutation tests may schedule the first automatic backup after 10 quiet seconds.
# Let that legitimate write finish before measuring stable idle; do not disable backups.
Start-Sleep -Seconds 12
$base = (Invoke-Cli @('workspace','get')).context
$idleInput = Join-Path $root 'idle-preview.json'
[IO.File]::WriteAllText($idleInput,(@{protocol_version=1;base=$base;operations=@(@{op='settings.update';values=@{'diagnostics.level'='error'}})} | ConvertTo-Json -Depth 20),[Text.UTF8Encoding]::new($false))
function Disk-Snapshot {
    $snapshot = [ordered]@{}
    foreach ($file in (Get-ChildItem -LiteralPath $root -Recurse -File | Sort-Object FullName)) {
        $snapshot[[IO.Path]::GetRelativePath($root,$file.FullName)] = @($file.Length,$file.LastWriteTimeUtc.Ticks,(Get-SharedHash $file.FullName))
    }
    return ($snapshot | ConvertTo-Json -Depth 5 -Compress)
}
$before = Disk-Snapshot
$app.Refresh()
$cpuBefore = $app.TotalProcessorTime.TotalMilliseconds
$privateBefore = $app.PrivateMemorySize64
$peakPrivate = $privateBefore
$timer = [Diagnostics.Stopwatch]::StartNew()
$queries = 0
$previews = 0
while ($timer.Elapsed.TotalSeconds -lt $IdleSeconds -or $previews -lt 80) {
    $null = Invoke-Cli @('status')
    $null = Invoke-Cli @('settings','get')
    $null = Invoke-Cli @('item','list')
    $preview = Invoke-Cli @('plan','preview','--input',$idleInput)
    if ($preview.data.changed) { throw 'No-op idle preview unexpectedly changed settings' }
    $queries += 4
    $previews++
    if ($previews % 10 -eq 0) {
        $null = Invoke-Cli @('font','list')
        $null = Invoke-Cli @('startup','get')
        $queries += 2
    }
    $app.Refresh()
    $peakPrivate = [Math]::Max($peakPrivate,$app.PrivateMemorySize64)
    Start-Sleep -Milliseconds 100
}
$app.Refresh()
$elapsed = $timer.Elapsed.TotalSeconds
$cpu = $app.TotalProcessorTime.TotalMilliseconds - $cpuBefore
$after = Disk-Snapshot
if ($before -cne $after) { throw ('Idle queries/previews changed files: before=' + $before + ' after=' + $after) }
[ordered]@{
    result='passed';seconds=[Math]::Round($elapsed,2);queries=$queries;previews=$previews
    files_unchanged=$true;gui_cpu_milliseconds=$cpu
    gui_average_cpu_cores=[Math]::Round($cpu / ($elapsed * 1000),4)
    gui_private_bytes_before=$privateBefore;gui_peak_private_bytes=$peakPrivate
    scope='Data directory file hashes, sizes and write times; GUI CPU/memory. Not a kernel disk-I/O trace.'
} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'idle-result.json')
