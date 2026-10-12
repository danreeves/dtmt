<#
.SYNOPSIS
  Runs snoopy-mod's automated in-game test: `init.lua` continues the title
  screen, selects the profile, enters the hub, spawns the pumpkin (the
  `standard_base` material's unit), scans for it, and quits itself. This script
  waits for that self-quit, captures screenshots along the way, and reads the
  flushed console log for the pumpkin confirmation.

.DESCRIPTION
  The title-screen harness (`shader-render-test.ps1`) samples ~10 s in and kills
  the game, so it never reaches the hub or the pumpkin. The mod's automation
  quits on its own after the spawn, which flushes the log, so this waits for the
  process to exit instead of killing it.

  Verdicts:
    PUMPKIN_OK    the log shows the pumpkin spawned, a world scan found it, and
                  no render-time crash followed.
    RENDER_FAIL   a render-time crash (`dispatch_loadtime`,
                  `ShaderTemplate::initialize`, an access violation,
                  `E_INVALIDARG`).
    NO_PUMPKIN    the game quit without logging a pumpkin spawn/scan.
    NO_LOAD       no `[snoopymod]` line: the mod never loaded.
    INDETERMINATE anything else, with the crash context.

.PARAMETER Label
  A name for the run, echoed in the verdict.

.PARAMETER Timeout
  Total seconds to wait for the game to quit itself before killing it.

.PARAMETER GameDir
  The Darktide install (the folder holding `bundle/`).
#>
param(
    [string]$Label = "pumpkin",
    [int]$Timeout = 240,
    [string]$GameDir = "E:\SteamLibrary\steamapps\common\Warhammer 40,000 DARKTIDE",
    [string]$LaunchBat = "C:\dev\dtmt\docs\scripts\launch.bat",
    [string]$OutDir = "$env:TEMP\dt_shader"
)

$ErrorActionPreference = "Stop"
$logDir = Join-Path $env:APPDATA "Fatshark\Darktide\console_logs"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

function Newest-Log {
    Get-ChildItem (Join-Path $logDir "*.log") -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
}

# Sample the game window by HWND with `PrintWindow`, the method that works for
# the borderless-fullscreen window (`CopyFromScreen` can capture the desktop).
function Get-WindowShot([string]$path) {
    try {
        Add-Type -AssemblyName System.Drawing -ErrorAction Stop
        if (-not ("PumpkinShot" -as [type])) {
            Add-Type @"
using System;
using System.Runtime.InteropServices;
public class PumpkinShot {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
}
"@
        }
        $p = Get-Process Darktide -ErrorAction SilentlyContinue |
            Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
        if (-not $p) { return $null }

        $r = New-Object PumpkinShot+RECT
        [void][PumpkinShot]::GetWindowRect($p.MainWindowHandle, [ref]$r)
        $w = $r.Right - $r.Left
        $h = $r.Bottom - $r.Top
        if ($w -le 0 -or $h -le 0) { return $null }

        $bmp = New-Object System.Drawing.Bitmap($w, $h)
        $gr = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $gr.GetHdc()
        [void][PumpkinShot]::PrintWindow($p.MainWindowHandle, $hdc, 2)
        $gr.ReleaseHdc($hdc)

        $sum = 0.0; $n = 0; $max = 0; $black = 0
        for ($x = 0; $x -lt $w; $x += 16) {
            for ($y = 0; $y -lt $h; $y += 16) {
                $px = $bmp.GetPixel($x, $y)
                $lum = ($px.R + $px.G + $px.B) / 3.0
                $sum += $lum; $n++
                if ($lum -gt $max) { $max = $lum }
                if ($lum -lt 3) { $black++ }
            }
        }
        $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
        $gr.Dispose(); $bmp.Dispose()
        return [pscustomobject]@{
            Mean = if ($n) { $sum / $n } else { 0 }
            Max = $max
            BlackFrac = if ($n) { $black / $n } else { 0 }
            Path = $path
        }
    } catch {
        return $null
    }
}

# The section the deployed mod carries, by its material-hash bundle name.
function Get-Environment {
    $out = @()
    $section = Get-ChildItem (Join-Path $GameDir "bundle\data") -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -eq "4cc21b79457af061" } | Select-Object -First 1
    if ($section) { $out += "section=$((Get-FileHash $section.FullName).Hash.Substring(0,16)) ($($section.Length)B)" }
    $dep = Join-Path $GameDir "dtmm-deployment.sjson"
    if (Test-Path $dep) {
        $bundles = (Get-Content $dep | Select-String -Pattern "^\s*[0-9a-f]{16}\s*$").Count
        $out += "deploy_bundles=$bundles"
    }
    return ($out -join "  ")
}

"ENV  [$Label]: $(Get-Environment)"

Get-Process Darktide -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2

$before = Newest-Log
$start = Get-Date
Start-Process -FilePath $LaunchBat -WorkingDirectory $GameDir | Out-Null

# Capture along the way (the hub is reached tens of seconds in), and stop when
# the mod quits itself. The log cannot be polled: it flushes on close.
$shots = @()
$deadline = $start.AddSeconds($Timeout)
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Seconds 3
    if (-not (Get-Process Darktide -ErrorAction SilentlyContinue)) { break }
    if (((Get-Date) - $start).TotalSeconds -ge 30) {
        $path = Join-Path $OutDir ("pumpkin_{0:hhmmss}.png" -f (Get-Date))
        $shot = Get-WindowShot $path
        if ($shot) { $shots += $shot }
    }
}

# If it did not quit itself, kill it to flush the log.
if (Get-Process Darktide -ErrorAction SilentlyContinue) {
    Get-Process Darktide | Stop-Process -Force
}
Start-Sleep -Seconds 3

$log = Newest-Log
$lines = @(Get-Content $log.FullName)
$snoopy = @($lines | Select-String "\[snoopymod\]")
$crash = $lines | Select-String "<<Crash>>" | Select-Object -First 1
$spawned = $lines | Select-String "auto-spawned the pumpkin" | Select-Object -First 1
$scan = $lines | Select-String "world scan: \d+ pumpkin unit\(s\)" | Select-Object -First 1
$located = $lines | Select-String "pumpkin \d+ at " | Select-Object -First 1
$quitting = $lines | Select-String "quitting for the test" | Select-Object -First 1

$crashKind = $null
if ($crash) {
    $from = [Math]::Max(0, $crash.LineNumber - 10)
    $context = ($lines[$from..($crash.LineNumber - 1)] -join "`n")
    if ($context -match "ShaderTemplate::initialize" -or $context -match "dispatch_loadtime") {
        $crashKind = "RENDER_FAIL"
    } elseif ($crash.Line -match "Access violation" -or $crash.Line -match "E_INVALIDARG") {
        $crashKind = "RENDER_FAIL"
    } elseif ($crash.Line -match "Trying to unload resource" -or $crash.Line -match "refcount") {
        $crashKind = $null
    } else {
        $crashKind = "INDETERMINATE"
    }
}

$best = $shots | Sort-Object BlackFrac | Select-Object -First 1
$detail = "shots=$($shots.Count)"
if ($spawned) { $detail += "; spawned" }
if ($scan) { $detail += "; $($scan.Line.Trim())" }
if ($located) { $detail += "; $($located.Line.Trim())" }
if ($quitting) { $detail += "; $($quitting.Line.Trim())" }
if ($best) { $detail += "; shot=$($best.Path) mean=$([Math]::Round($best.Mean,2)) black=$([Math]::Round($best.BlackFrac * 100,1))%" }
if ($crash) {
    $detail += "; crash: $($crash.Line.Trim().Substring(0,[Math]::Min(70, $crash.Line.Trim().Length)))"
}

if ($crashKind) {
    $verdict = $crashKind
} elseif ($snoopy.Count -eq 0) {
    $verdict = "NO_LOAD"
} elseif ($spawned -and $scan -and ($scan.Line -match "world scan: (\d+) pumpkin") -and [int]$Matches[1] -gt 0) {
    $verdict = "PUMPKIN_OK"
} else {
    $verdict = "NO_PUMPKIN"
}

"VERDICT [$Label]: $verdict -- $detail"
"       log: $($log.Name)"
$snoopy | Select-Object -Last 25 | ForEach-Object { "       $($_.Line.Trim())" }
if ($verdict -ne "PUMPKIN_OK") { exit 1 }
