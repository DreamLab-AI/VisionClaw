/**
 * Node-filter settings split into REMOVAL and GREY-OUT (ADR-2183 D2, D4).
 *
 * The split (characterised in hooks/__tests__/useGraphFiltering.test.ts):
 *
 *  - removal (population scope, D4): the linked_page gate. The server applies the
 *    same `include_linked_pages` scope, so the client gate only mirrors it and may
 *    never reveal what the server scoped out. Hierarchy collapse also stays a
 *    removal, but it needs the expansion state and is applied by the hook.
 *  - grey-out (interactive, D2): quality / authority (AND / OR), minConnections and
 *    minMaturity.
 *
 * Semantics (ADR-2183 implementation notes, 2026-11-06): only ENABLED predicates
 * combine. AND greys a node that fails any enabled predicate, OR greys one that
 * fails every enabled predicate, and with no predicate enabled nothing is greyed.
 * (Before, a disabled predicate passed vacuously, so OR with quality alone greyed
 * nothing.) A node with no authored score is neutral to that predicate (quality
 * and authority alike). The server (`src/actors/client_filter.rs`) and XR
 * (`xr-client/rust/src/settings_sync.rs`) copies apply the same rule, pinned by
 * the shared fixture `tests/node-filter-parity.json`.
 */
import type { Node as GraphNode } from '../managers/graphDataManager';
import type { NodeFilterSettings } from '../../settings/config/settings';

/** Ordinal OWL maturity rank (same table as before; tableSchema's MATURITY_ORDER). */
export const MATURITY_RANK: Readonly<Record<string, number>> = Object.freeze({
  draft: 1, developing: 2, emerging: 3, growing: 4, established: 5, mature: 6,
});

/** Settings with the client fallbacks applied. */
export interface ResolvedNodeFilter {
  enabled: boolean;
  /** Opt-in: the server also REMOVES non-matching nodes from this client's graph. */
  serverRemove: boolean;
  qualityThreshold: number;
  authorityThreshold: number;
  filterByQuality: boolean;
  filterByAuthority: boolean;
  filterMode: 'and' | 'or';
  includeLinkedPages: boolean;
  minConnections: number;
  minMaturityRank: number;
}

/** The node-filter fields read here (the store hands back a deep partial). */
export type NodeFilterInput = Partial<Pick<NodeFilterSettings,
  'enabled' | 'qualityThreshold' | 'authorityThreshold' | 'filterByQuality' | 'filterByAuthority'
  | 'filterMode' | 'includeLinkedPages' | 'minConnections' | 'minMaturity' | 'serverRemove'>>;

/**
 * Shipped node-filter defaults, mirrored by `NodeFilterSettings::default()`
 * (src/settings/models.rs) and the registry. Off by default: the live-corpus
 * analysis found the authored quality score greys hubs (Metaverse 0.35, degree
 * 882) and is no better than random above 0.5. Switched on, 0.4 greys only the
 * bottom authored band (0.35: 744 of 9,688 nodes, mostly drafts).
 */
export const NODE_FILTER_DEFAULTS = Object.freeze({
  enabled: false,
  serverRemove: false,
  qualityThreshold: 0.4,
  authorityThreshold: 0.5,
  filterByQuality: true,
  filterByAuthority: false,
  filterMode: 'or' as const,
  includeLinkedPages: false,
  minConnections: 0,
  minMaturity: 'off' as string,
});

const toScore = (v: unknown): number | undefined => {
  const n = typeof v === 'string' ? parseFloat(v) : typeof v === 'number' ? v : NaN;
  return Number.isFinite(n) ? n : undefined;
};

export function resolveNodeFilter(s: NodeFilterInput | undefined | null): ResolvedNodeFilter {
  const rawMin = s?.minMaturity ?? '';
  return {
    enabled: s?.enabled ?? NODE_FILTER_DEFAULTS.enabled,
    serverRemove: s?.serverRemove ?? NODE_FILTER_DEFAULTS.serverRemove,
    qualityThreshold: s?.qualityThreshold ?? NODE_FILTER_DEFAULTS.qualityThreshold,
    authorityThreshold: s?.authorityThreshold ?? NODE_FILTER_DEFAULTS.authorityThreshold,
    filterByQuality: s?.filterByQuality ?? NODE_FILTER_DEFAULTS.filterByQuality,
    filterByAuthority: s?.filterByAuthority ?? NODE_FILTER_DEFAULTS.filterByAuthority,
    filterMode: s?.filterMode ?? NODE_FILTER_DEFAULTS.filterMode,
    includeLinkedPages: s?.includeLinkedPages ?? NODE_FILTER_DEFAULTS.includeLinkedPages,
    minConnections: s?.minConnections ?? 0,
    minMaturityRank: rawMin && rawMin !== 'off' ? MATURITY_RANK[rawMin] ?? 0 : 0,
  };
}

/** The authoritative population origin: metadata.type, legacy top-level fallbacks. */
export function nodePopulationType(node: GraphNode): string {
  return (node.metadata?.type as string | undefined)
    || (node as unknown as { type?: string }).type
    || (node.metadata?.nodeType as string | undefined)
    || (node as unknown as { nodeType?: string }).nodeType
    || '';
}

/** Population-scope removal (ADR-2183 D4): a linked_page stub while they are excluded. */
export function isRemovedByScope(node: GraphNode, f: Pick<ResolvedNodeFilter, 'includeLinkedPages'>): boolean {
  return !f.includeLinkedPages && nodePopulationType(node) === 'linked_page';
}

/**
 * True when an interactive node-filter predicate greys this node out
 * (ADR-2183 D2). See the module comment for the combination rule.
 */
export function isGreyedByNodeFilter(
  node: GraphNode,
  f: ResolvedNodeFilter,
  connectionCountMap: ReadonlyMap<string, number>,
): boolean {
  const id = String(node.id);
  if (f.minConnections > 0 && (connectionCountMap.get(id) || 0) < f.minConnections) return true;

  if (f.minMaturityRank > 0) {
    const m = node.metadata?.maturity as string | undefined;
    const rank = m ? MATURITY_RANK[m] : undefined;
    if (rank !== undefined && rank < f.minMaturityRank) return true;
  }

  if (!f.enabled) return false;

  // Each enabled predicate's verdict; a disabled one contributes nothing.
  const verdicts: boolean[] = [];
  if (f.filterByQuality) {
    // Authored score (pages: quality_score; ontology: qualityScore). Unscored is neutral.
    const q = toScore(node.metadata?.quality_score ?? node.metadata?.quality ?? node.metadata?.qualityScore);
    verdicts.push(q === undefined || q >= f.qualityThreshold);
  }
  if (f.filterByAuthority) {
    // The server's key is authority_score. Unscored is neutral, as for quality.
    const a = toScore(node.metadata?.authority_score ?? node.metadata?.authority ?? node.metadata?.authorityScore);
    verdicts.push(a === undefined || a >= f.authorityThreshold);
  }
  if (verdicts.length === 0) return false;
  return f.filterMode === 'and' ? verdicts.some((p) => !p) : verdicts.every((p) => !p);
}
