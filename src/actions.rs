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
    GamepadButtonTap {
        button: GamepadButton,
        #[serde(default = "default_tap_ms")]
        duration_ms: u64,
    },
    GamepadDpad {
        direction: DpadDir,
        #[serde(default = "default_tap_ms")]
        duration_ms: u64,
    },
    /// Explicit forwarder suspend. The executor also auto-suspends for
    /// the duration of any action that contains gamepad steps, but this
    /// lets authors pad the window (e.g. to absorb the streamer's
    /// reaction time after the emote wheel opens).
    SuspendForwarder {
        duration_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GamepadButton {
    A,
    B,
    X,
    Y,
    LB,
    RB,
    LStick,
    RStick,
    Back,
    Start,
    Guide,
}

impl GamepadButton {
    pub fn bit(self) -> u16 {
        use crate::gamepad::buttons::*;
        match self {
            Self::A => A,
            Self::B => B,
            Self::X => X,
            Self::Y => Y,
            Self::LB => LB,
            Self::RB => RB,
            Self::LStick => LTHUMB,
            Self::RStick => RTHUMB,
            Self::Back => BACK,
            Self::Start => START,
            Self::Guide => GUIDE,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DpadDir {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl DpadDir {
    pub fn bits(self) -> u16 {
        use crate::gamepad::buttons::*;
        match self {
            Self::Up => DPAD_UP,
            Self::Down => DPAD_DOWN,
            Self::Left => DPAD_LEFT,
            Self::Right => DPAD_RIGHT,
            Self::UpLeft => DPAD_UP | DPAD_LEFT,
            Self::UpRight => DPAD_UP | DPAD_RIGHT,
            Self::DownLeft => DPAD_DOWN | DPAD_LEFT,
            Self::DownRight => DPAD_DOWN | DPAD_RIGHT,
        }
    }
}

fn default_tap_ms() -> u64 {
    80
}

impl InputStep {
    /// Conservative upper bound on the wall-clock duration of this step.
    /// Used by the executor to pre-compute the auto-suspend window for
    /// actions that contain any gamepad step.
    pub fn duration_ms(&self) -> u64 {
        match self {
            Self::KeyTap { duration_ms, .. } => *duration_ms,
            Self::KeyDown { .. } | Self::KeyUp { .. } => 0,
            Self::Delay { duration_ms } => *duration_ms,
            Self::GamepadButtonTap { duration_ms, .. } => *duration_ms,
            Self::GamepadDpad { duration_ms, .. } => *duration_ms,
            Self::SuspendForwarder { duration_ms } => *duration_ms,
        }
    }

    pub fn is_gamepad(&self) -> bool {
        matches!(
            self,
            Self::GamepadButtonTap { .. } | Self::GamepadDpad { .. }
        )
    }
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
                    enabled: false,
                    cooldown_ms: 60_000,
                    // Controller-mode emote: open wheel with D-pad Down,
                    // wait for the radial to render, then pick slot 1 (A).
                    // SuspendForwarder is explicit here so authors can see
                    // the gate; the executor also auto-suspends as a
                    // safety net.
                    input_sequence: vec![
                        InputStep::SuspendForwarder { duration_ms: 3000 },
                        InputStep::GamepadDpad {
                            direction: DpadDir::Down,
                            duration_ms: 200,
                        },
                        InputStep::Delay { duration_ms: 150 },
                        InputStep::GamepadButtonTap {
                            button: GamepadButton::A,
                            duration_ms: 80,
                        },
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
