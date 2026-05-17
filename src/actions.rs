//! Action library — same shape as emote-me's config.json so existing actions
//! can be ported by copying their config. Eventually the cloud will push
//! action definitions, so we keep this struct shape stable on the wire.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub input_sequence: Vec<InputStep>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ActionLibrary {
    pub actions: Vec<Action>,
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
                    input_sequence: vec![
                        InputStep::KeyTap { key: "H".into(), duration_ms: 80 },
                        InputStep::KeyTap { key: "I".into(), duration_ms: 80 },
                    ],
                },
                Action {
                    action_id: "fortnite_emote_1".into(),
                    label: "Fortnite Emote 1".into(),
                    enabled: false,
                    cooldown_ms: 60_000,
                    input_sequence: vec![
                        InputStep::KeyTap { key: "B".into(), duration_ms: 80 },
                        InputStep::Delay { duration_ms: 120 },
                        InputStep::KeyTap { key: "1".into(), duration_ms: 80 },
                    ],
                },
            ],
        }
    }

    pub fn load_or_default() -> Self {
        let path = Self::config_path();
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(lib) = serde_json::from_slice::<ActionLibrary>(&bytes) {
                return lib;
            }
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
