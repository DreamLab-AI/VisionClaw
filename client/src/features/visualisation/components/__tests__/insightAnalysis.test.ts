import { describe, it, expect } from 'vitest';
import {
  isDerivedNode,
  bridgeSet,
  authoredHubs,
  gapConcepts,
  hierarchyNeighbourhood,
  anomalyAnswer,
  type AnalysisNode,
  type AnalysisEdge,
} from '../insightAnalysis';

// Realistic shapes: integer ids, domain roots with no `domain`, `domain_member` spokes.
const nodes: AnalysisNode[] = [
  { id: 10, label: 'Smart Contract', metadata: { domain: 'blockchain', maturity: 'established' } },
  { id: 11, label: 'x402 and l402 payment protocols', metadata: { domain: 'blockchain', maturity: 'draft' } },
  { id: 12, label: 'Consensus', metadata: { domain: 'blockchain', maturity: 'draft' } },
  { id: 20, label: 'Swarm Robotics', metadata: { domain: 'robotics', maturity: 'emerging' } },
  { id: 21, label: 'Autonomous Robot', metadata: { domain: 'robotics', maturity: 'established' } },
  { id: 22, label: 'Gripper', metadata: { source_domain: 'robotics' } },
  { id: 90, label: 'Space Science and Systems', metadata: { type: 'domain_root', public: 'true' } },
  { id: 91, label: 'Infrastructure', metadata: {}, nodeType: 'domain_root' },
];
const e = (id: number, s: number, t: number, edgeType?: string, metadata?: Record<string, unknown>): AnalysisEdge =>
  ({ id: `e${id}`, source: s, target: t, edgeType, metadata });
const edges: AnalysisEdge[] = [
  e(1, 10, 20, 'bridge'), e(2, 11, 20), e(3, 11, 21), e(4, 12, 10), // intra-domain
  e(5, 20, 21), e(6, 90, 10, 'domain_member', { provenance: 'derived' }), e(7, 90, 20, 'domain_member'),
  e(8, 91, 22, 'domain_member'), e(9, 11, 10, 'subClassOf'), e(10, 12, 11, 'hierarchical') // child -> parent,
];

describe('isDerivedNode', () => {
  it('flags domain roots by type metadata or nodeType, not authored concepts', () => {
    expect(isDerivedNode(nodes[6])).toBe(true);
    expect(isDerivedNode(nodes[7])).toBe(true);
    expect(isDerivedNode(nodes[0])).toBe(false);
  });
  it('flags nodes whose provenance is derived', () => {
    expect(isDerivedNode({ id: 1, metadata: { provenance: 'derived' } })).toBe(true);
  });
});

describe('bridgeSet', () => {
  it('returns endpoints of edges crossing the two domains, ranked by cross-edge count', () => {
    const b = bridgeSet(nodes, edges, ['blockchain', 'robotics']);
    expect(b.crossEdges).toBe(3);
    expect(new Set(b.ids)).toEqual(new Set(['10', '11', '20', '21']));
    expect(b.top.slice(0, 2).map((t) => [t.label, t.count]).sort()).toEqual([
      ['Swarm Robotics', 2], ['x402 and l402 payment protocols', 2],
    ]);
    expect(b.ids).not.toContain('12'); // intra-domain only: not a bridge
    expect(b.ids).not.toContain('90'); // domain root spokes are not bridges
  });
  it('reports zero when nothing crosses', () => {
    const b = bridgeSet(nodes, edges, ['blockchain', 'nonexistent']);
    expect(b.crossEdges).toBe(0);
    expect(b.ids).toEqual([]);
  });
});

describe('authoredHubs', () => {
  const centrality = (id: string) => ({ '90': 0.9, '91': 0.8, '10': 0.5, '20': 0.4, '11': 0.3 } as Record<string, number>)[id] ?? 0;
  it('ranks only authored concepts, excluding domain roots', () => {
    const hubs = authoredHubs(nodes, centrality, 3);
    expect(hubs.map((h) => h.id)).toEqual(['10', '20', '11']);
  });
});

describe('gapConcepts', () => {
  const centrality = (id: string) => ({ '10': 0.9, '11': 0.95, '20': 0.7, '12': 0.1, '21': 0.05, '90': 1 } as Record<string, number>)[id] ?? 0;
  it('keeps only top-decile-centrality concepts that are draft or emerging', () => {
    // 6 authored nodes -> top decile is ceil(0.6)=1 node: 11 (draft). Domain root 90 (1.0) is excluded first.
    const g = gapConcepts(nodes, centrality);
    expect(g.map((x) => x.id)).toEqual(['11']);
  });
  it('is empty when centrality is all zero', () => {
    expect(gapConcepts(nodes, () => 0)).toEqual([]);
  });
});

describe('hierarchyNeighbourhood', () => {
  it('finds the concept and its parents/children via subClassOf and hierarchical edges only', () => {
    const h = hierarchyNeighbourhood(nodes, edges, 'x402 and l402 payment protocols');
    expect(h?.focus.id).toBe('11');
    expect(new Set(h?.ids)).toEqual(new Set(['11', '10', '12']));
    expect(h?.parents.map((p) => p.id)).toEqual(['10']);
    expect(h?.children.map((p) => p.id)).toEqual(['12']);
  });
  it('is null for an unknown concept', () => {
    expect(hierarchyNeighbourhood(nodes, edges, 'zzz')).toBeNull();
  });
});

describe('anomalyAnswer', () => {
  it('says so truthfully when nothing is flagged', () => {
    expect(anomalyAnswer({ anomalies: [], total: 0 }, nodes)).toMatch(/no outliers at the current threshold/i);
  });
  it('names flagged concepts', () => {
    const a = anomalyAnswer({ anomalies: [{ nodeId: '20', score: 0.9 }], total: 1 }, nodes);
    expect(a).toMatch(/1 concept flagged/i);
    expect(a).toContain('Swarm Robotics');
  });
});
