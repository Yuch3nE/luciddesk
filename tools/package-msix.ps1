[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourcePath,
    [string]$IdentityName = 'Yuchen95.LucidDesk',
    [string]$Publisher = 'CN=407B0E68-BE57-40C1-908A-6AD24F037975',
    [string]$PublisherDisplayName = 'Yuchen95',
    [string]$PackageVersion,
    [string]$CertificateThumbprint
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$source = (Resolve-Path -LiteralPath $SourcePath).ProviderPath
$build = Get-Content -LiteralPath (Join-Path $source 'build.json') -Raw | ConvertFrom-Json
if ($build.portable -or $build.renderingDiagnostics) { throw 'MSIX requires a normal production package.' }
if ($build.version -notmatch '^\d+\.\d+\.\d+$') { throw 'MSIX requires a numeric release version.' }
if (-not $PackageVersion) {
    # Store requires a nonzero major; offset every app major to preserve update ordering.
    $appVersion = [version]$build.version
    $PackageVersion = "$($appVersion.Major + 1).$($appVersion.Minor).$($appVersion.Build).0"
}
if ($PackageVersion -notmatch '^\d+\.\d+\.\d+\.0$') { throw 'Store package version must have four numeric parts and end in .0.' }
$versionParts = @($PackageVersion.Split('.') | ForEach-Object { [long]$_ })
if ($versionParts[0] -lt 1 -or @($versionParts | Where-Object { $_ -gt 65535 }).Count) {
    throw 'Store package version parts must be within 0..65535, with a nonzero major.'
}
if ($IdentityName -notmatch '^[A-Za-z0-9.-]{3,50}$') { throw 'Invalid MSIX identity name.' }
foreach ($file in @('luciddesk.exe', 'luciddesk_explorer.dll', 'luciddesk-cli.exe', 'cli.md', 'protocol.schema.json', 'skills/luciddesk-control/SKILL.md')) {
    $entry = @($build.files | Where-Object file -eq $file)
    if ($entry.Count -ne 1 -or (Get-FileHash -LiteralPath (Join-Path $source $file) -Algorithm SHA256).Hash -ne $entry[0].sha256) {
        throw "Source checksum mismatch: $file"
    }
}
& (Join-Path $PSScriptRoot 'use-windows-toolchain.ps1')
$sdkBin = Join-Path $env:WindowsSdkDir "bin/$($env:WindowsSDKVersion.TrimEnd('\'))/x64"
$makeAppx = Join-Path $sdkBin 'makeappx.exe'
$signTool = Join-Path $sdkBin 'signtool.exe'
if (-not (Test-Path -LiteralPath $makeAppx)) { throw 'Windows SDK MakeAppx.exe is missing.' }
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss-fff'
$outRoot = Join-Path $repoRoot "target/msix/$stamp"
$stage = Join-Path $outRoot 'stage'
$assets = Join-Path $stage 'Assets'
New-Item -ItemType Directory -Path $assets -Force | Out-Null
foreach ($file in @('luciddesk.exe', 'luciddesk_explorer.dll', 'luciddesk-cli.exe', 'build.json', 'LICENSE', 'cli.md', 'protocol.schema.json')) {
    Copy-Item -LiteralPath (Join-Path $source $file) -Destination $stage
}
Copy-Item -LiteralPath (Join-Path $source 'skills') -Destination $stage -Recurse
[IO.File]::WriteAllBytes((Join-Path $stage 'msix'), [byte[]]@())
$null = & (Join-Path $PSScriptRoot 'validation/test-agent-package.ps1') -Directory $stage

# Generate package logos from the application's existing icon.
Add-Type -AssemblyName System.Drawing
$stream = [IO.File]::OpenRead((Join-Path $repoRoot 'app/assets/luciddesk.ico'))
try {
    $icon = [Drawing.Icon]::new($stream, 256, 256)
    try {
        $bitmap = $icon.ToBitmap()
        try {
            foreach ($logo in @(@('Square44x44Logo.png', 44), @('Square150x150Logo.png', 150), @('StoreLogo.png', 50))) {
                $output = [Drawing.Bitmap]::new([int]$logo[1], [int]$logo[1])
                try {
                    $graphics = [Drawing.Graphics]::FromImage($output)
                    try {
                        $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                        $graphics.DrawImage($bitmap, 0, 0, $output.Width, $output.Height)
                    } finally { $graphics.Dispose() }
                    $output.Save((Join-Path $assets $logo[0]), [Drawing.Imaging.ImageFormat]::Png)
                } finally { $output.Dispose() }
            }
        } finally { $bitmap.Dispose() }
    } finally { $icon.Dispose() }
} finally { $stream.Dispose() }

$escape = { param($value) [Security.SecurityElement]::Escape($value) }
$identityXml = & $escape $IdentityName
$publisherXml = & $escape $Publisher
$displayPublisherXml = & $escape $PublisherDisplayName
$manifest = @"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
         xmlns:desktop="http://schemas.microsoft.com/appx/manifest/desktop/windows10"
         xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
         IgnorableNamespaces="uap desktop rescap">
  <Identity Name="$identityXml" Publisher="$publisherXml" Version="$PackageVersion" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>LucidDesk</DisplayName>
    <PublisherDisplayName>$displayPublisherXml</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
    <Description>Windows desktop organizer with desktop panels and file search.</Description>
  </Properties>
  <Resources><Resource Language="en-us" /><Resource Language="zh-cn" /></Resources>
  <Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0" /></Dependencies>
  <Applications>
    <Application Id="LucidDesk" Executable="luciddesk.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="LucidDesk" Description="Windows desktop organizer"
                          Square150x150Logo="Assets\Square150x150Logo.png"
                          Square44x44Logo="Assets\Square44x44Logo.png" BackgroundColor="transparent" />
      <Extensions>
        <desktop:Extension Category="windows.startupTask" Executable="luciddesk.exe" EntryPoint="Windows.FullTrustApplication">
          <desktop:StartupTask TaskId="LucidDeskStartup" Enabled="false" DisplayName="LucidDesk" />
        </desktop:Extension>
      </Extensions>
    </Application>
  </Applications>
  <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>
"@
$manifest | Set-Content -LiteralPath (Join-Path $stage 'AppxManifest.xml') -Encoding utf8
$suffix = if ($CertificateThumbprint) { '' } else { '-unsigned' }
$package = Join-Path $outRoot "LucidDesk-$PackageVersion-windows-x64$suffix.msix"
# Keep MakeAppx's full semantic validation enabled.
& $makeAppx pack /d $stage /p $package /h SHA256 /o
if ($LASTEXITCODE -ne 0) { throw 'MSIX packing or manifest validation failed.' }
if ($CertificateThumbprint) {
    if ($CertificateThumbprint -notmatch '^[0-9A-Fa-f]{40}$') { throw 'Invalid signing certificate thumbprint.' }
    $certificate = Get-Item -LiteralPath "Cert:/CurrentUser/My/$CertificateThumbprint"
    if (-not $certificate.HasPrivateKey -or $certificate.Subject -cne $Publisher) {
        throw 'Signing certificate must have a private key and match the manifest Publisher.'
    }
    & $signTool sign /fd SHA256 /sha1 $CertificateThumbprint $package
    if ($LASTEXITCODE -ne 0) { throw 'MSIX signing failed.' }
    & $signTool verify /pa $package
    if ($LASTEXITCODE -ne 0) { throw 'MSIX signature trust verification failed.' }
}
$hash = (Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $(Split-Path -Leaf $package)" | Set-Content -LiteralPath "$package.sha256" -Encoding ascii
Copy-Item -LiteralPath (Join-Path $repoRoot 'docs/msix.md') -Destination (Join-Path $outRoot 'README.md')
Write-Output $package
