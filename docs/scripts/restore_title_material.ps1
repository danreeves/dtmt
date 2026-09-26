$hex = @(
  "3D000000","1C000000","44000000","FFFFFFFF","00000000","FFFFFFFF","00000000",
  "00000000",
  "B48494EBFFD069D0",
  "0000000000000000",
  "00000000",
  "01000000",
  "2C1503E5",
  "CBE11812997CD2E1",
  "01000000",
  "4BA05402",
  "050338C7",
  "00000000",
  "00000000",
  "00000000",
  "00000000"
) -join ''
$bytes = for ($i = 0; $i -lt $hex.Length; $i += 2) { [Convert]::ToByte($hex.Substring($i, 2), 16) }
$out = "E:\SteamLibrary\steamapps\common\Warhammer 40,000 DARKTIDE\bundle\data\ad\ad37c1d1f6818047"
[System.IO.File]::WriteAllBytes($out, [byte[]]$bytes)
Write-Output ("wrote {0} bytes" -f $bytes.Count)
