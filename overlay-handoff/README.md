# Overlay handoff — drop-in files

These are ready-to-paste React files for the overlay Vercel repo. They
wire the helper's `emote_retry` events (see `../EMOTE_RETRY_OVERLAY.md`
for the wire spec) to the ProgressTimer component from Claude Design.

## Files

| file | what it is |
| --- | --- |
| `ProgressTimer.jsx` | React port of the Claude Design HUD prototype |
| `ProgressTimer.css` | styles for the timer (segments, spinner, cross) |
| `useEmoteRetryTimer.js` | hook: subscribes to the relay WS, returns props for the timer |
| `EmoteOverlay.jsx` | the OBS browser-source page component |
| `EmoteOverlay.css` | overlay container (fixed-position, transparent) |

## Minimal install (any React app)

1. Copy all five files into the overlay repo. Pick any folder — e.g.
   `src/components/emote-overlay/`.
2. Make sure your build has `react` ≥ 17 and supports JSX + CSS imports.
   No other runtime dependencies.
3. Mount the overlay on the page OBS loads as a browser source:

   ```jsx
   import { EmoteOverlay } from './components/emote-overlay/EmoteOverlay';

   export default function OverlayPage({ params }) {
     return (
       <EmoteOverlay
         streamerId={params.streamerId}
         relayBaseUrl="wss://relay.example.com"
       />
     );
   }
   ```

4. Set the OBS browser source CSS background to transparent (Vercel
   sites are normally white). For Next.js:

   ```jsx
   // app/overlay/[streamerId]/layout.jsx
   export default function Layout({ children }) {
     return (
       <html>
         <body style={{ background: 'transparent', margin: 0 }}>
           {children}
         </body>
       </html>
     );
   }
   ```

## What to confirm before shipping

- **Relay WS URL format.** `useEmoteRetryTimer.js` builds
  `${relayBaseUrl}/streams/${streamerId}/events` — change the path to
  match what the relay actually exposes.
- **Event envelope.** The hook assumes raw `emote_retry` JSON on
  `ws.onmessage`. If the relay wraps in `{event, data}` or uses SSE,
  swap the `onmessage` body — the rest of the hook is unchanged.
- **Multi-streamer multiplexing.** The hook holds one pending emote at
  a time. If a single WS carries events for multiple streamers, filter
  by `streamer_id` inside the handler before processing.
- **Fonts.** Styles reference `IBM Plex Mono`. Include it via your
  preferred method (Google Fonts link in `<head>`, `next/font`, etc.)
  or change the `font-family` in the CSS.

## Phase → visual mapping

| `phase` from helper | what the streamer sees |
| --- | --- |
| `started` | timer appears, 5s countdown ring starts draining |
| `input_active` | pink-X flash ("you moved"), then countdown restarts |
| `retry` | `RE-FIRE` text pulses above the timer |
| `idle_satisfied` | green flash, "LANDED!", fades after 1.5s |
| `timeout` / `superseded` | pink-X, fades after 600ms |

## Smoke test (no relay needed)

To verify the component renders without wiring the relay yet:

```jsx
import { ProgressTimer } from './ProgressTimer';

// Self-driven mode — runs its own clock.
<ProgressTimer duration={5000} size={260} isRunning={true}
               onComplete={() => console.log('done')} />
```

That should show the full animation cycle once.
