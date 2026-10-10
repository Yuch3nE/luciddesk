[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$SourcePath,
    [Parameter(Mandatory)][string]$RepoRoot,
    [Parameter(Mandatory)][string]$OutputFile
)
$ErrorActionPreference = 'Stop'
$SourcePath = (Resolve-Path -LiteralPath $SourcePath).Path
function Escape-Xml([string]$Value) { [Security.SecurityElement]::Escape($Value) }
function Get-DirectoryId([string]$Relative) {
    if (-not $Relative -or $Relative -eq '.') { return 'INSTALLFOLDER' }
    $hash = [Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Relative.ToLowerInvariant()))
    return 'Dir_' + [Convert]::ToHexString($hash).Substring(0,24)
}
$entries = @(Get-ChildItem -LiteralPath $SourcePath -Recurse -Force)
if ($entries | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) { throw 'Installer payload cannot contain reparse points.' }
$directories = foreach ($directory in ($entries | Where-Object PSIsContainer | Sort-Object FullName)) {
    $relative = [IO.Path]::GetRelativePath($SourcePath, $directory.FullName)
    $parent = Split-Path -Parent $relative
    '<DirectoryRef Id="' + (Get-DirectoryId $parent) + '"><Directory Id="' + (Get-DirectoryId $relative) + '" Name="' + (Escape-Xml $directory.Name) + '" /></DirectoryRef>'
}
$components = foreach ($file in ($entries | Where-Object { -not $_.PSIsContainer } | Sort-Object FullName)) {
    $relative = [IO.Path]::GetRelativePath($SourcePath, $file.FullName)
    if ($relative -in @('portable', 'portable.marker', 'msix', 'README.md', 'installed')) { continue }
    $parent = Split-Path -Parent $relative
    $id = if ($relative -eq 'luciddesk.exe') { 'AppExe' } elseif (-not $parent) { 'File_' + ($file.Name -replace '[^A-Za-z0-9_]', '_') } else {
        'File_' + (Get-DirectoryId $relative).Substring(4)
    }
    '<Component Guid="*" Directory="' + (Get-DirectoryId $parent) + '"><File Id="' + $id + '" Source="' + (Escape-Xml $file.FullName) + '" KeyPath="yes" /></Component>'
}
$components += '<Component Guid="*"><File Source="' + (Escape-Xml (Join-Path $RepoRoot 'installer/installed')) + '" KeyPath="yes" /></Component>'
$components += '<Component Guid="*"><File Name="README.md" Source="' + (Escape-Xml (Join-Path $RepoRoot 'docs/installer.md')) + '" KeyPath="yes" /></Component>'
('<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs"><Fragment>' + ($directories -join "`n") + '<ComponentGroup Id="Payload" Directory="INSTALLFOLDER">' + ($components -join "`n") + '</ComponentGroup></Fragment></Wix>') | Set-Content -LiteralPath $OutputFile -Encoding UTF8
