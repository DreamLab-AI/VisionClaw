/** ADR-2183: selection, neighbours and search hits emphasise through the highlight mask. */
import { describe, it, expect } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import * as THREE from 'three';
import type { GraphData, Node as GraphNode } from '../../managers/graphDataManager';
import {
  HighlightMask, HIGHLIGHT_SELECTED, HIGHLIGHT_FOCUS_NEIGHBOUR, HIGHLIGHT_SEARCH_HIT,
} from '../../state/highlightMask';
import { useGraphSelection, searchHitIds } from '../useGraphSelection';
import { ExploreStore } from '../../explore/exploreStore';

const mk = (id: string, label: string): GraphNode =>
  ({ id, label, position: { x: Number(id), y: 0, z: 0 }, metadata: {} } as unknown as GraphNode);

function setup() {
  const nodes = [mk('1', 'Robot arm'), mk('2', 'Robotics'), mk('3', 'Ledger'), mk('4', 'Arm robot')];
  const graphData = { nodes, edges: [{ id: 'e', source: '1', target: '3' }, { id: 'f', source: '4', target: '1' }] } as unknown as GraphData;
  const nodeIdToIndexMap = new Map(nodes.map((n, i) => [n.id, i]));
  const mask = new HighlightMask(() => nodeIdToIndexMap);
  const explore = new ExploreStore(mask);
  explore.setTable({ nodes, indexMap: nodeIdToIndexMap });
  const camera = new THREE.PerspectiveCamera();
  camera.position.set(0, 0, 100);
  const hook = renderHook(() => useGraphSelection({
    graphData, nodeIdToIndexMap, nodePositionsRef: { current: null }, connectionCountMap: new Map(), camera, mask, explore,
  }));
  const ids = (flag: number) => nodes.filter((n) => mask.bitsFor(n.id) & flag).map((n) => n.id);
  return { hook, mask, ids, explore };
}

describe('useGraphSelection → highlight mask', () => {
  it('selection emphasises the node and its neighbours; deselect clears both', () => {
    const { hook, ids, mask } = setup();
    act(() => { hook.result.current.setSelectedNodeId('1'); });
    expect(ids(HIGHLIGHT_SELECTED)).toEqual(['1']);
    expect(ids(HIGHLIGHT_FOCUS_NEIGHBOUR)).toEqual(['3', '4']);
    act(() => { window.dispatchEvent(new CustomEvent('visionclaw:node-deselect')); });
    expect(mask.activeSources()).toEqual([]);
  });

  it('a selection narrows the explore widgets to it and its neighbours (ADR-2185 D3)', () => {
    const { hook, explore } = setup();
    act(() => { hook.result.current.setSelectedNodeId('1'); });
    expect(explore.graphSelectionSize()).toBe(3);
    expect(explore.crossfilter!.selectedIds()).toEqual(['1', '3', '4']);
    act(() => { hook.result.current.setSelectedNodeId(null); });
    expect(explore.graphSelectionSize()).toBe(0);
  });

  it('a search emphasises every label hit, not only the fly-to target, and deselect clears it', () => {
    const { hook, ids, mask } = setup();
    act(() => { window.dispatchEvent(new CustomEvent('visionclaw:search', { detail: { query: 'robot' } })); });
    expect(ids(HIGHLIGHT_SEARCH_HIT)).toEqual(['1', '2', '4']);
    expect(hook.result.current.selectedNodeId).toBe('1');
    act(() => { window.dispatchEvent(new CustomEvent('visionclaw:node-deselect')); });
    expect(mask.activeSources()).toEqual([]);
  });

  it('searchHitIds matches substrings and all-words queries', () => {
    const nodes = [mk('1', 'Robot arm'), mk('2', 'Arm robot'), mk('3', 'Ledger')];
    expect(searchHitIds(nodes, 'arm robot')).toEqual(['1', '2']);
    expect(searchHitIds(nodes, '  ')).toEqual([]);
  });
});
