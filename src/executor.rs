//! Action executor.
//!
//! Runs an action's keyboard input sequence and, for actions that declare an
//! `input_suppression_ms` window, locks the streamer's OWN input out for that
//! window so their movement can't cancel a chat-triggered emote:
//!   - keyboard + mouse  → `suppression` (on-demand low-level hooks, no driver)
//!   - gamepad           → `guard_client` (the privileged scheduled task)
//!
//! The window is opened BEFORE the keystrokes are sent so the emote itself is
//! protected, not just the seconds after it.

use anyhow::Result;

use crate::actions::Action;
use crate::guard_client;
use crate::input;
use crate::suppression;

pub fn play(action: &Action) -> Result<()> {
    if action.input_suppression_ms > 0 {
        log::info!(
            "action {} firing — locking out keyboard/mouse/gamepad for {} ms",
            action.action_id,
            action.input_suppression_ms
        );
        // Keyboard + mouse: instant, in-process, no privileges needed.
        suppression::suppress_for(action.input_suppression_ms);
        // Gamepad: best-effort hand-off to the privileged guard. If the guard
        // isn't installed the emote still fires and keyboard/mouse are still
        // locked — the gamepad just isn't covered.
        guard_client::disable_gamepad(action.input_suppression_ms);
    }

    log::info!(
        "action {} emote keystroke sequence started",
        action.action_id
    );
    input::play(action)?;
    log::info!(
        "action {} emote keystroke sequence completed",
        action.action_id
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::InputStep;

    fn keyboard_action() -> Action {
        Action {
            action_id: "kb".into(),
            label: "kb".into(),
            enabled: true,
            cooldown_ms: 0,
            input_suppression_ms: 0,
            input_sequence: vec![InputStep::Delay { duration_ms: 1 }],
        }
    }

    #[test]
    fn keyboard_action_runs() {
        assert!(play(&keyboard_action()).is_ok());
    }
}
