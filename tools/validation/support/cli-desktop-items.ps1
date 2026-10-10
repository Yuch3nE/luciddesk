# Invoked by test-cli-plan.ps1 -LiveDesktopItems; inherits its isolated GUI/CLI helpers.
$ErrorActionPreference = 'Stop'
$probe = Join-Path $build 'examples/desktop_snapshot.exe'
if (-not (Test-Path -LiteralPath $probe)) { throw 'Build luciddesk-shell --example desktop_snapshot first' }
$desktop = [IO.Path]::GetFullPath([Environment]::GetFolderPath('DesktopDirectory'))
$fixtureTag = 'LucidDesk-CLI-test-' + [guid]::NewGuid().ToString('N')
$fixtures = @{}
$ownedPane = $null
function Native-Items {
    $raw = & $probe
    if ($LASTEXITCODE -ne 0) { throw 'Independent Explorer snapshot failed' }
    return ($raw | ConvertFrom-Json).items
}
function Same-Path($Left, $Right) {
    if (-not $Left -or -not $Right) { return $false }
    return ([IO.Path]::GetFullPath($Left) -replace '^\\\\\?\\','') -eq ([IO.Path]::GetFullPath($Right) -replace '^\\\\\?\\','')
}
function Fixture-Items {
    $all = (Invoke-Cli @('item','list')).data.items
    return @($all | Where-Object { $item = $_; @($fixtures.Keys | Where-Object { Same-Path $item.path $_ }).Count -gt 0 })
}
function Expect-Failure([string[]]$Command, [string]$Code) {
    $raw = & $cli @Command --data-dir $root --json
    $result = $raw | ConvertFrom-Json
    if ($LASTEXITCODE -eq 0 -or $result.error.code -ne $Code) { throw "Expected $Code : $raw" }
}
try {
    foreach ($suffix in @('A','B')) {
        $path = Join-Path $desktop "$fixtureTag-$suffix.txt"
        $content = "LucidDesk CLI owned fixture $fixtureTag $suffix"
        $stream = [IO.File]::Open($path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
        try {
            $bytes = [Text.Encoding]::UTF8.GetBytes($content)
            $stream.Write($bytes,0,$bytes.Length)
            $fixtures[$path] = $content
        } finally { $stream.Dispose() }
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        $items = @(Fixture-Items | Sort-Object path)
        if ($items.Count -eq 2) { break }
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Desktop fixtures not discovered' }
        Start-Sleep -Milliseconds 150
    } while ($true)
    $nativeBefore = @(Native-Items)
    $ids = @($items | ForEach-Object { [string]$_.id })
    $created = Invoke-Plan @(@{op='pane.create';ref='live';title=$fixtureTag},@{op='item.assign';item_ids=$ids;pane_ref='live'})
    $ownedPane = [string]$created.data.refs.live
    if ($created.data.presentation_status -ne 'applied') { throw ($created | ConvertTo-Json -Depth 20) }
    $inside = (Invoke-Cli @('item','list','--pane',$ownedPane)).data.items
    if ($inside.Count -ne 2) { throw 'Assigned pane does not contain both fixtures' }
    $nativeHidden = @(Native-Items)
    foreach ($path in $fixtures.Keys) {
        if ($nativeHidden | Where-Object { Same-Path $_.path $path }) { throw 'Confirmed receipt still leaves item on native desktop' }
        if ([IO.File]::ReadAllText($path) -cne $fixtures[$path]) { throw 'Assignment modified file contents' }
    }
    $null = Invoke-Plan @(@{op='item.reorder';pane_id=$ownedPane;item_ids=@($ids[1],$ids[0])})
    $ordered = @((Invoke-Cli @('item','list','--pane',$ownedPane)).data.items | Sort-Object { $_.placement.row }, { $_.placement.column })
    if ($ordered[0].id -ne $ids[1] -or $ordered[1].id -ne $ids[0]) { throw 'Explicit item order was not applied' }
    Expect-Failure @('pane','remove','--id',$ownedPane) 'INVALID_REQUEST'
    $null = Invoke-Plan @(@{op='pane.update';pane_id=$ownedPane;locked=$true})
    Expect-Failure @('item','release','--ids',$ids[0]) 'PANE_LOCKED'
    $null = Invoke-Plan @(@{op='pane.update';pane_id=$ownedPane;locked=$false})
    $stale = Invoke-Cli @('pane','update','--id',$ownedPane,'--title','stale','--dry-run')
    $null = Invoke-Plan @(@{op='pane.update';pane_id=$ownedPane;title='concurrent preserved'})
    Expect-Failure @('plan','apply','--token',$stale.data.plan_token,'--request-id',([guid]::NewGuid().ToString())) 'CONFLICT'
    if ((Invoke-Cli @('pane','get','--id',$ownedPane)).data.title -ne 'concurrent preserved') { throw 'Stale plan overwrote current title' }
    $released = Invoke-Plan @(@{op='item.release';item_ids=@($ids[0])})
    if ($released.data.presentation_status -ne 'applied') { throw 'Release was not confirmed' }
    if (-not (@(Native-Items) | Where-Object { Same-Path $_.path $items[0].path })) { throw 'Released item absent from native desktop' }
    $removed = Invoke-Plan @(@{op='pane.remove';pane_id=$ownedPane;release_items=$true})
    if ($removed.data.presentation_status -ne 'applied') { throw 'Remove-and-release was not confirmed' }
    $ownedPane = $null
    $nativeAfter = @(Native-Items)
    foreach ($item in $nativeBefore) {
        $after = @($nativeAfter | Where-Object key -eq $item.key)
        if ($after.Count -ne 1 -or $after[0].x -ne $item.x -or $after[0].y -ne $item.y) { throw "Desktop position changed for $($item.key)" }
    }
    foreach ($path in $fixtures.Keys) {
        if ([IO.File]::ReadAllText($path) -cne $fixtures[$path]) { throw 'Release changed file contents' }
    }
    @{result='passed';fixtures=2;native_visibility_verified=$true;positions_restored=$true;conflict_preserved=$true} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'desktop-items-result.json')
} finally {
    if ($ownedPane) {
        try { $null = Invoke-Plan @(@{op='pane.update';pane_id=$ownedPane;locked=$false},@{op='pane.remove';pane_id=$ownedPane;release_items=$true}) }
        catch { Write-Warning "Could not release fixture panel before GUI exit: $_" }
    }
    foreach ($path in $fixtures.Keys) {
        # Exact owned leaf files only; preserve any file edited by someone else during testing.
        if ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($path)) -ne $desktop -or -not [IO.Path]::GetFileName($path).StartsWith($fixtureTag)) { throw 'Invalid fixture cleanup target' }
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            if ([IO.File]::ReadAllText($path) -ceq $fixtures[$path]) { Remove-Item -LiteralPath $path }
            else { Write-Warning "Preserving changed fixture: $path" }
        }
    }
}
