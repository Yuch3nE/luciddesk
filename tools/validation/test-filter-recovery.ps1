param([string]$BuildDirectory = "$PSScriptRoot/../../target/debug")
$ErrorActionPreference = 'Stop'
$build = (Resolve-Path -LiteralPath $BuildDirectory).Path
if ((Split-Path $build -Leaf) -ne 'debug') { throw 'Only Debug builds allowed' }
if (Get-Process luciddesk -ErrorAction SilentlyContinue) { throw 'Stop the authorized LucidDesk instance first' }
$cli = Join-Path $build 'luciddesk-cli.exe'
$probe = Join-Path $build 'examples/desktop_snapshot.exe'
$reportRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../../target/filter-recovery-$([guid]::NewGuid().ToString('N'))"))
[IO.Directory]::CreateDirectory($reportRoot) | Out-Null
$desktop = [IO.Path]::GetFullPath([Environment]::GetFolderPath('DesktopDirectory'))
$tag = 'LucidDesk-Recovery-test-' + [guid]::NewGuid().ToString('N')
$fixture = Join-Path $desktop "$tag.txt"
$content = "Owned filter recovery fixture $tag"
$app = $null
$root = $null
$results = @()
Add-Type -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class NativeFilterShutdown {
 public delegate bool EnumProc(IntPtr h, IntPtr p);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr p);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder text, int n);
 [DllImport("user32.dll")] static extern bool PostThreadMessage(uint thread, uint msg, UIntPtr w, IntPtr l);
 public static bool Stop(uint target) {
  uint thread=0;
  EnumWindows((h,p)=>{uint pid; uint t=GetWindowThreadProcessId(h,out pid); var text=new StringBuilder(128); GetWindowText(h,text,128);
   if(pid==target && text.ToString()=="LucidDesk Hybrid Controller") {thread=t; return false;} return true;},IntPtr.Zero);
  return thread!=0 && PostThreadMessage(thread,0x12,UIntPtr.Zero,IntPtr.Zero);
 }
}
'@
function Wait-Check([scriptblock]$Check, [string]$Failure) {
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        if (& $Check) { return }
        if ([DateTime]::UtcNow -gt $deadline) { throw $Failure }
        Start-Sleep -Milliseconds 150
    } while ($true)
}
function Native-Snapshot {
    $raw = & $probe
    if ($LASTEXITCODE -ne 0) { throw 'Native snapshot failed' }
    $snapshot = $raw | ConvertFrom-Json
    if ($null -eq $snapshot.icons_hidden -or $snapshot.icons_hidden) { throw 'Native desktop must remain visible' }
    return $snapshot
}
function Invoke-FilterCli([string[]]$Command) {
    $raw = & $cli @Command --data-dir $root --json
    $value = $raw | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or -not $value.ok) { throw ($raw -join "`n") }
    if (-not $value.context.instance_id.StartsWith("$($app.Id)-")) { throw 'Wrong app instance' }
    return $value
}
function Invoke-FilterPlan([object[]]$Operations) {
    $context = (Invoke-FilterCli @('workspace','get')).context
    $path = Join-Path $root 'plan.json'
    [IO.File]::WriteAllText($path, (@{protocol_version=1;base=$context;operations=$Operations} | ConvertTo-Json -Depth 20))
    $preview = Invoke-FilterCli @('plan','preview','--input',$path)
    $requestId = [guid]::NewGuid().ToString()
    $result = Invoke-FilterCli @('plan','apply','--token',$preview.data.plan_token,'--request-id',$requestId)
    Wait-Check {
        $receipt = (Invoke-FilterCli @('request','get','--id',$requestId)).data.result
        if ($receipt.data.presentation_status -eq 'failed') { throw ($receipt | ConvertTo-Json -Depth 20) }
        $receipt.data.presentation_status -eq 'applied'
    } 'Plan did not reach applied state'
    return $result
}
try {
    $stream = [IO.File]::Open($fixture,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
    try { $bytes = [Text.Encoding]::UTF8.GetBytes($content); $stream.Write($bytes,0,$bytes.Length) } finally { $stream.Dispose() }
    Wait-Check { @((Native-Snapshot).items | Where-Object path -eq $fixture).Count -eq 1 } 'Fixture absent from Explorer'
    $baseline = Native-Snapshot
    foreach ($crash in @($false,$true)) {
        $root = Join-Path $reportRoot "$crash"
        [IO.Directory]::CreateDirectory($root) | Out-Null
        $app = Start-Process -FilePath (Join-Path $build 'luciddesk.exe') -Environment @{LUCIDDESK_DATA_DIR=$root} -WindowStyle Hidden -PassThru
        Wait-Check {
            if ($app.HasExited) { throw 'Application exited unexpectedly' }
            try { $status = (Invoke-FilterCli @('status')).data; $status.desktop_connected -and $status.desktop_sync_status -eq 'applied' } catch { $false }
        } 'App did not connect'
        $items = (Invoke-FilterCli @('item','list')).data.items
        $owned = @($items | Where-Object path -eq $fixture)
        if ($owned.Count -ne 1) { throw 'Fixture not in application inventory' }
        $id = [string]$owned[0].id
        $created = Invoke-FilterPlan @(@{op='pane.create';ref='owned';title=$tag})
        $pane = [string]$created.data.refs.owned
        for ($round=0; $round -lt 3; $round++) {
            $null = Invoke-FilterPlan @(@{op='item.assign';item_ids=@($id);pane_id=$pane})
            Wait-Check { @((Native-Snapshot).items | Where-Object path -eq $fixture).Count -eq 0 } 'RemoveObject did not hide fixture'
            $null = Invoke-FilterPlan @(@{op='item.release';item_ids=@($id)})
            Wait-Check { @((Native-Snapshot).items | Where-Object path -eq $fixture).Count -eq 1 } 'AddObject did not restore fixture'
        }
        $null = Invoke-FilterPlan @(@{op='item.assign';item_ids=@($id);pane_id=$pane})
        Wait-Check { @((Native-Snapshot).items | Where-Object path -eq $fixture).Count -eq 0 } 'Fixture not hidden before exit'
        if ($crash) { Stop-Process -Id $app.Id -Force }
        elseif (-not [NativeFilterShutdown]::Stop($app.Id)) { throw 'Controller thread not found' }
        if (-not $app.WaitForExit(10000)) { throw 'App did not exit' }
        Wait-Check { @((Native-Snapshot).items | Where-Object path -eq $fixture).Count -eq 1 } 'Owner exit did not restore fixture'
        $after = Native-Snapshot
        foreach ($before in $baseline.items) {
            $match = @($after.items | Where-Object key -eq $before.key)
            if ($match.Count -ne 1 -or $match[0].x -ne $before.x -or $match[0].y -ne $before.y) { throw 'Native identity or position changed' }
        }
        if ([IO.File]::ReadAllText($fixture) -cne $content) { throw 'Fixture contents changed' }
        Wait-Check { $check = Start-Process -FilePath (Join-Path $build 'luciddesk.exe') -ArgumentList '--check-desktop-component' -WindowStyle Hidden -PassThru; $check.WaitForExit(); $check.ExitCode -eq 0 } 'Component did not unload'
        $results += @{forced_exit=$crash;round_trips=3;native_visible=$true;layout_preserved=$true;source_unchanged=$true}
    }
    @{passed=$true;cases=$results} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $reportRoot 'result.json')
    Write-Output "Passed: $reportRoot/result.json"
} finally {
    if ($app -and -not $app.HasExited) { Stop-Process -Id $app.Id -Force; $app.WaitForExit(10000) | Out-Null }
    if ([IO.Path]::GetDirectoryName($fixture) -eq $desktop -and [IO.Path]::GetFileName($fixture) -eq "$tag.txt" -and (Test-Path -LiteralPath $fixture)) {
        if ([IO.File]::ReadAllText($fixture) -ceq $content) { Remove-Item -LiteralPath $fixture }
    }
}
