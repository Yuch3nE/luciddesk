[CmdletBinding()]
param([Parameter(Mandatory)][string]$Directory)
$ErrorActionPreference = 'Stop'
$Directory = (Resolve-Path -LiteralPath $Directory).Path
if (-not (Get-Command dumpbin.exe -ErrorAction SilentlyContinue)) {
    & (Join-Path $PSScriptRoot 'use-windows-toolchain.ps1') | Out-Host
}
foreach ($name in @('luciddesk.exe', 'luciddesk-cli.exe', 'luciddesk_explorer.dll')) {
    $path = Join-Path $Directory $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing runtime payload: $path" }
    # /DEPENDENTS includes both normal and delay-load DLL imports.
    $output = & dumpbin.exe /NOLOGO /DEPENDENTS $path
    if ($LASTEXITCODE -ne 0) { throw "Could not inspect runtime dependencies: $path" }
    $imports = @($output | ForEach-Object {
        if ($_ -match '^\s+([\w.+-]+\.dll)\s*$') { $Matches[1] }
    })
    if (-not $imports.Count) { throw "No DLL imports found while inspecting $path" }
    $externalCrt = @($imports | Where-Object { $_ -match '^(vcruntime|msvcp|msvcr|concrt|vcomp)\d.*\.dll$' })
    if ($externalCrt.Count) {
        throw "$name requires external VC++ runtime DLLs: $($externalCrt -join ', '). Rebuild with target-feature=+crt-static; check RUSTFLAGS overrides."
    }
    Write-Host "$name runtime imports verified: $($imports -join ', ')"
}
