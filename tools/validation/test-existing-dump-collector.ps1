$ErrorActionPreference = 'Stop'
$testRoot = Join-Path $PSScriptRoot ("../../target/dump-collector-regression-" + [guid]::NewGuid().ToString('N'))
$testRoot = [IO.Path]::GetFullPath($testRoot)
New-Item -ItemType Directory -Path $testRoot | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../render-diagnostics/Collect-Existing-Dumps.ps1') -Destination $testRoot
$previousProgramData = $env:ProgramData
$previousLocalData = $env:LOCALAPPDATA
try {
    $env:ProgramData = Join-Path $testRoot 'ProgramData'
    $env:LOCALAPPDATA = Join-Path $testRoot 'Local'
    $tempRoot = Join-Path $env:ProgramData 'Microsoft/Windows/WER/Temp'
    $archive = Join-Path $env:ProgramData 'Microsoft/Windows/WER/ReportArchive/AppCrash_explorer.exe_fixture'
    $unrelated = Join-Path $env:ProgramData 'Microsoft/Windows/WER/ReportArchive/AppCrash_other.exe_fixture'
    $local = Join-Path $env:LOCALAPPDATA 'CrashDumps'
    New-Item -ItemType Directory -Path $tempRoot,$archive,$unrelated,$local -Force | Out-Null
    $tempDump = Join-Path $tempRoot 'WER123.tmp.mdmp'
    $outsideDump = Join-Path $testRoot 'outside.dmp'
    foreach ($path in @($tempDump, $outsideDump, (Join-Path $archive 'app.dmp'), (Join-Path $unrelated 'private.dmp'), (Join-Path $local 'luciddesk.exe.123.dmp'))) {
        Set-Content -LiteralPath $path -Value 'fixture, not a real dump'
    }
    # WER event XML puts closing tags on lines that start with absolute paths.
    # Include malformed paths ending in .dmp as well as unrelated valid paths.
    @(
        ('\\?\' + $tempDump),
        $tempDump,
        ($tempDump + '</Data><Data>other data</Data>'),
        ($tempRoot + '\bad<name.dmp'),
        ($tempRoot + '\bad"name.dmp'),
        ($tempRoot + '\bad' + [char]0 + 'name.dmp'),
        ($tempRoot + '\x\..\..\..\..\..\outside.dmp'),
        $outsideDump,
        'C:\not a dump: explanatory text'
    ) | Set-Content -LiteralPath (Join-Path $testRoot 'explorer-errors.txt') -Encoding UTF8
    & (Join-Path $testRoot 'Collect-Existing-Dumps.ps1')
    $result = @(Get-ChildItem -LiteralPath $testRoot -Directory -Filter 'crash-dumps-*')
    if ($result.Count -ne 1) { throw 'Expected one collection output directory' }
    $dumps = @(Get-ChildItem -LiteralPath $result[0].FullName -File | Where-Object { $_.Extension -in @('.dmp', '.mdmp') })
    if ($dumps.Count -ne 3 -or ($dumps.Name -match 'private|outside')) {
        throw "Incorrect dump selection: $($dumps.Name -join ', ')"
    }
    if (-not (Test-Path -LiteralPath ($result[0].FullName + '.zip'))) { throw 'Missing archive' }
    Write-Host "PASS: malformed XML/path lines skipped; 3 intended dumps collected; unrelated paths excluded. PowerShell $($PSVersionTable.PSVersion)"
} finally {
    $env:ProgramData = $previousProgramData
    $env:LOCALAPPDATA = $previousLocalData
}
