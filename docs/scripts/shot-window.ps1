# Captures the Darktide window once (no launch, no kill).
param([string]$Out = "$env:TEMP\dt_shader\window-now.png")

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinShot {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
}
"@

$p = Get-Process Darktide -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { Write-Output "no Darktide window"; exit 1 }

$r = New-Object WinShot+RECT
[void][WinShot]::GetWindowRect($p.MainWindowHandle, [ref]$r)
$w = $r.Right - $r.Left
$h = $r.Bottom - $r.Top
Write-Output "window ${w}x${h} at ($($r.Left),$($r.Top))"

$bmp = New-Object System.Drawing.Bitmap($w, $h)
$gr = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $gr.GetHdc()
[void][WinShot]::PrintWindow($p.MainWindowHandle, $hdc, 2)
$gr.ReleaseHdc($hdc)
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$gr.Dispose(); $bmp.Dispose()
Write-Output "saved $Out ($((Get-Item $Out).Length) bytes)"
