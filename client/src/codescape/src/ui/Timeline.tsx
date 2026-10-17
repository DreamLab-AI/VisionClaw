/** Scrubber over the presence log: rewind, replay, pause, live. */
import { useEffect, useMemo, useRef } from 'react';
import { actorCss } from '../scene/Presence';
import { dispatch, getStore, useCs } from '../store';
import { logRange } from '../timeline';

function fmt(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

export function Timeline() {
  const presence = useCs((s) => s.presence);
  const tl = useCs((s) => s.timeline);
  const range = logRange(presence);
  const raf = useRef(0);

  // Replay advances the playhead at `speed`× until it reaches now, then goes live.
  useEffect(() => {
    if (tl.mode !== 'replay') return;
    let last = performance.now();
    const step = () => {
      const now = performance.now();
      const s = getStore().getState();
      const t = (s.timeline.t ?? Date.now()) + (now - last) * s.timeline.speed;
      last = now;
      if (t >= Date.now()) dispatch({ type: 'timeline', patch: { mode: 'live' } });
      else {
        dispatch({ type: 'timeline', patch: { t } });
        raf.current = requestAnimationFrame(step);
      }
    };
    raf.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf.current);
  }, [tl.mode]);

  const start = range ? Math.min(range[0], Date.now() - 60_000) : Date.now() - 60_000;
  const end = Date.now();
  const marks = useMemo(() => {
    const step = Math.max(1, Math.floor(presence.length / 300));
    const out: { x: number; actor: number; key: number }[] = [];
    for (let k = 0; k < presence.length; k += step) out.push({ x: presence[k].ts_ms, actor: presence[k].actor, key: k });
    return out;
  }, [presence]);
  const head = tl.t ?? end;
  const frac = (t: number) => Math.min(1, Math.max(0, (t - start) / Math.max(1, end - start)));

  return (
    <div className="cs-panel cs-timeline" aria-label="Timeline">
      <button title="Rewind 30 s" onClick={() => dispatch({ type: 'timeline', patch: { mode: 'paused', t: (tl.t ?? Date.now()) - 30_000 } })}>
        ⏪
      </button>
      {tl.mode === 'replay' ? (
        <button title="Pause" onClick={() => dispatch({ type: 'timeline', patch: { mode: 'paused', t: tl.t ?? Date.now() } })}>
          ⏸
        </button>
      ) : (
        <button title={`Replay at ${tl.speed}×`} onClick={() => dispatch({ type: 'timeline', patch: { mode: 'replay', t: tl.t ?? start } })}>
          ▶
        </button>
      )}
      <div className="cs-track">
        {marks.map((m) => (
          <span key={m.key} className="cs-mark" style={{ left: `${frac(m.x) * 100}%`, background: actorCss(m.actor) }} />
        ))}
        <input
          type="range"
          min={0}
          max={1000}
          value={Math.round(frac(head) * 1000)}
          aria-label="Playhead"
          onChange={(e) => {
            const t = start + (Number(e.target.value) / 1000) * (end - start);
            dispatch({ type: 'timeline', patch: t >= end - 500 ? { mode: 'live' } : { mode: 'paused', t } });
          }}
        />
      </div>
      <span className="cs-mono cs-dim">{tl.mode === 'live' ? 'LIVE' : fmt(head)}</span>
      <button className={tl.mode === 'live' ? 'cs-on' : ''} title="Back to live" onClick={() => dispatch({ type: 'timeline', patch: { mode: 'live' } })}>
        live
      </button>
    </div>
  );
}
