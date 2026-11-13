# PowerShell 7 on the release runner. Only the public modulus is embedded in the app.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ArtifactRoot,
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$Destination
)
$ErrorActionPreference = 'Stop'
if (!$env:UPDATE_RSA_MODULUS -and !$env:UPDATE_PRIVATE_KEY) {
    Write-Host 'Release signing is not configured. Publishing packages for manual download.'
    return
}
if ($Tag -cnotmatch '^v?(\d+\.\d+\.\d+)$') { throw 'Invalid release tag.' }
$version = $Matches[1]
$prefix = '^LucidDesk-' + [regex]::Escape($version)
$patterns = [ordered]@{
    green = $prefix + '-[0-9a-f]{7,40}-windows-x64-[0-9-]+\.zip$'
    portable = $prefix + '-windows-x64-portable\.zip$'
    installed = $prefix + '-windows-x64-setup\.exe$'
}
$packages = [ordered]@{}
$files = Get-ChildItem -LiteralPath $ArtifactRoot -Recurse -File
foreach ($kind in $patterns.Keys) {
    $matchesForKind = @($files | Where-Object { $_.Name -cmatch $patterns[$kind] })
    if ($matchesForKind.Count -ne 1) { throw "Expected exactly one $kind package." }
    $file = $matchesForKind[0]
    $packages[$kind] = [ordered]@{
        name = $file.Name
        size = $file.Length
        sha256 = (Get-FileHash -LiteralPath $file.FullName).Hash.ToLowerInvariant()
    }
}
$rsa = [Security.Cryptography.RSA]::Create()
try {
    $rsa.ImportFromPem($env:UPDATE_PRIVATE_KEY)
    $public = $rsa.ExportParameters($false)
    if ($rsa.KeySize -lt 2048 -or [Convert]::ToHexString($public.Modulus) -ne $env:UPDATE_RSA_MODULUS -or
        [Convert]::ToHexString($public.Exponent) -ne '010001') { throw 'Invalid or mismatched release signing key.' }
    $manifest = @{ version = $version; packages = $packages } | ConvertTo-Json -Depth 5 -Compress
    $bytes = [Text.Encoding]::UTF8.GetBytes($manifest)
    $signature = $rsa.SignData($bytes, [Security.Cryptography.HashAlgorithmName]::SHA256,
        [Security.Cryptography.RSASignaturePadding]::Pkcs1)
    @{ payload = [Convert]::ToBase64String($bytes); signature = [Convert]::ToBase64String($signature) } |
        ConvertTo-Json -Compress | Set-Content -LiteralPath $Destination -Encoding ascii
} finally { $rsa.Dispose() }
