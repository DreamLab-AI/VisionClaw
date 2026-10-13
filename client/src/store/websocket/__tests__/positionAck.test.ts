// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi } from 'vitest';

/**
 * ADR-2141 — a position chunk is acknowledged by ITS OWN sequence, after it has been applied.
 * Real encoder and parser; only the heavy collaborators are stubbed.
 */

const applied: { resolve: (() => void) | null; reject: ((e: unknown) => void) | null; calls: number } = {
  resolve: null,
  reject: null,
  calls: 0,
};

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));
vi.mock('../../../utils/clientDebugState', () => ({
  debugState: { isEnabled: () => false, isDataDebugEnabled: () => false },
}));
vi.mock('../../settingsStore', () => ({
  useSettingsStore: { getState: () => ({ settings: {} }) },
}));
vi.mock('../../../features/graph/managers/graphDataManager', () => ({
  graphDataManager: {
    getGraphType: () => 'knowledge',
    updateNodePositions: vi.fn(
      () =>
        new Promise<void>((resolve, reject) => {
          applied.calls++;
          applied.resolve = resolve;
          applied.reject = reject;
        }),
    ),
  },
}));
vi.mock('../../../features/analytics/store/nodeAnalyticsStore', () => ({
  nodeAnalyticsStore: { ingest: vi.fn() },
}));
vi.mock('../../transientBeamStore', () => ({ pushTransientBeams: vi.fn() }));
vi.mock('../connectionManager', () => ({ emit: vi.fn(), notifyBinaryMessageHandlers: vi.fn() }));
vi.mock('../../../utils/BatchQueue', () => ({
  NodePositionBatchQueue: class {},
  createWebSocketBatchProcessor: vi.fn(),
}));
vi.mock('../../../utils/validation', () => ({
  validateNodePositions: () => ({ valid: true, errors: [] }),
  createValidationMiddleware: vi.fn(),
}));

import { processBinaryData } from '../binaryProtocol';

/** A V5 chunk: [0x05][u64 sequence][n x 52-byte records]. */
function v5Chunk(sequence: number, nodes: number): ArrayBuffer {
  const buf = new ArrayBuffer(1 + 8 + nodes * 52);
  const v = new DataView(buf);
  v.setUint8(0, 5);
  v.setUint32(1, sequence >>> 0, true);
  v.setUint32(5, Math.floor(sequence / 2 ** 32) >>> 0, true);
  for (let i = 0; i < nodes; i++) {
    const o = 9 + i * 52;
    v.setUint32(o, i + 1, true);
    v.setFloat32(o + 4, i * 1.5, true);
  }
  return buf;
}

function harness() {
  const sent: ArrayBuffer[] = [];
  const socket = { readyState: WebSocket.OPEN, send: (b: ArrayBuffer) => sent.push(b) } as unknown as WebSocket;
  return { sent, get: () => ({ socket }), set: vi.fn() };
}

const seqOf = (b: ArrayBuffer) => {
  const v = new DataView(b);
  return v.getUint32(1, true) + v.getUint32(5, true) * 2 ** 32;
};

describe('position chunk acknowledgement', () => {
  beforeEach(() => {
    applied.calls = 0;
    applied.resolve = applied.reject = null;
  });

  it('acknowledges the chunk with its own sequence and node count, only once it has been applied', async () => {
    const h = harness();
    const done = processBinaryData(v5Chunk(48_113, 3), h.get, h.set);
    await Promise.resolve();
    expect(applied.calls).toBe(1);
    expect(h.sent).toHaveLength(0); // not applied yet: no acknowledgement

    applied.resolve!();
    await done;
    expect(h.sent).toHaveLength(1);
    const ack = h.sent[0];
    expect(new DataView(ack).getUint8(0)).toBe(0x34);
    expect(seqOf(ack)).toBe(48_113);
    expect(new DataView(ack).getUint32(9, true)).toBe(3);
  });

  it('acknowledges every chunk individually (no batching): ten chunks, ten acknowledgements', async () => {
    const h = harness();
    for (let seq = 1; seq <= 10; seq++) {
      const p = processBinaryData(v5Chunk(seq, 1), h.get, h.set);
      await Promise.resolve();
      applied.resolve!();
      await p;
    }
    expect(h.sent.map(seqOf)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
  });

  it('does not acknowledge a chunk the worker failed to apply (the server rebuilds it after its timeout)', async () => {
    const h = harness();
    const p = processBinaryData(v5Chunk(5, 2), h.get, h.set);
    await Promise.resolve();
    applied.reject!(new Error('worker error'));
    await p;
    expect(h.sent).toHaveLength(0);
  });

  it('a V3 frame carries no sequence, so there is nothing to acknowledge', async () => {
    const h = harness();
    const v3 = v5Chunk(0, 1);
    new DataView(v3).setUint8(0, 3);
    // V3 has no 8-byte sequence: [0x03][records]
    const frame = new Uint8Array(1 + 52);
    frame[0] = 3;
    frame.set(new Uint8Array(v3, 9, 52), 1);
    const p = processBinaryData(frame.buffer, h.get, h.set);
    await Promise.resolve();
    applied.resolve!();
    await p;
    expect(h.sent).toHaveLength(0);
  });
});
