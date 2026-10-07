import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
  resolveNodeIndex,
  resolveNodeWorldPosition,
  focusNodeById,
  CAMERA_FOCUS_EVENT,
  type CameraFocusDetail,
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
