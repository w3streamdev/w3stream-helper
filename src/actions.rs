//! Action library — same shape as emote-me's config.json so existing actions
//! can be ported by copying their config. Eventually the cloud will push
//! action definitions, so we keep this struct shape stable on the wire.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

use crate::emote_retry::EmoteRetryConfig;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputStep {
    KeyTap {
        key: String,
        #[serde(default = "default_tap_ms")]
        duration_ms: u64,
    },
    KeyDown {
        key: String,
    },
    KeyUp {
        key: String,
    },
    Delay {
        duration_ms: u64,
    },
}

fn default_tap_ms() -> u64 {
    80
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Action {
    pub action_id: String,
    pub label: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub cooldown_ms: u64,
    /// Input-lockout window opened the instant this action fires. For
    /// Fortnite emotes this freezes the streamer's OWN keyboard, mouse, and
    /// gamepad for the duration so their movement can't cancel the emote.
    #[serde(default)]
    pub input_suppression_ms: u64,
    pub input_sequence: Vec<InputStep>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ActionLibrary {
    pub actions: Vec<Action>,
    /// Movement-gated emote retry settings. Defaults are filled in for
    /// configs written before this section existed.
    #[serde(rename = "emoteRetry", default)]
    pub emote_retry: EmoteRetryConfig,
}

impl ActionLibrary {
    /// Built-in defaults so the helper works out of the box on first run
    /// before the cloud has pushed any config. Mirrors emote-me's seed config.
    pub fn defaults() -> Self {
        Self {
            actions: vec![
                Action {
                    action_id: "test_type_hi".into(),
                    label: "Test Type HI".into(),
                    enabled: true,
                    cooldown_ms: 5000,
                    input_suppression_ms: 0,
                    input_sequence: vec![
                        InputStep::KeyTap {
                            key: "H".into(),
                            duration_ms: 80,
                        },
                        InputStep::KeyTap {
                            key: "I".into(),
                            duration_ms: 80,
                        },
                    ],
                },
                Action {
                    action_id: "fortnite_emote_1".into(),
                    label: "Fortnite Emote 1".into(),
                    enabled: true,
                    cooldown_ms: 60_000,
                    input_suppression_ms: 5000,
                    // Keyboard emote: tap B to open the emote wheel, wait
                    // for the radial to render, then tap 1 to pick slot 1.
                    // input_suppression_ms freezes the streamer's OWN
                    // keyboard/mouse/gamepad for 5 s while this plays so
                    // their movement can't cancel the emote mid-animation.
                    input_sequence: vec![
                        InputStep::KeyTap {
                            key: "B".into(),
                            duration_ms: 80,
                        },
                        InputStep::Delay { duration_ms: 120 },
                        InputStep::KeyTap {
                            key: "1".into(),
                            duration_ms: 80,
                        },
                    ],
                },
            ],
            emote_retry: EmoteRetryConfig::default(),
        }
    }

    pub fn load_or_default() -> Self {
        let path = Self::config_path();
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(raw) = serde_json::from_slice::<Value>(&bytes) {
                if let Ok(mut lib) = serde_json::from_value::<ActionLibrary>(raw.clone()) {
                    if Self::migrate_legacy_defaults(&mut lib, &raw) {
                        if let Ok(bytes) = serde_json::to_vec_pretty(&lib) {
                            let _ = std::fs::write(&path, bytes);
                        }
                    }
                    return lib;
                }
            }
            // A config we cannot parse — e.g. a legacy gamepad actions.json
            // whose step types (gamepad_dpad, suspend_forwarder, …) no longer
            // exist — falls through to the keyboard defaults written below.
            // This is the migration: the streamer never edits config by hand.
        }
        let lib = Self::defaults();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(&lib) {
            let _ = std::fs::write(&path, bytes);
        }
        lib
    }

    /// Patch older configs that parsed cleanly but predate a field. Currently
    /// only fills in `input_suppression_ms` for a `fortnite_emote_1` that was
    /// seeded before that field existed.
    fn migrate_legacy_defaults(lib: &mut ActionLibrary, raw: &Value) -> bool {
        let mut changed = false;
        let raw_actions = raw
            .get("actions")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        for action in &mut lib.actions {
            if action.action_id != "fortnite_emote_1" {
                continue;
            }

            let raw_action = raw_actions.iter().find(|candidate| {
                candidate.get("action_id").and_then(Value::as_str) == Some("fortnite_emote_1")
            });
            let missing_suppression = raw_action
                .and_then(Value::as_object)
                .map(|object| !object.contains_key("input_suppression_ms"))
                .unwrap_or(true);

            if missing_suppression && action.input_suppression_ms == 0 {
                action.input_suppression_ms = 5000;
                changed = true;
            }

            if missing_suppression && !action.enabled {
                action.enabled = true;
                changed = true;
            }
        }

        changed
    }

    pub fn config_path() -> PathBuf {
        if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
            let mut p = PathBuf::from(dir);
            p.push("w3stream");
            p.push("actions.json");
            return p;
        }
        PathBuf::from("actions.json")
    }

    pub fn find(&self, action_id: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.action_id == action_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fortnite_emote_is_keyboard_with_suppression() {
        let library = ActionLibrary::defaults();
        let action = library.find("fortnite_emote_1").unwrap();

        assert!(action.enabled);
        assert_eq!(action.input_suppression_ms, 5000);
        // The emote fires as keystrokes — every step is keyboard.
        assert!(action
            .input_sequence
            .iter()
            .all(|s| matches!(s, InputStep::KeyTap { .. } | InputStep::Delay { .. })));
        assert!(action
            .input_sequence
            .iter()
            .any(|s| matches!(s, InputStep::KeyTap { .. })));
    }

    #[test]
    fn missing_suppression_field_defaults_to_zero_for_legacy_configs() {
        let action: Action = serde_json::from_value(serde_json::json!({
            "action_id": "legacy",
            "label": "Legacy",
            "enabled": true,
            "cooldown_ms": 0,
            "input_sequence": [{"type": "delay", "duration_ms": 1}]
        }))
        .unwrap();

        assert_eq!(action.input_suppression_ms, 0);
    }

    #[test]
    fn legacy_gamepad_config_fails_to_parse_so_defaults_take_over() {
        // Old gamepad actions.json: these step types no longer exist, so
        // deserialization MUST fail. load_or_default() then falls back to
        // the keyboard defaults — that is the auto-migration.
        let raw = serde_json::json!({
            "actions": [{
                "action_id": "fortnite_emote_1",
                "label": "Fortnite Emote 1",
                "enabled": true,
                "cooldown_ms": 60000,
                "input_sequence": [
                    {"type": "suspend_forwarder", "duration_ms": 3000},
                    {"type": "gamepad_dpad", "direction": "down", "duration_ms": 200},
                    {"type": "gamepad_button_tap", "button": "a", "duration_ms": 80}
                ]
            }]
        });
        assert!(serde_json::from_value::<ActionLibrary>(raw).is_err());
    }

    #[test]
    fn migrates_missing_suppression_on_keyboard_config() {
        let raw = serde_json::json!({
            "actions": [{
                "action_id": "fortnite_emote_1",
                "label": "Fortnite Emote 1",
                "enabled": false,
                "cooldown_ms": 60000,
                "input_sequence": [
                    {"type": "key_tap", "key": "B", "duration_ms": 80}
                ]
            }]
        });
        let mut library: ActionLibrary = serde_json::from_value(raw.clone()).unwrap();

        assert!(ActionLibrary::migrate_legacy_defaults(&mut library, &raw));
        let action = library.find("fortnite_emote_1").unwrap();
        assert!(action.enabled);
        assert_eq!(action.input_suppression_ms, 5000);
    }
}
