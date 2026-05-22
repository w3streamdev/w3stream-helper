# Installer helper — register the "w3stream-input-guard" Scheduled Task.
#
# Runs ONCE, elevated, during install (this is the single UAC prompt). It
# creates a task that runs the gamepad-lockout script as SYSTEM with NO
# trigger, so the task never fires on its own — the user-level helper invokes
# it on demand via `schtasks /run` when an emote plays.

param(
    [Parameter(Mandatory = $true)] [string] $ScriptPath,
    [string] $LogPath
)

$ErrorActionPreference = 'Stop'

function Log([string]$msg) {
    if ($LogPath) {
        try {
            "$([DateTime]::UtcNow.ToString('o')) $msg" |
                Out-File -FilePath $LogPath -Append -Encoding utf8
        } catch {}
    }
}

try {
    Log "registering w3stream-input-guard -> $ScriptPath"

    $arg = '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "{0}"' -f $ScriptPath
    $action    = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $arg
    $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -RunLevel Highest
    $settings  = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries `
                                              -DontStopIfGoingOnBatteries `
                                              -ExecutionTimeLimit (New-TimeSpan -Minutes 1) `
                                              -MultipleInstances IgnoreNew

    # No -Trigger: the task only ever runs on demand (schtasks /run).
    Register-ScheduledTask -TaskName 'w3stream-input-guard' `
                           -Action $action `
                           -Principal $principal `
                           -Settings $settings `
                           -Description 'w3stream input-guard: briefly disables the game controller while a chat-triggered emote plays.' `
                           -Force | Out-Null

    # Grant the run permission. Register-ScheduledTask creates a task that
    # only administrators can trigger — the user-level helper calls
    # `schtasks /run` and would hit "Access is denied". Set a DACL that keeps
    # Admins + SYSTEM full and adds Authenticated Users read+execute, so the
    # streamer's normal account can fire the task with no elevation.
    try {
        $svc = New-Object -ComObject 'Schedule.Service'
        $svc.Connect()
        $svc.GetFolder('\').GetTask('w3stream-input-guard').SetSecurityDescriptor(
            'D:(A;;FA;;;BA)(A;;FA;;;SY)(A;;FRFX;;;AU)', 0)
        Log "task ACL set: Authenticated Users may run the task"
    } catch {
        Log "WARNING: could not set task ACL ($_) — helper may hit Access denied"
    }

    Log "registered ok"
    exit 0
} catch {
    Log "FAILED: $_"
    exit 1
}
