# Build a release-signer interoperability fixture for the native updater test.
param([Parameter(Mandatory)][string]$Directory)
$ErrorActionPreference = 'Stop'
$key = [Security.Cryptography.RSA]::Create(2048)
try {
    $env:UPDATE_PRIVATE_KEY = $key.ExportPkcs8PrivateKeyPem()
    $env:UPDATE_RSA_MODULUS = [Convert]::ToHexString($key.ExportParameters($false).Modulus)
    foreach ($suffix in 'abcdef0-windows-x64-20260101-120000.zip','windows-x64-portable.zip','windows-x64-setup.exe') {
        [IO.File]::WriteAllBytes((Join-Path $Directory "LucidDesk-99.0.1-$suffix"), [byte[]](1,2,3))
    }
    & (Join-Path $PSScriptRoot '../release/sign-update.ps1') $Directory 'v99.0.1' (Join-Path $Directory 'update.json')
    [IO.File]::WriteAllText((Join-Path $Directory 'public.hex'), $env:UPDATE_RSA_MODULUS)
} finally { $key.Dispose() }
