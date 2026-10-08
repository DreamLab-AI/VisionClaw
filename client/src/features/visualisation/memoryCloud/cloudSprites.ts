/**
 * The memory cloud's points on the WebGPU renderer.
 *
 * WebGPU has no point size: three's WebGPURenderer draws a `THREE.Points`
 * as one-pixel point primitives and ignores `size`, and with no point
 * coordinate the disc sprite `map` samples one fixed texel, the sprite's
 * transparent corner, so `alphaTest` discarded every point. On WebGPU the
 * cloud drew nothing at all (operator report 2026-10-08, "is it off?").
 *
 * three r183's answer is an instanced `Sprite` with a `PointsNodeMaterial`:
 * one camera-facing quad per row, positioned from an instanced attribute,
 * sized by `material.size` with the same attenuation as WebGLRenderer
 * (size × half canvas height / depth). The disc is drawn in the shader from
 * the quad's UV, matching `discSpritePixels` (opaque inside 70% of the
 * radius, smooth to 0 at the rim). The colour attribute wraps the caller's
 * buffer, so the focus dimming written there reaches the sprites.
 *
 * The WebGL path keeps `THREE.Points` (gl_PointCoord drives the map there).
 * On both paths the `<points>` object stays mounted for hover picking.
 */

import type * as THREE from 'three';

export interface CloudSprites {
  /** add to the cloud's inner (cloud-local) group */
  object: THREE.Object3D;
  material: THREE.Material;
  /** the colour buffer the sprites read (the caller's array) */
  colourArray: Float32Array;
  /** rows drawn, clamped to the buffer */
  setCount(n: number): void;
  /** world point size; attenuated with depth like PointsMaterial */
  setSize(size: number): void;
  setOpacity(opacity: number): void;
  /** call after rewriting `colourArray` */
  markColoursDirty(): void;
  colourVersion(): number;
  dispose(): void;
}

/** soft edge of the disc, as a fraction of its radius (discSpritePixels) */
const DISC_INNER = 0.7;

/**
 * Build the instanced sprite cloud. `positions` and `colours` are flat xyz
 * and rgb per row; `count` is the number drawn.
 */
export async function createCloudSprites(
  positions: Float32Array,
  colours: Float32Array,
  count: number,
): Promise<CloudSprites> {
  const W = await import('three/webgpu');
  const T = await import('three/tsl');
  const rows = Math.floor(Math.min(positions.length, colours.length) / 3);
  const posAttr = new W.InstancedBufferAttribute(positions, 3);
  const colAttr = new W.InstancedBufferAttribute(colours, 3);

  const material = new W.PointsNodeMaterial({
    transparent: true,
    depthWrite: false,
    sizeAttenuation: true,
  });
  material.positionNode = T.instancedBufferAttribute(posAttr);
  material.colorNode = T.vec4(T.instancedBufferAttribute(colAttr), 1);
  const r = T.uv().sub(0.5).length().mul(2);
  material.opacityNode = T.float(T.materialOpacity).mul(T.float(1).sub(T.smoothstep(DISC_INNER, 1, r)));

  const sprite = new W.Sprite(material);
  sprite.name = 'embedding-cloud-sprites';
  sprite.frustumCulled = false;
  sprite.raycast = () => {};
  const clamp = (n: number) => Math.max(0, Math.min(rows, Math.floor(n)));
  sprite.count = clamp(count);

  return {
    object: sprite as unknown as THREE.Object3D,
    material: material as unknown as THREE.Material,
    colourArray: colours,
    setCount(n) {
      sprite.count = clamp(n);
    },
    setSize(size) {
      material.size = size;
    },
    setOpacity(opacity) {
      material.opacity = opacity;
    },
    markColoursDirty() {
      colAttr.needsUpdate = true;
    },
    colourVersion: () => colAttr.version,
    dispose() {
      // the quad geometry is three's shared Sprite geometry: leave it
      material.dispose();
    },
  };
}
