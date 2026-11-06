/**
 * Node-filter settings split into REMOVAL and GREY-OUT (ADR-2183 D2, D4).
 *
 * The predicates are those useGraphFiltering applied before ADR-2183, unchanged
 * (characterised in hooks/__tests__/useGraphFiltering.characterisation.test.ts):
 *
 *  - removal (population scope, D4): the linked_page gate. The server applies the
 *    same `include_linked_pages` scope, so the client gate only mirrors it and may
 *    never reveal what the server scoped out. Hierarchy collapse also stays a
 *    removal, but it needs the expansion state and is applied by the hook.
 *  - grey-out (interactive, D2): quality / authority (AND / OR), minConnections and
 *    minMaturity.
 *
 * The OR arm is ported as-is: a disabled predicate passes vacuously, so under the
 * server default (quality on, authority off, OR) nothing is greyed. That matches
 * the server's `client_filter.rs` and is reported, not "fixed", here.
 */
import type { Node as GraphNode } from '../managers/graphDataManager';
import type { NodeFilterSettings } from '../../settings/config/settings';
import type { HierarchyNode } from '../utils/hierarchyDetector';

/** Ordinal OWL maturity rank (same table as before; tableSchema's MATURITY_ORDER). */
export const MATURITY_RANK: Readonly<Record<string, number>> = Object.freeze({
  draft: 1, developing: 2, emerging: 3, growing: 4, established: 5, mature: 6,
});

/** Settings with the client fallbacks useGraphFiltering has always used. */
export interface ResolvedNodeFilter {
  enabled: boolean;
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
  | 'filterMode' | 'includeLinkedPages' | 'minConnections' | 'minMaturity'>>;

export function resolveNodeFilter(s: NodeFilterInput | undefined | null): ResolvedNodeFilter {
  const rawMin = s?.minMaturity ?? '';
  return {
    enabled: s?.enabled ?? false,
    qualityThreshold: s?.qualityThreshold ?? 0.7,
    authorityThreshold: s?.authorityThreshold ?? 0.5,
    filterByQuality: s?.filterByQuality ?? true,
    filterByAuthority: s?.filterByAuthority ?? false,
    filterMode: s?.filterMode ?? 'or',
    includeLinkedPages: s?.includeLinkedPages ?? false,
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
 * True when an interactive node-filter predicate greys this node out. Identical
 * to the removal predicate useGraphFiltering applied before ADR-2183.
 */
export function isGreyedByNodeFilter(
  node: GraphNode,
  f: ResolvedNodeFilter,
  connectionCountMap: ReadonlyMap<string, number>,
  hierarchyMap: ReadonlyMap<string, HierarchyNode>,
): boolean {
  const id = String(node.id);
  if (f.minConnections > 0 && (connectionCountMap.get(id) || 0) < f.minConnections) return true;

  if (f.minMaturityRank > 0) {
    const m = node.metadata?.maturity as string | undefined;
    const rank = m ? MATURITY_RANK[m] : undefined;
    if (rank !== undefined && rank < f.minMaturityRank) return true;
  }

  if (!f.enabled) return false;

  // Authored score first (pages: quality_score; ontology: qualityScore); unscored
  // nodes fall back to degree / 10.
  let quality: unknown = node.metadata?.quality_score ?? node.metadata?.quality ?? node.metadata?.qualityScore;
  if (typeof quality === 'string') quality = parseFloat(quality);
  if (quality === undefined || quality === null || Number.isNaN(quality)) {
    quality = Math.min(1.0, (connectionCountMap.get(id) || 0) / 10);
  }
  let authority: unknown = node.metadata?.authority ?? node.metadata?.authorityScore;
  if (authority === undefined || authority === null) {
    authority = Math.max(0, 1.0 - (hierarchyMap.get(id)?.depth ?? 0) * 0.2);
  }
  const passesQuality = !f.filterByQuality || (quality as number) >= f.qualityThreshold;
  const passesAuthority = !f.filterByAuthority || (authority as number) >= f.authorityThreshold;

  if (f.filterMode === 'and') return !passesQuality || !passesAuthority;
  // OR: a disabled predicate passes vacuously (see the module comment).
  if (f.filterByQuality || f.filterByAuthority) return !passesQuality && !passesAuthority;
  return false;
}
