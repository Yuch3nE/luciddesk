[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$SourcePath,
    [Parameter(Mandatory)][string]$OutputPath,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$InnoCompiler,
    [ValidateSet('Fast', 'Normal', 'Max')][string]$Compression = 'Fast'
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$SourcePath = (Resolve-Path -LiteralPath $SourcePath).Path
$InnoCompiler = (Resolve-Path -LiteralPath $InnoCompiler).Path
$build = Get-Content -LiteralPath (Join-Path $SourcePath 'build.json') -Raw | ConvertFrom-Json
if ($Version -notmatch '^\d+\.\d+\.\d+$' -or $build.version -ne $Version) { throw 'EXE version must match the staged build.' }
$null = & (Join-Path $PSScriptRoot '../validation/test-agent-package.ps1') -Directory $SourcePath
$OutputPath = [IO.Path]::GetFullPath($OutputPath)
New-Item -ItemType Directory -Path $OutputPath -Force | Out-Null
& $InnoCompiler /Q "/DAppVersion=$Version" "/DSourcePath=$SourcePath" "/DOutputPath=$OutputPath" "/DInstallerCompression=$($Compression.ToLowerInvariant())" (Join-Path $repo 'installer/LucidDesk.iss') | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed.' }
$output = Join-Path $OutputPath "LucidDesk-$Version-windows-x64-setup.exe"
if (-not (Test-Path -LiteralPath $output -PathType Leaf)) { throw 'EXE compiler did not produce the installer.' }
Write-Output $output
