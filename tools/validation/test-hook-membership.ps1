# Creates only uniquely named owned fixtures and a temporary panel in the running app.
param(
 [ValidateRange(1, 100)][int]$Count=20,
 [ValidateRange(1, 20)][int]$Rounds=5,
 [string]$CliPath="$env:LOCALAPPDATA/Programs/LucidDesk/luciddesk-cli.exe",
 [string]$ProbePath="$PSScriptRoot/../../target/debug/examples/desktop_snapshot.exe"
)
$ErrorActionPreference='Stop'
$cli=$CliPath
$probe=$ProbePath
$tag='LucidDesk-hook-check-'+[guid]::NewGuid().ToString('N')
$desktop=[Environment]::GetFolderPath('DesktopDirectory')
Add-Type @"
using System;using System.Runtime.InteropServices;
public static class HookFixtureNotify {
 [DllImport("shell32.dll",CharSet=CharSet.Unicode)] public static extern void SHChangeNotify(int e,uint f,string p,IntPtr other);
}
"@
$owned=@();$pane=$null;$ids=@()
function Invoke-HookCli([string[]]$Command) {
 $raw=& $cli @Command --json
 $value=$raw|ConvertFrom-Json
 if($LASTEXITCODE -ne 0 -or -not $value.ok){throw ($raw -join "`n")}
 return $value
}
function Wait-Check([scriptblock]$Check,[string]$Failure) {
 $watch=[Diagnostics.Stopwatch]::StartNew()
 $limit=[DateTime]::UtcNow.AddSeconds(20)
 do {if(& $Check){Write-Host "PASS: $Failure check completed in $($watch.ElapsedMilliseconds) ms";return};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow -lt $limit)
 throw $Failure
}
function Plan([object[]]$Ops) {
 $context=(Invoke-HookCli @('workspace','get')).context
 $path=Join-Path "$PSScriptRoot/../../target" "$tag-plan.json"
 @{protocol_version=1;base=$context;operations=$Ops}|ConvertTo-Json -Depth 15|Set-Content $path -Encoding utf8
 $preview=Invoke-HookCli @('plan','preview','--input',$path)
 $rid=[guid]::NewGuid().ToString()
 $watch=[Diagnostics.Stopwatch]::StartNew()
 $result=Invoke-HookCli @('plan','apply','--token',$preview.data.plan_token,'--request-id',$rid)
 Wait-Check { $receipt=(Invoke-HookCli @('request','get','--id',$rid)).data.result
  if($receipt.data.presentation_status -eq 'failed'){throw 'Presentation failed'}
  $receipt.data.presentation_status -eq 'applied'
 } 'Presentation timeout'
 Write-Host "Apply through presentation: $(($Ops | ForEach-Object op) -join ',') $($watch.ElapsedMilliseconds) ms"
 return $result
}
function Snapshot {
 $raw=& $probe
 if($LASTEXITCODE -ne 0){throw 'Native snapshot failed'}
 return ($raw|ConvertFrom-Json)
}
function Count-Owned {
 $s=Snapshot
 if($s.icons_hidden){throw 'Desktop icons unexpectedly hidden'}
 return @($s.items|Where-Object {$_.path -in $owned}).Count
}
$before=(Invoke-HookCli @('pane','list')).data|ConvertTo-Json -Depth 30 -Compress
$nativeBefore=Snapshot
try {
 for($i=0;$i -lt $Count;$i++){
  $path=Join-Path $desktop "$tag-$i.txt"
  $f=[IO.File]::Open($path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read);$f.Dispose();$owned+=$path
 }
 Wait-Check {(Count-Owned) -eq $Count} 'Fixtures not visible'
 Wait-Check { $script:ids=@((Invoke-HookCli @('item','list')).data.items|Where-Object {$_.path -in $owned}|ForEach-Object id);$script:ids.Count -eq $Count } 'Inventory incomplete'
 $pane=(Plan @(@{op='pane.create';ref='owned';title=$tag})).data.refs.owned
 for($i=0;$i -lt $Rounds;$i++){
  $null=Plan @(@{op='item.assign';item_ids=$ids;pane_id=$pane})
  Wait-Check {(Count-Owned) -eq 0} 'Hide failed'
  [HookFixtureNotify]::SHChangeNotify(0x1000,0x1005,$desktop,[IntPtr]::Zero)
  Start-Sleep -Milliseconds 200
  Wait-Check {(Count-Owned) -eq 0} 'Hide failed after Shell refresh'

  $null=Plan @(@{op='item.release';item_ids=$ids})
  Wait-Check {(Count-Owned) -eq $Count} 'Restore failed'
 }
 Write-Output "Verified $Rounds hide/restore cycles with $Count fixtures."
} finally {
 if($pane){$null=Plan @(@{op='pane.remove';pane_id=$pane;release_items=$true})}
 foreach($path in $owned){
  Remove-Item -LiteralPath $path -ErrorAction Continue
  if(-not (Test-Path -LiteralPath $path)){[HookFixtureNotify]::SHChangeNotify(4,0x1005,$path,[IntPtr]::Zero)}
 }
}
Wait-Check {(Snapshot).items.Count -eq $nativeBefore.items.Count} 'Native count changed after cleanup'
$after=(Invoke-HookCli @('pane','list')).data|ConvertTo-Json -Depth 30 -Compress
if($before -ne $after){throw 'Existing pane state changed'}
$afterNative=Snapshot
foreach($item in $nativeBefore.items){
 $match=@($afterNative.items|Where-Object key -eq $item.key)
 if($match.Count -ne 1 -or $match[0].x -ne $item.x -or $match[0].y -ne $item.y){throw 'Existing native item position changed'}
}
Write-Output 'Existing pane state and native icon coordinates unchanged; fixtures cleaned.'
