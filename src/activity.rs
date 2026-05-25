//! Keyboard + mouse activity observer.
//!
//! The movement-gated emote retry watches the gamepad via XInput, but most
//! streamers are on keyboard + mouse — the helper has to detect that input
//! too or the idle timer just runs out while they're holding WASD.
//!
//! This module installs low-level keyboard and mouse hooks (same hook
//! family as `suppression.rs`) that ONLY observe — they never block input.
//! Hook callbacks filter out events flagged `LLKHF_INJECTED` /
//! `LLMHF_INJECTED` so the helper's own re-fired emote keystrokes don't
//! count as user activity and trap the retry loop forever.
//!
//! `start_observer()` is idempotent and lazy: the first call spawns a
//! dedicated thread that installs the hooks and pumps the Windows message
//! queue for their lifetime. Subsequent calls are no-ops. Hooks remain
//! installed for the rest of the helper's lifetime — the cost of an
//! observer hook is a single atomic store per event, so leaving them up is
//! cheaper than tearing them down/back up between emotes.
//!
//! On non-Windows builds this is a no-op so the protocol layer stays
//! buildable / testable in CI.

#[cfg(windows)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(windows)]
use std::sync::Once;
#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
static LAST_ACTIVITY_MS: AtomicU64 = AtomicU64::new(0);

#[cfg(windows)]
static INIT: Once = Once::new();

#[cfg(windows)]
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Spawn the observer thread the first time this is called. Subsequent
/// calls are no-ops. On non-Windows builds this does nothing.
pub fn start_observer() {
    #[cfg(windows)]
    INIT.call_once(|| {
        let _ = thread::Builder::new()
            .name("w3stream-activity-observer".into())
            .spawn(observer_thread);
    });
}

/// Most recent observed real (non-injected) keyboard or mouse activity, as
/// UNIX-epoch milliseconds. `None` if the observer has not seen an event
/// yet, or is not running (non-Windows builds).
pub fn last_activity_ms() -> Option<u64> {
    #[cfg(windows)]
    {
        let v = LAST_ACTIVITY_MS.load(Ordering::Acquire);
        if v == 0 {
            None
        } else {
            Some(v)
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn observer_thread() {
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, PeekMessageW, SetWindowsHookExW, TranslateMessage,
        HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT,
        PM_REMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL,
    };

    unsafe extern "system" fn keyboard_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION as i32 {
            let event = *(lparam.0 as *const KBDLLHOOKSTRUCT);
            // Helper's own SendInput re-fires arrive with LLKHF_INJECTED
            // set — filter them so we don't trap our own idle timer.
            if !event.flags.contains(LLKHF_INJECTED) {
                LAST_ACTIVITY_MS.store(now_ms(), Ordering::Release);
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    unsafe extern "system" fn mouse_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION as i32 {
            let event = *(lparam.0 as *const MSLLHOOKSTRUCT);
            if event.flags & LLMHF_INJECTED == 0 {
                LAST_ACTIVITY_MS.store(now_ms(), Ordering::Release);
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    let kb = unsafe {
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), HINSTANCE::default(), 0)
    };
    let ms = unsafe {
        SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), HINSTANCE::default(), 0)
    };

    if kb.is_err() && ms.is_err() {
        log::warn!(
            "activity observer: no keyboard/mouse hooks installed — \
             keyboard/mouse activity will not reset the emote retry idle timer"
        );
        return;
    }
    log::info!("activity observer: keyboard + mouse hooks installed");

    // Pump the message queue forever — low-level hooks fire on this
    // thread's queue and will not run otherwise.
    loop {
        let mut msg = MSG::default();
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_activity_starts_none() {
        // On non-Windows or before observer has seen anything.
        // Note: cannot reliably assert this is None on Windows if a prior
        // test happened to install the observer — the static persists.
        // Just make sure the function doesn't panic.
        let _ = last_activity_ms();
    }

    #[test]
    fn start_observer_is_idempotent() {
        start_observer();
        start_observer();
        start_observer();
    }
}
