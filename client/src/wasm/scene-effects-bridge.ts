/**
 * scene-effects-bridge.ts
 *
 * TypeScript bridge for the scene-effects WASM module.
 * Provides typed wrappers around the raw WASM exports and creates
 * zero-copy Float32Array / Uint8Array views over WASM linear memory.
 *
 * Usage:
 *   const wasm = await initSceneEffects();
 *   const particles = wasm.createParticleField(256);
 *   particles.update(0.016, 0, 0, 5);
 *   const positions = particles.getPositions(); // Float32Array view
 */

import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('scene-effects');

// The WASM module is loaded dynamically, so these types mirror the
// generated .d.ts but we define them here to avoid import-time failures
// when WASM is unavailable.

export interface SceneEffectsModule {
  memory: WebAssembly.Memory;
  ParticleField: new (count: number) => WasmParticleField;
  AtmosphereField: new (width: number, height: number) => WasmAtmosphereField;
  EnergyWisps: new (count: number) => WasmEnergyWisps;
  version: () => string;
}

/** Raw WASM ParticleField handle. */
interface WasmParticleField {
  update(dt: number, camera_x: number, camera_y: number, camera_z: number): void;
  get_positions_ptr(): number;
  get_positions_len(): number;
  get_opacities_ptr(): number;
  get_opacities_len(): number;
  get_sizes_ptr(): number;
  get_sizes_len(): number;
  particle_count(): number;
  free(): void;
}

/** Raw WASM EnergyWisps handle. */
interface WasmEnergyWisps {
  update(dt: number, camera_x: number, camera_y: number, camera_z: number): void;
  set_drift_speed(speed: number): void;
  get_positions_ptr(): number;
  get_positions_len(): number;
  get_opacities_ptr(): number;
  get_opacities_len(): number;
  get_sizes_ptr(): number;
  get_sizes_len(): number;
  get_hues_ptr(): number;
  get_hues_len(): number;
  wisp_count(): number;
  free(): void;
}

/** Raw WASM AtmosphereField handle. */
interface WasmAtmosphereField {
  update(dt: number): void;
  get_pixels_ptr(): number;
  get_pixels_len(): number;
  get_width(): number;
  get_height(): number;
  set_frequency(freq: number): void;
  set_speed(speed: number): void;
  free(): void;
}

/**
 * Wrapped particle field that exposes typed array views over WASM memory.
 *
 * Phase 6 (ADR-04 D8 / T7): views are cached after first access and only
 * rebuilt when `WebAssembly.Memory` grows (detected by buffer.byteLength
 * changing). In steady-state — the expected path — getPositions() does NOT
 * allocate; it returns the cached `Float32Array` view. This satisfies the
 * zero-allocation rendering invariant for hot loops inside `useFrame`.
 *
 * Memory growth is a rare recovery path (typically zero events per session).
 * The growth detector rebuilds views and logs a single warn.
 */
export class ParticleFieldBridge {
  private inner: WasmParticleField;
  private memory: WebAssembly.Memory;
  private _disposed = false;
  // Cached views over WASM memory — null until first refresh.
  private _posView: Float32Array | null = null;
  private _opacityView: Float32Array | null = null;
  private _sizeView: Float32Array | null = null;
  private _lastBufferByteLength = 0;
  // Track the underlying pointer/len at last view construction. If the
  // WASM module reallocates internal buffers (move-on-grow), the pointers
  // shift even when buffer.byteLength is unchanged. Defensive comparison
  // keeps the views correct.
  private _lastPosPtr = -1;
  private _lastPosLen = -1;
  private _lastOpacityPtr = -1;
  private _lastOpacityLen = -1;
  private _lastSizePtr = -1;
  private _lastSizeLen = -1;

  constructor(inner: WasmParticleField, memory: WebAssembly.Memory) {
    this.inner = inner;
    this.memory = memory;
  }

  /** True after dispose() has been called. Guards against use-after-free. */
  get isDisposed(): boolean { return this._disposed; }

  /** Advance simulation. Call once per frame. */
  update(dt: number, cameraX: number, cameraY: number, cameraZ: number): void {
    if (this._disposed) return;
    this.inner.update(dt, cameraX, cameraY, cameraZ);
  }

  /**
   * Rebuild all cached views. Called lazily when buffer.byteLength has
   * changed (Memory.grow) or when the underlying pointer/len has shifted.
   * Logs at warn level on every rebuild after the first because steady-state
   * is "never grow"; rebuilds indicate a buffer relocation we should know about.
   */
  private refreshViews(): void {
    const byteLen = this.memory.buffer.byteLength;
    const posPtr = this.inner.get_positions_ptr();
    const posLen = this.inner.get_positions_len();
    const opPtr = this.inner.get_opacities_ptr();
    const opLen = this.inner.get_opacities_len();
    const szPtr = this.inner.get_sizes_ptr();
    const szLen = this.inner.get_sizes_len();
    const unchanged =
      this._posView !== null &&
      byteLen === this._lastBufferByteLength &&
      posPtr === this._lastPosPtr && posLen === this._lastPosLen &&
      opPtr === this._lastOpacityPtr && opLen === this._lastOpacityLen &&
      szPtr === this._lastSizePtr && szLen === this._lastSizeLen;
    if (unchanged) return;

    // Bounds-check before constructing views.
    if (posPtr + posLen * 4 > byteLen) throw new Error('WASM pointer out of bounds: positions');
    if (opPtr + opLen * 4 > byteLen) throw new Error('WASM pointer out of bounds: opacities');
    if (szPtr + szLen * 4 > byteLen) throw new Error('WASM pointer out of bounds: sizes');

    if (this._posView !== null) {
      // This is a *rebuild*, not the first build. ADR-04 R4: log once when
      // it happens so debugging can correlate with WASM memory events.
      logger.warn(
        `[scene-effects-bridge] ParticleFieldBridge: rebuilding views ` +
        `(buffer ${this._lastBufferByteLength}->${byteLen}, ` +
        `posPtr ${this._lastPosPtr}->${posPtr}). Memory.grow or buffer relocation detected.`
      );
    }
    this._posView = new Float32Array(this.memory.buffer, posPtr, posLen);
    this._opacityView = new Float32Array(this.memory.buffer, opPtr, opLen);
    this._sizeView = new Float32Array(this.memory.buffer, szPtr, szLen);
    this._lastBufferByteLength = byteLen;
    this._lastPosPtr = posPtr; this._lastPosLen = posLen;
    this._lastOpacityPtr = opPtr; this._lastOpacityLen = opLen;
    this._lastSizePtr = szPtr; this._lastSizeLen = szLen;
  }

  /** Zero-copy Float32Array view of particle positions [x,y,z, x,y,z, ...]. */
  getPositions(): Float32Array {
    if (this._disposed) return new Float32Array(0);
    this.refreshViews();
    return this._posView!;
  }

  /** Zero-copy Float32Array view of per-particle opacities. */
  getOpacities(): Float32Array {
    if (this._disposed) return new Float32Array(0);
    this.refreshViews();
    return this._opacityView!;
  }

  /** Zero-copy Float32Array view of per-particle sizes. */
  getSizes(): Float32Array {
    if (this._disposed) return new Float32Array(0);
    this.refreshViews();
    return this._sizeView!;
  }

  /** Number of particles in this field. */
  get count(): number {
    if (this._disposed) return 0;
    return this.inner.particle_count();
  }

  /** Release WASM resources. */
  dispose(): void {
    if (this._disposed) return;
    this._disposed = true;
    this._posView = null;
    this._opacityView = null;
    this._sizeView = null;
    this.inner.free();
  }
}

/**
 * Wrapped atmosphere field that provides a Uint8Array RGBA texture view.
 *
 * Phase 6 (ADR-04 D8 / T7): view cached across calls; rebuilt only when the
 * WASM memory buffer grows or the underlying pointer/length changes.
 */
export class AtmosphereFieldBridge {
  private inner: WasmAtmosphereField;
  private memory: WebAssembly.Memory;
  private _disposed = false;
  private _pixelView: Uint8Array | null = null;
  private _lastBufferByteLength = 0;
  private _lastPixelPtr = -1;
  private _lastPixelLen = -1;

  constructor(inner: WasmAtmosphereField, memory: WebAssembly.Memory) {
    this.inner = inner;
    this.memory = memory;
  }

  /** True after dispose() has been called. */
  get isDisposed(): boolean { return this._disposed; }

  /** Advance the atmosphere texture. Call once per frame. */
  update(dt: number): void {
    if (this._disposed) return;
    this.inner.update(dt);
  }

  private refreshViews(): void {
    const byteLen = this.memory.buffer.byteLength;
    const ptr = this.inner.get_pixels_ptr();
    const len = this.inner.get_pixels_len();
    if (
      this._pixelView !== null &&
      byteLen === this._lastBufferByteLength &&
      ptr === this._lastPixelPtr &&
      len === this._lastPixelLen
    ) {
      return;
    }
    if (ptr + len > byteLen) throw new Error('WASM pointer out of bounds: atmosphere pixels');
    if (this._pixelView !== null) {
      logger.warn(
        `[scene-effects-bridge] AtmosphereFieldBridge: rebuilding view ` +
        `(buffer ${this._lastBufferByteLength}->${byteLen}, ptr ${this._lastPixelPtr}->${ptr}).`
      );
    }
    this._pixelView = new Uint8Array(this.memory.buffer, ptr, len);
    this._lastBufferByteLength = byteLen;
    this._lastPixelPtr = ptr;
    this._lastPixelLen = len;
  }

  /** Zero-copy Uint8Array view of RGBA pixel data. */
  getPixels(): Uint8Array {
    if (this._disposed) return new Uint8Array(0);
    this.refreshViews();
    return this._pixelView!;
  }

  /** Texture width in pixels. */
  get width(): number {
    if (this._disposed) return 0;
    return this.inner.get_width();
  }

  /** Texture height in pixels. */
  get height(): number {
    if (this._disposed) return 0;
    return this.inner.get_height();
  }

  /** Set noise frequency (higher = finer detail). */
  setFrequency(freq: number): void {
    if (this._disposed) return;
    this.inner.set_frequency(freq);
  }

  /** Set animation speed multiplier. */
  setSpeed(speed: number): void {
    if (this._disposed) return;
    this.inner.set_speed(speed);
  }

  /** Release WASM resources. */
  dispose(): void {
    if (this._disposed) return;
    this._disposed = true;
    this._pixelView = null;
    this.inner.free();
  }
}

/**
 * Wrapped energy wisps field: ephemeral glowing orbs with lifecycle fade.
 *
 * Phase 6 (ADR-04 D8 / T7): all four typed array views (positions, opacities,
 * sizes, hues) are cached; refreshed only on buffer growth/relocation.
 */
export class WispFieldBridge {
  private inner: WasmEnergyWisps;
  private memory: WebAssembly.Memory;
  private _disposed = false;
  private _posView: Float32Array | null = null;
  private _opacityView: Float32Array | null = null;
  private _sizeView: Float32Array | null = null;
  private _hueView: Float32Array | null = null;
  private _lastBufferByteLength = 0;
  private _lastPosPtr = -1;
  private _lastPosLen = -1;
  private _lastOpacityPtr = -1;
  private _lastOpacityLen = -1;
  private _lastSizePtr = -1;
  private _lastSizeLen = -1;
  private _lastHuePtr = -1;
  private _lastHueLen = -1;

  constructor(inner: WasmEnergyWisps, memory: WebAssembly.Memory) {
    this.inner = inner;
    this.memory = memory;
  }

  /** True after dispose() has been called. */
  get isDisposed(): boolean { return this._disposed; }

  /** Advance simulation. Call once per frame. */
  update(dt: number, cameraX: number, cameraY: number, cameraZ: number): void {
    if (this._disposed) return;
    this.inner.update(dt, cameraX, cameraY, cameraZ);
  }

  /** Set drift speed multiplier (default 1.0). */
  setDriftSpeed(speed: number): void {
    if (this._disposed) return;
    this.inner.set_drift_speed(speed);
  }

  private refreshViews(): void {
    const byteLen = this.memory.buffer.byteLength;
    const posPtr = this.inner.get_positions_ptr();
    const posLen = this.inner.get_positions_len();
    const opPtr = this.inner.get_opacities_ptr();
    const opLen = this.inner.get_opacities_len();
    const szPtr = this.inner.get_sizes_ptr();
    const szLen = this.inner.get_sizes_len();
    const huPtr = this.inner.get_hues_ptr();
    const huLen = this.inner.get_hues_len();
    const unchanged =
      this._posView !== null &&
      byteLen === this._lastBufferByteLength &&
      posPtr === this._lastPosPtr && posLen === this._lastPosLen &&
      opPtr === this._lastOpacityPtr && opLen === this._lastOpacityLen &&
      szPtr === this._lastSizePtr && szLen === this._lastSizeLen &&
      huPtr === this._lastHuePtr && huLen === this._lastHueLen;
    if (unchanged) return;

    if (posPtr + posLen * 4 > byteLen) throw new Error('WASM pointer out of bounds: wisp positions');
    if (opPtr + opLen * 4 > byteLen) throw new Error('WASM pointer out of bounds: wisp opacities');
    if (szPtr + szLen * 4 > byteLen) throw new Error('WASM pointer out of bounds: wisp sizes');
    if (huPtr + huLen * 4 > byteLen) throw new Error('WASM pointer out of bounds: wisp hues');

    if (this._posView !== null) {
      logger.warn(
        `[scene-effects-bridge] WispFieldBridge: rebuilding views ` +
        `(buffer ${this._lastBufferByteLength}->${byteLen}). Memory.grow or relocation.`
      );
    }
    this._posView = new Float32Array(this.memory.buffer, posPtr, posLen);
    this._opacityView = new Float32Array(this.memory.buffer, opPtr, opLen);
    this._sizeView = new Float32Array(this.memory.buffer, szPtr, szLen);
    this._hueView = new Float32Array(this.memory.buffer, huPtr, huLen);
    this._lastBufferByteLength = byteLen;
    this._lastPosPtr = posPtr; this._lastPosLen = posLen;
    this._lastOpacityPtr = opPtr; this._lastOpacityLen = opLen;
    this._lastSizePtr = szPtr; this._lastSizeLen = szLen;
    this._lastHuePtr = huPtr; this._lastHueLen = huLen;
  }

  /** Zero-copy Float32Array view of wisp positions [x,y,z, ...]. */
  getPositions(): Float32Array {
    if (this._disposed) return new Float32Array(0);
    this.refreshViews();
    return this._posView!;
  }

  /** Zero-copy Float32Array view of per-wisp opacities. */
  getOpacities(): Float32Array {
    if (this._disposed) return new Float32Array(0);
    this.refreshViews();
    return this._opacityView!;
  }

  /** Zero-copy Float32Array view of per-wisp sizes. */
  getSizes(): Float32Array {
    if (this._disposed) return new Float32Array(0);
    this.refreshViews();
    return this._sizeView!;
  }

  /** Zero-copy Float32Array view of per-wisp hue offsets (0..1). */
  getHues(): Float32Array {
    if (this._disposed) return new Float32Array(0);
    this.refreshViews();
    return this._hueView!;
  }

  /** Number of wisps. */
  get count(): number {
    if (this._disposed) return 0;
    return this.inner.wisp_count();
  }

  /** Release WASM resources. */
  dispose(): void {
    if (this._disposed) return;
    this._disposed = true;
    this._posView = null;
    this._opacityView = null;
    this._sizeView = null;
    this._hueView = null;
    this.inner.free();
  }
}

/**
 * Public API returned from initSceneEffects().
 */
export interface SceneEffectsAPI {
  /** Create a particle field with up to `count` particles (max 512). */
  createParticleField(count: number): ParticleFieldBridge;
  /** Create an atmosphere texture generator of the given resolution. */
  createAtmosphereField(width: number, height: number): AtmosphereFieldBridge;
  /** Create an energy wisps field with up to `count` wisps (max 128). */
  createWispField(count: number): WispFieldBridge;
  /** WASM module version string. */
  version: string;
}

// Module-level singleton to prevent double-init.
// State machine prevents thundering-herd retries on failure.
let cachedAPI: SceneEffectsAPI | null = null;
let initPromise: Promise<SceneEffectsAPI> | null = null;
let initState: 'idle' | 'loading' | 'ready' | 'failed' = 'idle';

/**
 * Initialize the WASM scene effects module.
 *
 * Safe to call multiple times -- returns the cached instance after the
 * first successful init. On failure, the rejected promise is cached for
 * 1 second to prevent concurrent callers from stampeding retries.
 */
export async function initSceneEffects(): Promise<SceneEffectsAPI> {
  if (cachedAPI) return cachedAPI;
  if (initPromise) return initPromise;

  initState = 'loading';
  initPromise = (async () => {
    try {
      // Dynamic import of the wasm-pack generated glue code.
      // The path is relative to where this bridge file sits in the build output.
      const wasmModule = await import('./scene-effects/scene_effects.js');
      const initOutput = await wasmModule.default();
      const memory = initOutput.memory;

      cachedAPI = {
        createParticleField(count: number): ParticleFieldBridge {
          const inner = new wasmModule.ParticleField(count);
          return new ParticleFieldBridge(inner, memory);
        },
        createAtmosphereField(width: number, height: number): AtmosphereFieldBridge {
          const inner = new wasmModule.AtmosphereField(width, height);
          return new AtmosphereFieldBridge(inner, memory);
        },
        createWispField(count: number): WispFieldBridge {
          const inner = new wasmModule.EnergyWisps(count);
          return new WispFieldBridge(inner, memory);
        },
        version: wasmModule.version(),
      };

      initState = 'ready';
      logger.info(`WASM loaded, version ${cachedAPI.version}`);
      return cachedAPI;
    } catch (err) {
      initState = 'failed';
      cachedAPI = null;
      // Keep the rejected promise cached for 1 second so concurrent
      // callers receive the same rejection instead of stampeding retries.
      setTimeout(() => {
        if (initState === 'failed') {
          initPromise = null;
          initState = 'idle';
        }
      }, 1000);
      logger.warn('WASM failed to load, effects disabled:', err);
      throw err;
    }
  })();

  return initPromise;
}
