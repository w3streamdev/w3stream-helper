# w3stream-helper

Native helper for the w3stream Agent browser extension. Replaces the
emote-me HTTP service. Single-purpose: receive commands from the extension
via Chrome's Native Messaging API and emit OS-level keystrokes.

## Architecture

```
Twitch chat → cloud worker → w3s-relay (Cloud Run)
                                  │
                                  ▼ WebSocket
                            Browser extension
                                  │
                                  │ chrome.runtime.connectNative('io.connect3.w3stream.helper')
                                  │ length-prefixed JSON over stdio
                                  ▼
                              helper.exe (this crate)
                                  │
                                  ▼ Win32 SendInput
                              Fortnite / any focused window
```

The streamer downloads **one** installer (`w3stream-helper-setup-<ver>.exe`),
double-clicks it, and the extension's "Viewer Input" connector starts
working immediately. No port exposure, no token to paste, no terminal.

## Building

```bash
# Linux/Mac (compiles the protocol layer; SendInput stubs out as a no-op)
cargo build --release
cargo test --release

# Windows
cargo build --release --target x86_64-pc-windows-msvc
```

The installer is built in CI on `windows-latest`:
```bash
makensis -DVERSION=0.1.0 -DEXTENSION_ID=<chrome-extension-id> installer/w3stream-helper.nsi
```

## Wire protocol

Inbound (extension → helper):
```json
{"requestId": "req_123", "command": "trigger",
 "params": {"action_id": "fortnite_emote_1", "request_id": "viewer-abc"}}
```

Outbound (helper → extension):
```json
{"requestId": "req_123",
 "result": {"ok": true, "request_id": "viewer-abc",
            "action_id": "fortnite_emote_1", "status": "executed"}}
```

On every `connect`, the helper sends an unsolicited:
```json
{"type": "hello", "version": "0.1.0", "actions": [...]}
```

Commands: `health`, `actions.list`, `enabled`, `panic`, `panic.reset`, `trigger`.

## Action config

`%LOCALAPPDATA%\w3stream\actions.json` — same shape as `emote-me/config.json`.
Helper seeds defaults on first run; future versions will sync this from the
cloud.
