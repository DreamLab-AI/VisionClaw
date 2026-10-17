/** Bottom caption strip: the latest line per speaker (speech bubbles live in the scene). */
import { useEffect, useState } from 'react';
import { actorCss } from '../scene/Presence';
import { useCs } from '../store';

export const CAPTION_TTL_MS = 8000;

export function Captions() {
  const captions = useCs((s) => s.captions);
  const actors = useCs((s) => s.actors);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  const live = Object.values(captions)
    .filter((c) => now - c.ts_ms < CAPTION_TTL_MS || c.partial)
    .sort((a, b) => a.ts_ms - b.ts_ms)
    .slice(-3);
  if (!live.length) return null;
  return (
    <div className="cs-captions" role="log" aria-live="polite">
      {live.map((c) => (
        <div key={c.actor} className={`cs-caption${c.partial ? ' partial' : ''}`}>
          <span className="cs-speaker" style={{ color: actorCss(c.actor) }}>
            {actors[c.actor]?.label ?? `#${c.actor}`}
          </span>
          {c.text}
        </div>
      ))}
    </div>
  );
}
