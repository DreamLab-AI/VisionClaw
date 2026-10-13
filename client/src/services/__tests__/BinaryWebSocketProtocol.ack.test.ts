import { describe, it, expect, afterEach, vi } from 'vitest';
import { binaryProtocol } from '../BinaryWebSocketProtocol';

const hex = (b: ArrayBuffer) =>
  Array.from(new Uint8Array(b), (x) => x.toString(16).padStart(2, '0')).join('');

describe('createBroadcastAck (ADR-2141 position acknowledgement)', () => {
  afterEach(() => vi.useRealTimers());

  it('is byte-identical to the golden frame the server and XR encoders are tested against', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_700_000_000_123);
    const frame = binaryProtocol.createBroadcastAck(48_113, 2500);
    // [0x34][u64 sequence][u32 nodes][u64 timestamp_ms], little-endian, no generic header.
    // The same literal is asserted in crates/visionclaw-protocol/src/wire_fixtures.rs and in
    // xr-client/rust/src/binary_protocol.rs.
    expect(hex(frame)).toBe('34f1bb000000000000c40900007b68e5cf8b010000');
    expect(frame.byteLength).toBe(21);
  });

  it('has no six-byte message header: the first byte is the opcode and the sequence follows at once', () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    const v = new DataView(binaryProtocol.createBroadcastAck(0x0102030405, 7));
    expect(v.getUint8(0)).toBe(0x34);
    expect(v.getUint32(1, true)).toBe(0x02030405);
    expect(v.getUint32(5, true)).toBe(0x01);
    expect(v.getUint32(9, true)).toBe(7);
  });

  it('carries sequences beyond 2^32 intact', () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    const seq = 2 ** 40 + 5;
    const v = new DataView(binaryProtocol.createBroadcastAck(seq, 1));
    const lo = v.getUint32(1, true);
    const hi = v.getUint32(5, true);
    expect(hi * 2 ** 32 + lo).toBe(seq);
  });
});
