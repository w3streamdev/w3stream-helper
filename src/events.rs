//! Outbound event sink — single point any helper code uses to push an
//! unsolicited JSON message to the extension. Wraps stdout in a Mutex so
//! length prefix + body for one message never interleave with another
//! thread's write (the retry-loop thread emits status events concurrently
//! with the main loop's command replies).

use std::io::{stdout, Stdout};
use std::sync::Mutex;

use anyhow::Result;
use once_cell::sync::Lazy;
use serde_json::Value;

use crate::protocol::write_message;

static STDOUT: Lazy<Mutex<Stdout>> = Lazy::new(|| Mutex::new(stdout()));

/// Send a length-prefixed JSON message to the extension. Errors propagate so
/// the caller can exit cleanly when the pipe breaks; background emitters
/// should swallow the error since a transient write failure must not crash
/// the retry loop.
pub fn emit(value: &Value) -> Result<()> {
    let mut out = STDOUT.lock().expect("stdout mutex poisoned");
    write_message(&mut *out, value)
}
