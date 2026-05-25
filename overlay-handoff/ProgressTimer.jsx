import { useEffect, useMemo, useRef, useState } from 'react';
import './ProgressTimer.css';

/* =========================================================================
   ProgressTimer — React port of the Claude Design HUD prototype.

   Two modes:
   - Self-driven: pass `isRunning` / `resetTrigger`; the component runs its
     own RAF clock for `duration` ms then fires `onComplete`.
   - Parent-driven: pass `state` + `elapsedOverride`; the component renders
     exactly what the parent says. Use this mode when wiring to
     `useEmoteRetryTimer` — every visual transition is sourced from a
     helper event, so helper and overlay never disagree about phase.

   States: 'idle' | 'running' | 'done' | 'resetting'
   ========================================================================= */

// SVG arc path, 0deg = 12 o'clock, clockwise.
function arcPath(cx, cy, r, startDeg, endDeg) {
  const toRad = (d) => ((d - 90) * Math.PI) / 180;
  const sx = cx + r * Math.cos(toRad(startDeg));
  const sy = cy + r * Math.sin(toRad(startDeg));
  const ex = cx + r * Math.cos(toRad(endDeg));
  const ey = cy + r * Math.sin(toRad(endDeg));
  const large = Math.abs(endDeg - startDeg) > 180 ? 1 : 0;
  const sweep = endDeg > startDeg ? 1 : 0;
  return `M ${sx} ${sy} A ${r} ${r} 0 ${large} ${sweep} ${ex} ${ey}`;
}

// D-pad / Joystick icon. Native viewBox: 0 0 78.02 78.02
const DPAD_PATH =
  'M50.39 58.08c0-.24-.03-.47-.07-.69v-.02c-.14-.75-.51-1.44-1.03-1.99l-7.61-7.63a3.86 3.86 0 00-1.29-.85 3.718 3.718 0 00-2.07-.22c-.09.01-.17.04-.25.06-.12.03-.24.06-.36.11-.11.04-.21.08-.31.12-.09.04-.17.09-.26.14-.11.06-.22.12-.33.19-.23.15-.45.32-.66.52l-7.61 7.63c-.56.56-.91 1.25-1.04 2.01v.07c-.04.23-.06.46-.06.7v15.26c0 .16.03.3.05.46 0 .05 0 .1.02.15a3.816 3.816 0 002.18 2.86c.48.22 1.01.35 1.56.35h.43c2.37.45 4.82.71 7.32.71s4.95-.26 7.32-.71h.14c.55 0 1.07-.12 1.55-.34.43-.2.83-.47 1.16-.8.78-.7 1.28-1.7 1.28-2.82h-.04V58.09zM76.96 29.94c-.01.15-.05.28-.07.42-.03-.11-.08-.22-.12-.33-.54-1.44-1.91-2.48-3.53-2.48h-5.96v-.02h-9.31c-.24 0-.48.03-.72.07-.07.01-.13.05-.2.06-.18.05-.36.11-.53.18-.19.07-.37.17-.54.27-.03.02-.06.04-.1.06-.06.04-.13.06-.18.1-.01.01-.03.02-.04.03-.19.13-.37.26-.53.42l-7.61 7.63c-.76.76-1.1 1.72-1.1 2.71v.07c0 .99.38 1.95 1.1 2.71l7.61 7.63a3.851 3.851 0 002.7 1.11h15.22c.9 0 1.72-.34 2.38-.87 0 0 .01 0 .02-.01l.04-.04c.12-.1.24-.21.35-.33.33-.32.63-.68.83-1.11.02-.02.05-.04.07-.06.08-.07.16-.14.24-.2.68-2.88 1.05-5.89 1.05-8.98s-.38-6.15-1.07-9.05zM27.38 20.08c0 .24.03.48.07.72.14.75.51 1.44 1.03 1.99l7.61 7.63a3.772 3.772 0 005.36 0h.04l.12-.12h.01l.02-.02h.02l7.61-7.63c.49-.49.81-1.1.98-1.76.02-.08.05-.15.06-.23.04-.24.07-.48.07-.72V6.8 4.67c0-1.6-1-2.96-2.4-3.53h-.02s-.02 0-.03-.01c0-.02.01-.04.02-.06-.22-.05-.44-.09-.66-.14-.02 0-.05 0-.07-.01C44.59.33 41.83 0 38.99 0c-2.44 0-4.81.25-7.12.69-.37.07-.73.13-1.1.21-.04 0-.08.02-.12.03-.12.02-.24.05-.36.08-.27.06-.55.11-.82.18h.29c-.14.07-.27.16-.4.24-.41.23-.77.53-1.07.88-.04.04-.07.09-.11.13-.08.1-.15.2-.21.3-.08.13-.15.26-.21.39-.04.08-.07.17-.11.26-.06.16-.11.31-.15.48-.02.06-.03.13-.04.19-.04.22-.07.44-.07.67v15.33zM30.26 41.72c1.48-1.48 1.49-3.89 0-5.37s-7.61-7.63-7.61-7.63c-.54-.54-1.22-.89-1.96-1.03-.03 0-.06-.02-.09-.03h-.03a3.71 3.71 0 00-.67-.06H4.68c-.24 0-.48.03-.7.08-.03 0-.06.02-.09.02-.02 0-.03 0-.05.01 0 .01-.01.02-.02.03-.12.03-.22.08-.33.12-.74.22-1.39.66-1.87 1.24-.04.05-.09.1-.13.16-.07.1-.14.21-.21.32-.06.09-.11.19-.16.29-.05.09-.09.19-.13.29-.05.13-.09.27-.13.41-.02.08-.05.16-.06.24-.04.23-.07.46-.07.7v.13C.27 34.03 0 36.49 0 39.02s.27 4.95.74 7.33v.49c0 .67.19 1.29.49 1.84 0 .03.01.07.02.1 0 0 .02 0 .03-.01.66 1.12 1.87 1.89 3.26 1.89h15.22c.25 0 .5-.03.74-.08.17-.03.33-.08.49-.14.09-.03.17-.07.26-.11.06-.03.13-.05.19-.08l.33-.18c.02-.01.05-.03.07-.04.12-.08.24-.16.36-.25.16-.11.31-.24.45-.37l7.61-7.63v-.04zm.87-1.85z';

// Reset X — negative-space cutout. Native viewBox: 0 0 36.5 36.5
const RESET_X_PATH =
  'M36 29.38c-.09-.03-.17-.07-.26-.1-.06-.03-.13-.05-.19-.08-.11-.05-.22-.11-.33-.18-.02-.02-.05-.03-.07-.05-.12-.08-.24-.16-.35-.25-.1-.08-.21-.16-.3-.26l-7.58-7.58a3.784 3.784 0 010-5.34l7.58-7.58c.55-.52 1.23-.88 1.98-1.03l-.12-.18A21.723 21.723 0 0029.55 0c-.14.74-.49 1.43-1.03 1.97l-7.58 7.58c-.76.72-1.71 1.1-2.69 1.1s-1.93-.34-2.69-1.1-7.59-7.57-7.59-7.57C7.45 1.43 7.09.75 6.94 0c-.03.02-.07.04-.1.07A21.534 21.534 0 00.08 6.81c-.03.04-.05.08-.08.12.74.14 1.43.49 1.97 1.03l7.46 7.46.12.12v.04a3.784 3.784 0 010 5.34L1.97 28.5c-.55.52-1.23.88-1.98 1.03 1.72 2.77 4.05 5.11 6.8 6.85.05.03.11.07.16.11a3.8 3.8 0 011.04-2l7.58-7.58a3.784 3.784 0 015.34 0h.04l7.58 7.58c.08.09.15.18.22.27.09.11.18.23.25.36.01.02.03.05.04.07.07.11.13.22.18.33.03.06.05.12.08.18.04.08.07.17.1.26.06.16.1.32.14.49 2.81-1.75 5.18-4.13 6.92-6.95a3.07 3.07 0 01-.5-.13z';

function CrossShape({ size, color, glow }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 78.02 78.02"
      xmlns="http://www.w3.org/2000/svg"
      style={{ display: 'block', overflow: 'visible' }}
    >
      <g style={glow ? { filter: `drop-shadow(0 0 8px ${glow})` } : null}>
        <path d={DPAD_PATH} fill={color} />
      </g>
    </svg>
  );
}

function ResetX({ size, color }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 36.5 36.5"
      xmlns="http://www.w3.org/2000/svg"
      style={{
        display: 'block',
        overflow: 'visible',
        filter: `drop-shadow(0 0 8px ${color})`,
      }}
    >
      <path d={RESET_X_PATH} fill={color} />
    </svg>
  );
}

export function ProgressTimer({
  duration = 5000,
  size = 220,
  strokeWidth,
  isRunning = false,
  onComplete,
  onReset,
  resetTrigger = false,
  state: extState,
  elapsedOverride,
  showLabel = true,
  label,
}) {
  const [state, setState] = useState('idle');
  const [elapsed, setElapsed] = useState(0);
  const rafRef = useRef(0);
  const startRef = useRef(0);
  const baseRef = useRef(0);

  const driven = typeof extState === 'string';
  const realState = driven ? extState : state;
  const realElapsed =
    driven && typeof elapsedOverride === 'number' ? elapsedOverride : elapsed;

  // Self-driven RAF loop
  useEffect(() => {
    if (driven) return;
    if (state !== 'running') return;
    startRef.current = performance.now();
    const baseAtStart = baseRef.current;
    const tick = (now) => {
      const e = baseAtStart + (now - startRef.current);
      if (e >= duration) {
        setElapsed(duration);
        setState('done');
        onComplete && onComplete();
        return;
      }
      setElapsed(e);
      rafRef.current = requestAnimationFrame(tick);
    };
    rafRef.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafRef.current);
  }, [state, driven, duration, onComplete]);

  // External isRunning -> drive state
  useEffect(() => {
    if (driven) return;
    if (isRunning && state === 'idle') {
      baseRef.current = 0;
      setElapsed(0);
      setState('running');
    } else if (isRunning && state === 'done') {
      baseRef.current = 0;
      setElapsed(0);
      setState('running');
    } else if (!isRunning && state === 'running') {
      baseRef.current = elapsed;
      cancelAnimationFrame(rafRef.current);
      setState('idle');
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isRunning, driven]);

  // resetTrigger
  const prevReset = useRef(resetTrigger);
  const resetTimerRef = useRef(null);
  useEffect(() => {
    if (driven) return;
    if (resetTrigger && !prevReset.current) {
      cancelAnimationFrame(rafRef.current);
      setState('resetting');
      if (resetTimerRef.current) clearTimeout(resetTimerRef.current);
      resetTimerRef.current = setTimeout(() => {
        baseRef.current = 0;
        setElapsed(0);
        setState('idle');
        onReset && onReset();
        resetTimerRef.current = null;
      }, 500);
    }
    prevReset.current = resetTrigger;
  }, [resetTrigger, driven, onReset]);
  useEffect(
    () => () => {
      if (resetTimerRef.current) clearTimeout(resetTimerRef.current);
    },
    []
  );

  // Geometry
  const S = size;
  const cx = S / 2;
  const cy = S / 2;
  const stroke = strokeWidth || Math.max(8, Math.round(S * 0.085));
  const outerR = S * 0.5 - stroke * 0.5 - 1;
  const segGap = 6;
  const segCount = 5;
  const segSpan = 360 / segCount;
  const segments = useMemo(() => {
    const out = [];
    for (let i = 0; i < segCount; i++) {
      const startDeg = i * segSpan + segGap / 2;
      const endDeg = (i + 1) * segSpan - segGap / 2;
      out.push({ i, startDeg, endDeg });
    }
    return out;
  }, [segSpan]);

  const segColors = ['#EC4899', '#7C3AED', '#3B82F6', '#00E5FF', '#00E5FF'];
  const segLit = (i) => realElapsed >= ((i + 1) / segCount) * duration - 1;

  const spinnerOuter = (outerR - stroke * 0.85) * 2;
  const spinnerThickness = Math.max(5, S * 0.04);
  const spinnerInner = spinnerOuter - spinnerThickness * 2;

  let discFill = '#181a20';
  if (realState === 'done') discFill = '#39FF14';
  if (realState === 'resetting' || realState === 'idle-after-reset')
    discFill = '#1A1A1A';

  let crossColor = '#0e0f12';
  let crossGlow = null;
  if (realState === 'done') {
    crossColor = '#FFFFFF';
    crossGlow = 'rgba(255,255,255,0.6)';
  }
  if (realState === 'resetting') {
    crossColor = '#FF007A';
    crossGlow = 'rgba(255,0,122,0.7)';
  }

  const showX = realState === 'resetting';

  const formatTime = (ms) => {
    const totalCs = Math.max(0, Math.floor(ms / 10));
    const sec = Math.floor(totalCs / 100);
    const cs = totalCs % 100;
    return `${String(sec).padStart(2, '0')}:${String(cs).padStart(2, '0')}`;
  };
  const labelText =
    label !== undefined
      ? label
      : realState === 'done'
      ? 'DONE!'
      : realState === 'resetting'
      ? '00:00'
      : formatTime(realElapsed);

  return (
    <div
      className="pt"
      data-state={realState}
      style={{ width: S, height: S }}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={duration}
      aria-valuenow={Math.round(realElapsed)}
    >
      <div className="pt-stack">
        <svg className="pt-svg" viewBox={`0 0 ${S} ${S}`}>
          <defs>
            <radialGradient id="pt-disc-glow" cx="50%" cy="50%" r="50%">
              <stop offset="60%" stopColor="rgba(0,0,0,0)" />
              <stop offset="100%" stopColor="rgba(124,58,237,0.18)" />
            </radialGradient>
          </defs>

          <g style={{ transformOrigin: `${cx}px ${cy}px` }}>
            <circle
              className="pt-disc pt-disc-bg"
              cx={cx}
              cy={cy}
              r={outerR - stroke * 0.5 + 1}
              fill={discFill}
              style={{ transformOrigin: `${cx}px ${cy}px` }}
            />
            {realState !== 'done' && (
              <circle
                cx={cx}
                cy={cy}
                r={outerR - stroke * 0.5 + 1}
                fill="url(#pt-disc-glow)"
              />
            )}
          </g>

          {realState !== 'done' &&
            realState !== 'resetting' &&
            segments.map((seg) => (
              <path
                key={`bg-${seg.i}`}
                d={arcPath(cx, cy, outerR, seg.startDeg, seg.endDeg)}
                stroke="#111213"
                strokeWidth={stroke}
                strokeLinecap="butt"
                fill="none"
              />
            ))}

          {realState !== 'done' &&
            realState !== 'resetting' &&
            segments.map((seg) => {
              if (!segLit(seg.i)) return null;
              return (
                <path
                  key={`fg-${seg.i}`}
                  d={arcPath(cx, cy, outerR, seg.startDeg, seg.endDeg)}
                  stroke={segColors[seg.i]}
                  strokeWidth={stroke}
                  strokeLinecap="butt"
                  fill="none"
                />
              );
            })}

          {realState === 'done' && (
            <circle
              cx={cx}
              cy={cy}
              r={outerR}
              fill="none"
              stroke="rgba(255,255,255,0.18)"
              strokeWidth={stroke * 0.35}
            />
          )}
        </svg>

        <div className="pt-spinner-host">
          <div
            className="pt-spinner"
            style={{
              width: spinnerOuter,
              height: spinnerOuter,
              ['--inner']: `${spinnerInner / 2}px`,
            }}
          />
        </div>

        <div className="pt-cross-host">
          {showX ? (
            <div
              style={{
                position: 'relative',
                display: 'grid',
                placeItems: 'center',
              }}
            >
              <CrossShape size={S * 0.6} color="#3a3d44" />
              <div
                style={{
                  position: 'absolute',
                  inset: 0,
                  display: 'grid',
                  placeItems: 'center',
                }}
              >
                <ResetX size={S * 0.6 * (36.5 / 78.02)} color="#FF007A" />
              </div>
            </div>
          ) : (
            <CrossShape
              size={S * 0.6}
              color={crossColor}
              glow={crossGlow}
            />
          )}
        </div>
      </div>

      {showLabel && (
        <div className="pt-label" data-state={realState}>
          {labelText}
        </div>
      )}
    </div>
  );
}

export default ProgressTimer;
