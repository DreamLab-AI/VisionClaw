/**
 * Pure analysis behind the knowledge-engineer insight intents. Everything here
 * works on the graph data the client already holds (nodes, edges, wire
 * centrality) and returns ids plus a few labelled concepts for the answer.
 *
 * Domain roots are system-made navigation nodes (`metadata.type = 'domain_root'`,
 * `domain_member` spokes, no `domain` of their own); they are never authored
 * concepts, so none of these functions ever returns one.
 */
import { MATURITY_RANK } from '../../graph/explore/nodeFilter';

export interface AnalysisNode {
  id: string | number;
  label?: string;
  nodeType?: string;
  metadata?: Record<string, unknown>;
}
export interface AnalysisEdge {
  id?: string;
  source: string | number;
  target: string | number;
  edgeType?: string;
  label?: string;
  metadata?: Record<string, unknown>;
}
export interface Ranked { id: string; label: string; count: number }

const norm = (s: unknown) => String(s ?? '').toLowerCase().replace(/[\s_]+/g, '-').trim();
const labelOf = (n: AnalysisNode) => n.label || String(n.id);

/** System-made node (domain root or anything with derived provenance), not an authored concept. */
export function isDerivedNode(n: AnalysisNode): boolean {
  const md = n.metadata ?? {};
  return (
    n.nodeType === 'domain_root' ||
    norm(md.type) === 'domain-root' ||
    norm(md.node_type) === 'domain-root' ||
    norm(md.provenance) === 'derived'
  );
}

const domainsOf = (n: AnalysisNode): string[] => {
  const md = n.metadata ?? {};
  return [md.domain, md.source_domain, md.sourceDomain].map(norm).filter(Boolean);
};

const edgeKind = (e: AnalysisEdge) => norm(e.edgeType ?? e.label ?? e.metadata?.relationship_type ?? e.metadata?.relation);

/** Concepts at the ends of edges that cross between two domains, ranked by crossing-edge count. */
export function bridgeSet(
  nodes: readonly AnalysisNode[],
  edges: readonly AnalysisEdge[],
  domains: readonly [string, string] | readonly string[],
): { ids: string[]; crossEdges: number; top: Ranked[] } {
  const [a, b] = [norm(domains[0]), norm(domains[1])];
  const byId = new Map(nodes.map((n) => [String(n.id), n]));
  const counts = new Map<string, number>();
  let crossEdges = 0;
  for (const e of edges) {
    const s = byId.get(String(e.source));
    const t = byId.get(String(e.target));
    if (!s || !t || isDerivedNode(s) || isDerivedNode(t)) continue;
    const ds = domainsOf(s);
    const dt = domainsOf(t);
    const crosses = (ds.includes(a) && dt.includes(b) && !ds.includes(b) && !dt.includes(a)) ||
      (ds.includes(b) && dt.includes(a) && !ds.includes(a) && !dt.includes(b));
    if (!crosses) continue;
    crossEdges++;
    for (const id of [String(s.id), String(t.id)]) counts.set(id, (counts.get(id) ?? 0) + 1);
  }
  const top = [...counts.entries()]
    .map(([id, count]) => ({ id, label: labelOf(byId.get(id)!), count }))
    .sort((x, y) => y.count - x.count || x.label.localeCompare(y.label));
  return { ids: top.map((t) => t.id), crossEdges, top: top.slice(0, 5) };
}

/** Top `n` authored concepts by centrality (derived nodes excluded; zero-centrality never ranks). */
export function authoredHubs(
  nodes: readonly AnalysisNode[],
  centralityOf: (id: string) => number,
  n: number,
): Ranked[] {
  return nodes
    .filter((x) => !isDerivedNode(x))
    .map((x) => ({ id: String(x.id), label: labelOf(x), count: centralityOf(String(x.id)) }))
    .filter((x) => x.count > 0)
    .sort((x, y) => y.count - x.count || x.label.localeCompare(y.label))
    .slice(0, n);
}

/** Important-but-immature: top-decile centrality among authored concepts that are draft/emerging. */
export function gapConcepts(
  nodes: readonly AnalysisNode[],
  centralityOf: (id: string) => number,
): Ranked[] {
  const ranked = authoredHubs(nodes, centralityOf, Number.MAX_SAFE_INTEGER);
  const top = ranked.slice(0, Math.max(1, Math.ceil(ranked.length * 0.1)));
  const byId = new Map(nodes.map((x) => [String(x.id), x]));
  const immature = MATURITY_RANK.emerging;
  return top.filter((r) => {
    const m = byId.get(r.id)?.metadata?.maturity;
    const rank = typeof m === 'string' ? MATURITY_RANK[m.toLowerCase()] : undefined;
    return rank !== undefined && rank <= immature;
  });
}

/** The concept named by `query` plus its parents and children over hierarchical / subClassOf edges. */
export function hierarchyNeighbourhood(
  nodes: readonly AnalysisNode[],
  edges: readonly AnalysisEdge[],
  query: string,
): { focus: Ranked; ids: string[]; parents: Ranked[]; children: Ranked[] } | null {
  const q = norm(query);
  if (!q) return null;
  const authored = nodes.filter((n) => !isDerivedNode(n));
  const focus = authored.find((n) => norm(n.label) === q || norm(n.id) === q) ??
    authored.find((n) => norm(n.label).includes(q));
  if (!focus) return null;
  const byId = new Map(nodes.map((n) => [String(n.id), n]));
  const fid = String(focus.id);
  const parents: Ranked[] = [];
  const children: Ranked[] = [];
  // subClassOf / hierarchical edges point child -> parent.
  for (const e of edges) {
    const kind = edgeKind(e);
    if (kind !== 'subclassof' && kind !== 'hierarchical') continue;
    const s = String(e.source);
    const t = String(e.target);
    if (s === fid && byId.has(t)) parents.push({ id: t, label: labelOf(byId.get(t)!), count: 1 });
    else if (t === fid && byId.has(s)) children.push({ id: s, label: labelOf(byId.get(s)!), count: 1 });
  }
  return {
    focus: { id: fid, label: labelOf(focus), count: 1 },
    ids: [fid, ...parents.map((p) => p.id), ...children.map((c) => c.id)],
    parents,
    children,
  };
}

const list = (xs: readonly string[]) => xs.join(', ');

/** Truthful answer to an anomaly-detection response (`anomalies`, `total`). */
export function anomalyAnswer(
  res: { anomalies?: Array<{ nodeId?: string; node_id?: string; score?: number }>; total?: number } | undefined,
  nodes: readonly AnalysisNode[],
): string {
  const found = res?.anomalies ?? [];
  const total = res?.total ?? found.length;
  if (total === 0) return 'No outliers at the current threshold: every concept\'s links look like its neighbours\'.';
  const byId = new Map(nodes.map((n) => [String(n.id), n]));
  const names = [...found]
    .sort((a, b) => (b.score ?? 0) - (a.score ?? 0))
    .slice(0, 5)
    .map((a) => {
      const id = String(a.nodeId ?? a.node_id);
      const n = byId.get(id);
      return n ? labelOf(n) : id;
    });
  return `${total} ${total === 1 ? 'concept' : 'concepts'} flagged, drawn in red${names.length ? `: ${list(names)}` : ''}.`;
}
