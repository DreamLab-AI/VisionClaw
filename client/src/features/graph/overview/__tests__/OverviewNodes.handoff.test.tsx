/**
 * The sprite capacity hand-off, mounted (ADR-2182 D1).
 *
 * Live fault of 2026-11-07: 9,595 knowledge nodes, the sprite geometry stuck at
 * the initial 4,096 with instanceCount 0, so the gems never hid. This drives
 * OverviewNodes through the grow in the order the app does it — frames from
 * useFrame, the async TSL material build resolving later — and checks each
 * step of the hand-off from the 4,096 geometry to the 16,384 one.
 */
import React, { useEffect, useRef } from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, act, cleanup } from '@testing-library/react';
import * as THREE from 'three';

// --- @react-three/fiber: frames are driven by hand -------------------------
type FrameSub = { cb: React.MutableRefObject<() => void>; prio: number };
const frameSubs = new Set<FrameSub>();
vi.mock('@react-three/fiber', () => ({
  useThree: () => ({ size: { width: 1280, height: 900 } }),
  useFrame: (cb: () => void, prio = 0) => {
    // As in R3F: the latest closure runs, the subscription lives for the mount.
    const ref = useRef(cb);
    ref.current = cb;
    useEffect(() => {
      const sub = { cb: ref, prio };
      frameSubs.add(sub);
      return () => { frameSubs.delete(sub); };
    }, [prio]);
  },
}));

// --- the TSL material build: resolved on demand -----------------------------
interface PendingBuild { geometry: THREE.InstancedBufferGeometry; resolve: () => void; material: THREE.Material }
const builds: PendingBuild[] = [];
vi.mock('../../../../rendering/materials/OverviewNodeMaterial', async (orig) => {
  const real = await orig<typeof import('../../../../rendering/materials/OverviewNodeMaterial')>();
  return {
    ...real,
    createTslOverviewNodeMaterial: (_u: unknown, geometry: THREE.InstancedBufferGeometry) =>
      new Promise<THREE.Material>((res) => {
        const material = new THREE.MeshBasicMaterial();
        builds.push({ geometry, material, resolve: () => res(material) });
      }),
  };
});

// --- observe the meshes the component renders --------------------------------
// OverviewNodes hands each mesh it renders to useRenderFaultWatch, once per
// render, so the last call carrying the sprite mesh is what <primitive> mounts.
let renderedSprites: THREE.Mesh | null = null;
vi.mock('../useRenderFaultWatch', () => ({
  useRenderFaultWatch: (mesh: THREE.Mesh | null) => {
    // Called sprites first, halo second, on every render.
    if (renderCallIsSprites) renderedSprites = mesh;
    renderCallIsSprites = !renderCallIsSprites;
  },
}));
let renderCallIsSprites = true;

import { OverviewNodes } from '../OverviewNodes';
import { setOverviewPositions, isOverviewLive } from '../overviewLive';
import { NodePositionTexture } from '../nodePositionTexture';
import type { GemNodesHandle } from '../../components/GemNodes';

const N = 9595;

function makeSource(): React.RefObject<GemNodesHandle | null> {
  const renderIndex = new Int32Array(N).map((_, i) => i);
  const colors = new Float32Array(N * 3).fill(0.5);
  const handle = { getOverviewSource: () => ({ renderIndex, colors, count: N, colorVersion: 1 }) } as unknown as GemNodesHandle;
  return { current: handle };
}

const style = { spriteSizePx: 6, nodeSize: 1, greyoutOpacity: 0.1, background: '#000000', halo: false };

function tick(): void {
  act(() => {
    for (const s of [...frameSubs].sort((a, b) => a.prio - b.prio)) s.cb.current();
  });
}

async function resolvePending(): Promise<void> {
  const open = builds.splice(0);
  await act(async () => { for (const b of open) b.resolve(); await Promise.resolve(); });
}

const capacityOf = (g: THREE.BufferGeometry) => g.getAttribute('aIndex').count;

describe('OverviewNodes capacity hand-off (9,595 nodes over a 4,096 start)', () => {
  beforeEach(() => {
    builds.length = 0; frameSubs.clear(); renderedSprites = null; renderCallIsSprites = true;
    setOverviewPositions(true);
  });
  afterEach(() => { cleanup(); setOverviewPositions(false); });

  it.each([
    ['isolated, halo off', false, false],
    ['as the app mounts it: StrictMode, halo on', true, true],
  ])('%s: grows to 16,384, builds for the new geometry, mounts it and fills 9,595 instances', async (_name, strict, haloOn) => {
    const positions = new NodePositionTexture();
    const el = <OverviewNodes positions={positions} sourceRef={makeSource()} style={{ ...style, halo: haloOn }} />;
    render(strict ? <React.StrictMode>{el}</React.StrictMode> : el);

    // Mount: the 4,096 sprite build and the halo build start; resolve them.
    expect(new Set(builds.map((b) => capacityOf(b.geometry)))).toEqual(new Set([64, 4096]));
    await resolvePending();
    expect(renderedSprites && capacityOf(renderedSprites.geometry)).toBe(4096);

    // First frame: 9,595 > 4,096 → grow; the render that follows makes the
    // 16,384 geometry and starts its build. The old mesh must not survive it.
    tick();
    const grown = builds.filter((b) => capacityOf(b.geometry) === 16384).at(-1);
    expect(grown, 'a build for the 16,384 geometry').toBeDefined();
    expect(renderedSprites, 'no sprite mesh while the new material builds').toBeNull();

    // Frames keep running while the material builds.
    tick(); tick();

    // The build resolves: the mesh mounts with the new geometry and its material.
    await resolvePending();
    expect(renderedSprites, 'sprite mesh mounted after the build').not.toBeNull();
    const mesh = renderedSprites!;
    expect(capacityOf(mesh.geometry)).toBe(16384);
    expect(mesh.geometry).toBe(grown!.geometry);
    expect(mesh.material).toBe(grown!.material);
    expect(isOverviewLive('nodes')).toBe(true);

    // Next frame fills it.
    tick();
    const geo = mesh.geometry as THREE.InstancedBufferGeometry;
    expect(geo.instanceCount).toBe(N);
    expect((geo.getAttribute('aIndex').array as Float32Array)[N - 1]).toBe(N - 1);
  });

  // The live signature (capacity 4,096, instanceCount 0, aIndex all zero) is
  // what OverviewNodes produces when its source handle yields nothing: count 0
  // never grows (spriteCapacityToMount) and !src forces instanceCount 0.
  it('a source handle that yields null leaves exactly the live signature', async () => {
    const nullSource = { current: { getOverviewSource: () => null } as unknown as GemNodesHandle };
    render(<React.StrictMode><OverviewNodes positions={new NodePositionTexture()} sourceRef={nullSource} style={style} /></React.StrictMode>);
    await resolvePending();
    for (let i = 0; i < 5; i++) tick();
    await resolvePending();
    const geo = renderedSprites!.geometry as THREE.InstancedBufferGeometry;
    expect(capacityOf(geo)).toBe(4096);
    expect(geo.instanceCount).toBe(0);
    expect((geo.getAttribute('aIndex').array as Float32Array).every((v) => v === 0)).toBe(true);
    expect(isOverviewLive('nodes')).toBe(true); // so the gems WOULD hide over 0 sprites
  });
});
