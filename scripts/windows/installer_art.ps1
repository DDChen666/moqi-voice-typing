# Draws the Windows installer's side and header images from the app icon:
#   src-tauri\nsis\sidebar.bmp  164x314  welcome and finish pages
#   src-tauri\nsis\header.bmp   150x57   the other pages
# NSIS needs 24-bit BMPs, so this uses System.Drawing rather than render.html.
# Run from the repository root: .\scripts\windows\installer_art.ps1

Add-Type -AssemblyName System.Drawing
$root = Join-Path $PSScriptRoot '..\..'
$icon = [System.Drawing.Image]::FromFile((Resolve-Path (Join-Path $root 'yuyin\brand\app-icon-1024.png')))
$background = [System.Drawing.Color]::FromArgb(245, 245, 247)   # --light-color-background
$ink = [System.Drawing.Color]::FromArgb(29, 29, 31)             # --light-color-text

function New-Art([int]$Width, [int]$Height, [scriptblock]$Draw, [string]$Path) {
    $bmp = New-Object System.Drawing.Bitmap $Width, $Height, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'HighQuality'
    $g.InterpolationMode = 'HighQualityBicubic'
    $g.TextRenderingHint = 'AntiAliasGridFit'
    $g.Clear($background)
    & $Draw $g
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Bmp)
    $g.Dispose(); $bmp.Dispose()
    Write-Host "wrote $Path"
}

New-Art 164 314 {
    param($g)
    $g.DrawImage($icon, 34, 70, 96, 96)
    $font = New-Object System.Drawing.Font 'Microsoft JhengHei UI', 16, ([System.Drawing.FontStyle]::Bold)
    $format = New-Object System.Drawing.StringFormat
    $format.Alignment = 'Center'
    $g.DrawString('默契', $font, (New-Object System.Drawing.SolidBrush $ink), (New-Object System.Drawing.RectangleF 0, 180, 164, 34), $format)
    $small = New-Object System.Drawing.Font 'Segoe UI', 10
    $g.DrawString('Moqi', $small, (New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(110, 110, 115))), (New-Object System.Drawing.RectangleF 0, 214, 164, 24), $format)
} (Join-Path $root 'src-tauri\nsis\sidebar.bmp')

New-Art 150 57 {
    param($g)
    $g.DrawImage($icon, 98, 6, 44, 44)
} (Join-Path $root 'src-tauri\nsis\header.bmp')

$icon.Dispose()
