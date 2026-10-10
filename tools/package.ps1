[CmdletBinding()]
param(
    [switch]$Offline, [switch]$Portable, [switch]$RenderDiagnostics, [switch]$Installer, [switch]$All,
    [ValidateSet('Exe', 'Msi', 'Both')][string]$InstallerFormat = 'Exe',
    [ValidateSet('Fast', 'Normal', 'Max')][string]$ExeCompression = 'Fast',
    [string]$InnoCompiler
)
$ErrorActionPreference = 'Stop'
if ($All) {
    if ($Portable -or $RenderDiagnostics) {
        throw '-All generates both ZIPs and the selected installer format; do not combine it with -Portable or -RenderDiagnostics.'
    }
    $Installer = $true
}
if ($RenderDiagnostics -and -not $Portable) { throw 'Rendering comparison launchers require -Portable.' }
if ($Installer -and $Portable) { throw 'Installer and portable packages are separate channels.' }
if ($PSBoundParameters.ContainsKey('InstallerFormat') -and -not $Installer) { throw 'Use -Installer with -InstallerFormat.' }
if ($PSBoundParameters.ContainsKey('ExeCompression') -and (-not $Installer -or $InstallerFormat -eq 'Msi')) {
    throw '-ExeCompression requires -Installer with Exe or Both.'
}
$repoRoot = Split-Path -Parent $PSScriptRoot
$previousRevision = $env:LUCIDDESK_BUILD_REVISION
Push-Location -LiteralPath $repoRoot
try {
    # Fail before compiling when a requested installer cannot be produced.
    if ($Installer -and $InstallerFormat -in @('Exe', 'Both')) {
        if (-not $InnoCompiler) { $InnoCompiler = Join-Path $repoRoot 'target/tooling/inno-7.1.0/ISCC.exe' }
        if (-not (Test-Path -LiteralPath $InnoCompiler -PathType Leaf)) {
            throw 'Run ./tools/ensure-inno.ps1 first, or pass -InnoCompiler <ISCC.exe>.'
        }
        $InnoCompiler = (Resolve-Path -LiteralPath $InnoCompiler).Path
    }
    if ($Installer -and $InstallerFormat -in @('Msi', 'Both')) {
        . (Join-Path $PSScriptRoot 'packaging/installer-tooling.ps1')
        $wixTools = Get-WixTooling
        & $wixTools.Compiler --version | Out-Host
        if ($LASTEXITCODE -ne 0) { throw 'WiX cannot run. Check the installed .NET runtime.' }
    }
    $metadataArgs = @('metadata', '--no-deps', '--format-version', '1', '--locked')
    if ($Offline) { $metadataArgs += '--offline' }
    $metadataJson = & cargo @metadataArgs
    if ($LASTEXITCODE -ne 0) { throw 'Could not read package metadata.' }
    $metadata = $metadataJson | ConvertFrom-Json
    $version = ($metadata.packages | Where-Object name -eq 'luciddesk').version
    if (-not $version) { throw 'The luciddesk package version is missing.' }
    if (($metadata.packages | Where-Object name -eq 'luciddesk-cli').version -ne $version) { throw 'GUI and CLI versions must match.' }
    & (Join-Path $PSScriptRoot 'use-windows-toolchain.ps1')
    $revision = & git rev-parse --short HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Could not read Git revision.' }
    $gitStatus = & git status --porcelain --untracked-files=no
    if ($LASTEXITCODE -ne 0) { throw 'Could not read Git working-tree status.' }
    $dirty = [bool]$gitStatus
    $revisionLabel = if ($dirty) { "$revision-dirty" } else { $revision }
    $env:LUCIDDESK_BUILD_REVISION = $revisionLabel
    $toolchain = Get-Content -LiteralPath (Join-Path $repoRoot 'rust-toolchain.toml') -Raw
    $pinnedVersion = [regex]::Match($toolchain, 'channel\s*=\s*"([^"]+)"').Groups[1].Value
    $hostInfo = & rustc -vV
    if ($LASTEXITCODE -ne 0 -or $hostInfo -notcontains 'host: x86_64-pc-windows-msvc') {
        throw 'Packaging requires the Windows x64 MSVC Rust toolchain.'
    }
    if (-not $pinnedVersion -or $hostInfo -notcontains "release: $pinnedVersion") {
        throw "Packaging requires the pinned Rust $pinnedVersion toolchain."
    }
    $cargoVersion = & cargo -V
    if ($LASTEXITCODE -ne 0) { throw 'Could not read Cargo version.' }
    # Keep release artifacts separate from explicitly enabled diagnostic backends.
    $productionTarget = Join-Path $repoRoot 'target\production'
    $buildArgs = @('build', '--release', '--locked', '--no-default-features', '--target-dir', $productionTarget, '-p', 'luciddesk', '-p', 'luciddesk-explorer', '-p', 'luciddesk-cli')
    if ($Offline) { $buildArgs += '--offline' }
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    & (Join-Path $PSScriptRoot 'assets/verify-app-icon.ps1') -Executable (Join-Path $productionTarget 'release/luciddesk.exe')
    # All package modes share these exact binaries and build provenance.
    $binaryFiles = @('luciddesk.exe', 'luciddesk_explorer.dll', 'luciddesk-cli.exe')
    $files = @($binaryFiles | ForEach-Object {
        $fileHash = Get-FileHash -LiteralPath (Join-Path $productionTarget "release\$_") -Algorithm SHA256
        [ordered]@{ file = $_; sha256 = $fileHash.Hash.ToLowerInvariant() }
    })
    $builtAt = (Get-Date).ToUniversalTime().ToString('o')
    $buildEnvironment = [ordered]@{
        rustc = ($hostInfo -join "`n"); cargo = $cargoVersion
        runnerImage = $env:ImageOS; runnerVersion = $env:ImageVersion
        windows = ($env:LUCIDDESK_WINDOWS_BUILD_ENVIRONMENT | ConvertFrom-Json)
    }
    $packageModes = if ($All) { @($false, $true) } else { @([bool]$Portable) }
    foreach ($isPortable in $packageModes) {
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss-fff'
        $name = "LucidDesk-$version-$revisionLabel-windows-x64-$stamp"
        if ($isPortable) { $name = "LucidDesk-$version-windows-x64-portable" }
        if ($RenderDiagnostics) { $name += '-render-test' }
        $outRoot = Join-Path $repoRoot $(if ($isPortable) { "target\portable\$stamp" } else { 'target\packages' })
        $stage = Join-Path $outRoot $name
        New-Item -ItemType Directory -Path $stage | Out-Null
        foreach ($file in $binaryFiles) {
            Copy-Item -LiteralPath (Join-Path $productionTarget "release\$file") -Destination $stage
        }
        $agentFiles = & (Join-Path $PSScriptRoot 'packaging/stage-agent-payload.ps1') -SourceRoot $repoRoot -Destination $stage
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'assets/Refresh-App-Icon.ps1') -Destination $stage
        $readme = if ($isPortable) { 'docs\portable.md' } else { 'docs\package.md' }
        Copy-Item -LiteralPath (Join-Path $repoRoot $readme) -Destination (Join-Path $stage 'README.md')
        if ($isPortable) {
            'LucidDesk portable mode: store configuration in ./data.' | Set-Content -LiteralPath (Join-Path $stage 'portable') -Encoding ASCII
        }
        if ($RenderDiagnostics) {
            Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'render-diagnostics') -File | ForEach-Object {
                Copy-Item -LiteralPath $_.FullName -Destination $stage
            }
        }
        Copy-Item -LiteralPath (Join-Path $repoRoot 'docs\usage.md') -Destination (Join-Path $stage 'usage.md')
        Copy-Item -LiteralPath (Join-Path $repoRoot 'docs\brand.md') -Destination (Join-Path $stage 'brand.md')
        foreach ($guide in @('installer.md', 'portable.md')) {
            Copy-Item -LiteralPath (Join-Path $repoRoot "docs\$guide") -Destination (Join-Path $stage $guide)
        }
        Copy-Item -LiteralPath (Join-Path $repoRoot 'LICENSE') -Destination (Join-Path $stage 'LICENSE')
        foreach ($policy in @('PRIVACY.md', 'PRIVACY.en.md')) {
            Copy-Item -LiteralPath (Join-Path $repoRoot $policy) -Destination $stage
        }
        foreach ($changelog in @('CHANGELOG.md', 'CHANGELOG.en.md')) {
            Copy-Item -LiteralPath (Join-Path $repoRoot $changelog) -Destination (Join-Path $stage $changelog)
        }
        [ordered]@{
            version = $version; channel = 'release'; revision = $revision; uncommittedChanges = $dirty
            portable = [bool]$isPortable
            renderingDiagnostics = [bool]$RenderDiagnostics
            builtAt = $builtAt; architecture = 'windows-x64'
            buildEnvironment = $buildEnvironment
            files = @($files) + @($agentFiles)
        } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $stage 'build.json') -Encoding UTF8
        $null = & (Join-Path $PSScriptRoot 'validation/test-agent-package.ps1') -Directory $stage
        $archive = Join-Path $outRoot "$name.zip"
        Compress-Archive -LiteralPath $stage -DestinationPath $archive -CompressionLevel Optimal
        $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $name.zip" | Set-Content -LiteralPath "$archive.sha256" -Encoding ASCII
        Write-Output $archive
        if ($Installer -and -not $isPortable) {
            $setupRoot = Join-Path $repoRoot "target\installers\$version-$revisionLabel-$stamp"
            $setups = @()
            if ($InstallerFormat -in @('Exe', 'Both')) {
                $setups += & (Join-Path $PSScriptRoot 'packaging/build-exe.ps1') -SourcePath $stage -OutputPath $setupRoot -Version $version -InnoCompiler $InnoCompiler -Compression $ExeCompression
            }
            if ($InstallerFormat -in @('Msi', 'Both')) {
                $setups += & (Join-Path $PSScriptRoot 'packaging/build-msi.ps1') -SourcePath $stage -OutputPath $setupRoot -Version $version
            }
            foreach ($setup in $setups) {
                $setupHash = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
                "$setupHash  $(Split-Path -Leaf $setup)" | Set-Content -LiteralPath "$setup.sha256" -Encoding ASCII
                Write-Output $setup
            }
        }
    }
} finally {
    $env:LUCIDDESK_BUILD_REVISION = $previousRevision
    Pop-Location
}
