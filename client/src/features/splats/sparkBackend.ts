import * as THREE from 'three';
import { SparkRenderer, SplatFileType, SplatMesh } from '@sparkjsdev/spark';
import type { SplatBackendKind, SplatLayer } from './SplatLayer';

/**
 * WebGL path: Spark 2.4 `SparkRenderer` on the existing clean WebGLRenderer.
 * Spark is WebGL2-only and cannot run under WebGPURenderer, so this module is
 * imported only when the live renderer is a plain WebGLRenderer. The served
 * bytes are SPZ v3, passed as `fileBytes` with an explicit file type.
 */
export class SparkSplatLayer implements SplatLayer {
  readonly kind: SplatBackendKind = 'spark';
  readonly root = new THREE.Group();
  private mesh: SplatMesh | null = null;
  private spark: SparkRenderer | null = null;
  private parent: THREE.Object3D | null = null;
  private renderOrder = 0;

  constructor(private readonly gl: THREE.WebGLRenderer) {}

  async load(bytes: ArrayBuffer): Promise<void> {
    const mesh = new SplatMesh({ fileBytes: new Uint8Array(bytes), fileType: SplatFileType.SPZ });
    try {
      await mesh.initialized;
    } catch (e) {
      mesh.dispose();
      throw e;
    }
    this.mesh = mesh;
    this.root.add(mesh);
  }

  attach(parent: THREE.Object3D): void {
    this.parent = parent;
    // The SparkRenderer is created here, not in load(), so a layer that is
    // loaded but never shown never holds a render target.
    this.spark ??= new SparkRenderer({ renderer: this.gl, depthWrite: false });
    this.spark.renderOrder = this.renderOrder;
    parent.add(this.spark);
    parent.add(this.root);
  }

  detach(): void {
    if (this.spark) this.parent?.remove(this.spark);
    this.parent?.remove(this.root);
    this.parent = null;
  }

  setOpacity(opacity: number): void {
    if (this.mesh) this.mesh.opacity = opacity;
  }

  setRenderOrder(order: number): void {
    this.renderOrder = order;
    if (this.spark) this.spark.renderOrder = order;
  }

  dispose(): void {
    this.detach();
    if (this.mesh) {
      this.root.remove(this.mesh);
      this.mesh.dispose();
      this.mesh = null;
    }
    this.spark?.dispose();
    this.spark = null;
  }
}
