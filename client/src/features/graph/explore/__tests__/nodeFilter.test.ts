/**
 * Node quality / authority filter semantics (ADR-2183 implementation notes).
 *
 * The cases live in the repository-level fixture `tests/node-filter-parity.json`,
 * which the server (`src/actors/client_filter.rs`) and XR
 * (`xr-client/rust/src/settings_sync.rs`) tests load too, so the three copies
 * of the predicate cannot drift apart.
 */
import { describe, it, expect } from 'vitest';
import type { Node as GraphNode } from '../../managers/graphDataManager';
import type { NodeFilterSettings } from '../../../settings/config/settings';
import { isGreyedByNodeFilter, resolveNodeFilter, NODE_FILTER_DEFAULTS } from '../nodeFilter';
import { transformApiToClientSettings } from '../../../../api/settings/schemaMappings';
import parityFixture from '../../../../../../tests/node-filter-parity.json';

interface Fixture {
  nodes: { id: number; metadata: Record<string, string> }[];
  cases: { name: string; default?: boolean; filter: NodeFilterSettings; greyed: number[] }[];
}

const fixture = parityFixture as unknown as Fixture;
const nodes = fixture.nodes.map(
  (n) => ({ id: String(n.id), label: `n${n.id}`, position: { x: 0, y: 0, z: 0 }, metadata: n.metadata }) as unknown as GraphNode,
);
// Degree 0 everywhere: under the old degree/10 fallback the unscored nodes (5, 6)
// would score 0 and be greyed, so a regression to that fallback fails the cases.
const degree = new Map(nodes.map((n) => [String(n.id), 0]));
const greyedFor = (filter: Partial<NodeFilterSettings>) =>
  nodes.filter((n) => isGreyedByNodeFilter(n, resolveNodeFilter(filter), degree, new Map())).map((n) => Number(n.id));

describe('node filter parity fixture', () => {
  for (const c of fixture.cases) {
    it(c.name, () => {
      expect(greyedFor(c.filter)).toEqual(c.greyed);
    });
  }
});

describe('node filter semantics', () => {
  it('OR with only quality enabled tests quality alone (no vacuous authority pass)', () => {
    expect(greyedFor({ enabled: true, filterByQuality: true, filterByAuthority: false, filterMode: 'or', qualityThreshold: 0.6 }))
      .toEqual([1, 2, 7]);
  });

  it('an unscored node is neutral to the quality predicate, whatever its degree', () => {
    const lowDegree = new Map(nodes.map((n) => [String(n.id), 1]));
    const f = resolveNodeFilter({ enabled: true, filterByQuality: true, qualityThreshold: 0.9 });
    expect(isGreyedByNodeFilter(nodes[4], f, lowDegree, new Map())).toBe(false); // id 5, unscored
  });
});

describe('node filter defaults', () => {
  it('the fixture default case matches the code defaults', () => {
    const d = fixture.cases.find((c) => c.default)!;
    expect(NODE_FILTER_DEFAULTS).toMatchObject(d.filter);
    expect(NODE_FILTER_DEFAULTS.serverRemove).toBe(false);
  });

  it('resolveNodeFilter falls back to the shipped defaults', () => {
    const r = resolveNodeFilter(undefined);
    expect(r.enabled).toBe(false);
    expect(r.qualityThreshold).toBe(0.4);
    expect(r.serverRemove).toBe(false);
  });

  it('the settings mapping seeds the same defaults under a server response without nodeFilter', () => {
    const s = transformApiToClientSettings({} as never);
    expect(s.nodeFilter).toMatchObject({ enabled: false, qualityThreshold: 0.4, serverRemove: false });
  });

  it('a stored nodeFilter overrides the code defaults (the persisted document wins)', () => {
    const s = transformApiToClientSettings({ nodeFilter: { qualityThreshold: 0.74, enabled: true } } as never);
    expect(s.nodeFilter).toMatchObject({ enabled: true, qualityThreshold: 0.74, serverRemove: false });
  });
});
