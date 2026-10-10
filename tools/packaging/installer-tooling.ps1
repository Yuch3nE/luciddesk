# Shared paths and prerequisites for MSI packaging; never installs tools implicitly.
function Get-WixTooling {
    $wixRoot = Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'target/tooling/wix-5.0.2'
    $compiler = Join-Path $wixRoot 'wix.exe'
    $extension = Join-Path $wixRoot '.wix/extensions/WixToolset.UI.wixext/5.0.2/wixext5/WixToolset.UI.wixext.dll'
    if (-not (Test-Path -LiteralPath $compiler -PathType Leaf) -or
        -not (Test-Path -LiteralPath $extension -PathType Leaf)) {
        throw 'Run ./tools/ensure-wix.ps1 first. WiX is kept inside the project.'
    }
    [pscustomobject]@{ Compiler = $compiler; Extension = $extension }
}
