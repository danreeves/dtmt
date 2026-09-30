<#
.SYNOPSIS
  Launches Darktide with the deployed mod and reports whether the custom shader
  actually RENDERED, not merely whether the Lua material assignment happened.

.DESCRIPTION
  The trap this exists to avoid: `material set: background_image` is logged by
  the mod's Lua before the engine ever builds a pipeline for the shader. A
  section whose programs/descriptors are wrong still logs it, then crashes in
  `ShaderTemplate::initialize` ~1 s later. Checking only the Lua line reported
  broken builds as healthy for a whole session.

  The verdicts:
    RENDER_OK    the game survived -WaitAfterMaterial seconds after the material
                 was set, or it ended in the known benign unload crash
                 (`Trying to unload resource ... refcount`).
    RENDER_FAIL  it crashed while building the shader template or dispatching
                 (`ShaderTemplate::initialize` / `dispatch_loadtime`), or with an
                 access violation, before the material had a chance to draw.
    NO_LOAD      no material-set line within the timeout (deploy/wedge problem).
    INDETERMINATE anything else, printed with the crash context for a human.

.PARAMETER Label
  A name for the run, echoed in the verdict.

.PARAMETER WaitAfterMaterial
  Seconds the game must survive after the material-set line to count as OK.
  40 is long enough to be well past the ~1 s render crash and far short of the
  ~100 s benign unload crash.

.PARAMETER Timeout
  Seconds to wait for the material-set line.
#>
param(
    [string]$Label = "run",
    [int]$WaitAfterMaterial = 40,
    [int]$Timeout = 200,
    [string]$GameDir = "E:\SteamLibrary\steamapps\common\Warhammer 40,000 DARKTIDE",
    [string]$LaunchBat = "C:\dev\dtmt\docs\scripts\launch.bat"
)

$ErrorActionPreference = "Stop"
$logDir = Join-Path $env:APPDATA "Fatshark\Darktide\console_logs"

function Newest-Log {
    Get-ChildItem (Join-Path $logDir "*.log") |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
}

# A crash is benign when it is the known shutdown-time resource unload, not a
# render-time failure. The context lines decide; the message alone is not enough.
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

Get-Process Darktide -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2

# The environment snapshot: a verdict without these is not reproducible. The
# section hash in particular is what proved the toolchain was blameless during
# the 2026-10-01 regression bisect.
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

$environment = Get-Environment
"ENV  [$Label]: $environment"

$before = Newest-Log
$start = Get-Date
Start-Process -FilePath $LaunchBat -WorkingDirectory $GameDir | Out-Null

$materialSet = $null
$verdict = $null
$detail = ""
$logInfo = ""

while (((Get-Date) - $start).TotalSeconds -lt $Timeout) {
    Start-Sleep -Seconds 3
    $log = Newest-Log
    if ($log.FullName -eq $before.FullName) { continue }
    $lines = @(Get-Content $log.FullName)

    if (-not $materialSet) {
        $m = $lines | Select-String "material set: background_image" | Select-Object -First 1
        if ($m) {
            $materialSet = Get-Date
            $detail = "material set at $($m.Line.Substring(0,12))"
        }
    }

    $crash = $lines | Select-String "<<Crash>>" | Select-Object -First 1
    if ($crash) {
        $kind = Classify-Crash $lines ($crash.LineNumber - 1)
        $elapsed = [int]((Get-Date) - $start).TotalSeconds
        if ($kind -eq "BENIGN_UNLOAD") {
            $verdict = "RENDER_OK"
            $detail = "$detail; ended in the benign unload crash (t=${elapsed}s)"
        } else {
            $verdict = if ($kind -eq "INDETERMINATE") { "INDETERMINATE" } else { "RENDER_FAIL" }
            $contextLine = ($lines | Select-Object -Skip ([Math]::Max(0, $crash.LineNumber - 3)) -First 1)
            $detail = "$detail; crash t=${elapsed}s: $($crash.Line.Substring(0,12)) ($($contextLine.Trim().Substring(0,[Math]::Min(70, $contextLine.Trim().Length))))"
        }
        break
    }

    if ($materialSet -and ((Get-Date) - $materialSet).TotalSeconds -ge $WaitAfterMaterial) {
        $verdict = "RENDER_OK"
        $detail = "$detail; survived ${WaitAfterMaterial}s of rendering"
        break
    }
}

$logInfo = (Newest-Log).Name
Get-Process Darktide -ErrorAction SilentlyContinue | Stop-Process -Force

if (-not $verdict) {
    $verdict = if ($materialSet) { "INDETERMINATE" } else { "NO_LOAD" }
    if (-not $materialSet) { $detail = "no material-set line within ${Timeout}s" }
}

"VERDICT [$Label]: $verdict -- $detail"
"       log: $logInfo"
if ($verdict -ne "RENDER_OK") { exit 1 }
