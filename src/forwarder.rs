//! Physical → virtual XInput forwarder.
//!
//! Background thread that polls `XInputGetState` for slots 0..=3 at ~250 Hz
//! and pushes the first connected pad's report into [`VirtualPad`]. When
//! `suspend(N)` is called the loop stops forwarding for N ms and the
//! virtual pad keeps whatever state the action layer writes to it
//! directly — this is the gate that lets emote button presses survive
//! the streamer holding their stick.
//!
//! Hot path is lock-free: a single `AtomicU64` holds the epoch-millis
//! deadline until which forwarding is paused. The virtual pad lives
//! behind a `Mutex` because vigem-client requires `&mut self` for
//! `update`, but contention is rare (forwarder only touches it ~4 ms,
//! and the action layer suspends the forwarder before it writes).

#![allow(dead_code)]

use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::gamepad::{VirtualPad, XUSBReport};

const POLL_HZ: u64 = 250;
const POLL_PERIOD: Duration = Duration::from_millis(1000 / POLL_HZ);

pub struct Forwarder {
    pad: Arc<Mutex<VirtualPad>>,
    suspend_until_ms: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

/// Handle held by the action layer so it can suspend the forwarder
/// during an emote. Cheap to clone; all state is shared.
#[derive(Clone)]
pub struct ForwarderHandle {
    pad: Arc<Mutex<VirtualPad>>,
    suspend_until_ms: Arc<AtomicU64>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Forwarder {
    pub fn start(pad: VirtualPad) -> Self {
        let pad = Arc::new(Mutex::new(pad));
        let suspend_until_ms = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));

        let pad_t = Arc::clone(&pad);
        let suspend_t = Arc::clone(&suspend_until_ms);
        let stop_t = Arc::clone(&stop);

        let handle = thread::Builder::new()
            .name("w3stream-forwarder".into())
            .spawn(move || forward_loop(pad_t, suspend_t, stop_t))
            .ok();

        Self {
            pad,
            suspend_until_ms,
            stop,
            handle,
        }
    }

    pub fn handle(&self) -> ForwarderHandle {
        ForwarderHandle {
            pad: Arc::clone(&self.pad),
            suspend_until_ms: Arc::clone(&self.suspend_until_ms),
        }
    }
}

impl Drop for Forwarder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl ForwarderHandle {
    /// Pause forwarding for `duration_ms`. Extends an existing suspend if
    /// the requested window goes past the current deadline.
    pub fn suspend(&self, duration_ms: u64) {
        let target = now_ms().saturating_add(duration_ms);
        // Don't shorten an in-progress suspend if a longer one was queued.
        let _ =
            self.suspend_until_ms
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |cur| {
                    Some(cur.max(target))
                });
    }

    pub fn resume(&self) {
        self.suspend_until_ms.store(0, Ordering::Release);
    }

    pub fn is_suspended(&self) -> bool {
        self.suspend_until_ms.load(Ordering::Acquire) > now_ms()
    }

    /// Write a virtual-pad report directly. The action layer uses this
    /// to inject emote button presses while the forwarder is suspended.
    pub fn set_virtual(&self, report: XUSBReport) -> Result<()> {
        let mut pad = self.pad.lock().map_err(|_| anyhow::anyhow!("pad mutex poisoned"))?;
        pad.set_state(report)
    }

    /// Press a button on the virtual pad. Caller is responsible for
    /// calling `suspend` first if forwarding would clobber the press.
    pub fn press_button(&self, button: u16, duration_ms: u64) -> Result<()> {
        let mut pad = self.pad.lock().map_err(|_| anyhow::anyhow!("pad mutex poisoned"))?;
        pad.press_button(button, duration_ms)
    }
}

fn forward_loop(
    pad: Arc<Mutex<VirtualPad>>,
    suspend_until_ms: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        let deadline = suspend_until_ms.load(Ordering::Acquire);
        if deadline > now_ms() {
            // Suspended — sleep a little longer than the poll period so we
            // don't burn CPU spinning on the atomic during long emotes.
            thread::sleep(POLL_PERIOD);
            continue;
        }

        match read_physical_pad() {
            Ok(Some(report)) => {
                if let Ok(mut pad) = pad.lock() {
                    let _ = pad.set_state(report);
                }
            }
            Ok(None) => {
                // No physical pad connected; push neutral so the virtual
                // pad doesn't keep the last report forever.
                if let Ok(mut pad) = pad.lock() {
                    let _ = pad.set_state(XUSBReport::neutral());
                }
            }
            Err(_) => {
                // XInput error — back off briefly. Don't kill the loop;
                // pads come and go (battery death, USB unplug, etc.).
            }
        }

        thread::sleep(POLL_PERIOD);
    }
}

#[cfg(windows)]
fn read_physical_pad() -> Result<Option<XUSBReport>> {
    use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};

    for slot in 0u32..4 {
        let mut state = XINPUT_STATE::default();
        // SAFETY: XInputGetState takes a slot index and an out pointer to a
        // zero-initialised XINPUT_STATE. ERROR_SUCCESS (0) means connected.
        let rc = unsafe { XInputGetState(slot, &mut state as *mut _) };
        if rc == 0 {
            let g = state.Gamepad;
            return Ok(Some(XUSBReport {
                buttons: g.wButtons.0,
                left_trigger: g.bLeftTrigger,
                right_trigger: g.bRightTrigger,
                thumb_lx: g.sThumbLX,
                thumb_ly: g.sThumbLY,
                thumb_rx: g.sThumbRX,
                thumb_ry: g.sThumbRY,
            }));
        }
    }
    Ok(None)
}

#[cfg(not(windows))]
fn read_physical_pad() -> Result<Option<XUSBReport>> {
    Ok(None)
}
