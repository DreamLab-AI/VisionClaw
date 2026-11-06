/**
 * Characterisation of node filtering (ADR-2183, implementation notes 2026-11-06).
 *
 * Before the 2026-11-06 fix the server default was `enabled: true,
 * quality_threshold: 0.9, filter_by_quality: true, filter_by_authority: false,
 * filter_mode: "or"`, and in OR mode a disabled predicate passed vacuously, so
 * the 0.9 threshold greyed nothing. Now only ENABLED predicates combine: the same
 * settings test quality alone, in OR as in AND. Unscored nodes are neutral to the
 * quality predicate (they used to fall back to degree / 10). The shipped default
 * is off at 0.4 (src/settings/models.rs, explore/nodeFilter.ts); the old 0.9
 * object is kept below as a strict fixture because it exercises the predicate.
 *
 * ADR-2183 D2: what the predicates reject is GREYED through the highlight mask
 * (`filter:settings`) and the render set keeps it; `explore.hideFiltered`
 * restores the removal. The only removal is the linked_page scope (D4).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import type { GraphData, Node as GraphNode } from '../../managers/graphDataManager';
import type { NodeFilterSettings } from '../../../settings/config/settings';

let nodeFilter: NodeFilterSettings | undefined;
let hideFiltered: boolean | undefined;
vi.mock('../../../../store/settingsStore', () => ({
  useSettingsStore: (sel: (s: unknown) => unknown) =>
    sel({ settings: { nodeFilter, visualisation: { graphView: { explore: { hideFiltered } } } } }),
}));

import { useGraphFiltering } from '../useGraphFiltering';
import { HighlightMask, HIGHLIGHT_FILTERED_OUT } from '../../state/highlightMask';
import { writeFilter, activeFilterSources } from '../../explore/filterRegistry';

/** The pre-2026-11-06 server default (camelCase): a strict quality-only filter. */
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

let mask: HighlightMask;
const run = (data: GraphData, degree: Map<string, number>) => {
  const index = new Map(data.nodes.map((n, i) => [String(n.id), i]));
  mask = new HighlightMask(() => index);
  return renderHook(() => useGraphFiltering(data, new Map(), degree, { mask })).result.current;
};
const greyedOnMask = (data: GraphData) =>
  data.nodes.filter((n) => mask.bitsFor(String(n.id)) & HIGHLIGHT_FILTERED_OUT).map((n) => Number(n.id));

/** Kept by quality ≥ 0.9: the 2 scored ≥ 0.9 plus every unscored node (pages 79..100 and the
 *  three owl classes 111..113), which are neutral to the quality predicate. */
const AND_KEPT = [1, 2, ...Array.from({ length: 22 }, (_, k) => 79 + k), 111, 112, 113];
const NON_STUB = (data: GraphData) => data.nodes.filter((n) => n.metadata?.type !== 'linked_page').map((n) => Number(n.id));

describe('useGraphFiltering with a strict quality-only filter', () => {
  beforeEach(() => { nodeFilter = { ...SERVER_DEFAULT_NODE_FILTER }; hideFiltered = undefined; });

  it('OR mode with authority off tests quality alone (no vacuous pass)', () => {
    const { data, degree } = corpus();
    const r = run(data, degree);
    // 113 delivered − 10 linked_page = 103 rendered; the scored pages below 0.9 grey.
    expect(r.visibleNodes).toHaveLength(103);
    expect(r.visibleNodes.some((n) => n.metadata?.type === 'linked_page')).toBe(false);
    const expectedGreyed = NON_STUB(data).filter((id) => !AND_KEPT.includes(id));
    expect([...r.greyedNodeIds].map(Number).sort((a, b) => a - b)).toEqual(expectedGreyed);
    // the index map always covers the full delivered set
    expect(r.nodeIdToIndexMap.size).toBe(113);
  });

  it('the shipped default (off) greys nothing; only linked_page stubs are removed', () => {
    nodeFilter = undefined;
    const { data, degree } = corpus();
    const r = run(data, degree);
    expect(r.visibleNodes).toHaveLength(103);
    expect(r.greyedNodeIds.size).toBe(0);
    // nothing greyed → no active filter source (no "1 filter active" chip by default)
    expect(activeFilterSources(mask)).toEqual([]);
  });

  it('AND mode greys (does not remove) what fails the quality threshold', () => {
    nodeFilter = { ...SERVER_DEFAULT_NODE_FILTER, filterMode: 'and' };
    const { data, degree } = corpus();
    const r = run(data, degree);
    // ADR-2183 D2: the render set is unchanged by the filter
    expect(r.visibleNodes).toHaveLength(103);
    const expectedGreyed = NON_STUB(data).filter((id) => !AND_KEPT.includes(id));
    expect([...r.greyedNodeIds].map(Number).sort((a, b) => a - b)).toEqual(expectedGreyed);
    expect(greyedOnMask(data).sort((a, b) => a - b)).toEqual(expectedGreyed);
    expect(activeFilterSources(mask)).toEqual(['filter:settings']);
  });

  it('minConnections and minMaturity grey independently of the quality filter', () => {
    nodeFilter = { ...SERVER_DEFAULT_NODE_FILTER, enabled: false, minConnections: 11, minMaturity: 'growing' };
    const { data, degree } = corpus();
    const r = run(data, degree);
    expect(r.visibleNodes).toHaveLength(103);
    const kept = Array.from({ length: 100 }, (_, i) => i).filter((i) => i % 12 === 11).map((i) => i + 1);
    expect(greyedOnMask(data).sort((a, b) => a - b)).toEqual(NON_STUB(data).filter((id) => !kept.includes(id)));
  });

  it('a disabled filter greys nothing', () => {
    nodeFilter = { ...SERVER_DEFAULT_NODE_FILTER, enabled: false, filterMode: 'and' };
    const { data, degree } = corpus();
    const r = run(data, degree);
    expect(r.visibleNodes).toHaveLength(103);
    expect(r.greyedNodeIds.size).toBe(0);
  });

  it('hideFiltered restores removal, including what other filter sources grey', () => {
    nodeFilter = { ...SERVER_DEFAULT_NODE_FILTER, filterMode: 'and' };
    hideFiltered = true;
    const { data, degree } = corpus();
    const index = new Map(data.nodes.map((n, i) => [String(n.id), i]));
    mask = new HighlightMask(() => index);
    const { result } = renderHook(() => useGraphFiltering(data, new Map(), degree, { mask }));
    // the removal the quality threshold implies
    expect(result.current.visibleNodes.map((n) => Number(n.id)).sort((a, b) => a - b)).toEqual(AND_KEPT);
    // a widget filter (include only node 1) removes node 2 as well
    act(() => { writeFilter('filter:explore:test', ['1'], 'include', mask); });
    expect(result.current.visibleNodes.map((n) => Number(n.id))).toEqual([1]);
  });
});
