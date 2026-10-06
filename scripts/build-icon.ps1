# Rebuild the Reminders for Windows cloud/check artwork and Windows icon sizes.
# Run on Windows; System.Drawing supplies the vector drawing and PNG encoding.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$repository = Split-Path -Parent $PSScriptRoot
$appAssets = Join-Path $repository 'src/Reminders.WinUI/Assets'
$packageAssets = Join-Path $repository 'src/Reminders.Package/Assets'

function New-CloudReminderIcon([int]$size) {
    $canvas = [Drawing.Bitmap]::new(1024, 1024)
    $graphics = [Drawing.Graphics]::FromImage($canvas)
    $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.Clear([Drawing.Color]::Transparent)
    $graphics.ScaleTransform(2, 2)
    $tile = [Drawing.Drawing2D.GraphicsPath]::new()
    foreach ($corner in @(@(24,24,180), @(336,24,270), @(336,336,0), @(24,336,90))) {
        $tile.AddArc($corner[0], $corner[1], 152, 152, $corner[2], 90)
    }
    $tile.CloseFigure()
    $paper = [Drawing.Drawing2D.LinearGradientBrush]::new(
        [Drawing.Point]::new(0,24), [Drawing.Point]::new(0,488),
        [Drawing.ColorTranslator]::FromHtml('#173C58'), [Drawing.ColorTranslator]::FromHtml('#087F8C'))
    $edge = [Drawing.Pen]::new([Drawing.ColorTranslator]::FromHtml('#156274'), 2)
    $graphics.FillPath($paper, $tile)
    $graphics.DrawPath($edge, $tile)
    # One continuous cloud silhouette stays legible at taskbar sizes.
    $cloud = [Drawing.Drawing2D.GraphicsPath]::new()
    $cloud.StartFigure()
    $cloud.AddBezier(153, 338, 80, 338, 71, 236, 133, 213)
    $cloud.AddBezier(133, 213, 129, 151, 187, 117, 236, 152)
    $cloud.AddBezier(236, 152, 281, 99, 369, 132, 369, 205)
    $cloud.AddBezier(369, 205, 443, 209, 455, 322, 386, 336)
    $cloud.AddBezier(386, 336, 360, 342, 191, 338, 153, 338)
    $cloud.CloseFigure()
    $white = [Drawing.SolidBrush]::new([Drawing.ColorTranslator]::FromHtml('#F9FCFD'))
    $graphics.FillPath($white, $cloud)
    $check = [Drawing.Pen]::new([Drawing.ColorTranslator]::FromHtml('#126777'), 27)
    $check.StartCap = $check.EndCap = [Drawing.Drawing2D.LineCap]::Round
    $check.LineJoin = [Drawing.Drawing2D.LineJoin]::Round
    $checkPoints = [Drawing.PointF[]]@(
        [Drawing.PointF]::new(205, 263), [Drawing.PointF]::new(242, 298), [Drawing.PointF]::new(313, 225))
    $graphics.DrawLines($check, $checkPoints)
    $cloud.Dispose(); $white.Dispose(); $check.Dispose()
    $result = [Drawing.Bitmap]::new($size, $size)
    $output = [Drawing.Graphics]::FromImage($result)
    $output.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $output.DrawImage($canvas, 0, 0, $size, $size)
    $output.Dispose(); $graphics.Dispose(); $canvas.Dispose()
    $tile.Dispose(); $paper.Dispose(); $edge.Dispose()
    return $result
}

$preview = New-CloudReminderIcon 512
$preview.Save((Join-Path $appAssets 'icon-v2.png'), [Drawing.Imaging.ImageFormat]::Png)
$preview.Dispose()
foreach ($asset in @(@('StoreLogo.png',50), @('Square44x44Logo.png',44), @('Square150x150Logo.png',150))) {
    $bitmap = New-CloudReminderIcon $asset[1]
    $bitmap.Save((Join-Path $packageAssets $asset[0]), [Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
}
# PNG-backed ICO frames preserve alpha and provide crisp small/large shell icons.
$sizes = @(16,20,24,32,40,48,64,96,128,256)
$frames = foreach ($size in $sizes) {
    $bitmap = New-CloudReminderIcon $size
    $buffer = [IO.MemoryStream]::new()
    $bitmap.Save($buffer, [Drawing.Imaging.ImageFormat]::Png)
    ,$buffer.ToArray()
    $buffer.Dispose(); $bitmap.Dispose()
}
$stream = [IO.File]::Create((Join-Path $appAssets 'icon-v2.ico'))
$writer = [IO.BinaryWriter]::new($stream)
try {
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($index = 0; $index -lt $sizes.Count; $index++) {
        $dimension = if ($sizes[$index] -eq 256) { 0 } else { $sizes[$index] }
        $writer.Write([byte]$dimension); $writer.Write([byte]$dimension)
        $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]32)
        $writer.Write([uint32]$frames[$index].Length); $writer.Write([uint32]$offset)
        $offset += $frames[$index].Length
    }
    foreach ($frame in $frames) { $writer.Write([byte[]]$frame) }
} finally { $writer.Dispose(); $stream.Dispose() }
