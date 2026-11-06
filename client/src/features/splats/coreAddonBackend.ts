import * as THREE from 'three';
import { SPZLoader } from 'three/addons/loaders/SPZLoader.js';
import { GaussianSplat } from 'three/addons/objects/GaussianSplat.js';
import type { SplatLayer } from './SplatLayer';

/**
 * The splat backend: the three `GaussianSplat` addon (TSL; GPU counting sort
 * on the WebGPU backend). It runs under WebGPURenderer on either backend, the
 * only desktop renderer. `SPZLoader.parse` reads SPZ v3
 * (gzip) synchronously and v4 (zstd) asynchronously; the server serves v3.
 */
export class CoreAddonSplatLayer implements SplatLayer {
  readonly root = new THREE.Group();
  private splat: GaussianSplat | null = null;
  private source: THREE.BufferGeometry | null = null;
  private parent: THREE.Object3D | null = null;

  /** @param gl the live WebGPURenderer, used only to release compute pipelines on dispose */
  constructor(private readonly gl?: unknown) {}

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
      // when each attribute dispatches 'dispose', and compute nodes (the sort
      // passes and the spherical-harmonics pass) whose pipelines live in the
      // renderer until it is told to drop them.
      const internal = splat as unknown as Record<string, unknown>;
      for (const owner of [internal._buffers, internal._sort, internal._sphericalHarmonicsComputeNode]) {
        if (isNode(owner)) this.releaseNode(owner);
        else for (const value of Object.values((owner as object | undefined) ?? {})) this.releaseValue(value);
      }
      splat.geometry.dispose();
      splat.material.dispose();
    }
    this.source?.dispose();
    this.splat = null;
    this.source = null;
  }

  /** A storage attribute, a storage node wrapping one, or a compute node. */
  private releaseValue(value: unknown): void {
    if (isStorageAttribute(value)) {
      value.dispose?.();
    } else if (isNode(value)) {
      this.releaseNode(value);
    }
  }

  private releaseNode(node: NodeLike): void {
    if (isStorageAttribute(node.value)) node.value.dispose?.();
    if (node.isComputeNode === true) {
      // Pipelines.delete is not public API; guarded so a three change degrades to a leak, not a throw.
      try {
        (this.gl as { _pipelines?: { delete?: (o: unknown) => void } } | undefined)?._pipelines?.delete?.(node);
      } catch {
        /* the pipeline is released with the renderer */
      }
    }
    node.dispose?.();
  }
}

interface Disposable {
  dispose?: () => void;
}

interface NodeLike extends Disposable {
  isNode?: boolean;
  isComputeNode?: boolean;
  value?: unknown;
}

function isNode(v: unknown): v is NodeLike {
  return typeof v === 'object' && v !== null && (v as NodeLike).isNode === true;
}

function isStorageAttribute(v: unknown): v is Disposable {
  return typeof v === 'object' && v !== null && (v as { isStorageBufferAttribute?: boolean }).isStorageBufferAttribute === true;
}
