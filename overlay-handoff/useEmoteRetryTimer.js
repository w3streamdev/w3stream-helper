import { useEffect, useRef, useState } from 'react';

/* =========================================================================
   useEmoteRetryTimer

   Subscribes to a relay WebSocket carrying `emote_retry` events from the
   w3stream-helper and turns them into props for <ProgressTimer />.

   Helper event shape (see EMOTE_RETRY_OVERLAY.md):
   {
     "type": "emote_retry",
     "phase": "started" | "input_active" | "retry"
            | "idle_satisfied" | "timeout" | "superseded",
     "emote_id": "...",
     "idle_for_ms": 0,
     "idle_required_ms": 5000,
     "running_for_ms": 0,
     "max_duration_ms": 60000,
     "time_remaining_ms": 5000
   }

   Returns:
     state     — feed to ProgressTimer.state ('idle'|'running'|'resetting'|'done')
     elapsed   — feed to ProgressTimer.elapsedOverride (ms, interpolated locally)
     duration  — feed to ProgressTimer.duration (mirrors idle_required_ms)
     pending   — { emoteId } | null; render the overlay only when non-null
     retryFlash — bumps on each `retry` event; key a flash component on it

   The local RAF loop only interpolates BETWEEN events. Clock skew with the
   helper is irrelevant because lastResetAt is reset by every started /
   input_active event.
   ========================================================================= */

export function useEmoteRetryTimer(eventStreamUrl) {
  const [pending, setPending] = useState(null);
  const [state, setState] = useState('idle');
  const [elapsed, setElapsed] = useState(0);
  const [duration, setDuration] = useState(5000);
  const [retryFlash, setRetryFlash] = useState(0);

  const lastResetAt = useRef(0);
  const rafRef = useRef(0);
  const resumeTimer = useRef(null);
  const exitTimer = useRef(null);

  useEffect(() => {
    if (!eventStreamUrl) return undefined;
    const ws = new WebSocket(eventStreamUrl);

    ws.onmessage = (msg) => {
      let event;
      try {
        event = JSON.parse(msg.data);
      } catch {
        return;
      }
      if (!event || event.type !== 'emote_retry') return;

      setDuration(event.idle_required_ms);

      switch (event.phase) {
        case 'started':
          clearTimeout(resumeTimer.current);
          clearTimeout(exitTimer.current);
          setPending({ emoteId: event.emote_id });
          lastResetAt.current = performance.now();
          setElapsed(0);
          setState('running');
          break;

        case 'input_active':
          // Pink-X flash for ~360ms, then resume the countdown from zero.
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

        default:
          break;
      }
    };

    return () => {
      ws.close();
      clearTimeout(resumeTimer.current);
      clearTimeout(exitTimer.current);
    };
  }, [eventStreamUrl]);

  // Local interpolation — only runs while counting down.
  useEffect(() => {
    if (state !== 'running') return undefined;
    const tick = (now) => {
      setElapsed(Math.min(duration, now - lastResetAt.current));
      rafRef.current = requestAnimationFrame(tick);
    };
    rafRef.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafRef.current);
  }, [state, duration]);

  return { state, elapsed, duration, pending, retryFlash };
}

export default useEmoteRetryTimer;
