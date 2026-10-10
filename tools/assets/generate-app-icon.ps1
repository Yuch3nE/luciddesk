# Requires ImageMagick 7. Run only when the selected design changes.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$sourcePath = Join-Path $projectRoot 'docs\design\luciddesk-green-square-v19.png'
$iconOutput = Join-Path $projectRoot 'app\assets\luciddesk.ico'
# Measure the visible artwork, ignoring almost-transparent generation artifacts.
# Use this mask only for bounds; leave the selected design untouched.
$artBounds = & magick $sourcePath -alpha extract -threshold '25%' -format '%@' info:
if ($LASTEXITCODE -ne 0 -or $artBounds -notmatch '^(\d+)x(\d+)\+(\d+)\+(\d+)$') {
    throw 'Failed to measure the application icon artwork.'
}
$artWidth = [int]$Matches[1]
$artHeight = [int]$Matches[2]
if ($artWidth -le 0 -or $artHeight -le 0) {
    throw 'The application icon has no visible artwork.'
}
# Fill 87.5% of the square icon cell (14 of 16 pixels at tray size).
$canvasSize = [int][Math]::Ceiling([Math]::Max($artWidth, $artHeight) / 0.875)
# Normalize transparent padding before high-quality premultiplied resampling.
Add-Type -AssemblyName System.Drawing
$normalized = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName() + '.png')
$sizes = @(16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256)
$frames = [System.Collections.Generic.List[byte[]]]::new()
# Visible component bounds in the selected v19 artwork (25% alpha threshold).
$componentBounds = @('446x940+157+157', '446x444+651+157', '446x445+651+652')
$components = [System.Collections.Generic.List[System.Drawing.Image]]::new()
$componentFiles = [System.Collections.Generic.List[string]]::new()
function New-SmallIconImage([int]$size) {
    # Pixel-align the silhouette and gaps instead of blurring a fractional grid.
    $padding = [int][Math]::Round($size / 16)
    $extent = $size - 2 * $padding
    $gap = [Math]::Max(1, [int][Math]::Round($extent * 48 / 940))
    $first = [int][Math]::Ceiling(($extent - $gap) / 2)
    $second = $extent - $gap - $first
    $secondStart = $padding + $first + $gap
    $rectangles = @(
        [System.Drawing.Rectangle]::new($padding, $padding, $first, $extent),
        [System.Drawing.Rectangle]::new($secondStart, $padding, $second, $first),
        [System.Drawing.Rectangle]::new($secondStart, $secondStart, $second, $second)
    )
    $bitmap = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppPArgb)
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $attributes = [System.Drawing.Imaging.ImageAttributes]::new()
            try {
                # Mirror source edges to avoid sampling transparent black outside each crop.
                $attributes.SetWrapMode([System.Drawing.Drawing2D.WrapMode]::TileFlipXY)
                $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
                $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                for ($i = 0; $i -lt $components.Count; $i++) {
                    $image = $components[$i]
                    $graphics.DrawImage($image, $rectangles[$i], 0, 0, $image.Width, $image.Height,
                        [System.Drawing.GraphicsUnit]::Pixel, $attributes)
                }
            } finally { $attributes.Dispose() }
        } finally { $graphics.Dispose() }
        return ,$bitmap
    } catch { $bitmap.Dispose(); throw }
}
function ConvertTo-IconPng([System.Drawing.Image]$image, [int]$size) {
    $bitmap = if ($size -le 64) { New-SmallIconImage $size }
        else { Resize-IconImage $image $size 1.0 }
    try {
        $stream = [System.IO.MemoryStream]::new()
        try {
            $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
            return ,$stream.ToArray()
        } finally { $stream.Dispose() }
    } finally { $bitmap.Dispose() }
}
function Resize-IconImage([System.Drawing.Image]$image, [int]$size, [double]$scale,
    [System.Drawing.Drawing2D.InterpolationMode]$filter = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic) {
    $bitmap = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppPArgb)
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
            $graphics.InterpolationMode = $filter
            $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $extent = [single]($size * $scale)
            $inset = [single](($size - $extent) / 2)
            $graphics.DrawImage($image, [System.Drawing.RectangleF]::new($inset, $inset, $extent, $extent),
                [System.Drawing.RectangleF]::new(0, 0, $image.Width, $image.Height), [System.Drawing.GraphicsUnit]::Pixel)
        } finally { $graphics.Dispose() }
        return ,$bitmap
    } catch { $bitmap.Dispose(); throw }
}

try {
    foreach ($bounds in $componentBounds) {
        $path = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName() + '.png')
        $componentFiles.Add($path)
        & magick $sourcePath -crop $bounds +repage $path
        if ($LASTEXITCODE -ne 0) { throw "Failed to crop icon component: $bounds" }
        $components.Add([System.Drawing.Image]::FromFile($path))
    }
    & magick $sourcePath -crop $artBounds +repage -background none -gravity center `
        -extent "${canvasSize}x${canvasSize}" $normalized
    if ($LASTEXITCODE -ne 0) { throw 'Failed to normalize application icon padding.' }
    $levels = [System.Collections.Generic.List[System.Drawing.Image]]::new()
    try {
        $levels.Add([System.Drawing.Image]::FromFile($normalized))
        while ($levels[$levels.Count - 1].Width -gt 72 * 2) {
            $image = $levels[$levels.Count - 1]
            $levels.Add((Resize-IconImage $image ([int][Math]::Ceiling($image.Width / 2)) 1.0))
        }
        foreach ($size in $sizes) {
            $level = 0
            while ($size -gt 64 -and $levels[$level].Width -gt $size * 2) { $level++ }
            $png = ConvertTo-IconPng $levels[$level] $size
            $frames.Add($png)
        }
        $uiFrame = $png # Keep the 256px PNG straight-alpha for application rendering.
    } finally {
        foreach ($image in $levels) { $image.Dispose() }
    }
} finally {
    foreach ($image in $components) { $image.Dispose() }
    foreach ($path in $componentFiles) {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path }
    }
    if (Test-Path -LiteralPath $normalized) { Remove-Item -LiteralPath $normalized }
}
# Store every size as straight-alpha PNG so Shell and window loaders decode
# transparency consistently, without DIB premultiplication ambiguity.
$iconStream = [System.IO.MemoryStream]::new()
try {
    $writer = [System.IO.BinaryWriter]::new($iconStream)
    try {
        $writer.Write([uint16]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]$sizes.Count)
        $offset = 6 + 16 * $sizes.Count
        for ($index = 0; $index -lt $sizes.Count; ++$index) {
            $dimension = if ($sizes[$index] -eq 256) { 0 } else { $sizes[$index] }
            $writer.Write([byte]$dimension)
            $writer.Write([byte]$dimension)
            $writer.Write([byte]0)
            $writer.Write([byte]0)
            $writer.Write([uint16]1)
            $writer.Write([uint16]32)
            $writer.Write([uint32]$frames[$index].Length)
            $writer.Write([uint32]$offset)
            $offset += $frames[$index].Length
        }
        foreach ($frame in $frames) { $writer.Write($frame) }
        $iconBytes = $iconStream.ToArray()
    } finally { $writer.Dispose() }
} finally { $iconStream.Dispose() }
function Write-ChangedIconAsset([string]$path, [byte[]]$bytes) {
    if ([IO.File]::Exists($path) -and
        [Convert]::ToBase64String([IO.File]::ReadAllBytes($path)) -ceq [Convert]::ToBase64String($bytes)) {
        return
    }
    # Replace a complete file, leaving the previous asset intact on write failure.
    $temporary = $path + '.' + [IO.Path]::GetRandomFileName()
    try {
        [IO.File]::WriteAllBytes($temporary, $bytes)
        if ([IO.File]::Exists($path)) { [IO.File]::Replace($temporary, $path, [NullString]::Value) }
        else { [IO.File]::Move($temporary, $path) }
    } finally {
        if ([IO.File]::Exists($temporary)) { Remove-Item -LiteralPath $temporary }
    }
}
Write-ChangedIconAsset $iconOutput $iconBytes
Write-ChangedIconAsset (Join-Path $projectRoot 'docs/images/app-icon.png') $uiFrame

$encoded = [Convert]::ToBase64String($uiFrame)
foreach ($name in @('overview.svg', 'overview.en.svg')) {
    $path = Join-Path $projectRoot "docs/images/$name"
    $svg = [IO.File]::ReadAllText($path)
    $svg = [regex]::Replace($svg, 'data:image/png;base64,[A-Za-z0-9+/=]+', "data:image/png;base64,$encoded")
    Write-ChangedIconAsset $path ([Text.UTF8Encoding]::new($false).GetBytes($svg))
}
