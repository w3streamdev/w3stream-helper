import { ProgressTimer } from './ProgressTimer';
import { useEmoteRetryTimer } from './useEmoteRetryTimer';
import './EmoteOverlay.css';

/* =========================================================================
   EmoteOverlay

   OBS browser source. Transparent background, no pointer events.
   Subscribes to the relay event stream for one streamer and renders the
   ProgressTimer driven by helper emote_retry events.
   ========================================================================= */

export function EmoteOverlay({ streamerId, relayBaseUrl }) {
  const url = relayBaseUrl
    ? `${relayBaseUrl}/streams/${streamerId}/events`
    : null;

  const { state, elapsed, duration, pending, retryFlash } =
    useEmoteRetryTimer(url);

  if (!pending) return null;

  const label =
    state === 'done'
      ? 'LANDED!'
      : state === 'resetting'
      ? 'MOVED'
      : pending.emoteId.toUpperCase();

  return (
    <div className="emote-overlay">
      <ProgressTimer
        duration={duration}
        size={260}
        state={state}
        elapsedOverride={elapsed}
        label={label}
      />
      {/* Keying on retryFlash remounts the flash so the animation replays. */}
      <RefireFlash key={retryFlash} visible={retryFlash > 0} />
    </div>
  );
}

function RefireFlash({ visible }) {
  if (!visible) return null;
  return <div className="refire-flash">RE-FIRE</div>;
}

export default EmoteOverlay;
