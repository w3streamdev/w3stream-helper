//! Client for the privileged w3stream input-guard.
//!
//! Disabling a physical game controller is a privileged Windows operation — a
//! user-level process (which is all the helper can be, since Chrome launches
//! it) cannot block another process's gamepad. The installer registers a
//! Scheduled Task, `w3stream-input-guard`, that runs an elevated PowerShell
//! script (`disable-gamepad.ps1`) as SYSTEM. A standard user may *trigger*
//! that task without a UAC prompt — the one-time elevation happened when the
//! installer created it.
//!
//! The script owns the whole disable→wait→re-enable cycle and re-enables on
//! its own, so a helper crash can never strand the controller disabled.
//!
//! Everything here is best-effort: if the task isn't registered the gamepad
//! simply isn't covered (keyboard + mouse lockout still works). Errors are
//! logged, never propagated — an emote must never fail because of the guard.

/// Name of the Scheduled Task created by the installer. Only referenced by
/// the Windows implementation below.
#[cfg_attr(not(windows), allow(dead_code))]
pub const GUARD_TASK: &str = "w3stream-input-guard";

/// Fire the gamepad lockout: trigger the elevated guard task. Non-blocking —
/// the emote is never delayed waiting on this. `duration_ms` is informational
/// (the guard script holds a fixed 5 s window matching `input_suppression_ms`).
pub fn disable_gamepad(duration_ms: u64) {
    imp::disable_gamepad(duration_ms);
}

/// True if the guard Scheduled Task is registered and can be triggered.
pub fn guard_available() -> bool {
    imp::guard_available()
}

#[cfg(windows)]
mod imp {
    use super::GUARD_TASK;
    use log::{info, warn};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    // CREATE_NO_WINDOW — never flash a console when the helper shells out.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn disable_gamepad(duration_ms: u64) {
        // Run schtasks with explicit null stdin and captured stdout/stderr.
        //
        // The helper's own stdio handles ARE the Chrome native-messaging
        // pipes. A child that inherits them runs in a broken context and
        // would also corrupt the protocol stream by writing into it. We
        // isolate schtasks completely and capture its output so a failure
        // is diagnosable from helper.log instead of being a bare exit code.
        match Command::new("schtasks")
            .args(["/run", "/tn", GUARD_TASK])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
        {
            Ok(o) if o.status.success() => {
                info!("input-guard: triggered gamepad lockout (~{duration_ms} ms window)");
            }
            Ok(o) => {
                let detail = format!(
                    "{} {}",
                    String::from_utf8_lossy(&o.stdout).trim(),
                    String::from_utf8_lossy(&o.stderr).trim()
                );
                warn!(
                    "input-guard: schtasks /run failed (exit {}): {} — gamepad not \
                     locked out (keyboard/mouse lockout still active)",
                    o.status.code().unwrap_or(-1),
                    detail.trim()
                );
            }
            Err(e) => warn!(
                "input-guard: could not run schtasks ({e}); gamepad not locked out \
                 (keyboard/mouse lockout still active)"
            ),
        }
    }

    pub fn guard_available() -> bool {
        Command::new("schtasks")
            .args(["/query", "/tn", GUARD_TASK])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

#[cfg(not(windows))]
mod imp {
    use log::info;

    pub fn disable_gamepad(duration_ms: u64) {
        info!("input-guard: gamepad lockout ({duration_ms} ms) requested — no-op off Windows");
    }

    pub fn guard_available() -> bool {
        false
    }
}
