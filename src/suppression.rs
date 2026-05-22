//! Temporary, narrow input suppression used after a Fortnite emote trigger.
//!
//! The helper never installs persistent hooks. `suppress_for` only extends a
//! short in-memory deadline and a background thread removes its hooks as soon as
//! that deadline expires. On non-Windows targets this is a logged no-op so the
//! crate remains testable in CI.

#[cfg(not(windows))]
use log::info;
#[cfg(windows)]
use log::{info, warn};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const POLL_MS: u64 = 10;

static SUPPRESS_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
static HOOK_THREAD_RUNNING: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn suppress_for(duration_ms: u64) {
    if duration_ms == 0 {
        return;
    }

    let until = now_ms().saturating_add(duration_ms);
    let _ = SUPPRESS_UNTIL_MS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some(current.max(until))
    });

    if HOOK_THREAD_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        let _ = thread::Builder::new()
            .name("w3stream-input-suppression".into())
            .spawn(suppression_thread);
    }
}

fn suppression_thread() {
    loop {
        platform_suppress_until_deadline();

        HOOK_THREAD_RUNNING.store(false, Ordering::Release);
        if SUPPRESS_UNTIL_MS.load(Ordering::Acquire) <= now_ms() {
            break;
        }
        if HOOK_THREAD_RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            break;
        }
    }
}

#[cfg(not(windows))]
fn platform_suppress_until_deadline() {
    let deadline = SUPPRESS_UNTIL_MS.load(Ordering::Acquire);
    info!("input suppression requested until {deadline} ms; keyboard/mouse hooks are Windows-only");
    while SUPPRESS_UNTIL_MS.load(Ordering::Acquire) > now_ms() {
        thread::sleep(Duration::from_millis(POLL_MS));
    }
    info!("input suppression ended (non-Windows no-op)");
}

#[cfg(windows)]
fn platform_suppress_until_deadline() {
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, PeekMessageW, SetWindowsHookExW, TranslateMessage,
        UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED,
        MSG, MSLLHOOKSTRUCT, PM_REMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
        WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN,
        WM_MBUTTONUP, WM_MOUSEMOVE, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
        WM_NCMBUTTONDBLCLK, WM_NCMBUTTONDOWN, WM_NCMBUTTONUP, WM_NCMOUSEMOVE, WM_NCRBUTTONDBLCLK,
        WM_NCRBUTTONDOWN, WM_NCRBUTTONUP, WM_NCXBUTTONDBLCLK, WM_NCXBUTTONDOWN, WM_NCXBUTTONUP,
        WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
        WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP,
    };

    struct HookGuard {
        keyboard: Option<HHOOK>,
        mouse: Option<HHOOK>,
    }

    impl Drop for HookGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(hook) = self.keyboard.take() {
                    let _ = UnhookWindowsHookEx(hook);
                }
                if let Some(hook) = self.mouse.take() {
                    let _ = UnhookWindowsHookEx(hook);
                }
            }
            info!("input suppression ended; keyboard/mouse hooks removed");
        }
    }

    unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 && SUPPRESS_UNTIL_MS.load(Ordering::Acquire) > now_ms() {
            let msg = wparam.0 as u32;
            if matches!(msg, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP) {
                let event = *(lparam.0 as *const KBDLLHOOKSTRUCT);
                // Disable the keyboard outright for the lockout window —
                // every physical key is swallowed so the streamer cannot
                // cancel the emote. Our own emote keystrokes go through
                // SendInput and carry LLKHF_INJECTED, so they still pass.
                if !event.flags.contains(LLKHF_INJECTED) {
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 && SUPPRESS_UNTIL_MS.load(Ordering::Acquire) > now_ms() {
            let msg = wparam.0 as u32;
            let should_block = matches!(
                msg,
                WM_MOUSEMOVE
                    | WM_LBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_LBUTTONDBLCLK
                    | WM_MBUTTONDOWN
                    | WM_MBUTTONUP
                    | WM_MBUTTONDBLCLK
                    | WM_RBUTTONDOWN
                    | WM_RBUTTONUP
                    | WM_RBUTTONDBLCLK
                    | WM_XBUTTONDOWN
                    | WM_XBUTTONUP
                    | WM_XBUTTONDBLCLK
                    | WM_NCMOUSEMOVE
                    | WM_NCLBUTTONDOWN
                    | WM_NCLBUTTONUP
                    | WM_NCLBUTTONDBLCLK
                    | WM_NCMBUTTONDOWN
                    | WM_NCMBUTTONUP
                    | WM_NCMBUTTONDBLCLK
                    | WM_NCRBUTTONDOWN
                    | WM_NCRBUTTONUP
                    | WM_NCRBUTTONDBLCLK
                    | WM_NCXBUTTONDOWN
                    | WM_NCXBUTTONUP
                    | WM_NCXBUTTONDBLCLK
            );
            if should_block {
                let event = *(lparam.0 as *const MSLLHOOKSTRUCT);
                if event.flags & LLMHF_INJECTED == 0 {
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    let keyboard =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), HINSTANCE::default(), 0) };
    let keyboard = match keyboard {
        Ok(hook) => Some(hook),
        Err(e) => {
            warn!("keyboard suppression hook unavailable: {e}");
            None
        }
    };

    let mouse =
        unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), HINSTANCE::default(), 0) };
    let mouse = match mouse {
        Ok(hook) => Some(hook),
        Err(e) => {
            warn!("mouse suppression hook unavailable: {e}");
            None
        }
    };

    if keyboard.is_none() && mouse.is_none() {
        warn!("input suppression unavailable: no keyboard/mouse hooks installed");
        return;
    }

    let _guard = HookGuard { keyboard, mouse };
    info!("input suppression started; keyboard/mouse hooks active");

    while SUPPRESS_UNTIL_MS.load(Ordering::Acquire) > now_ms() {
        let mut msg = MSG::default();
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        thread::sleep(Duration::from_millis(POLL_MS));
    }
}
