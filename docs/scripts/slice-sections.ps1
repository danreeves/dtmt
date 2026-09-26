# Slices the sections of a material data file into individual .bin files.
param(
    [Parameter(Mandatory = $true)][string]$Material,
    [Parameter(Mandatory = $true)][string]$Out,
    [Parameter(Mandatory = $true)][string]$Tag
)

$data = [System.IO.File]::ReadAllBytes($Material)
function U32([int]$at) { [BitConverter]::ToUInt32($data, $at) }

$shaderOffset = U32 12
$shaderSize = U32 16
$shader = New-Object byte[] $shaderSize
[Array]::Copy($data, $shaderOffset, $shader, 0, $shaderSize) | Out-Null

function SU32([int]$at) { [BitConverter]::ToUInt32($shader, $at) }

New-Item -ItemType Directory -Force -Path $Out | Out-Null

$sections = @{
    header    = @(0, (SU32 8))
    contexts  = @((SU32 8), ((SU32 16) - (SU32 8)))
    conditions = @((SU32 16), ((SU32 24) - (SU32 16)))
    dependencies = @((SU32 24), ((SU32 32) - (SU32 24)))
    group     = @((SU32 32), (SU32 36))
    device    = @((SU32 40), (SU32 44))
    default   = @((SU32 20), ($shaderSize - (SU32 20)))
}

Write-Output ("shader: {0} bytes at {1:x}, contexts={2:x} conditions={3:x} deps={4:x} group={5:x}({6}) device={7:x}({8}) default={9:x}" -f `
        $shaderSize, $shaderOffset, (SU32 8), (SU32 16), (SU32 24), (SU32 32), (SU32 36), (SU32 40), (SU32 44), (SU32 20))

foreach ($name in $sections.Keys) {
    $start, $size = $sections[$name]
    if ($start + $size -gt $shaderSize) { Write-Output "$name out of range"; continue }
    $bytes = New-Object byte[] $size
    [Array]::Copy($shader, $start, $bytes, 0, $size) | Out-Null
    $path = Join-Path $Out "$Tag.$name.bin"
    [System.IO.File]::WriteAllBytes($path, $bytes)
    Write-Output ("{0,-13} {1,9} bytes -> {2}" -f $name, $size, $path)
}
