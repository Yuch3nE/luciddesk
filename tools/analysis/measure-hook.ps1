param(
    [ValidateRange(1,600)][int]$Seconds = 60,
    [string]$OutputPath
)
$ErrorActionPreference = 'Stop'
# This measures membership reconciliation inside Explorer, not total Explorer
# CPU. Only diagnostic HWND properties are written; desktop items are untouched.
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class LucidDeskHookMeasurement {
    public delegate bool Callback(IntPtr h, IntPtr p);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback c, IntPtr p);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr h, Callback c, IntPtr p);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr GetProp(IntPtr h, string n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool SetProp(IntPtr h, string n, IntPtr v);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr RemoveProp(IntPtr h, string n);
    public static IntPtr[] Find() {
        var result = new List<IntPtr>();
        EnumWindows((h,p) => {
            EnumChildWindows(h,(c,q) => {
                if(GetProp(c,"LucidDesk.Filter.Owner.v1") != IntPtr.Zero) result.Add(c);
                return true;
            },IntPtr.Zero);
            return true;
        },IntPtr.Zero);
        return result.ToArray();
    }
}
"@
$hooks = @([LucidDeskHookMeasurement]::Find())
if ($hooks.Count -ne 1) { throw 'Expected exactly one attached LucidDesk desktop hook.' }
$window = $hooks[0]
$owner = [LucidDeskHookMeasurement]::GetProp($window,'LucidDesk.Filter.Owner.v1')
$prefix = 'LucidDesk.Filter.Perf.'
if ([LucidDeskHookMeasurement]::GetProp($window,$prefix+'Enabled') -ne [IntPtr]::Zero) {
    throw 'Another hook measurement is already active.'
}
try {
    foreach ($name in @('Count','Micros')) {
        $null = [LucidDeskHookMeasurement]::RemoveProp($window,$prefix+$name)
    }
    if (-not [LucidDeskHookMeasurement]::SetProp($window,$prefix+'Enabled',[IntPtr]1)) {
        throw 'Cannot enable hook measurement. Use the same desktop/user permissions as Explorer.'
    }
    $watch = [Diagnostics.Stopwatch]::StartNew()
    Start-Sleep -Seconds $Seconds
    $elapsed = $watch.Elapsed.TotalSeconds
    if ([LucidDeskHookMeasurement]::GetProp($window,'LucidDesk.Filter.Owner.v1') -ne $owner -or
        [LucidDeskHookMeasurement]::GetProp($window,$prefix+'Enabled') -eq [IntPtr]::Zero) {
        throw 'Hook changed or detached during measurement; discard this sample.'
    }
    $count = [LucidDeskHookMeasurement]::GetProp($window,$prefix+'Count').ToInt64()
    $micros = [LucidDeskHookMeasurement]::GetProp($window,$prefix+'Micros').ToInt64()
    $json = [pscustomobject]@{
        seconds = $elapsed
        scans = $count
        totalMilliseconds = $micros / 1000
        meanMilliseconds = $(if ($count -gt 0) { $micros / 1000 / $count } else { $null })
        staWallPercent = $micros / 10000 / $elapsed
    } | ConvertTo-Json
    if ($OutputPath) { $json | Set-Content -LiteralPath $OutputPath -Encoding utf8 }
    $json
} finally {
    if ([LucidDeskHookMeasurement]::GetProp($window,'LucidDesk.Filter.Owner.v1') -eq $owner) {
        foreach ($name in @('Enabled','Count','Micros')) {
            $null = [LucidDeskHookMeasurement]::RemoveProp($window,$prefix+$name)
        }
    }
}
