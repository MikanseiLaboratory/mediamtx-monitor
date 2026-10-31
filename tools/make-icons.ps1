param([string]$OutDir)
Add-Type -AssemblyName System.Drawing
function New-Icon([int]$size, [string]$path) {
  $bmp = New-Object System.Drawing.Bitmap($size, $size)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.Clear([System.Drawing.Color]::Transparent)
  $bg = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 17, 22, 29))
  $g.FillEllipse($bg, 0, 0, $size - 1, $size - 1)
  $accent = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 76, 201, 240), [float][Math]::Max(1.0, $size * 0.09))
  $accent.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
  $accent.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
  $y = [float]($size * 0.55)
  $pts = [System.Drawing.PointF[]]@(
    (New-Object System.Drawing.PointF([float]($size * 0.16), $y)),
    (New-Object System.Drawing.PointF([float]($size * 0.33), $y)),
    (New-Object System.Drawing.PointF([float]($size * 0.42), [float]($size * 0.30))),
    (New-Object System.Drawing.PointF([float]($size * 0.56), [float]($size * 0.74))),
    (New-Object System.Drawing.PointF([float]($size * 0.66), $y)),
    (New-Object System.Drawing.PointF([float]($size * 0.84), $y))
  )
  $g.DrawLines($accent, $pts)
  $dot = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 247, 37, 133))
  $g.FillEllipse($dot, [float]($size * 0.70), [float]($size * 0.18), [float]($size * 0.16), [float]($size * 0.16))
  $g.Dispose()
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
}
New-Icon 32 "$OutDir\32x32.png"
New-Icon 128 "$OutDir\128x128.png"
New-Icon 256 "$OutDir\128x128@2x.png"
New-Icon 512 "$OutDir\icon.png"
Write-Output "icons generated"