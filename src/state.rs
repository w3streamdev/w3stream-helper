//! Runtime state: action library, enabled/panic flags, cooldown clocks,
//! idempotency cache.

use log::{info, warn};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::actions::ActionLibrary;
use crate::input;

const IDEMPOTENCY_TTL: Duration = Duration::from_secs(300);

pub struct AppState {
    library: ActionLibrary,
    enabled: bool,
    panic_mode: bool,
    cooldowns: HashMap<String, Instant>,
    idempotency: HashMap<String, (Instant, Value)>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            library: ActionLibrary::load_or_default(),
            enabled: false, // mirrors emote-me's start_enabled = false posture
            panic_mode: false,
            cooldowns: HashMap::new(),
            idempotency: HashMap::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn panic_mode(&self) -> bool {
        self.panic_mode
    }

    pub fn set_enabled(&mut self, on: bool) {
        info!("set_enabled {on}");
        self.enabled = on;
    }

    pub fn panic(&mut self) {
        warn!("panic engaged");
        self.panic_mode = true;
        self.enabled = false;
    }

    pub fn reset_panic(&mut self) {
        info!("panic reset");
        self.panic_mode = false;
    }

    pub fn actions_list(&self) -> Value {
        json!(self
            .library
            .actions
            .iter()
            .map(|a| json!({
                "action_id": a.action_id,
                "label": a.label,
                "enabled": a.enabled,
                "cooldown_ms": a.cooldown_ms,
            }))
            .collect::<Vec<_>>())
    }

    pub fn trigger(&mut self, params: &Value) -> Result<Value, String> {
        if self.panic_mode {
            return Err("panic_mode active".into());
        }
        if !self.enabled {
            return Err("helper disabled".into());
        }

        let action_id = params
            .get("action_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing action_id".to_string())?;

        // Idempotency: if the extension retries with the same request_id we
        // return the cached result instead of double-firing the keystroke.
        let req_id = params
            .get("request_id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        self.prune_idempotency();
        if let Some((_, cached)) = self.idempotency.get(&req_id) {
            return Ok(cached.clone());
        }

        let action = self
            .library
            .find(action_id)
            .cloned()
            .ok_or_else(|| format!("unknown action_id: {action_id}"))?;

        if !action.enabled {
            return Err(format!("action {action_id} disabled in library"));
        }

        // Per-action cooldown
        if let Some(&last) = self.cooldowns.get(action_id) {
            let elapsed = last.elapsed();
            let cooldown = Duration::from_millis(action.cooldown_ms);
            if elapsed < cooldown {
                let remaining = (cooldown - elapsed).as_millis() as u64;
                return Err(format!(
                    "action {action_id} on cooldown ({remaining} ms remaining)"
                ));
            }
        }

        // Execute. SendInput on Windows; no-op on other targets.
        let executed = match input::play(&action) {
            Ok(_) => true,
            Err(e) => {
                warn!("action {action_id} failed: {e}");
                let result = json!({
                    "ok": false,
                    "request_id": req_id,
                    "action_id": action_id,
                    "error": e.to_string(),
                });
                self.idempotency
                    .insert(req_id.clone(), (Instant::now(), result.clone()));
                return Ok(result);
            }
        };

        self.cooldowns.insert(action_id.to_string(), Instant::now());

        let result = json!({
            "ok": true,
            "request_id": req_id,
            "action_id": action_id,
            "status": if executed { "executed" } else { "queued" },
        });
        self.idempotency
            .insert(req_id, (Instant::now(), result.clone()));
        Ok(result)
    }

    fn prune_idempotency(&mut self) {
        self.idempotency
            .retain(|_, (when, _)| when.elapsed() < IDEMPOTENCY_TTL);
    }
}
