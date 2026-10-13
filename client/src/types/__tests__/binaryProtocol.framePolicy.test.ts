/**
 * ADR-2019 — per-opcode malformed/truncated-frame policy, browser decoder.
 *
 * The closeout condition asks that each opcode's policy be *documented and
 * tested* in both client decoders, using fixtures shaped like the real server
 * encoder's output. The fixture builders below mirror
 * `crates/visionclaw-protocol/src/wire_fixtures.rs` byte for byte — that Rust
 * module is pinned to the live encoder by an equivalence test in
 * `src/utils/binary_protocol.rs`, so these bytes are the encoder's bytes.
 *
 * The policies differ per opcode and per consumer, deliberately:
 *
 * | Opcode | Consumer | Malformed/truncated policy |
 * |---|---|---|
 * | 0x03 / 0x05 position | XR (Rust) | all-or-nothing: misaligned body rejected whole |
 * | 0x03 / 0x05 position | browser | tolerant: complete records kept, torn tail dropped |
 * | 0x23 agent action | XR (Rust) | tolerant prefix parse: complete events kept |
 * | 0x23 agent action | server | strict: truncated batch refused whole |
 * | any sibling opcode | browser | declined, never auto-detected as positions |
 *
 * The browser's tolerance is a rendering choice (a torn tail should not blank
 * the view), not an oversight — but it is only safe because a sibling opcode can
 * no longer be mistaken for a position frame.
 */
import { describe, it, expect } from 'vitest';
import {
  parseBinaryNodeData,
  parseBinaryFrameData,
  lastBroadcastSequence,
  SIBLING_OPCODES,
  PROTOCOL_V3,
  PROTOCOL_V5,
  BINARY_NODE_SIZE_V3,
  NODE_ID_MASK,
} from '../binaryProtocol';

// ── Fixtures: identical layout to wire_fixtures.rs ──────────────────────────

/** One 52-byte V3 node record. */
function nodeRecord(wireId: number, x = 0, y = 0, z = 0): Uint8Array {
  const buf = new ArrayBuffer(BINARY_NODE_SIZE_V3);
  const view = new DataView(buf);
  view.setUint32(0, wireId, true);
  view.setFloat32(4, x, true);
  view.setFloat32(8, y, true);
  view.setFloat32(12, z, true);
  // velocity 16..28 stays zero
  view.setFloat32(28, Infinity, true); // sssp distance
  view.setInt32(32, -1, true); // sssp parent
  view.setUint32(36, 0, true); // cluster
  view.setFloat32(40, 0, true); // anomaly
  view.setUint32(44, 0, true); // community
  view.setFloat32(48, 0, true); // centrality
  return new Uint8Array(buf);
}

function concat(parts: Uint8Array[]): ArrayBuffer {
  const total = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out.buffer;
}

/** `[0x03]` then N records. */
function v3Frame(count: number): ArrayBuffer {
  const parts: Uint8Array[] = [new Uint8Array([PROTOCOL_V3])];
  for (let i = 0; i < count; i++) parts.push(nodeRecord(i + 1, i, 0, 0));
  return concat(parts);
}

/** `[0x05][u64 seq]` then N records. */
function v5Frame(seq: number, count: number): ArrayBuffer {
  const header = new Uint8Array(9);
  const hv = new DataView(header.buffer);
  hv.setUint8(0, PROTOCOL_V5);
  hv.setUint32(1, seq >>> 0, true); // low word
  hv.setUint32(5, Math.floor(seq / 0x100000000), true); // high word
  const parts: Uint8Array[] = [header];
  for (let i = 0; i < count; i++) parts.push(nodeRecord(i + 1, i, 0, 0));
  return concat(parts);
}

/** `[0x23][u16 count]([u16 len][15-byte event])*` — the agent-action batch. */
function agentActionFrame(events: number): ArrayBuffer {
  const head = new Uint8Array(3);
  new DataView(head.buffer).setUint8(0, 0x23);
  new DataView(head.buffer).setUint16(1, events, true);
  const parts: Uint8Array[] = [head];
  for (let i = 0; i < events; i++) {
    const ev = new Uint8Array(2 + 15);
    const v = new DataView(ev.buffer);
    v.setUint16(0, 15, true); // event length
    v.setUint32(2, 11 + i, true); // source agent
    v.setUint32(6, 21 + i, true); // target node
    v.setUint8(10, 0); // action type
    v.setUint32(11, 1000 + i, true); // timestamp
    v.setUint16(15, 250, true); // duration
    parts.push(ev);
  }
  return concat(parts);
}

function truncate(buf: ArrayBuffer, dropBytes: number): ArrayBuffer {
  return buf.slice(0, Math.max(1, buf.byteLength - dropBytes));
}

// ── Position frames: 0x03 / 0x05 ───────────────────────────────────────────

describe('ADR-2019 position frame policy (0x03 / 0x05)', () => {
  it('decodes a well-formed V3 frame', () => {
    const nodes = parseBinaryNodeData(v3Frame(3));
    expect(nodes).toHaveLength(3);
    expect(nodes[0].nodeId).toBe(1);
  });

  it('decodes a V5 frame and exposes its broadcast sequence', () => {
    const parsed = parseBinaryFrameData(v5Frame(9000, 2));
    expect(parsed.nodes).toHaveLength(2);
    expect(parsed.broadcastSequence).toBe(9000);
    expect(lastBroadcastSequence).toBe(9000);
  });

  it('never decodes a delta frame: a V4 frame yields no nodes, and no frame carries a delta flavour', () => {
    // Hard rule: position updates are always full snapshots. A V4 frame (the retired delta
    // encoding: [4][frame][count:u16] then 20-byte records) must be declined, not applied.
    const v4 = new Uint8Array(4 + 20);
    v4[0] = 4; v4[2] = 1; // one "changed" node
    new DataView(v4.buffer).setUint32(4, 7, true);
    new DataView(v4.buffer).setUint8(8, 0x01);
    new DataView(v4.buffer).setInt16(12, 500, true); // would be dx = +5.0 under the old decoder
    expect(parseBinaryNodeData(v4.buffer)).toEqual([]);
    const parsed = parseBinaryFrameData(v4.buffer);
    expect(parsed.nodes).toEqual([]);
    expect('type' in parsed).toBe(false);
    expect('frameNumber' in parsed).toBe(false);
  });

  it('treats the V5 envelope as purely additive over the V3 body', () => {
    // Stripping the version byte and the 8 sequence bytes must leave exactly the
    // V3 body — the invariant ADR-2018 freezes.
    const v3 = new Uint8Array(v3Frame(2));
    const v5 = new Uint8Array(v5Frame(1, 2));
    expect(Array.from(v5.slice(9))).toEqual(Array.from(v3.slice(1)));
  });

  it('keeps complete records and drops a torn tail (tolerant by design)', () => {
    // The browser policy: a partial trailing record must not blank the view.
    const torn = truncate(v3Frame(3), 20);
    const nodes = parseBinaryNodeData(torn);
    expect(nodes).toHaveLength(2);
    expect(nodes.map((n) => n.nodeId)).toEqual([1, 2]);
  });

  it('returns nothing for a frame with no complete record', () => {
    expect(parseBinaryNodeData(truncate(v3Frame(1), 30))).toHaveLength(0);
  });

  it('returns nothing for an empty buffer', () => {
    expect(parseBinaryNodeData(new ArrayBuffer(0))).toHaveLength(0);
    expect(parseBinaryFrameData(new ArrayBuffer(0)).nodes).toHaveLength(0);
  });

  it('does not read a V5 frame cut inside its sequence as sequence zero', () => {
    // The sequence is envelope header, not payload. A truncated header must
    // yield no nodes rather than a bogus zero-sequence frame.
    for (let kept = 0; kept < 8; kept++) {
      const buf = new ArrayBuffer(1 + kept);
      new DataView(buf).setUint8(0, PROTOCOL_V5);
      expect(parseBinaryNodeData(buf)).toHaveLength(0);
    }
  });

  it('strips class flag bits from the 26-bit node id space', () => {
    const AGENT_FLAG = 0x80000000;
    const nodes = parseBinaryNodeData(concat([new Uint8Array([PROTOCOL_V3]), nodeRecord((AGENT_FLAG | 42) >>> 0)]));
    expect(nodes).toHaveLength(1);
    expect(nodes[0].nodeId & NODE_ID_MASK).toBe(42);
  });
});

// ── Unknown-tag handling at the demultiplexer ──────────────────────────────

describe('ADR-2019 unknown-tag handling', () => {
  it('declines every known sibling opcode instead of auto-detecting it', () => {
    // The hazard this closes: a 0x23 batch whose length happens to be a multiple
    // of 36 would otherwise be reinterpreted as V2 node records, fabricating
    // nodes at arbitrary positions from an agent-action payload.
    for (const opcode of SIBLING_OPCODES) {
      // Build a body that IS a clean multiple of the V2 stride (36), which is
      // exactly the case auto-detection used to accept.
      const body = new Uint8Array(36 * 2);
      const frame = concat([new Uint8Array([opcode]), body]);
      expect(parseBinaryNodeData(frame)).toHaveLength(0);
    }
  });

  it('declines a real agent-action batch', () => {
    expect(parseBinaryNodeData(agentActionFrame(3))).toHaveLength(0);
    expect(parseBinaryFrameData(agentActionFrame(3)).nodes).toHaveLength(0);
  });

  it('lists exactly the sibling opcodes the registry reserves', () => {
    expect([...SIBLING_OPCODES].sort((a, b) => a - b)).toEqual([0x23, 0x43, 0x44]);
  });

  it('still auto-detects a genuinely unknown, non-sibling version', () => {
    // Unchanged legacy behaviour: an unrecognised byte that is not a sibling
    // opcode falls through to size detection rather than being dropped.
    const body = new Uint8Array(36);
    const frame = concat([new Uint8Array([0x77]), body]);
    expect(() => parseBinaryNodeData(frame)).not.toThrow();
  });
});

// ── Robustness: no malformed frame may throw ───────────────────────────────

describe('ADR-2019 decoder robustness', () => {
  it('never throws on any truncation of a valid position frame', () => {
    const full = v5Frame(1, 3);
    for (let cut = 0; cut <= full.byteLength; cut++) {
      expect(() => parseBinaryFrameData(full.slice(0, cut))).not.toThrow();
    }
  });

  it('never throws on any truncation of an agent-action batch', () => {
    const full = agentActionFrame(3);
    for (let cut = 0; cut <= full.byteLength; cut++) {
      expect(() => parseBinaryFrameData(full.slice(0, cut))).not.toThrow();
    }
  });

  it('never throws on arbitrary version bytes', () => {
    for (let v = 0; v < 256; v++) {
      const buf = concat([new Uint8Array([v]), nodeRecord(1)]);
      expect(() => parseBinaryNodeData(buf)).not.toThrow();
    }
  });
});
