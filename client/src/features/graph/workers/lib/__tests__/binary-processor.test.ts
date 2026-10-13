import { describe, it, expect } from 'vitest';
import { processFrameUpdates, type BinaryFrameUpdate } from '../binary-processor';

// Hard rule (ADR-2139): position frames are full snapshots. Applying one REPLACES a node's
// target position; it never adds to it, so replaying the same frame is idempotent.
function harness(nodeCount: number) {
  const ids = Array.from({ length: nodeCount }, (_, i) => i);
  return {
    targetPositions: new Float32Array(nodeCount * 3),
    positionArray: new Float32Array(nodeCount * 4),
    reverseNodeIdMap: new Map(ids.map((i) => [i, `n${i}`])),
    nodeIndexMap: new Map(ids.map((i) => [`n${i}`, i])),
    pinnedNodeIds: new Set<number>(),
  };
}

const update = (nodeId: number, x: number, y: number, z: number): BinaryFrameUpdate => ({
  nodeId,
  position: { x, y, z },
});

describe('processFrameUpdates (full snapshots only)', () => {
  it('replaces target positions instead of accumulating them', () => {
    const h = harness(2);
    const frame = [update(0, 10, 20, 30), update(1, -5, 0, 5)];
    const run = () => processFrameUpdates({ nodeUpdates: frame, analyticsBuffer: null, ...h }, new Set());
    run();
    expect(Array.from(h.targetPositions)).toEqual([10, 20, 30, -5, 0, 5]);
    run(); // the same snapshot again: a delta decoder would now hold 20, 40, 60, ...
    expect(Array.from(h.targetPositions)).toEqual([10, 20, 30, -5, 0, 5]);
  });

  it('a later snapshot overwrites an earlier one completely', () => {
    const h = harness(1);
    processFrameUpdates({ nodeUpdates: [update(0, 1, 2, 3)], analyticsBuffer: null, ...h }, new Set());
    processFrameUpdates({ nodeUpdates: [update(0, 100, 200, 300)], analyticsBuffer: null, ...h }, new Set());
    expect(Array.from(h.targetPositions)).toEqual([100, 200, 300]);
    expect(Array.from(h.positionArray)).toEqual([0, 100, 200, 300]);
  });

  it('leaves pinned nodes where the user put them', () => {
    const h = harness(1);
    h.pinnedNodeIds.add(0);
    h.targetPositions.set([7, 8, 9]);
    processFrameUpdates({ nodeUpdates: [update(0, 1, 1, 1)], analyticsBuffer: null, ...h }, new Set());
    expect(Array.from(h.targetPositions)).toEqual([7, 8, 9]);
  });
});
