param(
    [Parameter(Mandatory = $true)][string]$DataFile,
    [Parameter(Mandatory = $true)][string]$Sjson
)

# Replaces the `shader_size` and `shader_data` fields of a DTMT material SJSON
# with the shader section of a compiled material data file.
$bytes = [System.IO.File]::ReadAllBytes($DataFile)
$offset = [BitConverter]::ToUInt32($bytes, 12)
$size = [BitConverter]::ToUInt32($bytes, 16)
$shader = $bytes[$offset..($offset + $size - 1)]
$hex = [System.BitConverter]::ToString($shader).Replace('-', '')
$text = [System.IO.File]::ReadAllText($Sjson)
$text = [regex]::Replace($text, '(?m)^shader_size = \d+', "shader_size = $size")
$text = [regex]::Replace($text, '(?m)^shader_data = "[0-9A-F]*"', "shader_data = `"$hex`"")
[System.IO.File]::WriteAllText($Sjson, $text)
Write-Output "shader_size=$size"
