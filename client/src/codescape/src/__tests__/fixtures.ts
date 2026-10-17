import type { CodeEdge, CodeNode, GraphSnapshot, Tier } from '../protocol';

export function node(id: number, tier: Tier, parent: number, extra: Partial<CodeNode> = {}): CodeNode {
  return {
    id,
    key: `k${id}`,
    kind: tier === 'l0' ? 'directory' : tier === 'l1' ? 'file' : 'function',
    lang: 'rust',
    label: `n${id}`,
    path: tier === 'l0' ? null : `src/f${tier === 'l1' ? id : parent}.rs`,
    span: null,
    parent,
    community: 1,
    cluster: 0,
    importance: 0.5,
    tier,
    size: 0,
    ...extra,
  };
}

export function edge(source: number, target: number, relation: CodeEdge['relation'] = 'calls'): CodeEdge {
  return { source, target, relation, weight: 1, confidence: 'extracted' };
}

/** dir 1 → files 10, 11 → symbols 100,101 (in 10) and 110 (in 11). */
export function smallSnapshot(): GraphSnapshot {
  return {
    repo: 'test',
    epoch: 1,
    revision: 1,
    generated_at_ms: 1,
    nodes: [
      node(1, 'l0', 0),
      node(10, 'l1', 1),
      node(11, 'l1', 1, { path: 'tests/f11_test.rs', community: 2 }),
      node(100, 'l2', 10),
      node(101, 'l2', 10),
      node(110, 'l2', 11, { community: 2 }),
    ],
    edges: [
      edge(1, 10, 'contains'),
      edge(1, 11, 'contains'),
      edge(10, 100, 'defines'),
      edge(10, 101, 'defines'),
      edge(11, 110, 'defines'),
      edge(100, 101),
      edge(101, 110),
      edge(100, 110, 'imports'),
    ],
  };
}
