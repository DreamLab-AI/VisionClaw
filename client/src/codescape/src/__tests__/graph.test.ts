import { describe, expect, it } from 'vitest';
import { applyDelta, buildModel, communityHue, kHop, looksLikeTest } from '../graph';
import { edge, node, smallSnapshot } from './fixtures';

describe('graph model', () => {
  it('indexes nodes, parents, regions and tiers', () => {
    const m = buildModel(smallSnapshot());
    expect(m.n).toBe(6);
    expect(m.index.get(110)).toBe(5);
    expect(Array.from(m.tier)).toEqual([0, 1, 1, 2, 2, 2]);
    expect(m.parent[m.index.get(100)!]).toBe(m.index.get(10));
    expect(Array.from(m.region)).toEqual([0, 0, 0, 0, 0, 0]);
    expect(m.isTest[m.index.get(11)!]).toBe(1);
    expect(m.langs).toEqual(['rust']);
    expect(m.communities).toEqual([1, 2]);
  });

  it('separates calls and lifts symbol imports to their files', () => {
    const m = buildModel(smallSnapshot());
    expect(m.calls.length / 2).toBe(2);
    expect(Array.from(m.fileImports)).toEqual([m.index.get(10), m.index.get(11)]);
  });

  it('k-hop neighbourhood respects k and the cap', () => {
    const m = buildModel(smallSnapshot());
    const h1 = kHop(m, [m.index.get(100)!], 1);
    expect(h1.get(m.index.get(100)!)).toBe(0);
    expect(h1.get(m.index.get(101)!)).toBe(1);
    expect(h1.get(m.index.get(110)!)).toBe(1); // via the import edge
    expect(h1.has(m.index.get(1)!)).toBe(false);
    expect(kHop(m, [m.index.get(100)!], 2).has(m.index.get(1)!)).toBe(true);
    expect(kHop(m, [0], 5, 3).size).toBe(3);
  });

  it('applies a delta: add, change, remove nodes and edges', () => {
    const s = smallSnapshot();
    const out = applyDelta(s, {
      from_revision: 1,
      to_revision: 2,
      generated_at_ms: 5,
      changed_paths: ['src/f10.rs'],
      added_nodes: [node(102, 'l2', 10)],
      changed_nodes: [node(101, 'l2', 10, { label: 'renamed' })],
      removed_nodes: [110],
      added_edges: [edge(102, 100), edge(102, 999)],
      removed_edges: [edge(100, 101)],
    });
    expect(out.revision).toBe(2);
    expect(out.nodes.map((n) => n.id)).toEqual([1, 10, 11, 100, 101, 102]);
    expect(out.nodes.find((n) => n.id === 101)?.label).toBe('renamed');
    expect(out.edges.some((e) => e.source === 101 && e.target === 110)).toBe(false);
    expect(out.edges.some((e) => e.source === 100 && e.target === 101)).toBe(false);
    expect(out.edges.some((e) => e.source === 102 && e.target === 100)).toBe(true);
    expect(out.edges.some((e) => e.target === 999)).toBe(false);
    expect(s.nodes.length).toBe(6);
  });

  it('detects test code and uses the estate community hue', () => {
    expect(looksLikeTest({ path: 'crates/x/tests/a.rs', label: 'a' })).toBe(true);
    expect(looksLikeTest({ path: 'client/src/foo.test.ts', label: 'foo' })).toBe(true);
    expect(looksLikeTest({ path: 'src/main.rs', label: 'main' })).toBe(false);
    expect(looksLikeTest({ path: 'src/lib.rs', label: 'test_parse' })).toBe(true);
    expect(communityHue(3)).toBe((3 * 83) % 360);
  });
});
