/**
 * The overview layer's GPU node source (ADR-2181 D1/D5, ADR-2182 D1).
 *
 * One RGBA float texture indexed by the shared node render index
 * (state/nodeIndex.ts): texel i = (x, y, z, level), where level is node i's
 * ADR-2183 highlight level (0 greyed, 1 normal, 2 emphasised). Overview edges
 * fetch both endpoints from it and overview sprites fetch their centre from it,
 * so neither keeps a per-edge or per-node CPU loop: the only per-frame CPU work
 * is this texture's refill, and it runs only when the drawn positions or the
 * highlight mask changed (PositionSnapshot + mask version). A settled graph
 * uploads nothing.
 *
 * Row-major, POSITION_TEX_WIDTH texels wide (WebGPU caps a texture dimension
 * at 8192; see GemNodes' METADATA_TEX_WIDTH), so index i is texel
 * (i % W, floor(i / W)). Shaders use texelFetch / textureLoad: no filtering.
 */
import * as THREE from 'three';
import { PositionSnapshot } from '../hooks/edgeBufferPlan';
import { highlightLevel } from '../state/highlightMask';

export const POSITION_TEX_WIDTH = 2048;

/** Texel (x, y) of render index `i`. */
export function texelOf(i: number, width = POSITION_TEX_WIDTH): [number, number] {
  return [i % width, Math.floor(i / width)];
}

export class NodePositionTexture {
  private _texture: THREE.DataTexture;
  private data: Float32Array;
  private rows = 1;
  private readonly snapshot = new PositionSnapshot();
  private maskVersion = -1;
  private _version = 0;
  /** Slots filled on the last upload. */
  private _count = 0;
  private listeners = new Set<() => void>();

  constructor(initialSlots = POSITION_TEX_WIDTH) {
    this.data = new Float32Array(0);
    this._texture = this.allocate(initialSlots);
  }

  get texture(): THREE.DataTexture { return this._texture; }
  get width(): number { return POSITION_TEX_WIDTH; }
  get height(): number { return this.rows; }
  /** Bumps on each upload (and on reallocation). */
  get version(): number { return this._version; }
  get count(): number { return this._count; }

  /** Called when the texture object is replaced (a capacity growth): rebind it. */
  onReallocate(l: () => void): () => void {
    this.listeners.add(l);
    return () => { this.listeners.delete(l); };
  }

  private allocate(slots: number): THREE.DataTexture {
    this.rows = Math.max(1, Math.ceil(Math.max(1, slots) / POSITION_TEX_WIDTH));
    this.data = new Float32Array(POSITION_TEX_WIDTH * this.rows * 4);
    const tex = new THREE.DataTexture(this.data, POSITION_TEX_WIDTH, this.rows, THREE.RGBAFormat, THREE.FloatType);
    tex.minFilter = THREE.NearestFilter;
    tex.magFilter = THREE.NearestFilter;
    tex.generateMipmaps = false;
    tex.needsUpdate = true;
    return tex;
  }

  /**
   * Refill from the drawn positions (xyz per render index) and the highlight
   * mask (one byte of bits per render index). Returns true when it uploaded.
   * No allocation unless the slot count outgrows the texture.
   */
  update(positions: Float32Array, nodeCount: number, mask: Uint8Array, maskVersion: number): boolean {
    const n = Math.max(0, Math.min(nodeCount, Math.floor(positions.length / 3)));
    const posSame = this.snapshot.matches(positions);
    if (posSame && maskVersion === this.maskVersion && n === this._count) return false;
    if (n > POSITION_TEX_WIDTH * this.rows) {
      const old = this._texture;
      // eslint-disable-next-line no-restricted-syntax -- grow-only, power-of-two rows
      this._texture = this.allocate(2 ** Math.ceil(Math.log2(n)));
      old.dispose();
      for (const l of this.listeners) l();
    }
    const d = this.data;
    for (let i = 0; i < n; i++) {
      const i3 = i * 3;
      const i4 = i * 4;
      d[i4] = positions[i3];
      d[i4 + 1] = positions[i3 + 1];
      d[i4 + 2] = positions[i3 + 2];
      d[i4 + 3] = i < mask.length ? highlightLevel(mask[i]) : 1;
    }
    this._count = n;
    this.snapshot.record(positions);
    this.maskVersion = maskVersion;
    this._texture.needsUpdate = true;
    this._version++;
    return true;
  }

  /** Read back one slot (tests, picking): [x, y, z, level]. */
  slot(i: number): [number, number, number, number] {
    const i4 = i * 4;
    return [this.data[i4], this.data[i4 + 1], this.data[i4 + 2], this.data[i4 + 3]];
  }

  dispose(): void {
    this._texture.dispose();
    this.listeners.clear();
  }
}
