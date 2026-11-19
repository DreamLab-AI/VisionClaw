/**
 * Emphasise the nodes of named domains (bridge intent). Uses the explore lane's
 * emphasis writer on its own `emph:domain` source, so it composes with selection,
 * hover and search, and is cleared by writing null to the same source.
 */
import { graphDataManager } from '../../graph/managers/graphDataManager';
import { writeEmphasis, EMPHASIS_PREFIX } from '../../graph/explore/filterRegistry';
import { HIGHLIGHT_SEARCH_HIT } from '../../graph/state/highlightMask';

export const DOMAIN_EMPHASIS_SOURCE = `${EMPHASIS_PREFIX}domain`;

const norm = (s: unknown) => String(s ?? '').toLowerCase().replace(/[\s_]+/g, '-').trim();

/** Ids of nodes whose `metadata.domain` or `source_domain` equals one of `domains`. */
export function matchDomainNodeIds(
  nodes: ReadonlyArray<{ id: string | number; metadata?: Record<string, unknown> }>,
  domains: readonly string[],
): string[] {
  const wanted = new Set(domains.map(norm));
  const ids: string[] = [];
  for (const n of nodes) {
    const md = n.metadata ?? {};
    if (wanted.has(norm(md.domain)) || wanted.has(norm(md.source_domain)) || wanted.has(norm(md.sourceDomain))) {
      ids.push(String(n.id));
    }
  }
  return ids;
}

/** Light up the two domains; returns how many nodes matched. */
export async function highlightDomains(domains: readonly string[]): Promise<number> {
  const data = await graphDataManager.getGraphData();
  const ids = matchDomainNodeIds(data.nodes as never, domains);
  writeEmphasis(DOMAIN_EMPHASIS_SOURCE, HIGHLIGHT_SEARCH_HIT, ids.length ? ids : null);
  return ids.length;
}
