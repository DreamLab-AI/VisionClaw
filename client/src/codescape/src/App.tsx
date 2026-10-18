import { useEffect } from 'react';
import { SceneView } from './scene/Scene';
import { Session } from './session';
import { useCs } from './store';
import { Captions } from './ui/Captions';
import { CommandPalette } from './ui/CommandPalette';
import { Hud, Inspector } from './ui/Hud';
import { Legend } from './ui/Legend';
import { PttButton } from './ui/PttButton';
import { SpacePilotButton } from './ui/SpacePilotButton';
import { Timeline } from './ui/Timeline';

function Connecting({ session }: { session: Session }) {
  const ev = useCs((s) => s.events);
  const pos = useCs((s) => s.positions);
  const snapErr = useCs((s) => s.snapshotError);
  return (
    <div className="cs-connecting" role="status">
      <div className="cs-spinner" />
      <h1>Connecting to {session.endpoints.label}</h1>
      <ul>
        <li>
          snapshot: <b>{snapErr ? `retrying — ${snapErr}` : 'loading'}</b>
        </li>
        <li>
          /ws/events: <b>{ev.status}</b>
          {ev.attempts > 0 ? ` (attempt ${ev.attempts})` : ''}
        </li>
        <li>
          /ws/positions: <b>{pos.status}</b>
          {pos.attempts > 0 ? ` (attempt ${pos.attempts})` : ''}
        </li>
      </ul>
      <p className="cs-dim">
        Override the server with <code>?server=http://host:4100</code>, or run without one: <a href="?demo=1">?demo=1</a>
      </p>
    </div>
  );
}

function Shell({ session }: { session: Session }) {
  const model = useCs((s) => s.model);
  const demo = useCs((s) => s.demo);
  return (
    <div className="cs-root">
      {model ? <SceneView session={session} /> : <Connecting session={session} />}
      {model && (
        <>
          <Hud session={session} />
          <Inspector />
          <Legend />
          <CommandPalette session={session} />
          <div className="cs-bottom">
            <Captions />
            <div className="cs-bottom-row">
              <Timeline />
              <SpacePilotButton />
              <PttButton session={session} />
            </div>
          </div>
          {demo && <div className="cs-demo-badge">DEMO · synthetic 5k-node graph</div>}
        </>
      )}
    </div>
  );
}

export function App({ session }: { session: Session }) {
  useEffect(() => {
    session.start();
    return () => session.stop();
  }, [session]);
  return <Shell session={session} />;
}
