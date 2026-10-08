import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
  resolveNodeIndex,
  resolveNodeWorldPosition,
  focusNodeById,
  CAMERA_FOCUS_EVENT,
  type CameraFocusDetail,
  frameRoutePose,
} from '../cameraFocus';
import { KNOWLEDGE_NODE_FLAG, AGENT_NODE_FLAG, getActualNodeId } from '@/types/binaryProtocol';

// A bare-id node keyed at index 2, and a flat SAB with a distinct point per idx.
const BASE_ID = 42;
const indexMap = new Map<string, number>([
  [String(BASE_ID), 2],
  ['7', 0],
]);
// 3 nodes × 3 floats: idx0=(0,0,0) idx1=(1,1,1) idx2=(9,-3,4.5)
const positions = new Float32Array([0, 0, 0, 1, 1, 1, 9, -3, 4.5]);

describe('resolveNodeIndex — id-space masking', () => {
  it('resolves a bare wire id directly', () => {
    expect(resolveNodeIndex(BASE_ID, indexMap)).toBe(2);
  });

  it('resolves a flagged wire id by masking off type bits (KNOWLEDGE)', () => {
    const flagged = BASE_ID | KNOWLEDGE_NODE_FLAG;
    expect(flagged).not.toBe(BASE_ID); // sanity: the flag actually changed the id
    expect(getActualNodeId(flagged)).toBe(BASE_ID);
    expect(resolveNodeIndex(flagged, indexMap)).toBe(2);
  });

  it('resolves a flagged wire id by masking off type bits (AGENT)', () => {
    // AGENT_NODE_FLAG is bit 31 → forces the number negative under `& `; the
    // mask must still recover the base id. `| 0` keeps it a 32-bit int.
    const flagged = (BASE_ID | AGENT_NODE_FLAG) | 0;
    expect(getActualNodeId(flagged)).toBe(BASE_ID);
    expect(resolveNodeIndex(flagged, indexMap)).toBe(2);
  });

  it('returns undefined for an unknown id', () => {
    expect(resolveNodeIndex(999, indexMap)).toBeUndefined();
  });
});

describe('resolveNodeWorldPosition — SAB lookup', () => {
  it('returns the world position for a bare id', () => {
    expect(resolveNodeWorldPosition(BASE_ID, indexMap, positions)).toEqual({ x: 9, y: -3, z: 4.5 });
  });

  it('returns the world position for a flagged id (masked lookup)', () => {
    const flagged = BASE_ID | KNOWLEDGE_NODE_FLAG;
    expect(resolveNodeWorldPosition(flagged, indexMap, positions)).toEqual({ x: 9, y: -3, z: 4.5 });
  });

  it('returns null when the position buffer is missing', () => {
    expect(resolveNodeWorldPosition(BASE_ID, indexMap, null)).toBeNull();
    expect(resolveNodeWorldPosition(BASE_ID, indexMap, undefined)).toBeNull();
  });

  it('returns null for an unknown id', () => {
    expect(resolveNodeWorldPosition(999, indexMap, positions)).toBeNull();
  });

  it('returns null when the resolved index runs past the buffer', () => {
    // idx 5 → i3 = 15, buffer only has 9 floats → out of range, no OOB read.
    const shortMap = new Map<string, number>([[String(BASE_ID), 5]]);
    expect(resolveNodeWorldPosition(BASE_ID, shortMap, positions)).toBeNull();
  });
});

describe('focusNodeById — event dispatch', () => {
  let received: CameraFocusDetail[] = [];
  const listener = (e: Event) => { received.push((e as CustomEvent<CameraFocusDetail>).detail); };

  beforeEach(() => {
    received = [];
    window.addEventListener(CAMERA_FOCUS_EVENT, listener);
  });
  afterEach(() => {
    window.removeEventListener(CAMERA_FOCUS_EVENT, listener);
  });

  it('dispatches the focus event with the masked node id as a string', () => {
    const ok = focusNodeById(BASE_ID | KNOWLEDGE_NODE_FLAG);
    expect(ok).toBe(true);
    expect(received).toHaveLength(1);
    expect(received[0]).toEqual({ nodeId: String(BASE_ID) });
  });

  it('is idempotent for an already-bare id', () => {
    focusNodeById(BASE_ID);
    expect(received[0].nodeId).toBe(String(BASE_ID));
  });

  it('returns false without a window (SSR guard)', () => {
    // Simulate SSR by removing the window global for the branch under test.
    vi.stubGlobal('window', undefined);
    try {
      expect(focusNodeById(BASE_ID)).toBe(false);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe('focusMemoryPoint', () => {
  it('dispatches the memory focus event with the sample row', async () => {
    const { focusMemoryPoint, MEMORY_FOCUS_EVENT } = await import('../cameraFocus');
    const seen: number[] = [];
    const h = (e: Event) => seen.push((e as CustomEvent<{ sampleIndex: number }>).detail.sampleIndex);
    window.addEventListener(MEMORY_FOCUS_EVENT, h);
    expect(focusMemoryPoint(42)).toBe(true);
    window.removeEventListener(MEMORY_FOCUS_EVENT, h);
    expect(seen).toEqual([42]);
  });

  it('dispatches a cloud position for a hit outside the sample', async () => {
    const { focusMemoryPosition, MEMORY_FOCUS_EVENT } = await import('../cameraFocus');
    const seen: unknown[] = [];
    const h = (e: Event) => seen.push((e as CustomEvent).detail);
    window.addEventListener(MEMORY_FOCUS_EVENT, h);
    expect(focusMemoryPosition([1, 2, 3])).toBe(true);
    expect(focusMemoryPosition([Number.NaN, 0, 0])).toBe(false);
    window.removeEventListener(MEMORY_FOCUS_EVENT, h);
    expect(seen).toEqual([{ sampleIndex: -1, position: [1, 2, 3] }]);
  });

  it('refuses negative or non-integer rows', async () => {
    const { focusMemoryPoint } = await import('../cameraFocus');
    expect(focusMemoryPoint(-1)).toBe(false);
    expect(focusMemoryPoint(1.5)).toBe(false);
  });
});

describe('flyPose', () => {
  it('eases camera and target from the start pose to the goal', async () => {
    const { flyPose } = await import('../cameraFocus');
    const from = { position: [0, 0, 100] as [number, number, number], target: [0, 0, 0] as [number, number, number] };
    const to = { position: [50, 0, 50] as [number, number, number], target: [50, 0, 0] as [number, number, number] };
    expect(flyPose(from, to, 0)).toEqual(from);
    expect(flyPose(from, to, 1)).toEqual(to);
    const mid = flyPose(from, to, 0.5);
    expect(mid.target[0]).toBeCloseTo(25, 6);
  });
});

describe('frameRoutePose', () => {
  const from = { position: [0, 0, 1000] as [number, number, number], target: [0, 0, 0] as [number, number, number] };
  const route: Array<[number, number, number]> = [[100, 0, 0], [140, 30, 10], [160, -20, 0], [120, 10, -30]];

  it('targets the route centre and keeps the current viewing direction', () => {
    const p = frameRoutePose(from, route, 75, 1.6);
    expect(p.target[0]).toBeCloseTo(130);
    expect(p.target[1]).toBeCloseTo(5);
    expect(p.target[2]).toBeCloseTo(-10);
    const d = [p.position[0] - p.target[0], p.position[1] - p.target[1], p.position[2] - p.target[2]];
    const n = Math.hypot(d[0], d[1], d[2]);
    expect(d[2] / n).toBeCloseTo(1);
  });

  it('puts every route point inside both the vertical and horizontal field of view', () => {
    for (const aspect of [0.5, 1, 2.2]) {
      const fov = 60;
      const p = frameRoutePose(from, route, fov, aspect);
      const halfV = (fov * Math.PI) / 360;
      const halfH = Math.atan(Math.tan(halfV) * aspect);
      const dist = Math.hypot(p.position[0] - p.target[0], p.position[1] - p.target[1], p.position[2] - p.target[2]);
      for (const q of route) {
        const r = Math.hypot(q[0] - p.target[0], q[1] - p.target[1], q[2] - p.target[2]);
        const ang = Math.asin(Math.min(1, r / dist));
        expect(ang).toBeLessThan(Math.min(halfV, halfH));
      }
    }
  });

  it('floors the radius so a one-point route is not framed from inside it', () => {
    const p = frameRoutePose(from, [[5, 5, 5]], 75, 1, 40);
    const dist = Math.hypot(p.position[0] - 5, p.position[1] - 5, p.position[2] - 5);
    expect(dist).toBeGreaterThan(40);
  });

  it('falls back to a raised front view when the camera sits on the target', () => {
    const p = frameRoutePose({ position: [1, 1, 1], target: [1, 1, 1] }, route, 75, 1);
    expect(p.position[2]).toBeGreaterThan(p.target[2]);
    expect(p.position[1]).toBeGreaterThan(p.target[1]);
  });
});

describe('frameRoutePose with screen insets', () => {
  const route: Array<[number, number, number]> = [[100, 0, 0], [140, 30, 10], [160, -20, 0], [120, 10, -30]];
  const from = { position: [130, 40, 900] as [number, number, number], target: [130, 5, -10] as [number, number, number] };

  /** NDC of a world point for a camera at pose p looking at its target, world up +y */
  function ndc(p: { position: number[]; target: number[] }, q: number[], fov: number, aspect: number) {
    const f = [p.target[0] - p.position[0], p.target[1] - p.position[1], p.target[2] - p.position[2]];
    const fl = Math.hypot(f[0], f[1], f[2]);
    const fw = f.map((v) => v / fl);
    let r = [fw[1] * 0 - fw[2] * 1, fw[2] * 0 - fw[0] * 0, fw[0] * 1 - fw[1] * 0];
    const rl = Math.hypot(r[0], r[1], r[2]);
    r = r.map((v) => v / rl);
    const u = [r[1] * fw[2] - r[2] * fw[1], r[2] * fw[0] - r[0] * fw[2], r[0] * fw[1] - r[1] * fw[0]];
    const d = [q[0] - p.position[0], q[1] - p.position[1], q[2] - p.position[2]];
    const z = d[0] * fw[0] + d[1] * fw[1] + d[2] * fw[2];
    const x = d[0] * r[0] + d[1] * r[1] + d[2] * r[2];
    const y = d[0] * u[0] + d[1] * u[1] + d[2] * u[2];
    const t = Math.tan((fov * Math.PI) / 360);
    return [x / (z * t * aspect), y / (z * t)];
  }

  it('fits the route inside the area the panel and dock leave free, centred there', () => {
    const fov = 60;
    const aspect = 1.6;
    const insets = { right: 0.28, bottom: 0.12 };
    const p = frameRoutePose(from, route, fov, aspect, 1, insets);
    const xs = route.map((q) => ndc(p, q, fov, aspect));
    for (const [x, y] of xs) {
      expect(x).toBeGreaterThan(-1);
      expect(x).toBeLessThan(1 - 2 * insets.right);
      expect(y).toBeGreaterThan(-1 + 2 * insets.bottom);
      expect(y).toBeLessThan(1);
    }
    const [cx, cy] = ndc(p, [130, 5, -10], fov, aspect);
    expect(cx).toBeCloseTo(-insets.right, 2);
    expect(cy).toBeCloseTo(insets.bottom, 2);
  });

  it('with no insets the route centre projects to the screen centre', () => {
    const p = frameRoutePose(from, route, 60, 1.6);
    const [cx, cy] = ndc(p, [130, 5, -10], 60, 1.6);
    expect(cx).toBeCloseTo(0, 5);
    expect(cy).toBeCloseTo(0, 5);
  });
});
