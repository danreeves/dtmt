param(
    [int]$TimeoutSec = 180,
    [string]$OutDir = "$env:TEMP\dt_shader",
    [string]$Launch = (Join-Path $PSScriptRoot "launch.bat")
)

$ErrorActionPreference = "Stop"
$game = "E:\SteamLibrary\steamapps\common\Warhammer 40,000 DARKTIDE"
$logDir = Join-Path $env:APPDATA "Fatshark\Darktide\console_logs"

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Fg {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
}
"@

function FocusGame {
    $p = Get-Process Darktide -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if ($p) {
        if ([Fg]::IsIconic($p.MainWindowHandle)) { [void][Fg]::ShowWindow($p.MainWindowHandle, 9) }
        [void][Fg]::SetForegroundWindow($p.MainWindowHandle)
        return $true
    }
    return $false
}

function Shot([string]$path) {
    [void](FocusGame)
    Start-Sleep -Milliseconds 500
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    return $path
}

function AvgColor([string]$path, [double]$x, [double]$y, [double]$w, [double]$h) {
    $img = [System.Drawing.Image]::FromFile($path)
    $r = 0.0; $g = 0.0; $b = 0.0; $n = 0
    for ($py = [int]($y * $img.Height); $py -lt [int](($y + $h) * $img.Height); $py += 8) {
        for ($px = [int]($x * $img.Width); $px -lt [int](($x + $w) * $img.Width); $px += 8) {
            $c = $img.GetPixel($px, $py)
            $r += $c.R; $g += $c.G; $b += $c.B; $n++
        }
    }
    $img.Dispose()
    return ("{0},{1},{2}" -f [int]($r / $n), [int]($g / $n), [int]($b / $n))
}

if (Get-Process Darktide -ErrorAction SilentlyContinue) {
    Get-Process Darktide | Stop-Process -Force
    Start-Sleep -Seconds 3
}

$before = (Get-ChildItem $logDir -Filter "*.log" | Sort-Object LastWriteTime -Descending | Select-Object -First 1).Name

Write-Host "Launching..."
Start-Process -FilePath $Launch -WorkingDirectory $game | Out-Null

# Advance the splash pages until the mod reports the title material applied.
$deadline = (Get-Date).AddSeconds($TimeoutSec)
$titleAt = $null
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Seconds 3
    if (FocusGame) { [System.Windows.Forms.SendKeys]::SendWait(" ") }
    $latest = Get-ChildItem $logDir -Filter "*.log" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($latest -and $latest.Name -ne $before) {
        $content = Get-Content $latest.FullName -Raw
        if ($content -match 'material set: background_image') {
            $titleAt = Get-Date
            break
        }
    }
}

if (-not $titleAt) {
    Write-Host "title material was not applied within the timeout"
    exit 1
}

Write-Host "title material applied at $($titleAt.ToString('HH:mm:ss'))"
Start-Sleep -Seconds 5

$a = Shot (Join-Path $OutDir "lua_tint_a.png")
$ca = AvgColor $a 0.02 0.02 0.28 0.18
Start-Sleep -Seconds 4
$b = Shot (Join-Path $OutDir "lua_tint_b.png")
$cb = AvgColor $b 0.02 0.02 0.28 0.18
Start-Sleep -Seconds 4
$c = Shot (Join-Path $OutDir "lua_tint_c.png")
$cc = AvgColor $c 0.02 0.02 0.28 0.18

Write-Host "sample A: $ca"
Write-Host "sample B: $cb"
Write-Host "sample C: $cc"
Write-Host "screenshots: $a, $b, $c"

$log = (Get-ChildItem $logDir -Filter "*.log" | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
Select-String -Path $log -Pattern 'snoopymod' | ForEach-Object { Write-Host $_.Line }
