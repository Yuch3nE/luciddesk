[CmdletBinding()]
param([string]$Executable = (Join-Path $PSScriptRoot 'luciddesk.exe'))
$ErrorActionPreference = 'Stop'
$resolved = (Resolve-Path -LiteralPath $Executable).ProviderPath
if (-not (Test-Path -LiteralPath $resolved -PathType Leaf) -or [IO.Path]::GetExtension($resolved) -ine '.exe') {
    throw 'Provide the path of the LucidDesk executable to refresh.'
}
if (-not ('LucidDesk.IconRefresh' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
namespace LucidDesk {
    public static class IconRefresh {
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        public struct FileInfo {
            public IntPtr icon;
            public int index;
            public uint attributes;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string displayName;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 80)] public string typeName;
        }
        [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
        public static extern IntPtr SHGetFileInfo(string path, uint attributes,
            out FileInfo info, uint size, uint flags);
        [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
        public static extern void SHUpdateImage(string path, int iconIndex, uint flags, int imageIndex);
        [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
        public static extern void SHChangeNotify(uint change, uint flags, string item, IntPtr other);
    }
}
'@
}
# Notify only this file and folder. Never delete global caches or restart Explorer.
$info = [LucidDesk.IconRefresh+FileInfo]::new()
$size = [uint32][Runtime.InteropServices.Marshal]::SizeOf($info)
if ([LucidDesk.IconRefresh]::SHGetFileInfo($resolved, 0, [ref]$info, $size, 0x4000) -ne [IntPtr]::Zero) {
    # LucidDesk's EXE uses its first embedded icon, without a Shell icon handler.
    # Notify image-list users too; UPDATEITEM alone can leave their old image cached.
    [LucidDesk.IconRefresh]::SHUpdateImage($resolved, 0, 0, $info.index)
}
[LucidDesk.IconRefresh]::SHChangeNotify(0x00002000, 0x00001005, $resolved, [IntPtr]::Zero)
[LucidDesk.IconRefresh]::SHChangeNotify(0x00001000, 0x00001005, [IO.Path]::GetDirectoryName($resolved), [IntPtr]::Zero)
Write-Output "Requested Shell icon refresh: $resolved"
