# Compiles an HLSL shader to a DXBC/DXIL container for splicing into a
# Darktide material with the `shader43` SDK example.
#
# Usage:
#   .\compile.ps1 -Input .\gui_tint.hlsl -Entry ps_main -Target ps_6_0 -Output ..\out\gui_tint.dxbc
#
# The Windows SDK's `dxc.exe` is used by default; pass -Dxc to override.

param(
    [Parameter(Mandatory = $true)][string]$Input,
    [Parameter(Mandatory = $true)][string]$Entry,
    [Parameter(Mandatory = $true)][string]$Target,
    [Parameter(Mandatory = $true)][string]$Output,
    [string]$Dxc = ""
)

$ErrorActionPreference = "Stop"

if (-not $Dxc) {
    $candidates = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter dxc.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } |
        Sort-Object FullName -Descending
    $Dxc = $candidates | Select-Object -First 1 -ExpandProperty FullName
}

if (-not $Dxc) {
    throw "Could not find dxc.exe; pass -Dxc <path>"
}

Write-Host "dxc: $Dxc"
& $Dxc -T $Target -E $Entry -Fo $Output $Input
if ($LASTEXITCODE -ne 0) {
    throw "Shader compilation failed with exit code $LASTEXITCODE"
}

Write-Host "wrote $Output ($((Get-Item $Output).Length) bytes)"
