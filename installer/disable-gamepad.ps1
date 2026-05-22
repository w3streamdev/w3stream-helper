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

# Game controllers: match XInput devices — Xbox / Xbox-compatible pads, which
# is what virtually every Fortnite controller player uses. Their device
# instance path contains "IG_". Keyboards and mice never match this, so this
# filter cannot accidentally disable the streamer's keyboard or mouse.
function Get-GamepadIds {
    try {
        @(Get-PnpDevice -PresentOnly -ErrorAction Stop |
            Where-Object { $_.InstanceId -match 'IG_' } |
            Select-Object -ExpandProperty InstanceId -Unique)
    } catch {
        Log "Get-PnpDevice failed: $_"
        @()
    }
}

function Set-Gamepads([string]$verb, [string[]]$ids) {
    foreach ($id in $ids) {
        try {
            if ($verb -eq 'disable') {
                Disable-PnpDevice -InstanceId $id -Confirm:$false -ErrorAction Stop
            } else {
                Enable-PnpDevice -InstanceId $id -Confirm:$false -ErrorAction Stop
            }
            Log "$verb ok: $id"
        } catch {
            Log "$verb failed: $id -- $_"
        }
    }
}

Log "guard run start (lockout ${LockoutMs} ms)"

$ids = Get-GamepadIds
if ($ids.Count -eq 0) {
    Log "no game controllers present; nothing to do"
    exit 0
}

# Crash recovery: a previous run killed mid-window may have left a controller
# disabled. Clear any stale disabled state before opening a fresh window.
Set-Gamepads 'enable' $ids

try {
    Set-Gamepads 'disable' $ids
    Start-Sleep -Milliseconds $LockoutMs
} finally {
    # Always re-enable, even if the disable threw or the sleep was interrupted.
    Set-Gamepads 'enable' $ids
    Log "guard run end"
}
