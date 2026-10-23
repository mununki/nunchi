# Draws Araso's icons at the sizes Windows asks for, packed as PNG entries into .ico files in
# crates/araso-tsf/res:
#   araso.ico     the brand icon: a white 아 on a blue rounded square (keyboard list, installer)
#   mode-on.ico   the mode icon in the taskbar's input indicator while Araso is on: 가 (the brand
#                 icon beside it, 아, says which input method it is)
#   mode-off.ico  the same while it's paused or off in the app (keys go in as typed): 가 with a
#                 small red x at its top right (not 가/A, which reads as Korean/English)
# Mode icons are a white glyph with a 1-pixel outline in black at 50%, as Windows asks of IME
# mode icons: they show on light and dark taskbars alike.
param([string]$Dir = (Join-Path $PSScriptRoot '..\crates\araso-tsf\res'))

Add-Type -AssemblyName System.Drawing

function New-Canvas([int]$size) {
    $bitmap = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bitmap)
    $g.SmoothingMode = 'AntiAlias'
    $g.TextRenderingHint = 'AntiAliasGridFit'
    $g.Clear([System.Drawing.Color]::Transparent)
    return $bitmap, $g
}

function Draw-Brand([int]$size) {
    $bitmap, $g = New-Canvas $size
    $r = [Math]::Max(2, [int]($size * 0.22))
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $w = $size - 1
    $path.AddArc(0, 0, $d, $d, 180, 90)
    $path.AddArc($w - $d, 0, $d, $d, 270, 90)
    $path.AddArc($w - $d, $w - $d, $d, $d, 0, 90)
    $path.AddArc(0, $w - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    $g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 47, 84, 235))), $path)
    $font = New-Object System.Drawing.Font 'Malgun Gothic', ($size * 0.58), ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
    $format = New-Object System.Drawing.StringFormat
    $format.Alignment = 'Center'
    $format.LineAlignment = 'Center'
    $rect = New-Object System.Drawing.RectangleF 0, ($size * 0.04), $size, $size
    $g.DrawString([string][char]0xC544, $font, [System.Drawing.Brushes]::White, $rect, $format)
    $g.Dispose()
    return $bitmap
}

function Draw-Mode([int]$size, [string]$glyph, [switch]$Off) {
    $bitmap, $g = New-Canvas $size
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $format = New-Object System.Drawing.StringFormat
    $format.Alignment = 'Center'
    $format.LineAlignment = 'Center'
    # Off, the glyph moves down and left a little, out of the red x's corner.
    $shift = if ($Off) { $size * 0.08 } else { 0 }
    $rect = New-Object System.Drawing.RectangleF (-$shift), ($size * 0.03 + $shift), $size, $size
    $family = New-Object System.Drawing.FontFamily 'Malgun Gothic'
    $path.AddString($glyph, $family, [int][System.Drawing.FontStyle]::Bold, [single]($size * 0.8), $rect, $format)
    # A pen 2 pixels wide on the glyph's edge: the fill covers its inner half.
    $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(128, 0, 0, 0)), 2
    $pen.LineJoin = 'Round'
    $g.DrawPath($pen, $path)
    $g.FillPath([System.Drawing.Brushes]::White, $path)
    if ($Off) {
        # A small red x at the top right, on a faint white halo so it shows on a light taskbar.
        $d = [Math]::Max(7, [Math]::Round($size * 0.5))
        $cx = $size - $d / 2 - 0.2
        $cy = $d / 2 + 0.2
        $arm = $d * 0.36
        $halo = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(160, 255, 255, 255)), ([Math]::Max(2.6, $size * 0.2))
        $red = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 232, 17, 35)), ([Math]::Max(1.4, $size * 0.11))
        foreach ($x in $halo, $red) {
            $x.StartCap = 'Round'
            $x.EndCap = 'Round'
            $g.DrawLine($x, [single]($cx - $arm), [single]($cy - $arm), [single]($cx + $arm), [single]($cy + $arm))
            $g.DrawLine($x, [single]($cx - $arm), [single]($cy + $arm), [single]($cx + $arm), [single]($cy - $arm))
        }
    }
    $g.Dispose()
    return $bitmap
}

function Write-Ico([string]$out, [int[]]$sizes, [scriptblock]$draw) {
    $pngs = foreach ($size in $sizes) {
        $bitmap = & $draw $size
        $stream = New-Object System.IO.MemoryStream
        $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Dispose()
        , $stream.ToArray()
    }
    $file = [System.IO.File]::Create($out)
    $writer = New-Object System.IO.BinaryWriter $file
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($i = 0; $i -lt $sizes.Count; $i++) {
        $s = $sizes[$i]
        $writer.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
        $writer.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
        $writer.Write([byte]0); $writer.Write([byte]0)
        $writer.Write([uint16]1); $writer.Write([uint16]32)
        $writer.Write([uint32]$pngs[$i].Length); $writer.Write([uint32]$offset)
        $offset += $pngs[$i].Length
    }
    foreach ($png in $pngs) { $writer.Write($png) }
    $writer.Close()
    Write-Output "wrote $out"
}

New-Item -ItemType Directory -Force $Dir | Out-Null
Write-Ico (Join-Path $Dir 'araso.ico') @(16, 20, 24, 32, 40, 48, 64, 256) { param($s) Draw-Brand $s }
Write-Ico (Join-Path $Dir 'mode-on.ico') @(16, 20, 24, 32, 40, 48) { param($s) Draw-Mode $s ([string][char]0xAC00) }
Write-Ico (Join-Path $Dir 'mode-off.ico') @(16, 20, 24, 32, 40, 48) { param($s) Draw-Mode $s ([string][char]0xAC00) -Off }
