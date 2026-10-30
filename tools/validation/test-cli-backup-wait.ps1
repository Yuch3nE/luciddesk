# Exercise the real backup helper with scripted CLI responses; never launch the GUI.
$ErrorActionPreference = 'Stop'
$tokens = $null; $errors = $null
$path = Join-Path $PSScriptRoot 'support/cli-backup.ps1'
$ast = [Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors -join "`n") }
$helper = $ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Complete-Backup'}, $false)
if (-not $helper) { throw 'Complete-Backup helper missing' }
. ([scriptblock]::Create($helper.Extent.Text))
$root = Join-Path ([IO.Path]::GetTempPath()) ('luciddesk-backup-wait-' + [guid]::NewGuid().ToString('N'))
$cli = 'Invoke-BackupTestCli'
function Invoke-BackupTestCli {
    $script:calls.Add(@($args))
    if ($script:responses.Count -eq 0) { throw 'Unexpected CLI call' }
    $entry = $script:responses.Dequeue()
    $global:LASTEXITCODE = $entry.code
    return ($entry.body | ConvertTo-Json -Depth 20 -Compress)
}
function Start-Sleep { param([int]$Milliseconds) if ($Milliseconds -ne 250) { throw 'Unexpected wait interval' } }
function Reset-Responses([object[]]$Entries) {
    $script:responses = [Collections.Generic.Queue[object]]::new()
    foreach ($entry in $Entries) { $script:responses.Enqueue($entry) }
    $script:calls = [Collections.Generic.List[object]]::new()
}
$ready = @{code=0;body=@{ok=$true;data=@{next_step=@{action='review_then_apply';args=@('plan','apply','--token','fresh')}}}}
$done = @{code=0;body=@{ok=$true;request_id='submitted';data=@{operation_status='completed'}}}
$passed = 0
try {
    foreach ($reason in @('workspace_save_pending','backup_task_running')) {
        $field = if ($reason -eq 'workspace_save_pending') { 'workspace_save_pending' } else { 'busy' }
        $query = if ($reason -eq 'workspace_save_pending') { @('status') } else { @('backup','list') }
        $busy = @{code=6;body=@{ok=$false;request_id='preview';error=@{code='BUSY'};data=@{
            operation_submitted=$false;busy=@{phase='preview';reason=$reason}
            next_step=@{action='wait_for_idle';args=$query;automatic_retry=$false;retry_after_ms=250;until=@{path="data.$field";equals=$false}}
        }}}
        Reset-Responses @($busy, @{code=0;body=@{data=@{$field=$true}}}, @{code=0;body=@{data=@{$field=$false}}}, $ready, $done, $done)
        $result = Complete-Backup @('backup','create')
        if ($result.request_id -ne 'submitted' -or $script:responses.Count -ne 0 -or $script:calls.Count -ne 6) { throw 'Wait flow did not consume the expected calls' }
        foreach ($index in @(0,3)) {
            if ('--dry-run' -notin $script:calls[$index]) { throw 'Wait recovery must request a fresh pure preview' }
        }
        foreach ($index in @(4,5)) {
            if ('fresh' -notin $script:calls[$index]) { throw 'Apply/replay must use the new preview token' }
        }
        $passed++
        # Busy without a confirmed non-submission must never lead to a retry.
        $busy.body.data.operation_submitted = $true
        Reset-Responses @($busy)
        $rejected = $false
        try { $null = Complete-Backup @('backup','create') } catch { $rejected = $true }
        if (-not $rejected -or $script:calls.Count -ne 1) { throw 'Uncertain submission was retried' }
        $passed++
    }
    Reset-Responses @(@{code=2;body=@{ok=$false;error=@{code='INVALID_REQUEST'}}})
    $rejected = $false
    try { $null = Complete-Backup @('backup','create') } catch { $rejected = $true }
    if (-not $rejected -or $script:calls.Count -ne 1) { throw 'Invalid request was retried' }
    $passed++
    Write-Output "Passed $passed backup wait scenarios; no application state touched."
} finally {
    if (Test-Path -LiteralPath ($root + '.commands.jsonl')) { Remove-Item -LiteralPath ($root + '.commands.jsonl') }
}
