mod actions;
mod activity;
mod emote_retry;
mod events;
mod executor;
mod guard_client;
mod input;
mod protocol;
mod state;
mod suppression;

use anyhow::Result;
use log::{error, info};
use serde_json::{json, Value};
use simplelog::{ConfigBuilder, LevelFilter, WriteLogger};
use std::fs::OpenOptions;
use std::io::stdin;
use std::path::PathBuf;

use crate::protocol::read_message;
use crate::state::AppState;

fn log_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        let mut p = PathBuf::from(dir);
        p.push("w3stream");
        let _ = std::fs::create_dir_all(&p);
        p.push("helper.log");
        return p;
    }
    PathBuf::from("w3stream-helper.log")
}

fn init_logging() {
    let path = log_path();
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = WriteLogger::init(
            LevelFilter::Info,
            ConfigBuilder::new().set_time_format_rfc3339().build(),
            file,
        );
    }
}

fn main() -> Result<()> {
    init_logging();
    info!("w3stream-helper {} starting", env!("CARGO_PKG_VERSION"));

    let mut state = AppState::new();

    let mut stdin = stdin().lock();

    // Send a hello so the extension knows the helper is alive + which version.
    let hello = json!({
        "type": "hello",
        "version": env!("CARGO_PKG_VERSION"),
        "actions": state.actions_list(),
        "input_guard": { "available": guard_client::guard_available() },
    });
    if let Err(e) = events::emit(&hello) {
        error!("failed to send hello: {e}");
        return Err(e);
    }

    loop {
        match read_message(&mut stdin) {
            Ok(None) => {
                info!("extension closed stdin, exiting");
                break;
            }
            Ok(Some(msg)) => {
                let reply = handle(&mut state, msg);
                if let Err(e) = events::emit(&reply) {
                    error!("write failed: {e}; exiting");
                    break;
                }
            }
            Err(e) => {
                error!("read error: {e}; exiting");
                break;
            }
        }
    }
    Ok(())
}

fn handle(state: &mut AppState, msg: Value) -> Value {
    let request_id = msg.get("requestId").cloned().unwrap_or(Value::Null);
    let command = msg
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let params = msg.get("params").cloned().unwrap_or(json!({}));

    let result = match command.as_str() {
        "health" => Ok(json!({
            "status": "running",
            "enabled": state.enabled(),
            "panic_mode": state.panic_mode(),
            "queue_size": 0,
            "version": env!("CARGO_PKG_VERSION"),
            "input_guard": { "available": guard_client::guard_available() },
        })),
        "actions.list" => Ok(json!({ "actions": state.actions_list() })),
        "enabled" => {
            let enabled = params
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            state.set_enabled(enabled);
            Ok(json!({ "ok": true, "enabled": enabled }))
        }
        "panic" => {
            state.panic();
            Ok(json!({ "ok": true, "panic_mode": true }))
        }
        "panic.reset" => {
            state.reset_panic();
            Ok(json!({ "ok": true, "panic_mode": false }))
        }
        "trigger" => state.trigger(&params),
        other => Err(format!("unknown command: {other}")),
    };

    match result {
        Ok(value) => json!({ "requestId": request_id, "result": value }),
        Err(e) => json!({ "requestId": request_id, "error": e }),
    }
}
