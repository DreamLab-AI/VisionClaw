import { describe, it, expect } from 'vitest';
import * as THREE from 'three';
import { NodePositionTexture, texelOf, POSITION_TEX_WIDTH } from '../nodePositionTexture';
import { LabelBudgetRunner } from '../labelRunner';
import { LABEL_REFRESH_MS, TIER_SELECTION } from '../labelBudget';
import { resolveInferredPairs } from '../../components/InferredEdges';
import { resolveGraphView } from '../OverviewLayer';
import { HIGHLIGHT_FILTERED_OUT, HIGHLIGHT_SELECTED, highlightMask } from '../../state/highlightMask';
import { sharedNodeIndexResolver } from '../../state/nodeIndex';
import { DEFAULT_GRAPH_VIEW } from '../../../../api/settings/defaults';
import type { Node as GraphNode } from '../../managers/graphDataManager';

describe('NodePositionTexture (GPU node source)', () => {
  it('packs xyz + highlight level per render index and uploads only on change', () => {
    const t = new NodePositionTexture(4);
    const pos = new Float32Array([1, 2, 3, 4, 5, 6]);
    const mask = new Uint8Array([0, HIGHLIGHT_FILTERED_OUT]);
    expect(t.update(pos, 2, mask, 1)).toBe(true);
    expect(t.slot(0)).toEqual([1, 2, 3, 1]);
    expect(t.slot(1)).toEqual([4, 5, 6, 0]);
    const v = t.version;
    // settled: same positions, same mask version → no upload
    expect(t.update(pos, 2, mask, 1)).toBe(false);
    expect(t.version).toBe(v);
    // mask changed
    mask[1] = HIGHLIGHT_SELECTED;
    expect(t.update(pos, 2, mask, 2)).toBe(true);
    expect(t.slot(1)[3]).toBe(2);
    // positions changed in place
    pos[0] = 9;
    expect(t.update(pos, 2, mask, 2)).toBe(true);
    expect(t.slot(0)[0]).toBe(9);
    t.dispose();
  });

  it('grows past one row and tells its users to rebind', () => {
    const t = new NodePositionTexture(1);
    let rebinds = 0;
    t.onReallocate(() => { rebinds++; });
    const n = POSITION_TEX_WIDTH + 5;
    t.update(new Float32Array(n * 3), n, new Uint8Array(n), 1);
    expect(rebinds).toBe(1);
    expect(t.height).toBeGreaterThanOrEqual(2);
    expect(texelOf(POSITION_TEX_WIDTH + 1)).toEqual([1, 1]);
    t.dispose();
  });
});

describe('inferred edges on the overview path', () => {
  it('resolves to render-index pairs and drops unmapped endpoints', () => {
    const map = new Map([['a', 0], ['b', 5]]);
    const r = resolveInferredPairs([{ sourceId: 'a', targetId: 'b' }, { sourceId: 'a', targetId: 'zzz' }], map);
    expect(r.count).toBe(1);
    expect([r.src[0], r.tgt[0]]).toEqual([0, 5]);
  });
});

describe('resolveGraphView', () => {
  it('layers stored values over the shipped defaults', () => {
    const gv = resolveGraphView({ nodes: { bloomScope: 'off' } as never });
    expect(gv.nodes.bloomScope).toBe('off');
    expect(gv.nodes.gemLodDistance).toBe(DEFAULT_GRAPH_VIEW.nodes.gemLodDistance);
    expect(gv.overviewEdges).toEqual(DEFAULT_GRAPH_VIEW.overviewEdges);
    expect(gv.fitTarget).toBe('knowledge');
  });
});

describe('LabelBudgetRunner (shared by both label paths)', () => {
  const nodes = Array.from({ length: 6 }, (_, i) => ({ id: String(i), label: `n${i}`, position: { x: 0, y: 0, z: 0 } }) as unknown as GraphNode);
  const map = new Map(nodes.map((n, i) => [String(n.id), i]));
  const cc = new Map(nodes.map((n, i) => [String(n.id), i])); // node 5 has the highest degree
  // spread along x so nothing collides
  const positions = new Float32Array(nodes.flatMap((_, i) => [(i - 2.5) * 30, 0, 0]));
  const cam = new THREE.PerspectiveCamera(60, 1, 0.1, 1000);
  cam.position.set(0, 0, 200);
  cam.lookAt(0, 0, 0);
  cam.updateMatrixWorld();
  cam.updateProjectionMatrix();

  it('ranks by degree without centrality, within the budget', () => {
    const r = new LabelBudgetRunner();
    const sel = r.run(0, nodes, map, cc, positions, cam, { width: 1000, height: 1000 }, { budget: 2, dynamicBudget: 1, selectedBudget: 0 }, 13);
    expect(r.prioritySource).toBe('degree');
    expect(Array.from(sel.nodes.slice(0, sel.count))).toEqual([5, 4, 3]);
  });

  it('runs at the label cadence and on camera stop, not per frame', () => {
    const r = new LabelBudgetRunner();
    expect(r.due(0, false)).toBe(true);
    r.run(0, nodes, map, cc, positions, cam, { width: 1000, height: 1000 }, { budget: 1, dynamicBudget: 0, selectedBudget: 0 }, 13);
    expect(r.due(16, false)).toBe(false);
    expect(r.due(32, true)).toBe(false); // moving
    expect(r.due(48, false)).toBe(true); // just stopped
    r.run(48, nodes, map, cc, positions, cam, { width: 1000, height: 1000 }, { budget: 1, dynamicBudget: 0, selectedBudget: 0 }, 13);
    expect(r.due(48 + LABEL_REFRESH_MS - 1, false)).toBe(false);
    expect(r.due(48 + LABEL_REFRESH_MS, false)).toBe(true);
  });

  it('reads the shared highlight mask: selected first, greyed excluded', () => {
    // the runner reads the shared mask: point it at this test's index, then restore
    highlightMask.setIndexResolver(() => map);
    highlightMask.batch(() => {
      highlightMask.setEmphasis('test:sel', HIGHLIGHT_SELECTED, ['0']);
      highlightMask.setFilter('test:f', ['5'], 'exclude');
    });
    const r = new LabelBudgetRunner();
    const sel = r.run(0, nodes, map, cc, positions, cam, { width: 1000, height: 1000 }, { budget: 2, dynamicBudget: 0, selectedBudget: 1 }, 13);
    const got = Array.from(sel.nodes.slice(0, sel.count));
    expect(got[0]).toBe(0);
    expect(sel.tiers[0]).toBe(TIER_SELECTION);
    expect(got).not.toContain(5);
    highlightMask.clearAll();
    highlightMask.setIndexResolver(sharedNodeIndexResolver);
  });
});

describe('overview render faults (ADR-2181 D4: no silent skip)', () => {
  it('a fault drops the layer out of live, is reported, and clears on unmount', async () => {
    const live = await import('../overviewLive');
    live.clearOverviewFaults();
    live.setOverviewLive('edges', true);
    live.setOverviewLive('nodes', true);
    expect(live.isOverviewLive('edges')).toBe(true);
    expect(live.overviewFaultNoticeText({})).toBeNull();

    live.markOverviewFault('edges', 'drawIndexed(Infinity)');
    expect(live.isOverviewLive('edges')).toBe(false);
    expect(live.isOverviewLive('nodes')).toBe(true);
    const text = live.overviewFaultNoticeText({ edges: 'x' });
    expect(text).toContain('overview edges');
    expect(text).toContain('gems and glass tubes');
    expect(live.overviewFaultNoticeText({ inferred: 'x' })).not.toContain('gems');

    live.clearOverviewFaults();
    expect(live.isOverviewLive('edges')).toBe(true);
    live.setOverviewLive('edges', false);
    live.setOverviewLive('nodes', false);
  });

  it('the guard hook calls the object handler once and survives a throwing handler', async () => {
    const { notifyRenderFault, onRenderFault, webglProgramFailure } = await import('../../../../rendering/renderFaults');
    const obj = new THREE.Mesh();
    const seen: unknown[] = [];
    onRenderFault(obj, (e) => seen.push(e));
    notifyRenderFault(obj, new Error('boom'));
    expect(seen).toHaveLength(1);
    onRenderFault(obj, () => { throw new Error('handler'); });
    expect(() => notifyRenderFault(obj, 'x')).not.toThrow();
    notifyRenderFault(null, 'x');
    // WebGL: diagnostics.runnable false → the failure text; runnable or missing → null
    const mat = new THREE.MeshBasicMaterial();
    const gl = (d: unknown) => ({ properties: { get: () => ({ currentProgram: { diagnostics: d } }) } });
    expect(webglProgramFailure(gl({ runnable: false, programLog: 'link failed' }), mat)).toBe('link failed');
    expect(webglProgramFailure(gl({ runnable: true }), mat)).toBeNull();
    expect(webglProgramFailure(gl(undefined), mat)).toBeNull();
    expect(webglProgramFailure({}, mat)).toBeNull();
  });
});

describe('render faults need repeated throws (the guard absorbs transients)', () => {
  it('fires only after throws on FAULT_FRAMES separate frames, once', async () => {
    const { debounceFault, FAULT_FRAMES } = await import('../../../../rendering/renderFaults');
    let t = 0;
    const seen: unknown[] = [];
    const h = debounceFault((e) => seen.push(e), FAULT_FRAMES, () => t);
    h('a'); h('a-same-frame'); t += 2; h('still-same-frame');
    expect(seen).toHaveLength(0);
    for (let k = 1; k < FAULT_FRAMES; k++) { t += 16; h(`f${k}`); }
    expect(seen).toEqual([`f${FAULT_FRAMES - 1}`]);
    t += 16; h('later');
    expect(seen).toHaveLength(1);
  });
});
