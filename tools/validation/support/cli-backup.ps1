# Invoked only within test-cli-plan.ps1's isolated application data directory.
function Complete-Backup([string[]]$Command) {
    $saveDeadline = [DateTime]::UtcNow.AddSeconds(15)
    while ((Invoke-Cli @('status')).data.workspace_save_pending) {
        if ([DateTime]::UtcNow -gt $saveDeadline) { throw 'Workspace save did not settle before backup' }
        Start-Sleep -Milliseconds 150
    }
    $preview = Invoke-Cli ($Command + @('--dry-run'))
    if ($preview.data.next_step.action -ne 'review_then_apply') { throw 'Backup preview did not provide apply instructions' }
    $applyArgs = [string[]]$preview.data.next_step.args
    $raw = & $cli @applyArgs
    if ($LASTEXITCODE -ne 0) { throw ($raw -join "`n") }
    $result = $raw | ConvertFrom-Json
    $requestId = $result.request_id
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while ($result.data.operation_status -eq 'pending') {
        if ([DateTime]::UtcNow -gt $deadline) { throw "Backup request still pending: $requestId" }
        Start-Sleep -Milliseconds 150
        $result = (Invoke-Cli @('request','get','--id',$requestId)).data.result
    }
    if (-not $result.ok -or $result.data.operation_status -ne 'completed') { throw ($result | ConvertTo-Json -Depth 30) }
    $replayedRaw = & $cli @applyArgs
    $replayed = $replayedRaw | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or $replayed.request_id -ne $requestId -or $replayed.data.operation_status -ne 'completed') { throw 'Backup replay did not return completed original receipt' }
    return $result
}
$baseline = Invoke-Cli @('workspace','get')
$createdBackup = Complete-Backup @('backup','create')
$backups = Invoke-Cli @('backup','list')
if (-not (Test-Path -LiteralPath $createdBackup.data.path)) { throw 'Manual backup missing' }
$destination = Join-Path $root 'cli-backup-export.db'
$exported = Complete-Backup @('backup','export','--source',$createdBackup.data.path,'--path',$destination)
if (-not (Test-Path -LiteralPath $destination)) { throw 'Export missing' }
$overwrite = & $cli backup export --path $destination --dry-run --data-dir $root --json
if ($LASTEXITCODE -eq 0) { throw 'Export allowed an existing destination' }
$temporary = Invoke-Plan @(@{op='pane.create';ref='restore_probe';title='CLI backup restore probe'})
$restored = Complete-Backup @('backup','restore','--path',$destination)
$after = Invoke-Cli @('workspace','get')
if ($temporary.data.refs.restore_probe -in $after.data.panes.id -or $after.data.panes.Count -ne $baseline.data.panes.Count) { throw 'Restore did not recover baseline panels' }
if (-not (Test-Path -LiteralPath $restored.data.rollback_path)) { throw 'Pre-restore backup missing' }
@{result='passed';create=$true;export=$true;overwrite_rejected=$true;restore=$true;replay=$true} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'backup-result.json')
