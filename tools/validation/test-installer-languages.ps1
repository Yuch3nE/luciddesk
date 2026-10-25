[CmdletBinding()]
param([string]$InnoCompiler)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $InnoCompiler) { $InnoCompiler = Join-Path $repo 'target/tooling/inno-7.1.0/ISCC.exe' }
$InnoCompiler = (Resolve-Path -LiteralPath $InnoCompiler).Path
$source = Get-Content -LiteralPath (Join-Path $repo 'installer/LucidDesk.iss') -Raw -Encoding utf8
function Section([string]$name) {
    [regex]::Match($source, "(?ms)^\[$name\]\s*\r?\n(.*?)(?=^\[|\z)").Groups[1].Value.Trim()
}
$languages = Section 'Languages'
$custom = Section 'CustomMessages'
$names = @([regex]::Matches($languages, 'Name: "([^"]+)"') | ForEach-Object { $_.Groups[1].Value })
$messages = @([regex]::Matches($custom, '(?m)^([a-z]+)\.([A-Za-z]+)=(.+)\r?$'))
$keys = @($messages | Where-Object { $_.Groups[1].Value -eq 'english' } | ForEach-Object { $_.Groups[2].Value })
$root = Join-Path $repo ('target/installer-language-test/' + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $root | Out-Null
# Compile the production language sections, but stop in InitializeSetup.
# No application payload, registration, configuration or installed files are touched.
$checks = ($keys | ForEach-Object { "  Log('L10N:$_=' + CustomMessage('$_'));" }) -join "`n"
$fixture = @"
[Setup]
AppId=LucidDeskLanguageTest-$([guid]::NewGuid())
AppName=LucidDesk Language Test
AppVersion=1.0
DefaultDirName={tmp}\LucidDeskLanguageTest
PrivilegesRequired=lowest
Uninstallable=no
UsePreviousLanguage=no
ShowLanguageDialog=no
OutputDir=$root
OutputBaseFilename=language-test
[Languages]
$languages
[CustomMessages]
$custom
[Code]
function InitializeSetup(): Boolean;
begin
  Log('L10N:Language=' + ActiveLanguage);
$checks
  Result := False;
end;
"@
$script = Join-Path $root 'language-test.iss'
[IO.File]::WriteAllText($script, $fixture, [Text.UTF8Encoding]::new($true))
& $InnoCompiler /Q $script
if ($LASTEXITCODE -ne 0) { throw 'Language fixture compilation failed.' }
foreach ($name in $names) {
    $log = Join-Path $root "$name.log"
    $process = Start-Process -FilePath (Join-Path $root 'language-test.exe') -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LANG=$name", "/LOG=`"$log`"") -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(30000)) { throw "Language fixture timed out: $name" }
    # InitializeSetup=False intentionally aborts before installation.
    if ($process.ExitCode -ne 1) { throw "Unexpected exit for ${name}: $($process.ExitCode)" }
    $text = Get-Content -LiteralPath $log -Raw -Encoding utf8
    if (-not $text.Contains("L10N:Language=$name")) { throw "Wrong active language: $name" }
    foreach ($message in $messages | Where-Object { $_.Groups[1].Value -eq $name }) {
        $expected = 'L10N:' + $message.Groups[2].Value + '=' + $message.Groups[3].Value.TrimEnd("`r")
        if (-not $text.Contains($expected)) { throw "Missing or altered message: $name.$($message.Groups[2].Value)" }
    }
    Write-Output "PASS: $name ($($keys.Count) custom messages)"
}
Write-Output "Language logs: $root"
