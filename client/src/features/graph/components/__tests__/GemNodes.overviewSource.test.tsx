/**
 * GemNodesHandle.getOverviewSource on a mounted knowledge gem mesh (ADR-2182 D1).
 *
 * OverviewNodes mirrors this source; a handle that yields null leaves the
 * sprites at 4,096 / instanceCount 0 (see overview/__tests__/
 * OverviewNodes.handoff.test.tsx). This checks the source across the states
 * the live app passes through: mounted before any frame, after the first
 * frame's colour pass, and while the stream grows the population past the
 * initial 4,096-instance mesh (which recreates it).
 */
import React, { useEffect, useRef } from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, act, cleanup } from '@testing-library/react';
import * as THREE from 'three';

// --- @react-three/fiber: frames are driven by hand -------------------------
type FrameSub = { cb: React.MutableRefObject<(s: unknown) => void>; prio: number };
const frameSubs = new Set<FrameSub>();
const scene = new THREE.Scene();
const frameState = {
  clock: new THREE.Clock(false),
  camera: new THREE.PerspectiveCamera(50, 1280 / 900, 0.1, 1e5),
  scene,
  size: { width: 1280, height: 900 },
};
frameState.camera.position.set(0, 0, 500);
shared.scene = scene;
vi.mock('@react-three/fiber', () => ({
  useThree: () => frameState,
  useFrame: (cb: (s: unknown) => void, prio = 0) => {
    const ref = useRef(cb);
    ref.current = cb;
    useEffect(() => {
      const sub = { cb: ref, prio };
      frameSubs.add(sub);
      return () => { frameSubs.delete(sub); };
    }, [prio]);
  },
}));

// --- <primitive>: R3F attaches the committed object and calls its ref with it.
// Under react-dom the tag would be a DOM element and the callback ref would get
// that element, so stand in for R3F's commit-time attach. Built inside the
// factory: the JSX runtime is mocked before this file's own bindings exist.
const shared = vi.hoisted(() => ({ scene: null as unknown as { add: (o: unknown) => void; remove: (o: unknown) => void } }));
async function jsxRuntimeWithPrimitive<R extends object>(real: R, names: (keyof R)[]): Promise<R> {
  const { useLayoutEffect: ule } = await import('react');
  function Primitive({ object, ref }: { object: unknown; ref?: (o: unknown) => void }) {
    ule(() => {
      shared.scene.add(object);
      ref?.(object);
      return () => { shared.scene.remove(object); ref?.(null); };
    }, [object, ref]);
    return null;
  }
  const out = { ...real } as Record<string, unknown>;
  for (const n of names) {
    const fn = real[n] as unknown as (...a: unknown[]) => unknown;
    out[n as string] = (type: unknown, ...rest: unknown[]) => fn(type === 'primitive' ? Primitive : type, ...rest);
  }
  return out as R;
}
vi.mock('react/jsx-dev-runtime', async (orig) =>
  jsxRuntimeWithPrimitive(await orig<typeof import('react/jsx-dev-runtime')>(), ['jsxDEV']));
vi.mock('react/jsx-runtime', async (orig) =>
  jsxRuntimeWithPrimitive(await orig<typeof import('react/jsx-runtime')>(), ['jsx', 'jsxs']));

// --- the async TSL augments load three/webgpu; not under test ----------------
vi.mock('../../../../rendering/materials/GemNodeMaterial', async (orig) => {
  const real = await orig<typeof import('../../../../rendering/materials/GemNodeMaterial')>();
  return { ...real, createTslGemMaterial: async () => undefined, applyTslInstanceTransform: async () => false };
});

import { GemNodes, type GemNodesHandle } from '../GemNodes';
import type { Node as GraphNode } from '../../managers/graphDataManager';

function makeNodes(n: number): GraphNode[] {
  return Array.from({ length: n }, (_, i) => ({
    id: String(i), label: `n${i}`, position: { x: i, y: 0, z: 0 },
  }) as unknown as GraphNode);
}

function props(nodes: GraphNode[], positionsRef: React.MutableRefObject<Float32Array | null>) {
  const nodeIdToIndexMap = new Map(nodes.map((n, i) => [String(n.id), i]));
  const noop = () => {};
  return {
    nodes, edges: [], graphMode: 'knowledge_graph' as const, forceMode: 'knowledge_graph' as const,
    perNodeVisualModeMap: new Map(), nodePositionsRef: positionsRef,
    connectionCountMap: new Map(), hierarchyMap: new Map(), nodeIdToIndexMap,
    settings: undefined, ssspResult: null, selectedNodeId: null,
    onPointerDown: noop, onPointerMove: noop, onPointerUp: noop, onPointerMissed: noop, onDoubleClick: noop,
  };
}

function tick(): void {
  act(() => {
    for (const s of [...frameSubs].sort((a, b) => a.prio - b.prio)) s.cb.current(frameState);
  });
}

describe.each([
  ['plain', false],
  ['StrictMode (as main.tsx mounts the app)', true],
])('GemNodes.getOverviewSource — %s', (_name, strict) => {
  beforeEach(() => { frameSubs.clear(); scene.clear(); });
  afterEach(() => cleanup());

  const wrap = (el: React.ReactElement) => (strict ? <React.StrictMode>{el}</React.StrictMode> : el);

  it('(a) is null before any frame runs, (b) holds all 200 nodes after the first frame', () => {
    const nodes = makeNodes(200);
    const positionsRef = { current: new Float32Array(200 * 3) };
    const ref = React.createRef<GemNodesHandle>();
    render(wrap(<GemNodes ref={ref} {...props(nodes, positionsRef)} />));

    expect(ref.current, 'handle attached').not.toBeNull();
    expect(ref.current!.getOverviewSource(), '(a) before the colour pass').toBeNull();

    tick();
    const src = ref.current!.getOverviewSource();
    expect(src, '(b) after the first frame').not.toBeNull();
    expect(src!.count).toBe(200);
    expect(src!.renderIndex[199]).toBe(199);
    // The source is the mesh that is actually in the scene.
    expect(ref.current!.getMesh()!.parent).toBe(scene);
  });

  it('(c) follows the stream past the 4,096-instance mesh: 1,680 → 9,595 nodes', () => {
    const ref = React.createRef<GemNodesHandle>();
    const positionsRef: React.MutableRefObject<Float32Array | null> = { current: new Float32Array(9595 * 3) };
    const { rerender } = render(wrap(<GemNodes ref={ref} {...props(makeNodes(1680), positionsRef)} />));
    tick();
    expect(ref.current!.getOverviewSource()?.count).toBe(1680);
    const first = ref.current!.getMesh();

    rerender(wrap(<GemNodes ref={ref} {...props(makeNodes(9595), positionsRef)} />));
    tick();
    const mesh = ref.current!.getMesh();
    expect(mesh, 'the mesh was recreated at 16,384').not.toBe(first);
    expect(mesh!.instanceMatrix.count).toBe(16384);
    expect(mesh!.parent).toBe(scene);
    const src = ref.current!.getOverviewSource();
    expect(src, 'source after growth').not.toBeNull();
    expect(src!.count).toBe(9595);
    expect(src!.colors).toBe(mesh!.instanceColor!.array);
  });
});
