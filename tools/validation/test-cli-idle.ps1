# Deterministic virtual-time tests for the real pre-measurement quiet-window helper.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'support/cli-idle-state.ps1')
function Run-Scenario([scriptblock]$Sample, [double]$ExpectedTime, [switch]$Timeout) {
    $script:seconds = 0.0
    $failed = $false
    try {
        $null = Wait-CliIdleSnapshot -ReadState $Sample -RecordState {} -Clock { $script:seconds } -Pause { $script:seconds++ }
    } catch {
        if (-not $Timeout -or $_.Exception.Message -notlike '*deadline*') { throw }
        $failed = $true
    }
    if ($failed -ne [bool]$Timeout -or $script:seconds -ne $ExpectedTime) {
        throw "Unexpected quiet-window result: seconds=$script:seconds timeout=$failed"
    }
}
Run-Scenario { @{snapshot='unchanged';busy=$false} } 12
# Restore returns at t=0, deferred save at t=3, automatic backup at t=13.
# The old fixed wait started measurement at t=12, before the backup appeared.
Run-Scenario {
    $snapshot = if ($script:seconds -lt 3) { 'original' } elseif ($script:seconds -lt 13) { 'saved' } else { 'saved+backup' }
    @{snapshot=$snapshot;busy=$false}
} 25
# Busy completion must start a new quiet window even without a disk change.
Run-Scenario { @{snapshot='same';busy=($script:seconds -lt 6)} } 18
Run-Scenario { @{snapshot="write-$script:seconds";busy=$false} } 60 -Timeout
Run-Scenario { @{snapshot='same';busy=$true} } 60 -Timeout
Run-Scenario { $script:seconds=61; @{snapshot='same';busy=$false} } 61 -Timeout
$rejected = $false
try { $null = Wait-CliIdleSnapshot -ReadState { @{snapshot='same';busy=$null} } -RecordState {} } catch { $rejected = $_.Exception.Message -eq 'Invalid idle observation' }
if (-not $rejected) { throw 'Missing status must not count as idle' }

# Verify the measurement snapshot still detects backup additions; do not exempt them.
$tokens=$null; $errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'support/cli-idle.ps1'),[ref]$tokens,[ref]$errors)
if ($errors.Count) { throw ($errors -join "`n") }
$definition=$ast.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Disk-Snapshot'},$false)
. ([scriptblock]::Create($definition.Extent.Text))
function Get-SharedHash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash }
$root=Join-Path ([IO.Path]::GetTempPath()) ('luciddesk-idle-test-' + [guid]::NewGuid().ToString('N'))
$backup=Join-Path $root 'backups'
[IO.Directory]::CreateDirectory($backup) | Out-Null
try {
    $file=Join-Path $root 'workspace.db'
    [IO.File]::WriteAllText($file,'fixture')
    $before=Disk-Snapshot
    $new=Join-Path $backup 'auto-fixture.db'
    [IO.File]::WriteAllText($new,'backup')
    $after=Disk-Snapshot
    if ($before -ceq $after -or -not ($after | ConvertFrom-Json -AsHashtable).Contains('backups\auto-fixture.db')) { throw 'Measurement ignored a backup write' }
} finally {
    if (Test-Path -LiteralPath $new) { Remove-Item -LiteralPath $new }
    if (Test-Path -LiteralPath $file) { Remove-Item -LiteralPath $file }
    Remove-Item -LiteralPath $backup
    Remove-Item -LiteralPath $root
}
Write-Output 'Passed 8 idle validation scenarios; delayed backup reproduced with virtual time, measurement remains strict.'
