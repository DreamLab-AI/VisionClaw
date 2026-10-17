/** Push-to-talk: hold the button (or the backquote key) to take the floor and stream mic audio. */
import { useEffect, useMemo, useRef, useState } from 'react';
import { voiceAvailable, VoiceClient } from '../audio/voice';
import type { Session } from '../session';
import { parseServerEvent } from '../protocol';
import { dispatch, useCs } from '../store';

export function PttButton({ session }: { session: Session }) {
  const avail = useMemo(() => voiceAvailable(), []);
  const demo = useCs((s) => s.demo);
  const floor = useCs((s) => s.floor);
  const [talking, setTalking] = useState(false);
  const [level, setLevel] = useState(0);
  const [err, setErr] = useState<string | null>(null);
  const client = useRef<VoiceClient | null>(null);
  const disabled = !avail.ok || demo;
  const reason = !avail.ok ? avail.reason : demo ? 'Voice is not available in demo mode (no server).' : floor.state === 'busy' ? 'The shared ASR is busy.' : 'Hold to talk (or hold `)';

  const begin = () => {
    if (disabled || talking) return;
    if (!client.current) {
      client.current = new VoiceClient(session.socketUrl('voice'));
      client.current.onLevel = setLevel;
      // floor / query / error frames share the ServerEvent shapes: feed them to the store.
      client.current.onMessage = (m) => {
        const ev = parseServerEvent(JSON.stringify(m));
        if (ev) dispatch({ type: 'server', event: ev, nowMs: Date.now() });
      };
    }
    setTalking(true);
    setErr(null);
    client.current.start().catch((e: unknown) => {
      setErr(e instanceof Error ? e.message : String(e));
      setTalking(false);
      client.current?.stop();
    });
  };
  const end = () => {
    if (!talking) return;
    setTalking(false);
    client.current?.stop();
  };

  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.key === '`' && !e.repeat && !(document.activeElement instanceof HTMLInputElement)) begin();
    };
    const up = (e: KeyboardEvent) => {
      if (e.key === '`') end();
    };
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    return () => {
      window.removeEventListener('keydown', down);
      window.removeEventListener('keyup', up);
    };
  });

  useEffect(() => () => client.current?.dispose(), []);

  return (
    <div className="cs-ptt-wrap" title={err ?? reason}>
      <button
        className={`cs-ptt${talking ? ' on' : ''}`}
        disabled={disabled}
        aria-pressed={talking}
        aria-label="Push to talk"
        onPointerDown={(e) => {
          (e.target as HTMLElement).setPointerCapture(e.pointerId);
          begin();
        }}
        onPointerUp={end}
        onPointerCancel={end}
        style={{ boxShadow: talking ? `0 0 0 ${2 + Math.min(14, level * 120)}px rgba(255,120,80,0.35)` : undefined }}
      >
        🎙
      </button>
      {err && <span className="cs-warn">{err}</span>}
    </div>
  );
}
