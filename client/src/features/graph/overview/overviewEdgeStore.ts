/**
 * Overview edge capacity report (ADR-2181 D4: no silent cap).
 *
 * The overview edge renderer publishes what it allocated and drew; the menus
 * lane mounts `OverviewEdgeNotice` (or reads `useOverviewEdgeReport`) to show
 * "N of M edges drawn" in the canvas whenever `truncated` is true.
 */
import { useSyncExternalStore } from 'react';

export interface OverviewEdgeReport {
  /** Buffer size allocated (edges). */
  capacity: number;
  /** Edges drawn. */
  drawn: number;
  /** Renderable edges in the current graph (after server scope and the render set). */
  total: number;
  /** True when drawn < total: the notice must be shown. */
  truncated: boolean;
}

const EMPTY: OverviewEdgeReport = { capacity: 0, drawn: 0, total: 0, truncated: false };

let current: OverviewEdgeReport = EMPTY;
const listeners = new Set<() => void>();

/** Publish a report; a no-op when nothing changed (keeps the snapshot stable). */
export function publishOverviewEdgeReport(r: OverviewEdgeReport): void {
  if (
    r.capacity === current.capacity && r.drawn === current.drawn &&
    r.total === current.total && r.truncated === current.truncated
  ) return;
  current = { capacity: r.capacity, drawn: r.drawn, total: r.total, truncated: r.truncated };
  for (const l of listeners) l();
}

export function getOverviewEdgeReport(): OverviewEdgeReport {
  return current;
}

export function subscribeOverviewEdgeReport(l: () => void): () => void {
  listeners.add(l);
  return () => { listeners.delete(l); };
}

/** Reset to empty (tests; overview edges unmounted). */
export function resetOverviewEdgeReport(): void {
  publishOverviewEdgeReport(EMPTY);
}

/** React binding: re-renders when the report changes. */
export function useOverviewEdgeReport(): OverviewEdgeReport {
  return useSyncExternalStore(subscribeOverviewEdgeReport, getOverviewEdgeReport, getOverviewEdgeReport);
}

/** The notice text, or null when nothing is hidden. */
export function overviewEdgeNoticeText(r: OverviewEdgeReport): string | null {
  if (!r.truncated) return null;
  const fmt = (n: number) => n.toLocaleString('en-GB');
  return `${fmt(r.drawn)} of ${fmt(r.total)} edges drawn (highest weight first)`;
}
