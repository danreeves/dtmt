<#
.SYNOPSIS
  Samples the whole primary screen every `Interval` seconds for `Duration`
  seconds, saves each frame, then reports the frames with the most lit pixels
  first.

.DESCRIPTION
  `capture-title.ps1` and `pumpkin-render-test.ps1` grab the game window by
  HWND with `PrintWindow`. That returns no handle while the game hides its
  window during boot - which is exactly when the loading screen and its
  `boot_show_text` quote are on screen, so a font or text change cannot be
  photographed by them. This samples the desktop instead, so a surface that is
  drawn only briefly is still found after the fact.

.PARAMETER Duration
  How many seconds to keep sampling.

.PARAMETER Interval
  Seconds between frames.

.PARAMETER OutDir
  Where the PNGs go (defaults to the same folder the other harnesses use).

.PARAMETER Prefix
  Frame name prefix; frames are `<prefix>_<second>.png`.

.PARAMETER KeepAll
  Keep every frame. Without it only the lit frames are kept, so the boot's
  black padding does not fill the disk - the disk filled up once already.
#>
param(
    [int]$Duration = 45,
    [double]$Interval = 1.5,
    [string]$OutDir = "$env:TEMP\dt_shader",
    [string]$Prefix = "screen",
    [switch]$KeepAll,
    [string]$GameDir = "E:\SteamLibrary\steamapps\common\Warhammer 40,000 DARKTIDE",
    [string]$Launch = "C:\dev\dtmt\docs\scripts\launch.bat"
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$w = $bounds.Width
$h = $bounds.Height

Get-Process Darktide -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 3
Start-Process -FilePath $Launch -WorkingDirectory $GameDir | Out-Null

$shots = @()
$dropped = 0
$start = Get-Date

while (((Get-Date) - $start).TotalSeconds -lt $Duration) {
    Start-Sleep -Milliseconds ([int]($Interval * 1000))

    $sec = [int]((Get-Date) - $start).TotalSeconds
    $path = Join-Path $OutDir ("{0}_{1:d3}.png" -f $Prefix, $sec)

    # Sampling the desktop, so no window handle is needed. A hidden window
    # shows the desktop, which reads as a lit frame; that is reported too.
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $gr = [System.Drawing.Graphics]::FromImage($bmp)
    $gr.CopyFromScreen($bounds.X, $bounds.Y, 0, 0, $bmp.Size)
    $gr.Dispose()

    $sum = 0.0; $n = 0; $lit = 0
    for ($x = 0; $x -lt $w; $x += 16) {
        for ($y = 0; $y -lt $h; $y += 16) {
            $px = $bmp.GetPixel($x, $y)
            $lum = ($px.R + $px.G + $px.B) / 3.0
            $sum += $lum; $n++
            if ($lum -gt 20) { $lit++ }
        }
    }

    # A near-black frame is the loading screen before anything is drawn; those
    # are the ones that filled the disk last time, so they are not kept.
    if ($lit -gt 2 -or $KeepAll) {
        $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
        $shots += [pscustomobject]@{
            Name = (Split-Path $path -Leaf)
            Mean = [Math]::Round($sum / $n, 2)
            Lit  = $lit
        }
    } else {
        $dropped++
    }
    $bmp.Dispose()
}

Get-Process Darktide -ErrorAction SilentlyContinue | Stop-Process -Force
"kept $($shots.Count) frame(s), dropped $dropped near-black frame(s)"
$shots | Sort-Object Lit -Descending | Format-Table -AutoSize

$log = Get-ChildItem "$env:APPDATA\Fatshark\Darktide\console_logs" -Filter *.log |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
Select-String -Path $log.FullName -Pattern 'snoopymod|StateTitle' | Select-Object -First 8 |
    ForEach-Object { $_.Line.Trim() }
