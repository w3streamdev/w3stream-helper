# TODO: Gamepad suppression + virtual-pad emote injection

**Status:** spec only — not started.
**Owner:** unassigned. Pick this up if you're the next agent on this repo.
**Scope estimate:** 1–2 focused days. Most of it is driver bundling, signing, and
Windows-side install testing. The Rust is the smallest part.

Read `/home/td/.claude/plans/soft-brewing-gem.md` (the viewer-input post-mortem)
before starting. The "verify end-to-end before claiming done" rule applies
doubly here — a half-built version of this looks identical from outside to a
finished one, and the owner has been burned by that exact pattern.

---

## Why this exists

The helper currently fires keyboard inputs via Win32 SendInput. Streamers play
Fortnite (and similar) on Xbox controller. When a chat-triggered emote fires,
the streamer's stick is non-neutral, so the character keeps moving and Fortnite
cancels the emote. Result: chat command runs, helper reports `ok=true`, nothing
visible happens in the game. From the streamer's perspective, the feature is
broken.

We need to:

1. Suppress the streamer's physical controller during the emote duration.
2. Inject the emote as a *controller* input (Fortnite uses D-pad + face buttons
   for the emote wheel when in controller mode — keyboard inputs don't help).

## The architecture (the reWASD model)

This is the only userland approach that actually works on EAC-protected games:

1. **HidHide kernel driver** (bundled in installer). After install, the
   streamer's physical Xbox controller is **hidden from Fortnite's process**.
   Fortnite no longer sees it at all.
2. **ViGEmBus kernel driver** (bundled in installer). Helper creates an
   emulated Xbox 360 controller via ViGEm. Fortnite sees ONLY the virtual one.
3. **Forwarder loop** in the helper: reads physical XInput state from the
   (hidden-from-Fortnite-but-visible-to-us) physical pad and writes the same
   state to the virtual ViGEm pad at ~250 Hz. Streamer experiences normal play.
4. **Suspend during emote:** when a chat-triggered emote fires:
   - Forwarder enters "suspend" mode for N ms (per-action configurable, default
     3000–5000 ms).
   - During suspend: stop forwarding physical → virtual. Virtual pad receives
     ONLY the emote button sequence (D-pad Down opens the emote wheel, then
     A/B/X/Y picks a slot).
   - Resume forwarding.

Net effect: streamer can be holding the stick like crazy, but during the emote
window the virtual pad reports neutral stick + emote button press, because the
forwarder isn't relaying their input. After the emote completes, forwarding
resumes mid-stride and the character moves again.

## Deliverables (ship each as its own commit)

### A — Rust deps
- `Cargo.toml`: add `vigem-client` (https://crates.io/crates/vigem-client),
  latest version pinned. Use `windows` crate's `Win32::UI::Input::XboxController`
  for reading physical XInput (already a dep, no new crate needed).

### B — `src/gamepad.rs` (new)
- `struct VirtualPad` owns a `vigem_client::Client` + `Xbox360Wired` target.
- Methods: `new() -> Result<VirtualPad>`, `set_state(report: XUSBReport)`,
  `press_button(button, ms)`, `Drop` impl that cleanly removes the virtual pad.
- `XUSBReport` mirrors `XINPUT_GAMEPAD` (button bitfield + thumbsticks i16x4 +
  triggers u8x2).
- "ViGEmBus driver not installed" must return `Err` with a clear message — the
  helper still works for keystroke-only actions.

### C — `src/forwarder.rs` (new)
- Background thread, ~250 Hz, reads `XInputGetState` for pads 0/1/2/3.
- Forwards to `VirtualPad.set_state()` unless suspend flag is set.
- Public API: `suspend(duration_ms)`, `resume()`, `is_suspended()`.
- Use `Arc<AtomicU64>` for "suspended until (epoch ms)" — lock-free hot path.

### D — Extend `src/actions.rs`
- New `InputStep` variants:
  - `GamepadButtonTap { button: GamepadButton, duration_ms: u64 }`
  - `GamepadDpad { direction: DpadDir, duration_ms: u64 }`
  - `SuspendForwarder { duration_ms: u64 }` (explicit; lets actions choose
    their own suspend window)
- `GamepadButton`: A, B, X, Y, LB, RB, LStick, RStick, Back, Start, Guide.
- `DpadDir`: Up, Down, Left, Right (and combinations if needed).
- Update `defaults()`: rewrite `fortnite_emote_1` from keyboard `B`,`1` to:
  `SuspendForwarder 3000ms` → `GamepadDpad Down 200ms` → `Delay 150ms` →
  `GamepadButtonTap A 80ms`.

### E — Refactor `src/input.rs`
- Rename to `src/executor.rs` (or split if you prefer). The `play()` loop
  dispatches keystroke steps to SendInput and gamepad steps to `VirtualPad`.
- **Safety net:** if an action contains any gamepad step, automatically call
  `forwarder.suspend(total_action_duration + 500ms)` at the start, even without
  an explicit `SuspendForwarder` step. People writing custom actions shouldn't
  have to remember the gate.

### F — Lifecycle in `src/main.rs`
- On startup: try to initialize `VirtualPad` + `Forwarder`. If init fails
  (drivers not installed yet), log and continue without gamepad support.
- Extend health response:
  ```
  { ..., gamepad: { available: bool, vigem_status: str, hidhide_status: str } }
  ```
- On shutdown: drop `VirtualPad` cleanly so the virtual pad disappears from
  Fortnite's XInput list.

### G — HidHide integration
- Bundle HidHide's signed .msi in NSIS
  (https://github.com/nefarius/HidHide/releases — latest).
- NSIS installs HidHide silently via `ExecWait`.
- Add the helper's own EXE path to HidHide's **allowed list** (otherwise the
  helper can't read the physical pad either).
- Add `FortniteClient-Win64-Shipping.exe` to HidHide's **blocked apps**.
- Use `HidHideCLI.exe` from the NSIS for these — document the exact commands in
  `installer/w3stream-helper.nsi`.
- At runtime, `src/hidhide.rs` shells out to `HidHideCLI.exe` to confirm config
  is still correct; surface in health.
- **Store our own list of HidHide entries we added** in
  `%LOCALAPPDATA%\w3stream\hidhide-managed.json` so uninstall can cleanly
  revert without touching entries the user (or another app) added.

### H — ViGEmBus integration
- Bundle ViGEmBus's signed installer in NSIS
  (https://github.com/nefarius/ViGEmBus/releases — latest).
- NSIS runs it silently. If install fails because the system already has it,
  that's fine — keep going.
- **Never uninstall ViGEmBus on helper uninstall.** Other apps (reWASD,
  DS4Windows) depend on it.

### I — Installer workflow (.github/workflows/release.yml)
- Fetch `HidHide_*.msi` and `ViGEmBus_Setup_*.exe` from their official Nefarius
  release URLs during CI build, drop into `installer/vendor/`.
- **Pin SHA-256** of each downloaded driver installer in the workflow and fail
  the build on mismatch. Never trust auto-latest in CI.

### J — Smoke test: `scripts/poke-gamepad.bat`
- Self-contained Windows .bat (same PowerShell-polyglot pattern as
  `scripts/poke-helper.bat`) that:
  1. Spawns the helper.
  2. Confirms `health.gamepad.available = true`.
  3. Suspends forwarder for 5 s.
  4. Presses D-pad Down then A.
  5. Resumes.
- Streamer focuses Fortnite (or https://gamepad-tester.com/), runs the .bat,
  sees the emote button mash. Holding the physical stick during the test
  should have NO effect on what gamepad-tester shows.

## Acceptance criteria

Do not claim done without each of these verified:

1. `cargo build --release` and `cargo test` green on Windows.
2. NSIS build (gh action `workflow_dispatch` on `release.yml`) produces an
   installer artifact that includes the bundled drivers.
3. Manual install on a Windows 11 box: drivers install, reboot prompt handled,
   helper starts, `health.gamepad.available = true`.
4. Open https://gamepad-tester.com/ in Chrome. Physical Xbox controller is
   invisible (HidHide working). Virtual Xbox 360 controller is visible.
5. Run `scripts/poke-gamepad.bat`. During the 5 s suspend, holding the physical
   stick has no effect on the virtual pad's reported state. After suspend ends,
   physical input is forwarded again.
6. Fire a real Twitch chat command for `fortnite_emote_1` with the streamer
   playing Fortnite. Emote completes even while the streamer continues pushing
   the stick.

## Constraints — do not violate

- Per-action cooldown and `panic_mode` flag MUST still work — don't bypass them
  in the gamepad path.
- All errors in gamepad/forwarder code MUST be caught and turned into
  action-failure responses. The Native Messaging main loop MUST NEVER
  panic-exit on a bad action.
- Keystroke-only actions (`test_type_hi`) MUST still work even if ViGEm/HidHide
  aren't installed. Graceful degradation, not all-or-nothing.
- Don't break the existing Native Messaging frame protocol. Run
  `scripts/poke-helper.bat` after every change to confirm the legacy keystroke
  path still works.
- The installer MUST NOT silently downgrade an existing newer HidHide/ViGEm.
  Check versions before re-running bundled installers.
- HidHide config changes affect the whole system. Only add Fortnite + the
  helper to the lists. Never modify entries we didn't create. The
  `hidhide-managed.json` ledger above is how you guarantee this.

## Known gotchas

- **ViGEm + EAC:** Easy Anti-Cheat is OK with ViGEm — it's widely accepted.
  Don't try to obfuscate that the virtual pad is virtual; that's what gets
  people banned.
- **HidHide whitelist:** the helper's own path must be in HidHide's allow-list
  or the helper itself can't read the physical pad. Set this up at install
  time, double-check at runtime via CLI query.
- **XInput slot numbering:** when both physical (hidden from Fortnite) and
  virtual coexist, the virtual takes the lowest free slot. Fortnite reads from
  slot 0 by default. If something is weird, dump `XInputGetCapabilities` for
  all 4 slots during health check.
- **First install needs a reboot.** Both HidHide and ViGEmBus install kernel
  drivers. The streamer onboarding flow needs to expect this.

## Verification recipe before claiming done

1. `cd /home/td/w3stream-helper && cargo build --release && cargo test` — green.
2. Push branch, trigger `release.yml` `workflow_dispatch`, confirm artifact built.
3. Install on Windows VM. Verify all 6 acceptance-criteria points above.
4. Run `scripts/poke-helper.bat` first (legacy path still works), then
   `scripts/poke-gamepad.bat`.
5. Only after all of the above, message the owner "ready for owner to test in
   real Fortnite session."

If anything in this doc contradicts what you find in the repo, ASK before
guessing. The owner has been burned multiple times by features that "looked
right" but didn't actually solve the integration.
