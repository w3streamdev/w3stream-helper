# Movement-gated emote retry — overlay handoff

**Status:** helper side shipped (`w3streamdev/w3stream-helper` PR #4,
branch `claude/peaceful-mccarthy-U5Kpx`). Extension, relay, and overlay
work not started.

**Audience:** drop this file into any of the downstream repos
(extension, relay, overlay) and hand it to ClaudeCode. It's
self-contained — you do not need to read the helper source to
implement against it.

---

## What the feature does

A chat-triggered emote that lands while the streamer is moving gets
cancelled by the game, so the command succeeds on the wire but nothing
plays. The helper now fires the emote once, then assumes the controller
is idle and counts down a 5 s idle window. Any non-neutral controller
input resets that countdown **and** re-fires the emote (throttled so a
held stick can't spam keystrokes). When the streamer has been
continuously idle for 5 s the emote is considered landed.

Mid-flight the streamer has no visible feedback: easy to miscount the
idle window by milliseconds and get frustrated. The overlay closes
that loop — a countdown ring + re-fire flash + landed/timeout state.

## Architecture

```
Twitch chat
   │
   ▼
Cloud worker  ──►  w3s-relay (Cloud Run)
                       │
              WebSocket │ (existing channel — emote triggers go down,
                       │  status events come up the SAME channel)
                       ▼
              Browser extension
                       │
                       │ Chrome Native Messaging (stdio)
                       ▼
                 helper.exe  ◄── this is where emote_retry events originate
```

Overlay deployment (separate Vercel app) subscribes to the relay's
public read-only events stream for the streamer's channel and renders
in OBS as a browser source.

```
              w3s-relay
                  │
                  ▼ public read-only event stream (per-streamer channel)
              overlay (Vercel)
                  │
                  ▼ rendered in OBS as a browser source
```

## Helper event wire shape (already shipped)

On each retry-loop state transition the helper pushes one unsolicited
JSON message on stdout (length-prefixed per Chrome Native Messaging,
same framing the extension already handles for `hello`).

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

All fields are always present.

| field | type | meaning |
| --- | --- | --- |
| `type` | string | always `"emote_retry"` — discriminator from `hello` / command replies |
| `phase` | string | state transition; see table below |
| `emote_id` | string | action id of the pending emote |
| `idle_for_ms` | int | ms since the last non-neutral controller input |
| `idle_required_ms` | int | configured idle window (default 5000) |
| `running_for_ms` | int | ms since the emote was first requested |
| `max_duration_ms` | int | configured safety cap (default 60000) |
| `time_remaining_ms` | int | `max(0, idle_required_ms - idle_for_ms)` — convenience |

### `phase` values

| phase | when it fires | overlay action |
| --- | --- | --- |
| `started` | emote just fired for the first time | fade in countdown ring sized to `idle_required_ms` |
| `input_active` | non-neutral controller input detected after an idle tick | snap ring back to full, tint briefly to signal "you moved" |
| `retry` | the emote keystroke sequence was re-fired | flash a re-fire indicator |
| `idle_satisfied` | streamer was idle for `idle_required_ms` | show landed state, fade out |
| `timeout` | `max_duration_ms` elapsed without idle | show warning state, fade out |
| `superseded` | a new emote request replaced this one | fade out — the new emote's `started` event takes over |

Events fire **only on transitions**. The overlay must drive its own
RAF / interval loop to render the countdown smoothly between events.

## Per-repo task list

### Extension repo

1. The native-messaging port already deserializes JSON messages from
   the helper. Today the only unsolicited shape is `{"type":"hello"}`.
   Add a branch for `{"type":"emote_retry"}` alongside it.
2. Forward `emote_retry` events to the relay over the existing
   WebSocket. **Do not** mutate the payload — pass it through verbatim
   plus whatever envelope the relay already uses (streamer id, auth,
   etc.).
3. Don't try to render anything in the extension UI — the overlay is a
   separate consumer.
4. Verify: with the helper running, trigger an emote and confirm the
   extension logs / forwards the `started`, `retry`, `idle_satisfied`
   events.

### Relay repo

1. Accept `emote_retry` events from the extension (auth scoped to the
   streamer the helper is paired with — same scoping the existing
   emote trigger messages use).
2. Re-broadcast to the per-streamer public read-only events stream
   that the overlay subscribes to. The overlay is unauthenticated (OBS
   browser source has no secrets), so only push fields that are safe
   to expose — the wire shape above is all non-sensitive.
3. No state — relay should be a pure fan-out. The helper is the
   source of truth for retry state; the overlay reconstructs from
   the event stream.
4. Drop events for streamers with no active overlay subscription
   silently — the helper doesn't need backpressure.

### Overlay repo (Vercel)

1. New page (e.g. `/overlay/[streamerId]`) that the streamer adds in
   OBS as a Browser Source. Transparent background, fixed size.
2. Connect to the relay's events stream for that streamer. Subscribe
   to `type=emote_retry`.
3. State machine, per pending emote (only one is active at a time —
   `superseded` retires the previous):

   ```
   ┌──────────┐  started      ┌────────────────┐  idle_satisfied  ┌─────────┐
   │  hidden  │ ────────────► │ counting down  │ ───────────────► │ landed  │
   └──────────┘               └───────┬────────┘                  └────┬────┘
        ▲                             │ input_active                   │
        │                             │ (reset countdown)              │
        │                             ▼                                │ fade
        │                     ┌────────────────┐                       │ out
        │                     │  retry flash   │                       │
        │                     └───────┬────────┘                       ▼
        │                             │ resumes countdown         ┌─────────┐
        │ superseded                  │                           │ hidden  │
        │ / fade out timer            ▼                           └─────────┘
        │                     ┌────────────────┐
        └─────────────────────┤  timeout/warn  │
                              └────────────────┘
   ```

4. Countdown rendering:
   - On `started` or `input_active`, set local `deadline =
     performance.now() + time_remaining_ms`.
   - Each animation frame, draw remaining = `max(0, deadline -
     performance.now())`. Render as ring/bar that drains over
     `idle_required_ms`.
   - Don't trust the helper's clock to be the same as the overlay's;
     always use `time_remaining_ms` as the *target* and interpolate
     locally.
5. Re-fire flash on `retry`: 150 ms pulse on a small icon next to the
   ring.
6. `idle_satisfied`: green check, 1 s hold, fade.
   `timeout`: yellow warning, 2 s hold, fade.
   `superseded`: fade out without success/failure styling.
7. If the streamer's helper disconnects mid-countdown the overlay
   stops receiving events. Add a 2× `idle_required_ms` watchdog —
   if no event arrives in that window, hide the overlay.

## Test plan (end-to-end)

1. Start helper, extension, relay, overlay.
2. Add overlay as OBS Browser Source.
3. Sit idle on a controller, trigger `fortnite_emote_1` from chat.
   Expect: overlay shows 5 s countdown, drains, `idle_satisfied`,
   fades.
4. Trigger the emote, then keep moving the left stick. Expect:
   countdown repeatedly snaps back to full, `retry` flashes every
   `retry_interval_ms` (default 750 ms). Stop moving. Expect:
   countdown drains and lands.
5. Trigger the emote, keep moving for 60+ seconds. Expect: warning
   state at the cap, fade out.
6. Trigger emote A, then trigger emote B before A lands. Expect: A's
   countdown disappears, B's `started` countdown takes over.

## Edge cases / gotchas

- **No controller connected.** Helper still watches keyboard + mouse via
  low-level observer hooks, so a KB+M player's input resets the idle
  timer the same way controller input does. If the streamer is truly
  idle on every input surface, the overlay sees `started` → 5 s of
  nothing → `idle_satisfied`. Render that the same as a normal
  idle-completed flow.
- **Helper config disabled** (`emoteRetry.enabled = false`). No events
  fire at all — the emote runs through the legacy one-shot path. The
  overlay should not assume an event will arrive after every chat
  trigger; only react when events show up.
- **Out-of-order events.** WebSocket delivery is ordered. If a relay
  redesign breaks that assumption, drop events whose
  `running_for_ms` goes backwards relative to the current emote.
- **Clock skew.** Don't display `idle_for_ms` raw — only use it to
  reset the local countdown deadline. The helper's `Instant::now()`
  has no relation to overlay wall-clock time.

## What's *not* in scope here

- No new helper commands. Existing `trigger` is unchanged.
- No backwards-incompatible changes to the relay's existing emote
  trigger flow.
- No overlay auth — it's a public read-only fan-out tied to a streamer
  id. If you need per-streamer secrecy, that's a relay redesign and
  out of scope for this feature.

---

## Wiring the ProgressTimer component

The overlay ships with a `ProgressTimer` React component (handoff
bundle: `progress-timer/project/Progress Timer.html`). It supports two
modes:

- **Self-driven**: pass `isRunning` / `resetTrigger`, component runs
  its own RAF clock for `duration` ms then fires `onComplete`.
- **Parent-driven**: pass `state` + `elapsedOverride`, component
  renders exactly what the parent says. **Use this mode** — the helper
  is the source of truth for retry state, and we want every visual
  transition to match an event from upstream.

### Phase → component state mapping

| `emote_retry.phase` | `ProgressTimer.state` | extra overlay action |
| --- | --- | --- |
| `started` | `running` (elapsed=0) | reset local clock; ensure overlay visible |
| `input_active` | `resetting` for ~360ms, then `running` (elapsed=0) | the pink-X flash *is* the "you moved" signal |
| `retry` | (no state change) | pulse a small `RE-FIRE` badge next to the ring |
| `idle_satisfied` | `done` | hold ~1.5 s then fade to `idle`, hide overlay |
| `timeout` | `resetting` → `idle` | hold ~600 ms then hide; optional yellow tint via CSS |
| `superseded` | `resetting` → `idle` | hide; the new emote's `started` event takes over |

### `useEmoteRetryTimer` hook (drop-in)

```jsx
import { useEffect, useRef, useState } from 'react';

export function useEmoteRetryTimer(eventStreamUrl) {
  const [pending, setPending] = useState(null);   // { emoteId } | null
  const [state, setState] = useState('idle');     // idle | running | resetting | done
  const [elapsed, setElapsed] = useState(0);
  const [duration, setDuration] = useState(5000); // mirrors idle_required_ms
  const [retryFlash, setRetryFlash] = useState(0); // bump on each `retry`

  const lastResetAt = useRef(0);
  const rafRef = useRef(0);
  const flashTimer = useRef(null);
  const resumeTimer = useRef(null);
  const exitTimer = useRef(null);

  useEffect(() => {
    const ws = new WebSocket(eventStreamUrl);

    ws.onmessage = (msg) => {
      const e = JSON.parse(msg.data);
      if (e.type !== 'emote_retry') return;

      setDuration(e.idle_required_ms);

      switch (e.phase) {
        case 'started':
          clearTimeout(resumeTimer.current);
          clearTimeout(exitTimer.current);
          setPending({ emoteId: e.emote_id });
          lastResetAt.current = performance.now();
          setElapsed(0);
          setState('running');
          break;

        case 'input_active':
          // Pink-X flash, then resume the countdown from zero.
          setState('resetting');
          clearTimeout(resumeTimer.current);
          resumeTimer.current = setTimeout(() => {
            lastResetAt.current = performance.now();
            setElapsed(0);
            setState('running');
          }, 360);
          break;

        case 'retry':
          // A retry event means the helper just re-fired the emote because
          // input is still active — the helper's idle timer was reset to
          // now. Reset our local clock too so the visual ring never drifts
          // past current input (max drift becomes retry_interval_ms).
          lastResetAt.current = performance.now();
          setElapsed(0);
          setRetryFlash((n) => n + 1);
          clearTimeout(flashTimer.current);
          flashTimer.current = setTimeout(() => {}, 250);
          break;

        case 'idle_satisfied':
          setState('done');
          clearTimeout(exitTimer.current);
          exitTimer.current = setTimeout(() => {
            setPending(null);
            setState('idle');
          }, 1500);
          break;

        case 'timeout':
        case 'superseded':
          setState('resetting');
          clearTimeout(exitTimer.current);
          exitTimer.current = setTimeout(() => {
            setPending(null);
            setState('idle');
          }, 600);
          break;
      }
    };

    return () => {
      ws.close();
      clearTimeout(resumeTimer.current);
      clearTimeout(exitTimer.current);
      clearTimeout(flashTimer.current);
    };
  }, [eventStreamUrl]);

  // Local interpolation — only runs while counting down.
  useEffect(() => {
    if (state !== 'running') return;
    const tick = (now) => {
      setElapsed(Math.min(duration, now - lastResetAt.current));
      rafRef.current = requestAnimationFrame(tick);
    };
    rafRef.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafRef.current);
  }, [state, duration]);

  return { state, elapsed, duration, pending, retryFlash };
}
```

### Overlay page

```jsx
import { useEmoteRetryTimer } from './useEmoteRetryTimer';
import { ProgressTimer } from './ProgressTimer';

export default function EmoteOverlay({ streamerId }) {
  const url = `wss://relay.example.com/streams/${streamerId}/events`;
  const { state, elapsed, duration, pending, retryFlash } =
    useEmoteRetryTimer(url);

  if (!pending) return null; // OBS browser source stays transparent

  const label =
    state === 'done' ? 'LANDED!' :
    state === 'resetting' ? 'MOVED' :
    pending.emoteId.toUpperCase();

  return (
    <div className="emote-overlay">
      <ProgressTimer
        duration={duration}
        size={260}
        state={state}
        elapsedOverride={elapsed}
        label={label}
      />
      <RefireFlash key={retryFlash} />
    </div>
  );
}

// 250ms pulse on each retry event. Keyed on retryFlash so a new
// event remounts and replays the animation cleanly.
function RefireFlash() {
  return <div className="refire-flash">RE-FIRE</div>;
}
```

```css
.emote-overlay {
  position: fixed;
  bottom: 32px;
  right: 32px;
  display: grid;
  place-items: center;
  pointer-events: none; /* OBS overlay is non-interactive */
}
.refire-flash {
  position: absolute;
  top: -12px;
  font: 600 11px/1 'IBM Plex Mono', monospace;
  letter-spacing: 0.18em;
  color: #ff007a;
  text-shadow: 0 0 8px rgba(255, 0, 122, 0.7);
  opacity: 0;
  animation: refire-pulse 250ms ease-out forwards;
}
@keyframes refire-pulse {
  0%   { opacity: 0; transform: scale(0.85); }
  30%  { opacity: 1; transform: scale(1.05); }
  100% { opacity: 0; transform: scale(1); }
}
```

### Why this works

- `state` + `elapsedOverride` drive the component in parent-driven
  mode — every visual transition is sourced from an upstream event,
  so the helper and overlay can never disagree about which phase
  we're in.
- The RAF loop in `useEmoteRetryTimer` only interpolates *between*
  events. It uses `performance.now()` so clock skew with the helper
  doesn't matter — `lastResetAt` resets every `started` /
  `input_active`, and the local clock only ever counts up to
  `idle_required_ms`.
- The `resetting` state is reused for three different "negative"
  transitions (input reset, timeout, superseded) because the pink-X
  animation reads as "interrupted" in every case. If yellow timeout
  styling is wanted later, override `[data-state="resetting"]` via a
  modifier class set on a wrapper.
- `pending = null` keeps the overlay completely transparent when no
  emote is in flight — OBS gets nothing in front of the game.

### Things to confirm before wiring

- **Relay event stream URL & framing.** Above assumes a plain
  WebSocket carrying raw `emote_retry` JSON. If the relay wraps events
  in an envelope (`{event: "...", data: {...}}`) or uses SSE, swap
  the `ws.onmessage` handler accordingly — the rest of the hook is
  unchanged.
- **One overlay per streamer.** The hook holds one pending emote at
  a time. `superseded` is the only way the previous one gets
  replaced; if the relay multiplexes multiple streamers onto one
  socket you'll need to key by `streamer_id`.
- **OBS browser source size.** `ProgressTimer` is responsive but the
  outer `.emote-overlay` positions it; pick the size/position once
  with the streamer rather than making it draggable in the overlay.
