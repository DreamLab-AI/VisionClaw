import { describe, expect, it } from 'vitest';
import * as shared from '../../../types/binaryProtocol';
import {
  AGENT_FLAG,
  applyPositionFrame,
  createNodeBuffers,
  decodeAck,
  decodeAgentActions,
  encodeAck,
  encodeV5Frame,
  frameVersion,
  NODE_RECORD_BYTES,
  OFF,
  parseServerEvent,
  floorOf,
  presenceOf,
} from '../protocol';

/** Build one record by hand at the documented offsets (independent of encodeV5Frame). */
function record(v: DataView, o: number, r: { id: number; pos: [number, number, number]; vel?: [number, number, number]; sssp: number; parent: number; cluster: number; anomaly: number; community: number; centrality: number }) {
  v.setUint32(o + 0, r.id >>> 0, true);
  v.setFloat32(o + 4, r.pos[0], true);
  v.setFloat32(o + 8, r.pos[1], true);
  v.setFloat32(o + 12, r.pos[2], true);
  const vel = r.vel ?? [0.5, -0.5, 0.25];
  v.setFloat32(o + 16, vel[0], true);
  v.setFloat32(o + 20, vel[1], true);
  v.setFloat32(o + 24, vel[2], true);
  v.setFloat32(o + 28, r.sssp, true);
  v.setInt32(o + 32, r.parent, true);
  v.setUint32(o + 36, r.cluster, true);
  v.setFloat32(o + 40, r.anomaly, true);
  v.setUint32(o + 44, r.community, true);
  v.setFloat32(o + 48, r.centrality, true);
}

function v5(seq: bigint, recs: Parameters<typeof record>[2][]): ArrayBuffer {
  const buf = new ArrayBuffer(9 + recs.length * 52);
  const v = new DataView(buf);
  v.setUint8(0, 0x05);
  v.setBigUint64(1, seq, true);
  recs.forEach((r, k) => record(v, 9 + k * 52, r));
  return buf;
}

describe('V5 position frames', () => {
  const index = new Map([
    [42, 0],
    [7, 1],
    [1000, 2],
  ]);

  it('decodes every field into the right slot by id→index', () => {
    const frame = v5(99n, [
      { id: 7, pos: [1, 2, 3], sssp: 2, parent: 42, cluster: 3, anomaly: 0.25, community: 11, centrality: 0.5 },
      { id: 42, pos: [-4, 5.5, 6], sssp: 0, parent: -1, cluster: 1, anomaly: 1, community: 2, centrality: 0.125 },
    ]);
    const b = createNodeBuffers(4, false);
    const r = applyPositionFrame(frame, b, index)!;
    expect(r).toEqual({ seq: 99, count: 2, applied: 2, actors: 0, unknown: 0 });
    expect(Array.from(b.positions.subarray(3, 6))).toEqual([1, 2, 3]);
    expect(Array.from(b.positions.subarray(0, 3))).toEqual([-4, 5.5, 6]);
    expect(b.sssp[1]).toBe(2);
    expect(b.ssspParent[1]).toBe(42);
    expect(b.ssspParent[0]).toBe(-1);
    expect(b.cluster[1]).toBe(3);
    expect(b.heat[1]).toBe(0.25);
    expect(b.community[1]).toBe(11);
    expect(b.centrality[1]).toBe(0.5);
    expect(b.centrality[0]).toBe(0.125);
    expect(frameVersion(b)).toBe(1);
  });

  it('skips agent-flagged actors and counts unknown ids', () => {
    const frame = v5(5n, [
      { id: (3 | AGENT_FLAG) >>> 0, pos: [9, 9, 9], sssp: 0, parent: 0, cluster: 0, anomaly: 0.5, community: 0, centrality: 0 },
      { id: 123456, pos: [1, 1, 1], sssp: 0, parent: 0, cluster: 0, anomaly: 0, community: 0, centrality: 0 },
      { id: 1000, pos: [7, 8, 9], sssp: 0, parent: 0, cluster: 0, anomaly: 0, community: 4, centrality: 0 },
    ]);
    const b = createNodeBuffers(4, false);
    const r = applyPositionFrame(frame, b, index)!;
    expect(r).toMatchObject({ seq: 5, count: 3, applied: 1, actors: 1, unknown: 1 });
    expect(Array.from(b.positions.subarray(6, 9))).toEqual([7, 8, 9]);
  });

  it('masks flag bits 26-30 off code-node ids', () => {
    const frame = v5(1n, [{ id: (42 | 0x0400_0000) >>> 0, pos: [3, 3, 3], sssp: 0, parent: 0, cluster: 0, anomaly: 0, community: 0, centrality: 0 }]);
    const b = createNodeBuffers(4, false);
    expect(applyPositionFrame(frame, b, index)!.applied).toBe(1);
    expect(b.positions[0]).toBe(3);
  });

  it('handles a u64 sequence above 2^32', () => {
    const frame = v5(2n ** 40n + 3n, []);
    expect(applyPositionFrame(frame, createNodeBuffers(1, false), index)!.seq).toBe(2 ** 40 + 3);
  });

  it('accepts a bare V3 frame (seq 0) and rejects siblings, truncation and V4', () => {
    const v3 = new ArrayBuffer(1 + 52);
    const dv = new DataView(v3);
    dv.setUint8(0, 0x03);
    record(dv, 1, { id: 42, pos: [1, 0, 0], sssp: 0, parent: 0, cluster: 0, anomaly: 0, community: 0, centrality: 0 });
    expect(applyPositionFrame(v3, createNodeBuffers(4, false), index)).toMatchObject({ seq: 0, applied: 1 });
    expect(applyPositionFrame(new Uint8Array([0x23, 0, 0]).buffer, createNodeBuffers(4, false), index)).toBeNull();
    expect(applyPositionFrame(new Uint8Array([0x04, 1]).buffer, createNodeBuffers(4, false), index)).toBeNull();
    expect(applyPositionFrame(v5(1n, [{ id: 42, pos: [0, 0, 0], sssp: 0, parent: 0, cluster: 0, anomaly: 0, community: 0, centrality: 0 }]).slice(0, 40), createNodeBuffers(4, false), index)).toBeNull();
    expect(applyPositionFrame(new ArrayBuffer(0), createNodeBuffers(4, false), index)).toBeNull();
  });

  it('round-trips through encodeV5Frame', () => {
    const frame = encodeV5Frame(12, [{ id: 7, position: [1.5, 2.5, 3.5], community: 9, anomaly: 0.75, centrality: 0.25, cluster: 2, ssspDistance: 4, ssspParent: 42 }]);
    expect(frame.byteLength).toBe(9 + NODE_RECORD_BYTES);
    const b = createNodeBuffers(4, false);
    applyPositionFrame(frame, b, index);
    expect([b.positions[3], b.positions[4], b.positions[5], b.community[1], b.heat[1], b.centrality[1], b.cluster[1], b.sssp[1], b.ssspParent[1]]).toEqual([1.5, 2.5, 3.5, 9, 0.75, 0.25, 2, 4, 42]);
  });

  it('agrees with the shared client decoder (client/src/types/binaryProtocol.ts)', () => {
    expect(OFF).toEqual({
      id: shared.BINARY_NODE_ID_OFFSET,
      position: shared.BINARY_POSITION_OFFSET,
      velocity: shared.BINARY_VELOCITY_OFFSET,
      ssspDistance: shared.BINARY_SSSP_DISTANCE_OFFSET,
      ssspParent: shared.BINARY_SSSP_PARENT_OFFSET,
      cluster: shared.BINARY_CLUSTER_ID_OFFSET,
      anomaly: shared.BINARY_ANOMALY_SCORE_OFFSET,
      community: shared.BINARY_COMMUNITY_ID_OFFSET,
      centrality: shared.BINARY_CENTRALITY_OFFSET,
    });
    expect(NODE_RECORD_BYTES).toBe(shared.BINARY_NODE_SIZE_V3);
    const frame = v5(77n, [{ id: 42, pos: [1, 2, 3], vel: [0.5, 0.25, 0.125], sssp: 1, parent: 7, cluster: 5, anomaly: 0.5, community: 6, centrality: 0.75 }]);
    const parsed = shared.parseBinaryFrameData(frame);
    expect(parsed.broadcastSequence).toBe(77);
    expect(parsed.nodes[0]).toMatchObject({ nodeId: 42, position: { x: 1, y: 2, z: 3 }, ssspDistance: 1, ssspParent: 7, clusterId: 5, anomalyScore: 0.5, communityId: 6, centrality: 0.75 });
    const b = createNodeBuffers(4, false);
    applyPositionFrame(frame, b, index);
    expect([b.positions[0], b.positions[1], b.positions[2], b.cluster[0], b.heat[0], b.community[0], b.centrality[0]]).toEqual([1, 2, 3, 5, 0.5, 6, 0.75]);
  });
});

describe('0x34 ack', () => {
  it('is 21 bytes: opcode, u64 seq, u32 nodes, u64 ms, little-endian', () => {
    const a = encodeAck(7, 1000, 123_456);
    expect(a.length).toBe(21);
    expect(a[0]).toBe(0x34);
    expect(Array.from(a.subarray(1, 9))).toEqual([7, 0, 0, 0, 0, 0, 0, 0]);
    expect(Array.from(a.subarray(9, 13))).toEqual([0xe8, 0x03, 0, 0]);
    expect(Array.from(a.subarray(13, 21))).toEqual([0x40, 0xe2, 0x01, 0, 0, 0, 0, 0]);
    expect(decodeAck(a)).toEqual({ seq: 7, nodes: 1000, timestampMs: 123_456 });
    expect(decodeAck(a.subarray(0, 20))).toBeNull();
  });

  it('carries large sequences and real timestamps', () => {
    const ts = 1_760_000_000_123;
    const a = encodeAck(2 ** 33 + 1, 52, ts);
    expect(decodeAck(a)).toEqual({ seq: 2 ** 33 + 1, nodes: 52, timestampMs: ts });
  });
});

describe('0x23 agent actions', () => {
  it('decodes the batch shape written by wire::agent_action_frame', () => {
    // [0x23][u16 count=1][u16 len=15][u32 agent|flag][u32 node][u8 type][u32 ts][u16 dur]
    const buf = new ArrayBuffer(3 + 2 + 15);
    const v = new DataView(buf);
    v.setUint8(0, 0x23);
    v.setUint16(1, 1, true);
    v.setUint16(3, 15, true);
    v.setUint32(5, (1 | AGENT_FLAG) >>> 0, true);
    v.setUint32(9, 2, true);
    v.setUint8(13, 1);
    v.setUint32(14, 3, true);
    v.setUint16(18, 4, true);
    expect(decodeAgentActions(buf)).toEqual([{ agent: 1, node: 2, action: 1, ts: 3, durationMs: 4 }]);
  });
});

describe('ServerEvent JSON', () => {
  it('parses the serde shapes from events.rs', () => {
    const floor = parseServerEvent('{"type":"floor","state":"held","holder":3,"queue":[4]}');
    expect(floor?.type).toBe('floor');
    expect(floorOf(floor as Extract<typeof floor & object, { type: 'floor' }>)).toEqual({ state: 'held', holder: 3, queue: [4] });
    const p = parseServerEvent('{"type":"presence","actor":5,"actor_label":"claude:tab0","actor_kind":"agent","kind":"update","node_ids":[1,2],"paths":["a.rs"],"text":"Edit","ts_ms":10}');
    expect(p && p.type === 'presence' && presenceOf(p)).toEqual({ actor: 5, actor_label: 'claude:tab0', actor_kind: 'agent', kind: 'update', node_ids: [1, 2], paths: ['a.rs'], text: 'Edit', ts_ms: 10 });
    const q = parseServerEvent('{"type":"query","command":"focus x","node_ids":[1],"edge_pairs":[[1,2]],"path":[],"candidates":[["x — a.rs",1]],"answer":"ok"}');
    expect(q?.type === 'query' && q.candidates[0]).toEqual(['x — a.rs', 1]);
    expect(parseServerEvent('{"type":"nope"}')).toBeNull();
    expect(parseServerEvent('not json')).toBeNull();
    expect(parseServerEvent('[1,2]')).toBeNull();
  });
});

describe('socket session keys and voice PCM', () => {
  it('appends session/repo/name/tier and skips empties', async () => {
    const { withQuery, newSessionKey } = await import('../net/config');
    const u = withQuery('ws://h:4100/ws/positions', { session: 'abc', repo: null, name: undefined, tier: 'l1' });
    expect(u).toBe('ws://h:4100/ws/positions?session=abc&tier=l1');
    expect(newSessionKey()).not.toBe(newSessionKey());
  });
  it('decodes the server s16le TTS PCM', async () => {
    const { s16leToF32 } = await import('../audio/pcm');
    const b = new ArrayBuffer(6);
    const v = new DataView(b);
    v.setInt16(0, 16384, true);
    v.setInt16(2, -32768, true);
    v.setInt16(4, 0, true);
    expect(Array.from(s16leToF32(b))).toEqual([0.5, -1, 0]);
  });
});
