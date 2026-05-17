mod protocol;
mod actions;
mod input;
mod state;

use anyhow::Result;
use log::{error, info};
use serde_json::{json, Value};
use simplelog::{ConfigBuilder, LevelFilter, WriteLogger};
use std::fs::OpenOptions;
use std::io::{stdin, stdout};
use std::path::PathBuf;

use crate::protocol::{read_message, write_message};
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
    let mut stdout = stdout().lock();

    // Send a hello so the extension knows the helper is alive + which version.
    let hello = json!({
        "type": "hello",
        "version": env!("CARGO_PKG_VERSION"),
        "actions": state.actions_list(),
    });
    if let Err(e) = write_message(&mut stdout, &hello) {
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
                if let Err(e) = write_message(&mut stdout, &reply) {
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
        })),
        "actions.list" => Ok(json!({ "actions": state.actions_list() })),
        "enabled" => {
            let enabled = params.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
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
