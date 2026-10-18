[CmdletBinding()]
param(
    [string[]]$Plugins = @('system-monitor'),
    [switch]$Release,
    [switch]$Offline,
    [string]$TargetDirectory = 'target',
    [string]$Destination,
    [switch]$NoStage
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $repoRoot
try {
    $metadataArgs = @('metadata', '--no-deps', '--format-version', '1', '--locked')
    if ($Offline) { $metadataArgs += '--offline' }
    $metadata = (& cargo @metadataArgs) | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Could not read plugin metadata.' }
    $packages = foreach ($id in ($Plugins | Select-Object -Unique)) {
        $matches = @($metadata.packages | Where-Object { $_.metadata.luciddesk.plugin -eq $id })
        if ($matches.Count -ne 1) { throw "Expected one workspace plugin for '$id'." }
        $matches[0]
    }
    if (-not $packages) { throw 'Select at least one plugin.' }
    $buildArgs = @('build', '--locked', '--target-dir', $TargetDirectory)
    if ($Release) { $buildArgs += '--release' }
    if ($Offline) { $buildArgs += '--offline' }
    foreach ($package in $packages) { $buildArgs += @('-p', $package.name) }
    & cargo @buildArgs | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Plugin build failed.' }
    $profile = if ($Release) { 'release' } else { 'debug' }
    if (-not $Destination) { $Destination = Join-Path $TargetDirectory "$profile/plugins" }
    if (-not $NoStage) { New-Item -ItemType Directory -Path $Destination -Force | Out-Null }
    foreach ($package in $packages) {
        $target = @($package.targets | Where-Object { $_.crate_types -contains 'cdylib' })
        if ($target.Count -ne 1) { throw "Expected one DLL target in $($package.name)." }
        $dll = (Resolve-Path -LiteralPath (Join-Path $TargetDirectory "$profile/$($target[0].name).dll")).Path
        if (-not $NoStage) { Copy-Item -LiteralPath $dll -Destination $Destination -Force }
        # Package callers consume these paths; build progress stays on the host stream.
        $dll
    }
} finally {
    Pop-Location
}
