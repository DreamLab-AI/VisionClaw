/**
 * Phase 6 (ADR-04 D8 / T7) — Heap-snapshot test for the scene-effects bridge.
 *
 * Asserts that `getPositions()` / `getOpacities()` / `getSizes()` /
 * `getHues()` do NOT allocate new typed-array views in steady state. The
 * first call constructs a view; every subsequent call returns the cached
 * view as long as the underlying WASM memory buffer / pointer / length is
 * unchanged. Memory.grow is the only path that forces a rebuild — a rare
 * recovery, never a per-frame event.
 *
 * The 1000-frame loop measures `process.memoryUsage().heapUsed` before and
 * after; a delta over 64 KB fails the test.
 *
 * This test mocks the raw WASM handle so the binary itself is not required.
 */

import { describe, it, expect, beforeEach } from 'vitest';
import {
  ParticleFieldBridge,
  AtmosphereFieldBridge,
  WispFieldBridge,
} from '../wasm/scene-effects-bridge';

/** Minimal mock implementing the raw WASM ParticleField surface. */
class MockWasmParticleField {
  private _ptr: number;
  private _len: number;
  private _opacityPtr: number;
  private _opacityLen: number;
  private _sizePtr: number;
  private _sizeLen: number;
  private _count: number;
  constructor(memory: WebAssembly.Memory, count: number) {
    this._count = count;
    // Lay out pointers contiguously inside the WASM buffer.
    this._ptr = 0;
    this._len = count * 3;
    this._opacityPtr = count * 3 * 4;
    this._opacityLen = count;
    this._sizePtr = count * 3 * 4 + count * 4;
    this._sizeLen = count;
    // Make sure the buffer is large enough; if not, grow.
    const required = this._sizePtr + count * 4;
    const PAGE = 64 * 1024;
    while (memory.buffer.byteLength < required) {
      memory.grow(1);
    }
    void PAGE;
  }
  update(_dt: number, _x: number, _y: number, _z: number): void { /* no-op */ }
  get_positions_ptr(): number { return this._ptr; }
  get_positions_len(): number { return this._len; }
  get_opacities_ptr(): number { return this._opacityPtr; }
  get_opacities_len(): number { return this._opacityLen; }
  get_sizes_ptr(): number { return this._sizePtr; }
  get_sizes_len(): number { return this._sizeLen; }
  particle_count(): number { return this._count; }
  free(): void { /* no-op */ }
}

class MockWasmAtmosphereField {
  private _ptr: number;
  private _len: number;
  private _w: number;
  private _h: number;
  constructor(memory: WebAssembly.Memory, w: number, h: number) {
    this._w = w;
    this._h = h;
    this._ptr = 0;
    this._len = w * h * 4;
    while (memory.buffer.byteLength < this._len) memory.grow(1);
  }
  update(_dt: number): void { /* no-op */ }
  get_pixels_ptr(): number { return this._ptr; }
  get_pixels_len(): number { return this._len; }
  get_width(): number { return this._w; }
  get_height(): number { return this._h; }
  set_frequency(_f: number): void { /* no-op */ }
  set_speed(_s: number): void { /* no-op */ }
  free(): void { /* no-op */ }
}

class MockWasmEnergyWisps {
  private _count: number;
  private _posPtr: number;
  private _posLen: number;
  private _opPtr: number;
  private _opLen: number;
  private _szPtr: number;
  private _szLen: number;
  private _huPtr: number;
  private _huLen: number;
  constructor(memory: WebAssembly.Memory, count: number) {
    this._count = count;
    let off = 0;
    this._posPtr = off; this._posLen = count * 3; off += this._posLen * 4;
    this._opPtr = off; this._opLen = count; off += this._opLen * 4;
    this._szPtr = off; this._szLen = count; off += this._szLen * 4;
    this._huPtr = off; this._huLen = count; off += this._huLen * 4;
    while (memory.buffer.byteLength < off) memory.grow(1);
  }
  update(_dt: number, _x: number, _y: number, _z: number): void { /* no-op */ }
  set_drift_speed(_s: number): void { /* no-op */ }
  get_positions_ptr(): number { return this._posPtr; }
  get_positions_len(): number { return this._posLen; }
  get_opacities_ptr(): number { return this._opPtr; }
  get_opacities_len(): number { return this._opLen; }
  get_sizes_ptr(): number { return this._szPtr; }
  get_sizes_len(): number { return this._szLen; }
  get_hues_ptr(): number { return this._huPtr; }
  get_hues_len(): number { return this._huLen; }
  wisp_count(): number { return this._count; }
  free(): void { /* no-op */ }
}

function freshMemory(): WebAssembly.Memory {
  return new WebAssembly.Memory({ initial: 4, maximum: 256 });
}

describe('Phase 6 (ADR-04 D8/T7) — scene-effects bridge allocation discipline', () => {
  it('ParticleFieldBridge: same view returned on every call after first', () => {
    const memory = freshMemory();
    const inner = new MockWasmParticleField(memory, 256);
    const bridge = new ParticleFieldBridge(inner as any, memory);

    const v1 = bridge.getPositions();
    const v2 = bridge.getPositions();
    const v3 = bridge.getPositions();

    // Cached: identity equality (not just structural). This proves no new
    // Float32Array is being constructed per call.
    expect(v2).toBe(v1);
    expect(v3).toBe(v1);
  });

  it('ParticleFieldBridge: 1000 getPositions() calls allocate < 64 KB', () => {
    const memory = freshMemory();
    const inner = new MockWasmParticleField(memory, 256);
    const bridge = new ParticleFieldBridge(inner as any, memory);

    // Warm-up: pull views once so the cache is populated.
    bridge.getPositions();
    bridge.getOpacities();
    bridge.getSizes();

    // Allow any vitest internal GC + warmup to settle.
    if (global.gc) global.gc();
    const heapBefore = process.memoryUsage().heapUsed;

    for (let i = 0; i < 1000; i++) {
      const pos = bridge.getPositions();
      const op = bridge.getOpacities();
      const sz = bridge.getSizes();
      // Touch each so the optimiser cannot DCE them.
      void pos.length;
      void op.length;
      void sz.length;
    }

    if (global.gc) global.gc();
    const heapAfter = process.memoryUsage().heapUsed;
    const delta = heapAfter - heapBefore;
    // Tolerance covers Vitest/Node internal allocations from the test loop.
    expect(delta).toBeLessThan(64 * 1024);
  });

  it('AtmosphereFieldBridge: same view returned across calls', () => {
    const memory = freshMemory();
    const inner = new MockWasmAtmosphereField(memory, 64, 64);
    const bridge = new AtmosphereFieldBridge(inner as any, memory);
    const a = bridge.getPixels();
    const b = bridge.getPixels();
    expect(b).toBe(a);
  });

  it('WispFieldBridge: same views returned across calls', () => {
    const memory = freshMemory();
    const inner = new MockWasmEnergyWisps(memory, 64);
    const bridge = new WispFieldBridge(inner as any, memory);
    expect(bridge.getPositions()).toBe(bridge.getPositions());
    expect(bridge.getOpacities()).toBe(bridge.getOpacities());
    expect(bridge.getSizes()).toBe(bridge.getSizes());
    expect(bridge.getHues()).toBe(bridge.getHues());
  });

  it('ParticleFieldBridge: rebuilds views after Memory.grow', () => {
    const memory = freshMemory();
    const inner = new MockWasmParticleField(memory, 256);
    const bridge = new ParticleFieldBridge(inner as any, memory);
    const v1 = bridge.getPositions();
    expect(v1.length).toBe(256 * 3);
    const v1BufferRef = v1.buffer;

    // Grow memory — detaches the previous ArrayBuffer. After this point,
    // touching v1 directly throws. We compare bridge-returned identities.
    memory.grow(2);

    const v2 = bridge.getPositions();
    // After growth, the new view backs a fresh ArrayBuffer; the bridge must
    // have rebuilt. The cached `v2` object is a new Float32Array instance —
    // compare against the captured v1 reference variable (not a direct .buffer
    // access, which would touch v1).
    expect(Object.is(v2, v1)).toBe(false);
    expect(v2.length).toBe(256 * 3);
    // The cached view's underlying buffer must be the live memory.buffer.
    expect(v2.buffer).toBe(memory.buffer);
    // The previous view was over the detached buffer (not the live one).
    expect(v1BufferRef).not.toBe(memory.buffer);
  });

  it('dispose() releases cached views and reports isDisposed', () => {
    const memory = freshMemory();
    const inner = new MockWasmParticleField(memory, 64);
    const bridge = new ParticleFieldBridge(inner as any, memory);
    bridge.getPositions();
    expect(bridge.isDisposed).toBe(false);
    bridge.dispose();
    expect(bridge.isDisposed).toBe(true);
    // After dispose, getPositions returns an empty array — no throw.
    expect(bridge.getPositions().length).toBe(0);
  });
});
