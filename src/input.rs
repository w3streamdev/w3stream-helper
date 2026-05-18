//! Keystroke driver. Win32 SendInput on Windows; no-op (returns Ok) on other
//! platforms so we can still build/test the protocol layer on Linux/macOS.

use anyhow::{anyhow, Result};
use std::thread::sleep;
use std::time::Duration;

use crate::actions::{Action, InputStep};

pub fn play(action: &Action) -> Result<()> {
    for step in &action.input_sequence {
        match step {
            InputStep::KeyTap { key, duration_ms } => {
                let vk = vk_for(key)?;
                press(vk)?;
                sleep(Duration::from_millis(*duration_ms));
                release(vk)?;
            }
            InputStep::KeyDown { key } => {
                let vk = vk_for(key)?;
                press(vk)?;
            }
            InputStep::KeyUp { key } => {
                let vk = vk_for(key)?;
                release(vk)?;
            }
            InputStep::Delay { duration_ms } => {
                sleep(Duration::from_millis(*duration_ms));
            }
            // Gamepad steps are dispatched by the executor (next commit).
            // input::play() is kept around for the keystroke-only path
            // and refuses gamepad steps explicitly so a misconfigured
            // action surfaces an action-level failure instead of silently
            // skipping the gamepad part.
            InputStep::GamepadButtonTap { .. }
            | InputStep::GamepadDpad { .. }
            | InputStep::SuspendForwarder { .. } => {
                return Err(anyhow!(
                    "gamepad step requires the gamepad executor; use executor::play()"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn press(vk: u16) -> Result<()> {
    send_key(vk, false)
}

#[cfg(windows)]
fn release(vk: u16) -> Result<()> {
    send_key(vk, true)
}

#[cfg(windows)]
fn send_key(vk: u16, key_up: bool) -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY,
    };

    let flags: KEYBD_EVENT_FLAGS = if key_up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };

    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        return Err(anyhow!("SendInput failed; only {sent} of 1 events sent"));
    }
    Ok(())
}

#[cfg(not(windows))]
fn press(_vk: u16) -> Result<()> {
    // No-op stub for non-Windows builds (CI/dev).
    Ok(())
}

#[cfg(not(windows))]
fn release(_vk: u16) -> Result<()> {
    Ok(())
}

/// Map a textual key name to a Win32 virtual-key code.
/// Accepts: single ASCII letter ("A"..="Z", "0".."="9"), F1..F24, common modifiers
/// and named keys ("Enter", "Space", "Tab", "Escape", "Up", "Down", "Left", "Right",
/// "Shift", "Ctrl", "Alt", numeric pad "Num0".."Num9"), case-insensitive.
fn vk_for(key: &str) -> Result<u16> {
    let k = key.trim();
    let upper = k.to_ascii_uppercase();

    // Single letter A-Z
    if upper.len() == 1 {
        let c = upper.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return Ok(c as u16);
        }
        if c.is_ascii_digit() {
            return Ok(c as u16); // '0'..'9' VKs are the ASCII codes
        }
    }

    if let Some(rest) = upper.strip_prefix('F') {
        if let Ok(n) = rest.parse::<u16>() {
            if (1..=24).contains(&n) {
                return Ok(0x70 + (n - 1)); // VK_F1 = 0x70
            }
        }
    }

    if let Some(rest) = upper.strip_prefix("NUM") {
        if let Ok(n) = rest.parse::<u16>() {
            if n <= 9 {
                return Ok(0x60 + n); // VK_NUMPAD0 = 0x60
            }
        }
    }

    Ok(match upper.as_str() {
        "ENTER" | "RETURN" => 0x0D,
        "SPACE" => 0x20,
        "TAB" => 0x09,
        "ESCAPE" | "ESC" => 0x1B,
        "BACKSPACE" => 0x08,
        "DELETE" | "DEL" => 0x2E,
        "INSERT" | "INS" => 0x2D,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" | "PGUP" => 0x21,
        "PAGEDOWN" | "PGDN" => 0x22,
        "UP" => 0x26,
        "DOWN" => 0x28,
        "LEFT" => 0x25,
        "RIGHT" => 0x27,
        "SHIFT" | "LSHIFT" => 0xA0,
        "RSHIFT" => 0xA1,
        "CTRL" | "CONTROL" | "LCTRL" => 0xA2,
        "RCTRL" => 0xA3,
        "ALT" | "LALT" => 0xA4,
        "RALT" => 0xA5,
        "CAPSLOCK" => 0x14,
        _ => return Err(anyhow!("unknown key: {key}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters() {
        assert_eq!(vk_for("a").unwrap(), b'A' as u16);
        assert_eq!(vk_for("Z").unwrap(), b'Z' as u16);
    }

    #[test]
    fn digits() {
        assert_eq!(vk_for("0").unwrap(), b'0' as u16);
        assert_eq!(vk_for("9").unwrap(), b'9' as u16);
    }

    #[test]
    fn function_keys() {
        assert_eq!(vk_for("F1").unwrap(), 0x70);
        assert_eq!(vk_for("F24").unwrap(), 0x87);
    }

    #[test]
    fn named() {
        assert_eq!(vk_for("Enter").unwrap(), 0x0D);
        assert_eq!(vk_for("Space").unwrap(), 0x20);
    }

    #[test]
    fn unknown_errors() {
        assert!(vk_for("Quack").is_err());
    }
}
