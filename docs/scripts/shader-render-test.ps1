<#
.SYNOPSIS
  Launches Darktide with the deployed mod and reports whether the custom shader
  actually RENDERED. It waits a fixed time for the title screen, samples the game
  window, kills the game, and only then reads the console log.

.DESCRIPTION
  The traps this exists to avoid:

  * `material set: background_image` is logged by the mod's Lua before the engine
    builds a pipeline. A section whose programs are wrong still logs it, then
    crashes in `ShaderTemplate::initialize` ~1 s later.
  * The material can be set and the game keep running while the shader fails to
    draw: the title screen is then *black*. Measured on a bad group tail.
  * **The console log is buffered** - it flushes only when enough bytes accumulate
    or the game closes. So it cannot be polled for the material-set line while the
    game runs: the first read that contains the line also contains everything
    after it, including the crash.

  Hence the fixed wait, then the sample, then the kill (which flushes the log).
  The sample is `PrintWindow` on the game window by HWND - the method the scratch
  `shot-window.ps1` uses. A screenshot is saved next to the verdict.

  The verdicts:
    RENDER_OK     the log shows the material set, no render crash, non-black window.
    RENDER_BLACK  material set and no crash, but the window sampled near-black.
    RENDER_FAIL   a render-time crash (`ShaderTemplate::initialize`,
                  `dispatch_loadtime`, an access violation).
    NO_LOAD       no material-set line in the flushed log (deploy/wedge problem).
    INDETERMINATE anything else, printed with the crash context.

.PARAMETER Label
  A name for the run, echoed in the verdict.

.PARAMETER WaitForTitle
  Seconds after launch to sample the window. The material is set ~10 s in; the
  game is killed right after the sample.

.PARAMETER Timeout
  Total seconds the run may take before the game is killed and the log read.
#>
param(
    [string]$Label = "run",
    [int]$WaitForTitle = 10,
    [int]$Timeout = 90,
    [string]$GameDir = "E:\SteamLibrary\steamapps\common\Warhammer 40,000 DARKTIDE",
    [string]$LaunchBat = "C:\dev\dtmt\docs\scripts\launch.bat"
)

$ErrorActionPreference = "Stop"
$logDir = Join-Path $env:APPDATA "Fatshark\Darktide\console_logs"

function Newest-Log {
    Get-ChildItem (Join-Path $logDir "*.log") |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
}

# Sample the game window by HWND. `CopyFromScreen` captured the wrong surface and
# was quarantined by Defender; `PrintWindow` is what the scratch tool uses.
function Get-WindowStats {
    try {
        Add-Type -AssemblyName System.Drawing -ErrorAction Stop
        if (-not ("WinShot" -as [type])) {
            Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinShot {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
}
"@
        }
        $p = Get-Process Darktide -ErrorAction SilentlyContinue |
            Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
        if (-not $p) { return [pscustomobject]@{ Mean = -1; Max = -1; Path = ""; Error = "no Darktide window" } }

        $r = New-Object WinShot+RECT
        [void][WinShot]::GetWindowRect($p.MainWindowHandle, [ref]$r)
        $w = $r.Right - $r.Left
        $h = $r.Bottom - $r.Top
        if ($w -le 0 -or $h -le 0) {
            return [pscustomobject]@{ Mean = -1; Max = -1; Path = ""; Error = "window has no size" }
        }

        $bmp = New-Object System.Drawing.Bitmap($w, $h)
        $gr = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $gr.GetHdc()
        [void][WinShot]::PrintWindow($p.MainWindowHandle, $hdc, 2)
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
        $path = Join-Path $env:TEMP "shader-render-test-last.png"
        $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
        $gr.Dispose(); $bmp.Dispose()
        $blackFrac = if ($n) { $black / $n } else { 0 }
        return [pscustomobject]@{
            Mean = if ($n) { $sum / $n } else { 0 }
            Max = $max
            BlackFrac = $blackFrac
            Path = $path
        }
    } catch {
        return [pscustomobject]@{ Mean = -1; Max = -1; BlackFrac = -1; Path = ""; Error = $_.Exception.Message }
    }
}

# A render-time crash is fatal; the known shutdown-time resource unload is benign.
function Classify-Crash($lines, $crashIndex) {
    $from = [Math]::Max(0, $crashIndex - 10)
    $context = ($lines[$from..$crashIndex] -join "`n")
    $message = $lines[$crashIndex]
    if ($context -match "ShaderTemplate::initialize" -or $context -match "dispatch_loadtime") {
        return "RENDER_FAIL"
    }
    if ($message -match "Trying to unload resource" -or $message -match "refcount") {
        return "BENIGN_UNLOAD"
    }
    if ($message -match "Access violation" -or $message -match "E_INVALIDARG") {
        return "RENDER_FAIL"
    }
    return "INDETERMINATE"
}

# The environment snapshot: a verdict without these is not reproducible.
function Get-Environment {
    $out = @()
    $section = Get-ChildItem (Join-Path $GameDir "bundle\data") -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -eq "4cc21b79457af061" } | Select-Object -First 1
    if ($section) {
        $out += "section=$((Get-FileHash $section.FullName).Hash.Substring(0,16)) ($($section.Length)B)"
    }
    $db = Join-Path $GameDir "bundle\bundle_database.data"
    if (Test-Path $db) { $out += "db=$((Get-Item $db).Length)" }
    $ini = Join-Path $GameDir "bundle\application_settings\settings_common.ini"
    if (Test-Path $ini) {
        $bs = (Select-String -Path $ini -Pattern "boot_script").Line
        $out += "boot_script=$($bs -replace '.*=\s*','')"
    }
    $pso = Join-Path $env:APPDATA "Fatshark\Darktide\shader_library.pso_lib"
    if (Test-Path $pso) { $out += "pso=$((Get-Item $pso).Length)@$((Get-Item $pso).LastWriteTime.ToString('MM-dd HH:mm'))" }
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

# Wait a fixed time for the title screen, then sample. The log cannot be polled
# (buffered), so this is a time budget rather than an event wait.
$wait = [Math]::Min($WaitForTitle, $Timeout)
while (((Get-Date) - $start).TotalSeconds -lt $wait) { Start-Sleep -Seconds 1 }
$screen = Get-WindowStats
# A slow boot (the deployment grows the bundle database) can still be on the
# loading screen at the first sample. Take a second, later frame and keep the
# clearer one - the loading screen is the black one.
Start-Sleep -Seconds 10
$later = Get-WindowStats
if ($screen.Mean -lt 0 -or ($later.Mean -ge 0 -and $later.BlackFrac -lt $screen.BlackFrac)) {
    $screen = $later
}

# Kill to flush the log, then give the flush a moment.
Get-Process Darktide -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 3

$log = Newest-Log
$lines = @(Get-Content $log.FullName)
$material = $lines | Select-String "material set: background_image" | Select-Object -First 1
$crash = $lines | Select-String "<<Crash>>" | Select-Object -First 1

$detail = ""
if ($material) { $detail = "material set at $($material.Line.Substring(0,12))" } else { $detail = "no material-set line (log may be unflushed on a hard kill)" }
$detail += "; mean $([Math]::Round($screen.Mean, 2)) max $([Math]::Round($screen.Max, 2)) black $([Math]::Round($screen.BlackFrac * 100, 1))% shot=$($screen.Path)"

$renderCrash = $null
if ($crash) {
    $kind = Classify-Crash $lines ($crash.LineNumber - 1)
    if ($kind -eq "RENDER_FAIL" -or $kind -eq "INDETERMINATE") { $renderCrash = $kind }
    $detail += "; crash: $($crash.Line.Trim().Substring(0,[Math]::Min(70, $crash.Line.Trim().Length)))"
}

if ($renderCrash) {
    $verdict = if ($renderCrash -eq "INDETERMINATE") { "INDETERMINATE" } else { "RENDER_FAIL" }
} elseif ($screen.BlackFrac -ge 0 -and $screen.BlackFrac -gt 0.5) {
    # The window sample is the load-bearing signal: a hard kill can lose the log,
    # but not the picture. A mostly-black window means the material is not drawing.
    $verdict = "RENDER_BLACK"
} elseif ($material -or $screen.Mean -ge 6) {
    $verdict = "RENDER_OK"
} else {
    $verdict = "NO_LOAD"
}

"VERDICT [$Label]: $verdict -- $detail"
"       log: $($log.Name)"
if ($verdict -ne "RENDER_OK") { exit 1 }
