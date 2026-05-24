# w3stream-helper

Native helper for the w3stream Agent browser extension. Replaces the
emote-me HTTP service. Single-purpose: receive commands from the extension
via Chrome's Native Messaging API and emit OS-level keystrokes or virtual
Xbox controller input.

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
                                  ▼ Win32 SendInput / ViGEm virtual gamepad
                              Fortnite / any focused window
                                  │
                                  ▼ temporary input suppression, then restore
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

### Unsolicited events

While a movement-gated emote is in-flight the helper pushes status events on
each state transition. The extension can forward these to the relay so an
overlay (separate deploy) can render a countdown ring / re-fire flashes
without polling.

```json
{
  "type": "emote_retry",
  "phase": "started",
  "emote_id": "fortnite_emote_1",
  "idle_for_ms": 0,
  "idle_required_ms": 5000,
  "running_for_ms": 0,
  "max_duration_ms": 60000,
  "time_remaining_ms": 5000
}
```

`phase` is one of:

- `started` — emote just fired; overlay should show the countdown bar at
  `idle_required_ms`.
- `input_active` — non-neutral controller input detected; overlay resets
  countdown to `idle_required_ms` (the helper has reset its idle timer).
- `retry` — the emote keystroke sequence has been re-fired; overlay can
  flash a re-fire indicator.
- `idle_satisfied` — streamer was idle long enough; overlay shows landed
  state and fades out.
- `timeout` — `max_duration_ms` reached without idle; overlay shows
  warning state and fades out.
- `superseded` — a new emote request replaced this one; overlay can clean
  up the old countdown.

Events fire only on state transitions; the overlay interpolates the
countdown locally between events using its own clock.

## Fortnite emote input suppression

Fortnite emote actions can set `input_suppression_ms` in their action config.
The built-in `fortnite_emote_1` action defaults this to `5000`, so the helper
sends the emote input sequence first and then starts a short suppression window.
During that window:

- controller input is handled by keeping the physical-to-virtual gamepad
  forwarder suspended while direct virtual-pad emote input remains available;
- on Windows, short-lived low-level hooks suppress common movement/cancel
  keyboard input (`WASD`, arrows, space, shift, ctrl) and mouse movement/clicks;
- injected helper input is not suppressed, and the hooks are removed
  automatically when the deadline expires.

The suppression logic is deliberately temporary and auditable: it does not
install drivers, services, persistence, telemetry, or stealth behavior. Multiple
rapid emote triggers only extend the in-memory deadline, and process exit drops
all hooks/devices.

## Movement-gated emote retry

When `emoteRetry.enabled` is `true` (the default), an emote request is no
longer one-shot. The helper fires the emote immediately and then assumes the
physical Xbox controller is idle, counting down `idleRequiredMs`. Any
non-neutral controller input resets that countdown *and* re-fires the emote
(throttled by `retryIntervalMs` so a held stick can't spam keystrokes); an
idle controller is left alone so the emote can land. Controller state is
sampled with the read-only `XInputGetState` API — no driver, no device
changes, no input suppression. Once the streamer has been continuously idle
for `idleRequiredMs` the emote is considered landed and the loop stops;
`maxDurationMs` caps it so it can never run forever. A new emote request
replaces any in-flight one. Set `emoteRetry.enabled` to `false` to keep the
legacy one-shot + input-suppression behavior.

`%LOCALAPPDATA%\w3stream\actions.json` carries the settings under `emoteRetry`:

```json
{
  "emoteRetry": {
    "enabled": true,
    "idleRequiredMs": 5000,
    "retryIntervalMs": 750,
    "stickDeadzone": 0.15,
    "triggerDeadzone": 0.10,
    "maxDurationMs": 60000
  }
}
```

Tiny analog drift inside `stickDeadzone` / `triggerDeadzone` is ignored so it
can't keep the loop alive forever.

## Action config

`%LOCALAPPDATA%\w3stream\actions.json` — same shape as `emote-me/config.json`.
Helper seeds defaults on first run; future versions will sync this from the
cloud. Legacy configs that omit `input_suppression_ms` default to `0` (no
keyboard/mouse suppression), except the old seeded `fortnite_emote_1` entry is
migrated to enabled + `5000` ms so existing installs regain the intended emote
flow.
