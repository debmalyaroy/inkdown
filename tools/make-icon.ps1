# Draws the Inkdown icon and writes assets\icon.ico (+ a 256px PNG for the UI).
Add-Type -AssemblyName System.Drawing
$root = Split-Path $PSScriptRoot -Parent

function New-IconBitmap([int]$s) {
    $bmp = New-Object System.Drawing.Bitmap $s, $s, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.PixelOffsetMode = 'HighQuality'
    $g.Clear([System.Drawing.Color]::Transparent)

    $m = [float]($s * 0.04)
    $w = [float]($s - 2 * $m)
    $r = [float]($s * 0.23)
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = 2 * $r
    $path.AddArc($m, $m, $d, $d, 180, 90)
    $path.AddArc($m + $w - $d, $m, $d, $d, 270, 90)
    $path.AddArc($m + $w - $d, $m + $w - $d, $d, $d, 0, 90)
    $path.AddArc($m, $m + $w - $d, $d, $d, 90, 90)
    $path.CloseFigure()

    $rect = New-Object System.Drawing.RectangleF 0, 0, $s, $s
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush $rect, `
        ([System.Drawing.Color]::FromArgb(255, 99, 102, 241)), `
        ([System.Drawing.Color]::FromArgb(255, 168, 85, 247)), 50.0
    $g.FillPath($brush, $path)

    # "M" + down arrow, the Markdown mark, in white
    $pw = [float]($s * 0.085)
    $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::White), $pw
    $pen.LineJoin = 'Round'; $pen.StartCap = 'Round'; $pen.EndCap = 'Round'
    $u = { param($x, $y) New-Object System.Drawing.PointF ([float]($s * $x)), ([float]($s * $y)) }
    $g.DrawLines($pen, [System.Drawing.PointF[]]@(
        (& $u 0.20 0.68), (& $u 0.20 0.33), (& $u 0.355 0.52), (& $u 0.51 0.33), (& $u 0.51 0.68)))
    $g.DrawLine($pen, (& $u 0.72 0.33), (& $u 0.72 0.66))
    $g.DrawLines($pen, [System.Drawing.PointF[]]@((& $u 0.60 0.54), (& $u 0.72 0.68), (& $u 0.84 0.54)))
    $g.Dispose()
    return $bmp
}

$sizes = 16, 20, 24, 32, 40, 48, 64, 128, 256
$images = foreach ($s in $sizes) {
    $bmp = New-IconBitmap $s
    $ms = New-Object System.IO.MemoryStream
    if ($s -ge 256) {
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Save("$root\assets\icon.png", [System.Drawing.Imaging.ImageFormat]::Png)
    } else {
        # 32bpp DIB: BITMAPINFOHEADER, bottom-up BGRA, then an empty AND mask
        $bw = New-Object System.IO.BinaryWriter $ms
        $bw.Write([int]40); $bw.Write([int]$s); $bw.Write([int]($s * 2))
        $bw.Write([int16]1); $bw.Write([int16]32); $bw.Write([int]0)
        $bw.Write([int]0); $bw.Write([int]0); $bw.Write([int]0); $bw.Write([int]0); $bw.Write([int]0)
        for ($y = $s - 1; $y -ge 0; $y--) {
            for ($x = 0; $x -lt $s; $x++) {
                $c = $bmp.GetPixel($x, $y)
                $bw.Write([byte]$c.B); $bw.Write([byte]$c.G); $bw.Write([byte]$c.R); $bw.Write([byte]$c.A)
            }
        }
        $rowBytes = [int]([Math]::Ceiling($s / 32.0) * 4)
        $bw.Write((New-Object byte[] ($rowBytes * $s)))
        $bw.Flush()
    }
    $bmp.Dispose()
    , @($s, $ms.ToArray())
}

$out = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $out
$w.Write([int16]0); $w.Write([int16]1); $w.Write([int16]$images.Count)
$offset = 6 + 16 * $images.Count
foreach ($img in $images) {
    $s = $img[0]; $data = $img[1]
    $w.Write([byte]($s % 256)); $w.Write([byte]($s % 256)); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([int16]1); $w.Write([int16]32); $w.Write([int]$data.Length); $w.Write([int]$offset)
    $offset += $data.Length
}
foreach ($img in $images) { $w.Write([byte[]]$img[1]) }
$w.Flush()
[System.IO.File]::WriteAllBytes("$root\assets\icon.ico", $out.ToArray())
"icon.ico written: $($out.Length) bytes, $($images.Count) sizes"
