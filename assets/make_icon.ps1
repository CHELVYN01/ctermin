# Regenerates assets/icon.ico and assets/icon-64.rgba. Run it from the repository root:
#   powershell -NoProfile -ExecutionPolicy Bypass -File assets\make_icon.ps1
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

function New-Icon([int]$size) {
    $bitmap = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.Clear([System.Drawing.Color]::Transparent)

    # Rounded dark tile.
    $radius = [float]($size * 0.2)
    $diameter = $radius * 2
    $edge = [float]($size - 1)
    $tile = New-Object System.Drawing.Drawing2D.GraphicsPath
    $tile.AddArc(0, 0, $diameter, $diameter, 180, 90)
    $tile.AddArc($edge - $diameter, 0, $diameter, $diameter, 270, 90)
    $tile.AddArc($edge - $diameter, $edge - $diameter, $diameter, $diameter, 0, 90)
    $tile.AddArc(0, $edge - $diameter, $diameter, $diameter, 90, 90)
    $tile.CloseFigure()
    $background = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 18, 20, 24))
    $graphics.FillPath($background, $tile)

    # The prompt chevron.
    $accent = [System.Drawing.Color]::FromArgb(255, 58, 150, 221)
    $pen = New-Object System.Drawing.Pen $accent, ([float]($size * 0.1))
    $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
    $points = [System.Drawing.PointF[]]@(
        (New-Object System.Drawing.PointF ([float]($size * 0.24)), ([float]($size * 0.3))),
        (New-Object System.Drawing.PointF ([float]($size * 0.48)), ([float]($size * 0.5))),
        (New-Object System.Drawing.PointF ([float]($size * 0.24)), ([float]($size * 0.7)))
    )
    $graphics.DrawLines($pen, $points)

    # The cursor bar.
    $cursor = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 230, 230, 230)), ([float]($size * 0.1))
    $cursor.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $cursor.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $graphics.DrawLine($cursor, [float]($size * 0.54), [float]($size * 0.72), [float]($size * 0.78), [float]($size * 0.72))

    $graphics.Dispose()
    return $bitmap
}

function Get-PngBytes($bitmap) {
    $stream = New-Object System.IO.MemoryStream
    $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    return , $stream.ToArray()
}

# ICO file: a header, one directory entry per size, then PNG data for each.
$sizes = 256, 64, 48, 32, 16
$images = foreach ($size in $sizes) { , (Get-PngBytes (New-Icon $size)) }
$output = New-Object System.IO.MemoryStream
$writer = New-Object System.IO.BinaryWriter $output
$writer.Write([uint16]0)
$writer.Write([uint16]1)
$writer.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($index = 0; $index -lt $sizes.Count; $index++) {
    $size = $sizes[$index]
    $dimension = if ($size -eq 256) { 0 } else { $size }
    $writer.Write([byte]$dimension)
    $writer.Write([byte]$dimension)
    $writer.Write([byte]0)
    $writer.Write([byte]0)
    $writer.Write([uint16]1)
    $writer.Write([uint16]32)
    $writer.Write([uint32]$images[$index].Length)
    $writer.Write([uint32]$offset)
    $offset += $images[$index].Length
}
foreach ($image in $images) { $writer.Write([byte[]]$image) }
[System.IO.File]::WriteAllBytes((Join-Path $PSScriptRoot 'icon.ico'), $output.ToArray())

# Raw RGBA pixels for the window icon, so the app needs no image decoder.
$bitmap = New-Icon 64
$raw = New-Object byte[] (64 * 64 * 4)
$position = 0
for ($y = 0; $y -lt 64; $y++) {
    for ($x = 0; $x -lt 64; $x++) {
        $pixel = $bitmap.GetPixel($x, $y)
        $raw[$position] = $pixel.R
        $raw[$position + 1] = $pixel.G
        $raw[$position + 2] = $pixel.B
        $raw[$position + 3] = $pixel.A
        $position += 4
    }
}
[System.IO.File]::WriteAllBytes((Join-Path $PSScriptRoot 'icon-64.rgba'), $raw)
