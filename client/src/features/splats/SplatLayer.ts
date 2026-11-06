import type * as THREE from 'three';

/**
 * One loaded splat cloud (ADR-2170 decision 3, three's GaussianSplat addon
 * under WebGPURenderer), read from the served SPZ v3 bytes. The caller places,
 * scales and fades `root`; the layer never animates its own transform.
 */
export interface SplatLayer {
  /** group holding the splats, in the asset's own units; placed by the caller */
  readonly root: THREE.Group;
  /** decode the SPZ bytes and build the GPU resources; resolves once drawable */
  load(bytes: ArrayBuffer): Promise<void>;
  /** add to a scene-level parent (never a graph node or the graph root) */
  attach(parent: THREE.Object3D): void;
  detach(): void;
  /** 0 to 1 multiplier on every splat's alpha */
  setOpacity(opacity: number): void;
  setRenderOrder(order: number): void;
  /** detach and free GPU resources; the layer is unusable afterwards */
  dispose(): void;
}
