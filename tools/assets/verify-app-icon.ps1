[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$Executable)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $Executable).ProviderPath
$ico = [IO.File]::ReadAllBytes((Join-Path $PSScriptRoot '../../app/assets/luciddesk.ico'))
if (-not ('LucidDesk.IconResourceCheck' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
namespace LucidDesk {
    public static class IconResourceCheck {
        [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        public static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);
        [DllImport("kernel32.dll")]
        public static extern bool FreeLibrary(IntPtr module);
        [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
        [DllImport("kernel32.dll")]
        static extern uint SizeofResource(IntPtr module, IntPtr resource);
        [DllImport("kernel32.dll")]
        static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
        [DllImport("kernel32.dll")]
        static extern IntPtr LockResource(IntPtr resource);
        public static bool Matches(byte[] actual, byte[] expected, int offset, int count) {
            if (actual.Length != count || offset < 0 || count < 0 ||
                offset > expected.Length - count) return false;
            for (int i = 0; i < count; i++) {
                if (actual[i] != expected[offset + i]) return false;
            }
            return true;
        }
        public static byte[] Read(IntPtr module, int type, int id) {
            var resource = FindResource(module, (IntPtr)id, (IntPtr)type);
            if (resource == IntPtr.Zero) throw new Exception("Missing icon resource " + id);
            var bytes = new byte[SizeofResource(module, resource)];
            var ptr = LockResource(LoadResource(module, resource));
            if (ptr == IntPtr.Zero) throw new Exception("Cannot read icon resource " + id);
            Marshal.Copy(ptr, bytes, 0, bytes.Length);
            return bytes;
        }
    }
}
'@
}
# Load as data; do not execute the application.
if ($ico.Length -lt 6 -or [BitConverter]::ToUInt16($ico, 0) -ne 0 -or
    [BitConverter]::ToUInt16($ico, 2) -ne 1) { throw 'Invalid source ICO header.' }
$count = [BitConverter]::ToUInt16($ico, 4)
if ($count -eq 0 -or $ico.Length -lt 6 + 16 * $count) { throw 'Invalid source ICO directory.' }
$module = [LucidDesk.IconResourceCheck]::LoadLibraryEx($exe, [IntPtr]::Zero, 0x22)
if ($module -eq [IntPtr]::Zero) { throw "Could not load EXE resources: $exe" }
try {
    $group = [LucidDesk.IconResourceCheck]::Read($module, 14, 1)
    if ($group.Length -lt 6 + 14 * $count -or [BitConverter]::ToUInt16($group, 0) -ne 0 -or
        [BitConverter]::ToUInt16($group, 2) -ne 1 -or [BitConverter]::ToUInt16($group, 4) -ne $count) { throw 'Embedded icon frame count differs from source ICO.' }
    for ($i = 0; $i -lt $count; $i++) {
        $entry = 6 + 16 * $i
        $groupEntry = 6 + 14 * $i
        for ($j = 0; $j -lt 12; $j++) {
            if ($ico[$entry + $j] -ne $group[$groupEntry + $j]) { throw "Icon frame $i metadata differs." }
        }
        $id = [BitConverter]::ToUInt16($group, $groupEntry + 12)
        $bytes = [LucidDesk.IconResourceCheck]::Read($module, 3, $id)
        $length = [BitConverter]::ToUInt32($ico, $entry + 8)
        $offset = [BitConverter]::ToUInt32($ico, $entry + 12)
        if ($bytes.Length -ne $length) { throw "Icon frame $i size differs." }
        if ($offset -lt 6 + 16 * $count -or [uint64]$offset + $length -gt $ico.Length) {
            throw "Icon frame $i is outside the source ICO."
        }
        if (-not [LucidDesk.IconResourceCheck]::Matches($bytes, $ico, [int]$offset, [int]$length)) {
            throw "Icon frame $i content differs."
        }
    }
    Write-Output "Verified all $count embedded icon frames: $exe"
} finally { [void][LucidDesk.IconResourceCheck]::FreeLibrary($module) }
