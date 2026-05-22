# w3stream input-guard — gamepad lockout for chat-triggered emotes.
#
# Runs ELEVATED (as SYSTEM) via the "w3stream-input-guard" Scheduled Task that
# the installer registered. The user-level w3stream helper triggers that task
# with `schtasks /run` when an emote fires — a standard user can trigger it
# with NO UAC prompt, because the elevation was granted once, at install time.
#
# It disables the streamer's game controller(s) for a fixed window so their
# stick/buttons can't cancel the emote, then re-enables them. The re-enable
# runs in a finally block AND again at the top of the next run, so a killed
# run can never leave a controller stranded in the disabled state.

$ErrorActionPreference = 'Continue'

# Fixed lockout window. Mirrors input_suppression_ms for fortnite_emote_1 in
# src/actions.rs (5000 ms) — the helper's keyboard/mouse lockout is the same.
$LockoutMs = 5000

$logDir = Join-Path $env:ProgramData 'w3stream'
$log    = Join-Path $logDir 'guard.log'
try { New-Item -ItemType Directory -Force -Path $logDir | Out-Null } catch {}

function Log([string]$msg) {
    try {
        "$([DateTime]::UtcNow.ToString('o')) $msg" |
            Out-File -FilePath $log -Append -Encoding utf8
    } catch {}
}

# Resolve which device node to actually disable for each controller.
#
# XInput controllers (InstanceId contains "IG_") expose interface-child nodes
# that Windows refuses to stop on their own — Disable-PnpDevice returns
# "Generic failure". The node that CAN be stopped is the controller's USB
# composite PARENT. So for each IG_ device we walk up to the parent and
# target that instead — but ONLY when the parent still carries the same
# VID&PID, so we can never walk up into a USB hub and disable other devices.
function Get-Targets {
    $out = [System.Collections.Generic.List[string]]::new()
    $pads = @()
    try {
        $pads = @(Get-PnpDevice -PresentOnly -ErrorAction Stop |
                  Where-Object { $_.InstanceId -match 'IG_' })
    } catch {
        Log "Get-PnpDevice failed: $_"
        return @()
    }

    foreach ($p in $pads) {
        $id = $p.InstanceId
        $target = $id
        $vidpid = ''
        if ($id -match '(VID_[0-9A-Fa-f]{4}&PID_[0-9A-Fa-f]{4})') { $vidpid = $matches[1] }
        try {
            $parent = (Get-PnpDeviceProperty -InstanceId $id `
                        -KeyName 'DEVPKEY_Device_Parent' -ErrorAction Stop).Data
            if ($parent -and $vidpid -and ($parent -match [regex]::Escape($vidpid))) {
                $target = $parent
            }
        } catch {
            Log "parent lookup failed for ${id}: $_"
        }
        if (-not $out.Contains($target)) { $out.Add($target) }
    }
    return $out
}

# Disable one device node. Try the cmdlet first; if it fails, fall back to
# pnputil /disable-device (Windows 11). Logs which method (if any) worked.
function Disable-One([string]$id) {
    try {
        Disable-PnpDevice -InstanceId $id -Confirm:$false -ErrorAction Stop
        Log "disable OK (cmdlet): $id"
        return $true
    } catch {
        Log "disable cmdlet failed: ${id}: $_"
    }
    $o = (& pnputil /disable-device "$id" 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -eq 0) {
        Log "disable OK (pnputil): $id"
        return $true
    }
    Log "disable pnputil failed (exit ${LASTEXITCODE}): ${id}: $o"
    return $false
}

function Enable-One([string]$id) {
    try {
        Enable-PnpDevice -InstanceId $id -Confirm:$false -ErrorAction Stop
        Log "enable OK (cmdlet): $id"
        return
    } catch {
        Log "enable cmdlet failed: ${id}: $_"
    }
    $o = (& pnputil /enable-device "$id" 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -eq 0) {
        Log "enable OK (pnputil): $id"
    } else {
        Log "enable pnputil failed (exit ${LASTEXITCODE}): ${id}: $o"
    }
}

Log "guard run start (lockout ${LockoutMs} ms)"

$targets = @(Get-Targets)
if ($targets.Count -eq 0) {
    Log "no game controllers present; nothing to do"
    exit 0
}
Log ("targets (" + $targets.Count + "): " + ($targets -join '  |  '))

# Crash recovery: a previous run killed mid-window may have left a controller
# disabled. Clear any stale disabled state before opening a fresh window.
foreach ($t in $targets) { Enable-One $t }

try {
    $disabledAny = $false
    foreach ($t in $targets) {
        if (Disable-One $t) { $disabledAny = $true }
    }
    if (-not $disabledAny) { Log "WARNING: no device could be disabled" }
    Start-Sleep -Milliseconds $LockoutMs
} finally {
    # Always re-enable, even if a disable threw or the sleep was interrupted.
    foreach ($t in $targets) { Enable-One $t }
    Log "guard run end"
}
