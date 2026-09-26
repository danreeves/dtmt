# Launches the game, captures the game window a few times in the first
# seconds after the title screen appears, then closes the game.
param(
    [int]$BootStall = 2,
    [int[]]$Wait = @(5, 8, 11),
    [string]$OutDir = "$env:TEMP\dt_shader",
    [string]$Launch = (Join-Path $PSScriptRoot "launch.bat")
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WC3 {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
}
"@

function Shot([string]$path) {
    $p = Get-Process Darktide -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if (-not $p) { Write-Output "no game window"; return }
    $r = New-Object WC3+RECT
    [void][WC3]::GetWindowRect($p.MainWindowHandle, [ref]$r)
    $bmp = New-Object System.Drawing.Bitmap(($r.Right - $r.Left), ($r.Bottom - $r.Top))
    $gr = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $gr.GetHdc()
    [void][WC3]::PrintWindow($p.MainWindowHandle, $hdc, 2)
    $gr.ReleaseHdc($hdc)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $gr.Dispose()
    $bmp.Dispose()
    Write-Output "captured $path"
}

$g = "E:\SteamLibrary\steamapps\common\Warhammer 40,000 DARKTIDE"
Get-Process Darktide -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 3
Start-Process -FilePath $Launch -WorkingDirectory $g | Out-Null

$elapsed = 0
foreach ($t in $Wait) {
    Start-Sleep -Seconds ($t - $elapsed)
    $elapsed = $t
    Shot "$OutDir\boot_t$t.png"
}

Get-Process Darktide -ErrorAction SilentlyContinue | Stop-Process -Force
$log = (Get-ChildItem "$env:APPDATA\Fatshark\Darktide\console_logs" -Filter *.log |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
Select-String -Path $log -Pattern 'snoopymod|StateTitle' | Select-Object -First 6 |
    ForEach-Object { $_.Line }
