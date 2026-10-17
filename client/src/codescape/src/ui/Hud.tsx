/** Top-left stats and the selection inspector. */
import { kindName } from '../protocol';
import type { Session } from '../session';
import { useCs } from '../store';

function floorText(f: ReturnType<typeof useFloor>, labelOf: (id: number) => string): string {
  if (f.state === 'free') return 'free';
  if (f.state === 'busy') return 'busy (shared ASR)';
  return `${labelOf(f.holder)}${f.queue.length ? ` +${f.queue.length} waiting` : ''}`;
}
const useFloor = () => useCs((s) => s.floor);

export function Hud({ session }: { session: Session }) {
  const hud = useCs((s) => s.hud);
  const model = useCs((s) => s.model);
  const hello = useCs((s) => s.hello);
  const floor = useFloor();
  const actors = useCs((s) => s.actors);
  const tier = useCs((s) => s.currentTier);
  const tierMode = useCs((s) => s.tierMode);
  const ev = useCs((s) => s.events.status);
  const pos = useCs((s) => s.positions.status);
  const labelOf = (id: number) => actors[id]?.label ?? `#${id}`;
  const nodes = model?.n ?? hello?.nodes ?? 0;
  const edges = model?.snapshot.edges.length ?? hello?.edges ?? 0;
  return (
    <div className="cs-panel cs-hud" aria-label="Status">
      <div className="cs-hud-title">
        Codescape <span className="cs-dim">{hello?.repo ?? model?.snapshot.repo ?? ''}</span>
      </div>
      <dl>
        <dt>nodes</dt>
        <dd>{nodes.toLocaleString()}</dd>
        <dt>edges</dt>
        <dd>{edges.toLocaleString()}</dd>
        <dt>revision</dt>
        <dd>{hello?.revision ?? model?.snapshot.revision ?? '–'}</dd>
        <dt>engine</dt>
        <dd>
          {hello?.engine ?? '–'}
          {hud.settled ? ' · settled' : ''}
        </dd>
        <dt>fps</dt>
        <dd className={hud.fps < 30 ? 'cs-warn' : ''}>{hud.fps}</dd>
        <dt>lag</dt>
        <dd className={hud.lag > 2 ? 'cs-warn' : ''}>
          {hud.lag} <span className="cs-dim">seq {hud.lastSeq}</span>
        </dd>
        <dt>tier</dt>
        <dd>
          {tier.toUpperCase()}
          {tierMode !== 'auto' ? ` (pinned ${tierMode.toUpperCase()})` : ''}
        </dd>
        <dt>floor</dt>
        <dd>{floorText(floor, labelOf)}</dd>
        <dt>links</dt>
        <dd>
          <span className={`cs-led ${ev}`} title={`events: ${ev}`} />
          <span className={`cs-led ${pos}`} title={`positions: ${pos}`} />
          <span className="cs-dim">{session.pipeline.shared ? 'shared mem' : 'main-thread decode'}</span>
        </dd>
      </dl>
      {hud.progress && (
        <div className="cs-dim">
          {hud.progress.phase} {hud.progress.done}
          {hud.progress.total ? `/${hud.progress.total}` : ''}
        </div>
      )}
      {hud.unknownRecords > 0 && <div className="cs-warn">{hud.unknownRecords} unknown ids in last frame — resyncing</div>}
    </div>
  );
}

export function Inspector() {
  const model = useCs((s) => s.model);
  const sel = useCs((s) => s.selection);
  const follow = useCs((s) => s.follow);
  const actors = useCs((s) => s.actors);
  if (!model || sel === null) return null;
  const i = model.index.get(sel);
  if (i === undefined) return null;
  const n = model.snapshot.nodes[i];
  const degree = model.adjStart[i + 1] - model.adjStart[i];
  return (
    <div className="cs-panel cs-inspector" aria-label="Selection">
      <div className="cs-insp-label">{n.label}</div>
      <div className="cs-dim">
        {kindName(n.kind)} · {n.lang.replace('_', '')} · {n.tier.toUpperCase()}
      </div>
      {n.path && (
        <div className="cs-mono">
          {n.path}
          {n.span ? `:${n.span.start_line}-${n.span.end_line}` : ''}
        </div>
      )}
      <div className="cs-dim">
        community {n.community} · {degree} links · importance {n.importance.toFixed(2)}
      </div>
      {follow !== null && <div className="cs-accent">following {actors[follow]?.label ?? follow}</div>}
    </div>
  );
}
