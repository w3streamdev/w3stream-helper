//! Movement-gated emote retry loop.
//!
//! An emote chat command can land while the streamer is actively moving,
//! fighting or aiming. Fortnite then cancels the emote, so the chat command
//! succeeds on the wire but nothing plays in-game.
//!
//! Instead of giving up after one attempt, an emote request here starts a
//! background retry loop: it fires the emote immediately and then assumes the
//! controller is idle, counting down `idle_required_ms`. Any non-neutral
//! controller input resets that countdown and re-fires the emote (throttled
//! by `retry_interval_ms`); an idle controller is left alone so the emote can
//! land. Once the streamer has been idle for a continuous `idle_required_ms`
//! window the emote is considered landed and the loop stops. A
//! `max_duration_ms` cap guarantees the loop can never run forever.
//!
//! Input is watched on two surfaces so the feature works whether the
//! streamer plays on controller or on keyboard + mouse:
//!   - Gamepad — `XInputGetState`, a read-only built-in Windows API.
//!   - Keyboard + mouse — low-level `WH_KEYBOARD_LL` / `WH_MOUSE_LL`
//!     observer hooks in `activity`. Hooks only observe, never block, and
//!     filter `LLKHF_INJECTED` / `LLMHF_INJECTED` so the helper's own
//!     re-fired keystrokes don't count as user activity.
//!
//! Nothing here installs drivers, suppresses the streamer's input, or
//! modifies the physical pad. On non-Windows builds both sources yield
//! `None`, which is treated as idle so the loop self-terminates.

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use log::info;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::actions::Action;
use crate::activity;
use crate::events;
use crate::input;

/// How often the retry loop wakes to sample the controller. Small enough to
/// notice the streamer going idle promptly without busy-spinning.
const TICK: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmoteRetryConfig {
    /// Enables the movement-gated retry behavior. When false the helper keeps
    /// the old one-shot emote path.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Continuous idle window the streamer must reach before retries stop.
    #[serde(default = "default_idle_required_ms")]
    pub idle_required_ms: u64,
    /// Spacing between emote retry attempts while waiting for idle.
    #[serde(default = "default_retry_interval_ms")]
    pub retry_interval_ms: u64,
    /// Minimum stick magnitude (0..1) that counts as real input.
    #[serde(default = "default_stick_deadzone")]
    pub stick_deadzone: f32,
    /// Minimum trigger value (0..1) that counts as real input.
    #[serde(default = "default_trigger_deadzone")]
    pub trigger_deadzone: f32,
    /// Hard safety cap on total retry-loop runtime.
    #[serde(default = "default_max_duration_ms")]
    pub max_duration_ms: u64,
}

fn default_enabled() -> bool {
    true
}
fn default_idle_required_ms() -> u64 {
    5000
}
fn default_retry_interval_ms() -> u64 {
    750
}
fn default_stick_deadzone() -> f32 {
    0.15
}
fn default_trigger_deadzone() -> f32 {
    0.10
}
fn default_max_duration_ms() -> u64 {
    60_000
}

impl Default for EmoteRetryConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            idle_required_ms: default_idle_required_ms(),
            retry_interval_ms: default_retry_interval_ms(),
            stick_deadzone: default_stick_deadzone(),
            trigger_deadzone: default_trigger_deadzone(),
            max_duration_ms: default_max_duration_ms(),
        }
    }
}

/// A single sample of physical controller state, normalized so the activity
/// check is platform-agnostic. Sticks and triggers are 0..1 magnitudes.
#[derive(Debug, Clone, Copy, Default)]
pub struct GamepadSnapshot {
    /// Raw XInput button bitfield — d-pad, bumpers, face buttons, stick
    /// clicks and start/back all live here. Non-zero means a button is held.
    pub buttons: u16,
    pub left_trigger: f32,
    pub right_trigger: f32,
    pub left_stick: (f32, f32),
    pub right_stick: (f32, f32),
}

fn stick_magnitude(stick: (f32, f32)) -> f32 {
    let (x, y) = stick;
    (x * x + y * y).sqrt()
}

/// True if any button, d-pad, trigger or stick is outside the configured
/// deadzone — i.e. the streamer is actively providing controller input.
pub fn is_gamepad_input_active(snapshot: &GamepadSnapshot, config: &EmoteRetryConfig) -> bool {
    if snapshot.buttons != 0 {
        return true;
    }
    if snapshot.left_trigger >= config.trigger_deadzone
        || snapshot.right_trigger >= config.trigger_deadzone
    {
        return true;
    }
    stick_magnitude(snapshot.left_stick) >= config.stick_deadzone
        || stick_magnitude(snapshot.right_stick) >= config.stick_deadzone
}

/// Generation counter. Every new emote request bumps it; a running retry
/// loop exits as soon as it sees a newer generation, so a fresh request
/// cleanly replaces any pending one.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Begin a movement-gated emote request.
///
/// Fires the emote once immediately (synchronously, so the caller learns of a
/// keystroke failure) and then spawns the retry loop. A new call supersedes
/// any in-flight loop. Returns the result of the immediate attempt.
pub fn on_emote_requested(action: Action, config: EmoteRetryConfig) -> anyhow::Result<()> {
    // Make sure the keyboard/mouse observer is running so the retry loop
    // can detect kbm activity. Idempotent — first call spawns the thread.
    activity::start_observer();

    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let emote_id = action.action_id.clone();

    let attempt = input::play(&action);
    info!("[emote-retry] started emote={emote_id}");
    emit_event("started", &emote_id, 0, 0, &config);

    let _ = thread::Builder::new()
        .name("w3stream-emote-retry".into())
        .spawn(move || retry_loop(generation, action, config));

    attempt
}

/// Push a retry-loop status event to the extension. The overlay listens for
/// `type=emote_retry` and uses `phase` + `time_remaining_ms` to drive the
/// countdown ring; `idle_required_ms` lets it size the ring correctly on the
/// first event without waiting for a separate config push.
fn emit_event(
    phase: &str,
    emote_id: &str,
    idle_for_ms: u64,
    running_for_ms: u64,
    config: &EmoteRetryConfig,
) {
    let time_remaining_ms = config.idle_required_ms.saturating_sub(idle_for_ms);
    let _ = events::emit(&json!({
        "type": "emote_retry",
        "phase": phase,
        "emote_id": emote_id,
        "idle_for_ms": idle_for_ms,
        "idle_required_ms": config.idle_required_ms,
        "running_for_ms": running_for_ms,
        "max_duration_ms": config.max_duration_ms,
        "time_remaining_ms": time_remaining_ms,
    }));
}

fn retry_loop(generation: u64, action: Action, config: EmoteRetryConfig) {
    let emote_id = action.action_id.as_str();
    let started_at = Instant::now();
    // The immediate attempt in `on_emote_requested` just happened.
    let mut last_attempt_at = started_at;
    let mut last_input_at = started_at;
    let mut was_active = false;
    // Snapshot the kbm activity counter so the first tick only treats
    // events that happen AFTER the loop starts as user activity.
    let mut last_kbm_seen = activity::last_activity_ms().unwrap_or(0);

    loop {
        // A newer emote request has taken over — emit a final superseded
        // event so the overlay can fade its old countdown, then drop this
        // loop.
        if GENERATION.load(Ordering::Acquire) != generation {
            let now = Instant::now();
            let idle_for_ms = now.duration_since(last_input_at).as_millis() as u64;
            let running_for_ms = now.duration_since(started_at).as_millis() as u64;
            info!("[emote-retry] superseded emote={emote_id} runningForMs={running_for_ms}");
            emit_event("superseded", emote_id, idle_for_ms, running_for_ms, &config);
            return;
        }

        thread::sleep(TICK);
        let now = Instant::now();

        // Watch BOTH input surfaces — gamepad and keyboard/mouse — so the
        // emote retry works whether the streamer is on a controller or on
        // KB+M. Keyboard/mouse activity comes from the global observer in
        // `activity`, which filters out our own injected re-fires.
        let gamepad_active = poll_gamepad()
            .map(|snapshot| is_gamepad_input_active(&snapshot, &config))
            .unwrap_or(false);
        let current_kbm = activity::last_activity_ms().unwrap_or(0);
        let kbm_active = current_kbm > last_kbm_seen;
        last_kbm_seen = current_kbm;
        let input_active = gamepad_active || kbm_active;

        // An idle controller is assumed to mean the emote can land, so the
        // idle countdown is left to run. Any non-neutral input resets that
        // countdown AND re-fires the emote; retry_interval_ms throttles the
        // re-fire so a held stick can't spam the keystroke sequence.
        if input_active {
            last_input_at = now;
            if !was_active {
                info!("[emote-retry] input active; idle timer reset");
                let running_for_ms = now.duration_since(started_at).as_millis() as u64;
                emit_event("input_active", emote_id, 0, running_for_ms, &config);
            }
            if now.duration_since(last_attempt_at).as_millis() as u64
                >= config.retry_interval_ms
            {
                if let Err(e) = input::play(&action) {
                    log::warn!("[emote-retry] retry emote={emote_id} attempt failed: {e}");
                }
                last_attempt_at = now;
                info!("[emote-retry] retry emote={emote_id}");
                let running_for_ms = now.duration_since(started_at).as_millis() as u64;
                emit_event("retry", emote_id, 0, running_for_ms, &config);
            }
        }
        was_active = input_active;

        let idle_for_ms = now.duration_since(last_input_at).as_millis() as u64;
        let running_for_ms = now.duration_since(started_at).as_millis() as u64;

        if running_for_ms >= config.max_duration_ms {
            info!("[emote-retry] timeout emote={emote_id}");
            emit_event("timeout", emote_id, idle_for_ms, running_for_ms, &config);
            return;
        }

        if idle_for_ms >= config.idle_required_ms {
            info!("[emote-retry] idle satisfied emote={emote_id} idleForMs={idle_for_ms}");
            emit_event("idle_satisfied", emote_id, idle_for_ms, running_for_ms, &config);
            return;
        }
    }
}

/// Read the first connected controller's state. `None` when no controller is
/// connected (or off Windows) — the caller treats that as idle.
#[cfg(windows)]
fn poll_gamepad() -> Option<GamepadSnapshot> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};

    for user_index in 0..4u32 {
        let mut state = XINPUT_STATE::default();
        let result = unsafe { XInputGetState(user_index, &mut state) };
        if result != ERROR_SUCCESS.0 {
            continue;
        }
        let pad = state.Gamepad;
        return Some(GamepadSnapshot {
            buttons: pad.wButtons.0,
            left_trigger: pad.bLeftTrigger as f32 / 255.0,
            right_trigger: pad.bRightTrigger as f32 / 255.0,
            left_stick: normalize_stick(pad.sThumbLX, pad.sThumbLY),
            right_stick: normalize_stick(pad.sThumbRX, pad.sThumbRY),
        });
    }
    None
}

/// Map a raw XInput thumbstick axis pair (-32768..32767) to a -1.0..1.0 pair.
#[cfg(windows)]
fn normalize_stick(x: i16, y: i16) -> (f32, f32) {
    ((x as f32 / 32767.0).clamp(-1.0, 1.0), (y as f32 / 32767.0).clamp(-1.0, 1.0))
}

#[cfg(not(windows))]
fn poll_gamepad() -> Option<GamepadSnapshot> {
    // No XInput off Windows — treated as "no controller", i.e. idle.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_match_prd() {
        let cfg = EmoteRetryConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.idle_required_ms, 5000);
        assert_eq!(cfg.retry_interval_ms, 750);
        assert_eq!(cfg.stick_deadzone, 0.15);
        assert_eq!(cfg.trigger_deadzone, 0.10);
        assert_eq!(cfg.max_duration_ms, 60_000);
    }

    #[test]
    fn partial_config_fills_in_defaults() {
        let cfg: EmoteRetryConfig =
            serde_json::from_value(serde_json::json!({ "idleRequiredMs": 2000 })).unwrap();
        assert_eq!(cfg.idle_required_ms, 2000);
        assert_eq!(cfg.retry_interval_ms, 750);
        assert!(cfg.enabled);
    }

    #[test]
    fn neutral_controller_is_idle() {
        let cfg = EmoteRetryConfig::default();
        assert!(!is_gamepad_input_active(&GamepadSnapshot::default(), &cfg));
    }

    #[test]
    fn tiny_stick_drift_inside_deadzone_is_idle() {
        let cfg = EmoteRetryConfig::default();
        let snapshot = GamepadSnapshot {
            left_stick: (0.10, 0.05),
            ..GamepadSnapshot::default()
        };
        assert!(!is_gamepad_input_active(&snapshot, &cfg));
    }

    #[test]
    fn stick_past_deadzone_is_active() {
        let cfg = EmoteRetryConfig::default();
        let snapshot = GamepadSnapshot {
            right_stick: (0.5, 0.4),
            ..GamepadSnapshot::default()
        };
        assert!(is_gamepad_input_active(&snapshot, &cfg));
    }

    #[test]
    fn small_trigger_pressure_inside_deadzone_is_idle() {
        let cfg = EmoteRetryConfig::default();
        let snapshot = GamepadSnapshot {
            left_trigger: 0.05,
            ..GamepadSnapshot::default()
        };
        assert!(!is_gamepad_input_active(&snapshot, &cfg));
    }

    #[test]
    fn trigger_past_deadzone_is_active() {
        let cfg = EmoteRetryConfig::default();
        let snapshot = GamepadSnapshot {
            right_trigger: 0.5,
            ..GamepadSnapshot::default()
        };
        assert!(is_gamepad_input_active(&snapshot, &cfg));
    }

    #[test]
    fn any_button_is_active() {
        let cfg = EmoteRetryConfig::default();
        let snapshot = GamepadSnapshot {
            buttons: 0x1000, // XINPUT_GAMEPAD_A
            ..GamepadSnapshot::default()
        };
        assert!(is_gamepad_input_active(&snapshot, &cfg));
    }
}
