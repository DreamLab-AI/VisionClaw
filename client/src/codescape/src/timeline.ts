/**
 * Client-side presence replay over the append-only event log the store keeps.
 * Heat decays exponentially with the collab crate's half-life (60 s).
 */
import type { PresenceEvent } from './protocol';

export const HEAT_HALF_LIFE_MS = 60_000;

/** Heat per node id at time `t`: Σ 0.5^((t - ts)/halfLife) over events at or before `t`, clamped to 1. */
export function heatAt(events: readonly PresenceEvent[], t: number, halfLifeMs = HEAT_HALF_LIFE_MS): Map<number, number> {
  const heat = new Map<number, number>();
  const horizon = t - halfLifeMs * 8;
  for (const e of events) {
    if (e.ts_ms > t || e.ts_ms < horizon) continue;
    const w = Math.pow(0.5, (t - e.ts_ms) / halfLifeMs) * (e.kind === 'update' || e.kind === 'create' ? 1 : 0.6);
    for (const id of e.node_ids) heat.set(id, Math.min(1, (heat.get(id) ?? 0) + w));
  }
  return heat;
}

/** Each actor's last touched node at or before `t`. */
export function actorFocusAt(events: readonly PresenceEvent[], t: number): Map<number, number> {
  const focus = new Map<number, number>();
  for (const e of events) {
    if (e.ts_ms > t) break;
    if (e.node_ids.length) focus.set(e.actor, e.node_ids[e.node_ids.length - 1]);
  }
  return focus;
}

/** The last `n` distinct nodes an actor touched at or before `t`, oldest first, with event times. */
export function trailOf(events: readonly PresenceEvent[], actor: number, t: number, n: number, windowMs = 120_000): { id: number; ts: number }[] {
  const out: { id: number; ts: number }[] = [];
  const seen = new Set<number>();
  for (let k = events.length - 1; k >= 0 && out.length < n; k--) {
    const e = events[k];
    if (e.actor !== actor || e.ts_ms > t) continue;
    if (e.ts_ms < t - windowMs) break;
    for (let j = e.node_ids.length - 1; j >= 0 && out.length < n; j--) {
      const id = e.node_ids[j];
      if (!seen.has(id)) {
        seen.add(id);
        out.push({ id, ts: e.ts_ms });
      }
    }
  }
  return out.reverse();
}

/** Time range covered by the log, or null when empty. */
export function logRange(events: readonly PresenceEvent[]): [number, number] | null {
  if (!events.length) return null;
  return [events[0].ts_ms, events[events.length - 1].ts_ms];
}
