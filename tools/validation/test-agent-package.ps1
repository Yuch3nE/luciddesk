[CmdletBinding()]
param([Parameter(Mandatory)][string]$Directory, [switch]$NoPlugins)
$ErrorActionPreference = 'Stop'
$Directory = (Resolve-Path -LiteralPath $Directory).Path
# Inspect imports before executing the CLI; a missing CRT can otherwise open a
# loader error dialog even when the installer or command is running silently.
& (Join-Path $PSScriptRoot 'test-runtime-dependencies.ps1') -Directory $Directory
$build = Get-Content -LiteralPath (Join-Path $Directory 'build.json') -Raw | ConvertFrom-Json
$pluginPath = 'plugins/luciddesk_monitor.dll'
$hasPlugin = Test-Path -LiteralPath (Join-Path $Directory $pluginPath) -PathType Leaf
if ($hasPlugin -ne [bool]$build.systemMonitorPlugin) { throw 'Plugin payload does not match build manifest' }
$pluginEntries = @($build.files | Where-Object { $_.file -like 'plugins/*' })
$actualPlugins = @(if (Test-Path -LiteralPath (Join-Path $Directory 'plugins')) {
    Get-ChildItem -LiteralPath (Join-Path $Directory 'plugins') -File -Filter '*.dll'
})
if ($NoPlugins -and ($build.plugins.Count -or $build.systemMonitorPlugin -or $actualPlugins.Count -or $pluginEntries.Count)) {
    throw 'Release payload must not contain plugins.'
}
if ($actualPlugins.Count -ne $pluginEntries.Count) { throw 'Plugin file count does not match build manifest' }
foreach ($plugin in $actualPlugins) {
    $entries = @($pluginEntries | Where-Object file -eq ('plugins/' + $plugin.Name))
    if ($entries.Count -ne 1 -or (Get-FileHash -LiteralPath $plugin.FullName -Algorithm SHA256).Hash -ne $entries[0].sha256) {
        throw "Plugin payload hash mismatch: $($plugin.Name)"
    }
}
$required = @('luciddesk-cli.exe', 'cli.md', 'protocol.schema.json', 'skills/luciddesk-control/SKILL.md')
foreach ($file in $required) {
    $entry = @($build.files | Where-Object file -eq $file)
    if ($entry.Count -ne 1) { throw "Missing or duplicate manifest entry: $file" }
    $hash = (Get-FileHash -LiteralPath (Join-Path $Directory $file) -Algorithm SHA256).Hash
    if ($hash -ne $entry[0].sha256) { throw "Payload hash mismatch: $file" }
}
$cli = Join-Path $Directory 'luciddesk-cli.exe'
$version = & $cli --version
if ($LASTEXITCODE -ne 0 -or $version -ne "luciddesk-cli $($build.version) protocol 1") { throw 'Packaged CLI version mismatch' }
$embedded = (& $cli skill show --json) | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or -not $embedded.ok) { throw 'Offline skill discovery failed' }
$skill = [IO.File]::ReadAllText((Join-Path $Directory 'skills/luciddesk-control/SKILL.md'))
if ($embedded.data.content -cne $skill) { throw 'Embedded and distributed Skills differ' }
if ($embedded.data.bundle_version -ne 1 -or -not $embedded.data.files) { throw 'Missing Skill bundle' }
foreach ($file in $embedded.data.files.PSObject.Properties) {
    $relative = "skills/luciddesk-control/$($file.Name)"
    $entry = @($build.files | Where-Object file -eq $relative)
    if ($entry.Count -ne 1) { throw "Missing or duplicate Skill manifest entry: $relative" }
    $path = Join-Path $Directory $relative
    if ([IO.File]::ReadAllText($path) -cne $file.Value) { throw "Skill bundle content mismatch: $relative" }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $entry[0].sha256) { throw "Skill bundle hash mismatch: $relative" }
}
$schema = (& $cli schema --json) -join "`n"
if ($LASTEXITCODE -ne 0) { throw 'Offline schema discovery failed' }
$packagedSchema = [IO.File]::ReadAllText((Join-Path $Directory 'protocol.schema.json'))
if (-not [System.Text.Json.Nodes.JsonNode]::DeepEquals([System.Text.Json.Nodes.JsonNode]::Parse($schema),[System.Text.Json.Nodes.JsonNode]::Parse($packagedSchema))) { throw 'Embedded and distributed schema differ' }
[ordered]@{ result='passed'; directory=$Directory; version=$build.version; files=$required } | ConvertTo-Json
