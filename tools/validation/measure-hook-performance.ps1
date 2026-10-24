# Opt-in Explorer timings. Run against an instrumented build in the interactive session.
param(
 [string]$CliPath = "$env:LOCALAPPDATA/Programs/LucidDesk/luciddesk-cli.exe",
 [ValidateRange(1, 10000)][int]$Rounds = 120,
 [string]$OutputPath = "$PSScriptRoot/../../target/hook-performance.json",
 [scriptblock]$Workload
)
$ErrorActionPreference = 'Stop'
$cli = $CliPath
$status = (& $cli status --json | ConvertFrom-Json).data
if (-not $status.desktop_connected) { throw 'Desktop disconnected' }
if ($status.desktop_diagnostic -notmatch 'view=0x([0-9a-f]+)') { throw 'Desktop view missing' }
$view = [IntPtr]::new([Convert]::ToInt64($Matches[1],16))
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class HookPerf {
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr GetProp(IntPtr h,string n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern bool SetProp(IntPtr h,string n,IntPtr v);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr RemoveProp(IntPtr h,string n);
 [DllImport("user32.dll")] public static extern bool RedrawWindow(IntPtr h,IntPtr rect,IntPtr region,uint flags);
 [DllImport("user32.dll",SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h,uint m,IntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
}
"@
$prefix = 'LucidDesk.Filter.Perf.'
$metrics = @('','Paint.','Request.','Restore.','Snapshot.','Names.','Validate.','Present.','Folder.','Read.','Add.','Remove.','Position.')
function Clear-Metrics {
 $null = [HookPerf]::RemoveProp($view,$prefix+'Enabled')
 foreach ($metric in $metrics) { foreach ($suffix in @('Count','Micros','MaxMicros')) { $null = [HookPerf]::RemoveProp($view,$prefix+$metric+$suffix) } }
}
$results = @()
try {
 Clear-Metrics
 if (-not [HookPerf]::SetProp($view,$prefix+'Enabled',[IntPtr]1)) { throw 'Cannot enable metrics' }
 if ($Workload) { & $Workload } else {
 for ($round=0;$round -lt $Rounds;$round++) {
  if (-not [HookPerf]::RedrawWindow($view,[IntPtr]::Zero,[IntPtr]::Zero,0x1)) { throw 'Redraw failed' }
  [UIntPtr]$result = [UIntPtr]::Zero
  if ([HookPerf]::SendMessageTimeout($view,0xF,[IntPtr]::Zero,[IntPtr]::Zero,2,2000,[ref]$result) -eq [IntPtr]::Zero) { throw 'Desktop paint timed out' }
  Start-Sleep -Milliseconds 15
 }
 }
 foreach ($metric in $metrics) {
  $count = [HookPerf]::GetProp($view,$prefix+$metric+'Count').ToInt64()
  $micros = [HookPerf]::GetProp($view,$prefix+$metric+'Micros').ToInt64()
  $max = [HookPerf]::GetProp($view,$prefix+$metric+'MaxMicros').ToInt64()
  $results += [pscustomobject]@{metric= $(if($metric){$metric.TrimEnd('.')}else{'Background'}); count=$count; total_us=$micros; mean_us=$(if($count){[math]::Round($micros/$count,2)}else{0}); max_us=$max}
 }
 $results | ConvertTo-Json | Set-Content $OutputPath -Encoding utf8
 if (($results | Where-Object count -GT 0).Count -eq 0) { throw 'No samples: check the installed DLL and active filtering' }
 $results | Format-Table
} finally { Clear-Metrics }
