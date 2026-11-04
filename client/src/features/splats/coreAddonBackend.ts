import * as THREE from 'three';
import { SPZLoader } from 'three/addons/loaders/SPZLoader.js';
import { GaussianSplat } from 'three/addons/objects/GaussianSplat.js';
import type { SplatBackendKind, SplatLayer } from './SplatLayer';

/**
 * WebGPU path: the three `GaussianSplat` addon (TSL, GPU counting sort). It
 * runs only under WebGPURenderer, so this module is imported only after the
 * live renderer has been identified as one. `SPZLoader.parse` reads SPZ v3
 * (gzip) synchronously and v4 (zstd) asynchronously; the server serves v3.
 */
export class CoreAddonSplatLayer implements SplatLayer {
  readonly kind: SplatBackendKind = 'core-addon';
  readonly root = new THREE.Group();
  private splat: GaussianSplat | null = null;
  private source: THREE.BufferGeometry | null = null;
  private parent: THREE.Object3D | null = null;

  async load(bytes: ArrayBuffer): Promise<void> {
    const geometry = await new SPZLoader().parse(bytes);
    if (!geometry) throw new Error('SPZ parse produced no geometry');
    const splat = new GaussianSplat(geometry);
    splat.frustumCulled = false;
    splat.material.transparent = true;
    splat.material.depthWrite = false;
    this.source = geometry;
    this.splat = splat;
    this.root.add(splat);
  }

  attach(parent: THREE.Object3D): void {
    this.parent = parent;
    parent.add(this.root);
  }

  detach(): void {
    this.parent?.remove(this.root);
    this.parent = null;
  }

  setOpacity(opacity: number): void {
    if (!this.splat) return;
    this.splat.material.opacity = opacity;
    this.splat.visible = opacity > 0;
  }

  setRenderOrder(order: number): void {
    if (this.splat) this.splat.renderOrder = order;
  }

  dispose(): void {
    this.detach();
    const splat = this.splat;
    if (splat) {
      this.root.remove(splat);
      // The addon has no dispose(): it owns storage buffers (splat data, the
      // sort's order, bin, histogram and offset buffers) that only release
      // when each attribute dispatches 'dispose'.
      const internal = splat as unknown as { _buffers?: object; _sort?: object };
      for (const owner of [internal._buffers, internal._sort]) {
        for (const value of Object.values(owner ?? {})) disposeStorage(value);
      }
      splat.geometry.dispose();
      splat.material.dispose();
    }
    this.source?.dispose();
    this.splat = null;
    this.source = null;
  }
}

/** Dispose a StorageBufferAttribute, or the storage node that wraps one. */
function disposeStorage(value: unknown): void {
  const v = value as { isStorageBufferAttribute?: boolean; value?: unknown; dispose?: () => void } | null;
  if (!v || typeof v !== 'object') return;
  if (v.isStorageBufferAttribute === true) {
    v.dispose?.();
    return;
  }
  const inner = v.value as { isStorageBufferAttribute?: boolean; dispose?: () => void } | undefined;
  if (inner?.isStorageBufferAttribute === true) inner.dispose?.();
}
