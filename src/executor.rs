//! Action executor: dispatches each `InputStep` to the right driver
//! (Win32 SendInput for keystrokes, ViGEm for gamepad) and applies the
//! forwarder-suspend safety net for actions that touch the virtual pad.

use anyhow::Result;
use std::thread::sleep;
use std::time::Duration;

use crate::actions::{Action, InputStep};
use crate::forwarder::ForwarderHandle;
use crate::gamepad::XUSBReport;
use crate::input;

/// Buffer added to the auto-suspend window so a slow Delay step at the
/// end of an action doesn't get clipped by forwarding resuming early.
const SUSPEND_BUFFER_MS: u64 = 500;

pub fn play(action: &Action, forwarder: Option<&ForwarderHandle>) -> Result<()> {
    let needs_gamepad = action.input_sequence.iter().any(InputStep::is_gamepad);

    if needs_gamepad {
        let fw = forwarder.ok_or_else(|| {
            anyhow::anyhow!(
                "action {} requires a gamepad but the forwarder is not available \
                 (ViGEmBus/HidHide may not be installed)",
                action.action_id
            )
        })?;
        let total: u64 = action.input_sequence.iter().map(InputStep::duration_ms).sum();
        // Safety-net suspend: even if the action author forgot the
        // explicit SuspendForwarder step, the forwarder won't clobber
        // the emote button presses for the full action window.
        fw.suspend(total.saturating_add(SUSPEND_BUFFER_MS));
    }

    for step in &action.input_sequence {
        match step {
            InputStep::KeyTap { .. }
            | InputStep::KeyDown { .. }
            | InputStep::KeyUp { .. }
            | InputStep::Delay { .. } => {
                play_keyboard_step(step)?;
            }
            InputStep::SuspendForwarder { duration_ms } => {
                if let Some(fw) = forwarder {
                    fw.suspend(*duration_ms);
                }
                // No sleep here: the suspend is non-blocking. The next
                // step's own delay/duration drives the wall clock.
            }
            InputStep::GamepadButtonTap { button, duration_ms } => {
                let fw = forwarder.ok_or_else(|| {
                    anyhow::anyhow!("GamepadButtonTap requires forwarder")
                })?;
                fw.press_button(button.bit(), *duration_ms)?;
            }
            InputStep::GamepadDpad { direction, duration_ms } => {
                let fw = forwarder.ok_or_else(|| {
                    anyhow::anyhow!("GamepadDpad requires forwarder")
                })?;
                let mut report = XUSBReport::neutral();
                report.buttons = direction.bits();
                fw.set_virtual(report)?;
                sleep(Duration::from_millis(*duration_ms));
                fw.set_virtual(XUSBReport::neutral())?;
            }
        }
    }
    Ok(())
}

/// Run a single keyboard-only step. Reuses input::play() by wrapping the
/// step in a one-shot Action so the existing Win32 SendInput path stays
/// the single source of truth for vk mapping.
fn play_keyboard_step(step: &InputStep) -> Result<()> {
    let action = Action {
        action_id: "_inline".into(),
        label: String::new(),
        enabled: true,
        cooldown_ms: 0,
        input_sequence: vec![step.clone()],
    };
    input::play(&action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{DpadDir, GamepadButton};

    fn keyboard_action() -> Action {
        Action {
            action_id: "kb".into(),
            label: "kb".into(),
            enabled: true,
            cooldown_ms: 0,
            input_sequence: vec![InputStep::Delay { duration_ms: 1 }],
        }
    }

    fn gamepad_action() -> Action {
        Action {
            action_id: "gp".into(),
            label: "gp".into(),
            enabled: true,
            cooldown_ms: 0,
            input_sequence: vec![
                InputStep::SuspendForwarder { duration_ms: 100 },
                InputStep::GamepadDpad {
                    direction: DpadDir::Down,
                    duration_ms: 10,
                },
                InputStep::GamepadButtonTap {
                    button: GamepadButton::A,
                    duration_ms: 10,
                },
            ],
        }
    }

    #[test]
    fn keyboard_only_runs_without_forwarder() {
        // Delay-only action — should work even with no gamepad available.
        assert!(play(&keyboard_action(), None).is_ok());
    }

    #[test]
    fn gamepad_action_without_forwarder_errors() {
        let err = play(&gamepad_action(), None).unwrap_err();
        assert!(err.to_string().contains("forwarder"));
    }
}
