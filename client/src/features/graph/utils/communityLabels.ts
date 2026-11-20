/**
 * ADR-2206 — community labels, cores and the rules that keep a label tied to the
 * community ids the client currently holds.
 *
 * Pure helpers (no React, no three) so the rules are unit-testable; the
 * ClusterHulls layer only wires them to the scene. Source of truth is
 * `GET /api/analytics/communities/labels` (docs/PROTOCOL-registry.md).
 */
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';
import { communityHsl } from '../state/analyticColours';

export interface DomainShare { domain: string; share: number }
export interface Exemplar { id: number; label: string }

export interface CommunityLabelEntry {
  /** The id on the wire (`community_id`) for the run the labels describe. */
  communityId: number;
  /** Stable across detection runs (Jaccard match); colours key on this. */
  stableId: number;
  /** Members the caller may see; equals the client's own count when fresh. */
  size: number;
  coreSize: number;
  /** Visible core members, the only points a hull is drawn around. */
  coreIds: number[];
  label: string;
  terms: string[];
  domainMix: DomainShare[];
  crossDomain: boolean;
  exemplars: Exemplar[];
  hasHull: boolean;
}

export interface CommunityLabelsResponse {
  generation: number;
  communities: CommunityLabelEntry[];
}

function isLabels(v: unknown): v is CommunityLabelsResponse {
  const r = v as CommunityLabelsResponse | null;
  return !!r && typeof r.generation === 'number' && Array.isArray(r.communities);
}

export async function fetchCommunityLabels(): Promise<CommunityLabelsResponse | null> {
  try {
    const res = await unifiedApiClient.get<unknown>('/analytics/communities/labels', { timeout: 10000 });
    // `res.data` is the HTTP body, which the server wraps as {success, data, ...}.
    const raw = (res as { data?: { data?: unknown } }).data;
    const body = raw && typeof raw === 'object' && 'data' in raw ? raw.data : raw;
    if (isLabels(body)) return body;
  } catch {
    // fail-open: no labels, hulls simply do not draw
  }
  return null;
}

/** A response older than the one already applied is ignored (out-of-order fetches). */
export function acceptLabels(
  current: CommunityLabelsResponse | null,
  incoming: CommunityLabelsResponse | null,
): CommunityLabelsResponse | null {
  if (!incoming) return current;
  if (current && incoming.generation < current.generation) return current;
  return incoming;
}

/**
 * Count the nodes the client holds per community id (id > 0).
 * `communityOf` yields the node's wire community id, or 0/NaN when none.
 */
export function localCommunityCounts(
  nodeCount: number,
  communityOf: (i: number) => number,
): Map<number, number> {
  const m = new Map<number, number>();
  for (let i = 0; i < nodeCount; i++) {
    const c = communityOf(i);
    if (c > 0) m.set(c, (m.get(c) ?? 0) + 1);
  }
  return m;
}

/** Stable text for "has the partition changed since the labels were fetched". */
export function partitionSignature(counts: Map<number, number>): string {
  return [...counts.entries()].sort((a, b) => a[0] - b[0]).map(([c, n]) => `${c}:${n}`).join(',');
}

export interface CommunityLayers {
  /** Communities to draw a hull around (core members only). */
  hulls: CommunityLabelEntry[];
  /** Communities with a label but no hull: a chip at the members' centroid. */
  chips: CommunityLabelEntry[];
  /** Entries dropped because they describe a different detection run than the client holds. */
  stale: number;
}

/** What the client holds: each node's wire community id (`undefined` = node not held), and per-id counts. */
export interface LocalCommunities {
  communityOf: (nodeId: number) => number | undefined;
  counts: Map<number, number>;
}

/** Fraction of resolvable sample ids that must still carry the entry's community id. */
const FRESH_SHARE = 0.9;

/**
 * An entry is fresh when the nodes the client holds agree with it: of the core
 * and exemplar ids the client can resolve, at least 90% still carry the entry's
 * `communityId`. With nothing to sample (a chip with no exemplars) the client's
 * own count must be non-zero and no larger than the server's `size`.
 *
 * The server `size` counts every member the caller may see, pages and domain
 * roots included, so a client that filters node types holds fewer; equality of
 * counts would call every entry stale.
 */
function isFresh(e: CommunityLabelEntry, local: LocalCommunities): boolean {
  let held = 0;
  let agree = 0;
  for (const id of [...e.coreIds, ...e.exemplars.map((x) => x.id)]) {
    const c = local.communityOf(id);
    if (c === undefined) continue;
    held++;
    if (c === e.communityId) agree++;
  }
  if (held > 0) return agree / held >= FRESH_SHARE;
  const n = local.counts.get(e.communityId) ?? 0;
  return n > 0 && n <= e.size;
}

/** Decide what to draw; a stale entry draws nothing and asks for a refetch. */
export function planCommunityLayers(
  labels: CommunityLabelsResponse | null,
  local: LocalCommunities,
): CommunityLayers {
  const out: CommunityLayers = { hulls: [], chips: [], stale: 0 };
  if (!labels) return out;
  for (const e of labels.communities) {
    if (!isFresh(e, local)) { out.stale++; continue; }
    if (e.hasHull && e.coreIds.length > 0) out.hulls.push(e);
    else out.chips.push(e);
  }
  return out;
}

/** Hull and label colour: keyed on the stable id so it survives a re-run. */
export function stableHslFor(entry: Pick<CommunityLabelEntry, 'stableId' | 'communityId'>): ReturnType<typeof communityHsl> {
  return communityHsl(entry.stableId > 0 ? entry.stableId : entry.communityId);
}

/** Centroid of the points `ids` map to in `positions` (xyz triples); null if none resolve. */
export function centroidOf(
  ids: ReadonlyArray<number | string>,
  positions: Float32Array,
  indexOf: (id: string) => number | undefined,
): [number, number, number] | null {
  let x = 0, y = 0, z = 0, n = 0;
  for (const id of ids) {
    const idx = indexOf(String(id));
    if (idx === undefined) continue;
    const b = idx * 3;
    if (b + 2 >= positions.length) continue;
    const px = positions[b], py = positions[b + 1], pz = positions[b + 2];
    if (!Number.isFinite(px) || !Number.isFinite(py) || !Number.isFinite(pz)) continue;
    x += px; y += py; z += pz; n++;
  }
  return n === 0 ? null : [x / n, y / n, z / n];
}

/** "AI 62% · Graphics 21%", with a cross-domain marker. */
export function domainMixLine(e: Pick<CommunityLabelEntry, 'domainMix' | 'crossDomain'>): string {
  const parts = e.domainMix.map((d) => `${d.domain.replace(/-/g, ' ')} ${Math.round(d.share * 100)}%`);
  const line = parts.join(' · ');
  return e.crossDomain ? (line ? `${line} · cross-domain` : 'cross-domain') : line;
}
