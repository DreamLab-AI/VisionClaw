/**
 * GPU id-buffer picking: the node instances are re-rendered with their
 * instance id encoded as RGB into a 1×1 render target whose frustum is the
 * single pixel under the pointer (camera view offset + scissor), only on
 * pointer events. No CPU raycast over 100k instances.
 */
import * as THREE from 'three';
import { PICK_FRAGMENT, PICK_VERTEX } from './shaders';

export function decodePickId(r: number, g: number, b: number): number {
  return (r | (g << 8) | (b << 16)) - 1;
}

export class NodePicker {
  private readonly scene = new THREE.Scene();
  private readonly target = new THREE.WebGLRenderTarget(1, 1, { type: THREE.UnsignedByteType, format: THREE.RGBAFormat, depthBuffer: true });
  private readonly pixel = new Uint8Array(4);
  private readonly material: THREE.ShaderMaterial;
  private mesh: THREE.InstancedMesh | null = null;

  constructor(uniforms: Record<string, THREE.IUniform>) {
    this.material = new THREE.ShaderMaterial({
      vertexShader: PICK_VERTEX,
      fragmentShader: PICK_FRAGMENT,
      uniforms,
      blending: THREE.NoBlending,
      depthTest: true,
      depthWrite: true,
    });
    this.scene.background = null;
  }

  /** Share the colour pass's geometry (same instance attributes, no copy). */
  attach(geometry: THREE.BufferGeometry, count: number): void {
    if (this.mesh) this.scene.remove(this.mesh);
    this.mesh = new THREE.InstancedMesh(geometry, this.material, Math.max(1, count));
    this.mesh.count = count;
    this.mesh.frustumCulled = false;
    this.scene.add(this.mesh);
  }

  /** Returns the instance index under CSS pixel (x, y) of a canvas of CSS size (w, h), or -1. */
  pick(gl: THREE.WebGLRenderer, camera: THREE.PerspectiveCamera, x: number, y: number, w: number, h: number): number {
    if (!this.mesh || this.mesh.count === 0) return -1;
    const dpr = gl.getPixelRatio();
    const prevTarget = gl.getRenderTarget();
    const prevClear = gl.getClearColor(new THREE.Color());
    const prevAlpha = gl.getClearAlpha();
    camera.setViewOffset(Math.floor(w * dpr), Math.floor(h * dpr), Math.floor(x * dpr), Math.floor(y * dpr), 1, 1);
    gl.setRenderTarget(this.target);
    gl.setScissorTest(true);
    gl.setScissor(0, 0, 1, 1);
    gl.setClearColor(0x000000, 0);
    gl.clear(true, true, false);
    gl.render(this.scene, camera);
    gl.setScissorTest(false);
    gl.readRenderTargetPixels(this.target, 0, 0, 1, 1, this.pixel);
    camera.clearViewOffset();
    gl.setRenderTarget(prevTarget);
    gl.setClearColor(prevClear, prevAlpha);
    if (this.pixel[3] === 0) return -1;
    return decodePickId(this.pixel[0], this.pixel[1], this.pixel[2]);
  }

  dispose(): void {
    this.target.dispose();
    this.material.dispose();
  }
}
