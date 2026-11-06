/**
 * Characterisation of node filtering under the shipped defaults (ADR-2183).
 *
 * The server's `NodeFilterSettings::default()` (src/settings/models.rs:147-158) is
 * `enabled: true, quality_threshold: 0.9, filter_by_quality: true,
 * filter_by_authority: false, filter_mode: "or", include_linked_pages: false`,
 * and the client store receives exactly that.
 *
 * WHY ~9.6k NODES RENDER although only 184 of 7,528 scored nodes reach 0.9:
 * in OR mode a disabled predicate passes vacuously. `filter_by_authority` is
 * false, so `passesAuthority = !filterByAuthority || … = true` for EVERY node,
 * and `passesQuality || passesAuthority` is always true. The quality threshold
 * removes nothing. The Rust mirror has the same arm (src/actors/client_filter.rs
 * :85-92, :134-140) and its own test says so ("Or mode with
 * filter_by_authority=false would pass all", client_filter.rs:215). The only
 * default removal is the linked_page gate (include_linked_pages: false).
 *
 * This file pins that behaviour. The predicates themselves (quality/authority,
 * minConnections, minMaturity) are characterised in AND mode, where they bite.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import type { GraphData, Node as GraphNode } from '../../managers/graphDataManager';
import type { NodeFilterSettings } from '../../../settings/config/settings';

let nodeFilter: NodeFilterSettings | undefined;
vi.mock('../../../../store/settingsStore', () => ({
  useSettingsStore: (sel: (s: unknown) => unknown) => sel({ settings: { nodeFilter } }),
}));

import { useGraphFiltering } from '../useGraphFiltering';

/** The server default, as the client store receives it (camelCase). */
const SERVER_DEFAULT_NODE_FILTER: NodeFilterSettings = {
  enabled: true,
  qualityThreshold: 0.9,
  authorityThreshold: 0.5,
  filterByQuality: true,
  filterByAuthority: false,
  filterMode: 'or',
  includeLinkedPages: false,
};

const mk = (id: number, metadata: Record<string, string>): GraphNode =>
  ({ id: String(id), label: `n${id}`, position: { x: 0, y: 0, z: 0 }, metadata } as unknown as GraphNode);

/**
 * A scaled-down live corpus: 100 pages, 78 scored of which 2 reach 0.9 (live: 7,528
 * scored, 184 ≥ 0.9), 22 unscored, plus 10 linked_page stubs and a few ontology
 * classes with maturity.
 */
function corpus(): { data: GraphData; degree: Map<string, number> } {
  const nodes: GraphNode[] = [];
  const degree = new Map<string, number>();
  let id = 1;
  for (let i = 0; i < 100; i++, id++) {
    const md: Record<string, string> = { type: 'page' };
    if (i < 78) md.quality_score = i < 2 ? '0.95' : '0.7';
    nodes.push(mk(id, md));
    degree.set(String(id), i % 12); // unscored pages: degree 0..11
  }
  for (let i = 0; i < 10; i++, id++) nodes.push(mk(id, { type: 'linked_page' }));
  for (const m of ['draft', 'growing', 'mature']) { nodes.push(mk(id, { type: 'owl_class', maturity: m })); id++; }
  return { data: { nodes, edges: [] } as GraphData, degree };
}

const run = (data: GraphData, degree: Map<string, number>) =>
  renderHook(() => useGraphFiltering(data, new Map(), degree)).result.current;

describe('useGraphFiltering characterisation (pre-ADR-2183)', () => {
  beforeEach(() => { nodeFilter = { ...SERVER_DEFAULT_NODE_FILTER }; });

  it('server defaults: OR mode with authority off passes every node; only linked_page stubs are removed', () => {
    const { data, degree } = corpus();
    const r = run(data, degree);
    // 113 delivered − 10 linked_page = 103: the 0.9 quality threshold removes nothing.
    expect(r.visibleNodes).toHaveLength(103);
    expect(r.visibleNodes.some((n) => n.metadata?.type === 'linked_page')).toBe(false);
    // the index map always covers the full delivered set
    expect(r.nodeIdToIndexMap.size).toBe(113);
  });

  it('AND mode makes the quality threshold bite: scored < 0.9 go, unscored fall back to degree/10', () => {
    nodeFilter = { ...SERVER_DEFAULT_NODE_FILTER, filterMode: 'and' };
    const { data, degree } = corpus();
    const ids = run(data, degree).visibleNodes.map((n) => Number(n.id));
    // 2 scored ≥ 0.9, plus unscored pages (ids 79..100) with degree ≥ 9 (i%12 ∈ {9,10,11}),
    // plus ontology classes (unscored, degree 0 → quality 0) are removed.
    const unscoredHigh = Array.from({ length: 22 }, (_, k) => 78 + k).filter((i) => i % 12 >= 9).map((i) => i + 1);
    expect(ids.sort((a, b) => a - b)).toEqual([1, 2, ...unscoredHigh]);
  });

  it('minConnections and minMaturity are hard removals independent of filterMode', () => {
    nodeFilter = { ...SERVER_DEFAULT_NODE_FILTER, minConnections: 11, minMaturity: 'growing' };
    const { data, degree } = corpus();
    const ids = run(data, degree).visibleNodes.map((n) => Number(n.id));
    // degree 11 ↔ i%12 = 11; ontology classes have degree 0 and are dropped by minConnections anyway
    expect(ids).toEqual(Array.from({ length: 100 }, (_, i) => i).filter((i) => i % 12 === 11).map((i) => i + 1));
  });

  it('a disabled filter keeps everything except linked_page stubs', () => {
    nodeFilter = { ...SERVER_DEFAULT_NODE_FILTER, enabled: false, filterMode: 'and' };
    const { data, degree } = corpus();
    expect(run(data, degree).visibleNodes).toHaveLength(103);
  });
});
