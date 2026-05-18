//! Virtual Xbox 360 pad backed by ViGEmBus.
//!
//! The helper creates one `VirtualPad` at startup. The forwarder writes
//! physical-pad state into it ~250 Hz; chat-triggered actions overlay
//! emote button presses by suspending the forwarder for the action's
//! duration and pushing reports directly.
//!
//! On non-Windows targets this module compiles to a stub that always
//! returns `Err("unsupported")` from `VirtualPad::new`. That keeps the
//! protocol/state crates buildable on Linux for tests.

#![allow(dead_code)]

use anyhow::Result;
use std::thread::sleep;
use std::time::Duration;

/// XInput-compatible button bitfield. Re-exported under a stable name
/// so the rest of the crate doesn't import vigem-client directly.
///
/// Layout matches `XINPUT_GAMEPAD` exactly so we can pass through reports
/// read from the physical pad with zero translation.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct XUSBReport {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

impl XUSBReport {
    pub const fn neutral() -> Self {
        Self {
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
            thumb_lx: 0,
            thumb_ly: 0,
            thumb_rx: 0,
            thumb_ry: 0,
        }
    }
}

/// XInput button bits — values copied from `vigem_client::XButtons` so the
/// rest of the crate has stable names even on non-Windows builds.
pub mod buttons {
    pub const DPAD_UP: u16 = 0x0001;
    pub const DPAD_DOWN: u16 = 0x0002;
    pub const DPAD_LEFT: u16 = 0x0004;
    pub const DPAD_RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LTHUMB: u16 = 0x0040;
    pub const RTHUMB: u16 = 0x0080;
    pub const LB: u16 = 0x0100;
    pub const RB: u16 = 0x0200;
    pub const GUIDE: u16 = 0x0400;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;
}

#[cfg(windows)]
mod imp {
    use super::*;
    use anyhow::{anyhow, Context, Result};
    use vigem_client::{Client, TargetId, XGamepad, Xbox360Wired};

    pub struct VirtualPad {
        target: Xbox360Wired<Client>,
    }

    impl VirtualPad {
        pub fn new() -> Result<Self> {
            let client = Client::connect().map_err(|e| {
                anyhow!(
                    "ViGEmBus driver not reachable ({:?}). Install ViGEmBus and reboot.",
                    e
                )
            })?;
            let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
            target
                .plugin()
                .map_err(|e| anyhow!("Xbox360 plugin failed: {:?}", e))
                .context("ViGEm Xbox360Wired::plugin")?;
            target
                .wait_ready()
                .map_err(|e| anyhow!("Xbox360 wait_ready failed: {:?}", e))
                .context("ViGEm Xbox360Wired::wait_ready")?;
            Ok(Self { target })
        }

        pub fn set_state(&mut self, report: XUSBReport) -> Result<()> {
            let pad = XGamepad {
                buttons: vigem_client::XButtons { raw: report.buttons },
                left_trigger: report.left_trigger,
                right_trigger: report.right_trigger,
                thumb_lx: report.thumb_lx,
                thumb_ly: report.thumb_ly,
                thumb_rx: report.thumb_rx,
                thumb_ry: report.thumb_ry,
            };
            self.target
                .update(&pad)
                .map_err(|e| anyhow!("ViGEm update failed: {:?}", e))?;
            Ok(())
        }

        /// Press `button` for `duration_ms` then release. Sticks/triggers
        /// are forced neutral for the duration so the emote isn't
        /// cancelled by leftover stick state from the forwarder.
        pub fn press_button(&mut self, button: u16, duration_ms: u64) -> Result<()> {
            let mut report = XUSBReport::neutral();
            report.buttons = button;
            self.set_state(report)?;
            std::thread::sleep(std::time::Duration::from_millis(duration_ms));
            self.set_state(XUSBReport::neutral())?;
            Ok(())
        }
    }

    impl Drop for VirtualPad {
        fn drop(&mut self) {
            // Best-effort: zero the report so the game sees a clean release
            // before the device disappears, then unplug. The Drop impl on
            // Xbox360Wired also unplugs, but doing it explicitly here lets
            // us swallow the error rather than panic.
            let _ = self.target.update(&XGamepad::default());
            let _ = self.target.unplug();
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use anyhow::anyhow;

    pub struct VirtualPad;

    impl VirtualPad {
        pub fn new() -> Result<Self> {
            Err(anyhow!(
                "VirtualPad is Windows-only; ViGEmBus driver required"
            ))
        }
        pub fn set_state(&mut self, _report: XUSBReport) -> Result<()> {
            Ok(())
        }
        pub fn press_button(&mut self, _button: u16, duration_ms: u64) -> Result<()> {
            sleep(Duration::from_millis(duration_ms));
            Ok(())
        }
    }
}

pub use imp::VirtualPad;
