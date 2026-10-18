[CmdletBinding()]
param([string]$TargetDirectory = 'target/plugin-tests', [switch]$Offline)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Push-Location -LiteralPath $repoRoot
$previousMonitor = $env:LUCIDDESK_TEST_MONITOR_PLUGIN
$previousExample = $env:LUCIDDESK_TEST_EXAMPLE_PLUGIN
try {
    $common = @('--locked', '--target-dir', $TargetDirectory)
    if ($Offline) { $common += '--offline' }
    & cargo test -p luciddesk-plugin-api -p luciddesk-monitor @common -- --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Plugin unit tests failed.' }
    $artifacts = & cargo test -p luciddesk --bin luciddesk @common --no-run --message-format=json-render-diagnostics
    if ($LASTEXITCODE -ne 0) { throw 'Plugin host build failed.' }
    $executables = @($artifacts | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object {
        $_.reason -eq 'compiler-artifact' -and $_.profile.test -and $_.target.name -eq 'luciddesk' -and $_.executable
    } | ForEach-Object executable)
    if ($executables.Count -ne 1) { throw 'Expected one host test executable.' }
    $executable = $executables[0]
    $destination = Join-Path (Split-Path -Parent $executable) 'plugins'
    $filters = @('pane::monitor::', 'pane::settings::monitor_page::',
        'pane::settings::tests::native_monitor_settings_switch_')
    # The host uses the Windows GUI subsystem; a pipeline makes PowerShell wait.
    $listing = @(& $executable --list | ForEach-Object { $_ })
    if ($LASTEXITCODE -ne 0) { throw 'Could not list host tests.' }
    foreach ($filter in ($filters + @('plugins::tests::'))) {
        if (-not ($listing | Where-Object { $_.StartsWith($filter) -and $_.EndsWith(': test') })) {
            throw "No host tests found for $filter"
        }
    }
    $null = & (Join-Path $PSScriptRoot '../build-plugins.ps1') -TargetDirectory $TargetDirectory -Destination $destination -Offline:$Offline
    & cargo build -p luciddesk-plugin-api --example minimal @common
    if ($LASTEXITCODE -ne 0) { throw 'Example plugin build failed.' }
    Copy-Item -LiteralPath (Join-Path $TargetDirectory 'debug/examples/minimal.dll') -Destination (Join-Path $destination 'echo-example.dll') -Force
    $env:LUCIDDESK_TEST_EXAMPLE_PLUGIN = '1'
    $env:LUCIDDESK_TEST_MONITOR_PLUGIN = '1'
    & $executable @filters plugins::tests:: --test-threads=1 | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Plugin host tests failed.' }
    # Temporarily disable only our staged monitor DLL; leave other plugins untouched.
    $dll = Join-Path $destination 'luciddesk_monitor.dll'
    $disabled = "$dll.disabled"
    Move-Item -LiteralPath $dll -Destination $disabled
    try {
        $env:LUCIDDESK_TEST_MONITOR_PLUGIN = '0'
        & $executable @filters --test-threads=1 | Out-Host
        if ($LASTEXITCODE -ne 0) { throw 'Missing-plugin host tests failed.' }
    } finally {
        Move-Item -LiteralPath $disabled -Destination $dll
    }
} finally {
    $env:LUCIDDESK_TEST_MONITOR_PLUGIN = $previousMonitor
    $env:LUCIDDESK_TEST_EXAMPLE_PLUGIN = $previousExample
    Pop-Location
}
