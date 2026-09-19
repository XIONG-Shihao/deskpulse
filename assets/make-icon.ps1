Add-Type -AssemblyName System.Drawing
$dest = Join-Path $PSScriptRoot 'icon.ico'
$outDir = Split-Path -Parent $dest
if (-not (Test-Path -LiteralPath $outDir)) { New-Item -ItemType Directory -Path $outDir | Out-Null }

$sizes = @(16, 32, 48, 256)
$images = @()
foreach ($s in $sizes) {
    $d = [double]$s
    $inset = 0.5
    if ($s -ge 32) { $inset = $d / 16.0 }
    $stroke = $d / 16.0
    if ($stroke -lt 1.0) { $stroke = 1.0 }
    $side = $d - (2.0 * $inset)

    $bmp = [System.Drawing.Bitmap]::new($s, $s)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)

    $fill = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 40, 200, 210))
    $edge = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(255, 16, 120, 130), [single]$stroke)
    $rect = [System.Drawing.RectangleF]::new([single]$inset, [single]$inset, [single]$side, [single]$side)

    $g.FillEllipse($fill, $rect)
    $g.DrawEllipse($edge, $rect)

    $g.Dispose()
    $fill.Dispose()
    $edge.Dispose()

    $ms = [System.IO.MemoryStream]::new()
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $images += , $ms.ToArray()
    $bmp.Dispose()
    $ms.Dispose()
}

$fs = [System.IO.File]::Create($dest)
$bw = [System.IO.BinaryWriter]::new($fs)
$bw.Write([UInt16]0)
$bw.Write([UInt16]1)
$bw.Write([UInt16]$sizes.Count)

$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]
    $len = $images[$i].Length
    $dim = if ($s -ge 256) { 0 } else { $s }
    $bw.Write([Byte]$dim)
    $bw.Write([Byte]$dim)
    $bw.Write([Byte]0)
    $bw.Write([Byte]0)
    $bw.Write([UInt16]1)
    $bw.Write([UInt16]32)
    $bw.Write([UInt32]$len)
    $bw.Write([UInt32]$offset)
    $offset += $len
}
foreach ($img in $images) { $bw.Write($img) }
$bw.Close()
$fs.Close()

Write-Output "wrote $dest ($((Get-Item -LiteralPath $dest).Length) bytes)"
