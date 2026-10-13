/**
 * ADR-2057 — fixture-backed cross-check for the LIVE TypeScript position decoder.
 *
 * Both Rust decoders pin their wire constants against
 * `crates/visionclaw-protocol/src/wire_fixtures.rs` (asserted at
 * `xr-client/rust/src/binary_protocol.rs:916-918`). The TypeScript decoder was
 * the only one with no such cross-check, which is precisely why it drifted —
 * advertising a V2 the server rejects. These tests pin the same constants and
 * round-trip a synthetic V5 envelope so the drift cannot recur silently.
 *
 * Constants mirrored from wire_fixtures.rs:
 *   NODE_RECORD_BYTES = 52, NODE_ID_MASK = 0x03FF_FFFF
 * and from `src/utils/binary_protocol.rs` / `xr-client/rust/src/binary_protocol.rs`:
 *   PROTOCOL_V3 = 3, PROTOCOL_V5 = 5, V5 sequence prefix = 8 bytes.
 */
import { describe, it, expect } from 'vitest';
import { SUPPORTED_HEADER_VERSIONS } from '../../services/binaryProtocol/frameTypes';
import {
  BINARY_NODE_SIZE_V3,
  NODE_ID_MASK,
  PROTOCOL_V3,
  PROTOCOL_V5,
  parseBinaryNodeData,
  getActualNodeId,
} from '../binaryProtocol';

const V5_SEQ_BYTES = 8;

/** Build one 52-byte V3 record with known field values. */
function writeRecord(view: DataView, off: number, rawId: number): void {
  view.setUint32(off + 0, rawId, true);
  view.setFloat32(off + 4, 1.5, true); // position x
  view.setFloat32(off + 8, -2.25, true); // position y
  view.setFloat32(off + 12, 3.75, true); // position z
  view.setFloat32(off + 16, 0.5, true); // velocity x
  view.setFloat32(off + 20, -0.25, true); // velocity y
  view.setFloat32(off + 24, 0.125, true); // velocity z
  view.setFloat32(off + 28, 12.5, true); // sssp distance
  view.setInt32(off + 32, 7, true); // sssp parent
  view.setUint32(off + 36, 3, true); // cluster id
  view.setFloat32(off + 40, 0.75, true); // anomaly
  view.setUint32(off + 44, 9, true); // community id
  view.setFloat32(off + 48, 0.5, true); // centrality
}

function v3Frame(rawId: number): ArrayBuffer {
  const buf = new ArrayBuffer(1 + BINARY_NODE_SIZE_V3);
  const view = new DataView(buf);
  view.setUint8(0, PROTOCOL_V3);
  writeRecord(view, 1, rawId);
  return buf;
}

function v5Frame(rawId: number, seq: bigint): ArrayBuffer {
  const buf = new ArrayBuffer(1 + V5_SEQ_BYTES + BINARY_NODE_SIZE_V3);
  const view = new DataView(buf);
  view.setUint8(0, PROTOCOL_V5);
  view.setBigUint64(1, seq, true); // little-endian, per the server
  writeRecord(view, 1 + V5_SEQ_BYTES, rawId);
  return buf;
}

describe('wire constants (pinned against crates/visionclaw-protocol/src/wire_fixtures.rs)', () => {
  it('pins the V3/V5 node record at 52 bytes', () => {
    expect(BINARY_NODE_SIZE_V3).toBe(52);
  });

  it('pins the node-id mask to bits 0-25', () => {
    expect(NODE_ID_MASK).toBe(0x03ffffff);
  });

  it('pins the protocol version bytes the server emits', () => {
    expect(PROTOCOL_V3).toBe(3);
    expect(PROTOCOL_V5).toBe(5);
  });

  it('gates the framed header on V3/V5 only — 4 was never written there (ADR-2078)', () => {
    // The framed header's version byte carries the POSITION protocol version.
    // `PROTOCOL_V4` names exactly one thing: the RETIRED delta tag, in
    // client/src/types/binaryProtocol.ts, which the decoder refuses. It is not a header version.
    expect([...SUPPORTED_HEADER_VERSIONS].sort()).toEqual([PROTOCOL_V3, PROTOCOL_V5]);
    expect(SUPPORTED_HEADER_VERSIONS).not.toContain(4);
  });
});

describe('live position decoder', () => {
  it('decodes a bare V3 frame', () => {
    const nodes = parseBinaryNodeData(v3Frame(42));
    expect(nodes).toHaveLength(1);
    expect(nodes[0].nodeId).toBe(42);
    expect(nodes[0].position.x).toBeCloseTo(1.5);
    expect(nodes[0].position.y).toBeCloseTo(-2.25);
    expect(nodes[0].position.z).toBeCloseTo(3.75);
    expect(nodes[0].velocity.x).toBeCloseTo(0.5);
  });

  it('decodes a V5 envelope — the body carries no inner 0x03 byte', () => {
    const nodes = parseBinaryNodeData(v5Frame(42, 123456789n));
    expect(nodes).toHaveLength(1);
    expect(nodes[0].nodeId).toBe(42);
    expect(nodes[0].position.x).toBeCloseTo(1.5);
    expect(nodes[0].velocity.z).toBeCloseTo(0.125);
  });

  it('yields identical nodes for a V3 frame and the same body inside a V5 envelope', () => {
    expect(parseBinaryNodeData(v5Frame(7, 1n))).toEqual(parseBinaryNodeData(v3Frame(7)));
  });

  it('preserves node-class flag bits on the wire id and strips them via getActualNodeId', () => {
    const AGENT_FLAG = 0x80000000;
    const raw = (AGENT_FLAG | 42) >>> 0;
    const nodes = parseBinaryNodeData(v3Frame(raw));
    // The decoder deliberately returns the RAW id: bits 26-31 carry the node
    // class and callers need them. Stripping is the caller's explicit step.
    expect(nodes[0].nodeId).toBe(raw);
    expect(getActualNodeId(nodes[0].nodeId)).toBe(42);
    expect(getActualNodeId(nodes[0].nodeId)).toBe(raw & NODE_ID_MASK);
  });

  it('declines a V2 frame — the server rejects V2 outright (ADR-2057)', () => {
    const buf = new ArrayBuffer(1 + 36);
    new DataView(buf).setUint8(0, 2);
    expect(parseBinaryNodeData(buf)).toEqual([]);
  });

  it('declines an unknown protocol version instead of auto-detecting (ADR-2057)', () => {
    const buf = new ArrayBuffer(1 + 36);
    new DataView(buf).setUint8(0, 0x7f);
    expect(parseBinaryNodeData(buf)).toEqual([]);
  });

  it('declines a V5 frame shorter than the 8-byte sequence, matching the server', () => {
    // Server: `if payload.len() < WIRE_V5_SEQ_SIZE` -> "V5 frame too small for
    // broadcast sequence" (src/utils/binary_protocol.rs:594). The client's
    // equivalent boundary is the whole frame: 1 version byte + 8 sequence bytes.
    for (const len of [1, 4, 8]) {
      const buf = new ArrayBuffer(len);
      new DataView(buf).setUint8(0, PROTOCOL_V5);
      expect(parseBinaryNodeData(buf)).toEqual([]);
    }
    // Exactly the header with no body is also empty, not garbage.
    const headerOnly = new ArrayBuffer(1 + V5_SEQ_BYTES);
    new DataView(headerOnly).setUint8(0, PROTOCOL_V5);
    expect(parseBinaryNodeData(headerOnly)).toEqual([]);
  });

  it('declines a sibling opcode rather than reinterpreting it as node records', () => {
    for (const opcode of [0x23, 0x43, 0x44]) {
      const buf = new ArrayBuffer(1 + BINARY_NODE_SIZE_V3);
      new DataView(buf).setUint8(0, opcode);
      expect(parseBinaryNodeData(buf)).toEqual([]);
    }
  });
});
