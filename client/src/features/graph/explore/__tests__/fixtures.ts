import type { Node as GraphNode } from '../../managers/graphDataManager';

export const node = (id: string | number, label: string, metadata: Record<string, string>): GraphNode =>
  ({ id: String(id), label, position: { x: 0, y: 0, z: 0 }, metadata } as unknown as GraphNode);

/**
 * Twelve nodes: three domains, quality 0.1..0.95, two dates, one linked_page stub
 * (id 12) that population scope removes.
 */
export function exploreNodes(): GraphNode[] {
  const domains = ['robotics', 'robotics', 'robotics', 'robotics', 'blockchain', 'blockchain', 'blockchain',
    'artificial-intelligence', 'artificial-intelligence', 'artificial-intelligence', 'artificial-intelligence'];
  const out: GraphNode[] = domains.map((d, i) => node(i + 1, `Node ${i + 1}${i === 2 ? ' robot arm' : ''}`, {
    type: 'page',
    domain: d,
    quality_score: String([0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95, 0.95][i]),
    maturity: ['draft', 'growing', 'mature'][i % 3],
    ...(i < 6 ? { last_commit: i < 3 ? '2026-01-10T00:00:00Z' : '2026-09-10T00:00:00Z' } : {}),
    ...(i === 4 ? { tags: 'ledger, consensus' } : {}),
  }));
  out.push(node(12, 'Stub', { type: 'linked_page', domain: 'robotics' }));
  return out;
}
