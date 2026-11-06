/**
 * Overview node sprite material (ADR-2182 D1, ADR-2183 D3).
 *
 * Camera-facing instanced quads sized in CSS pixels: the centre is fetched
 * from the node position texture by render index (nodePositionTexture.ts),
 * projected, and the corner offset added in clip space (the PointsNodeMaterial
 * technique, done by hand). Flat
 * soft-edged disc with a thin darker rim for contrast on dark backgrounds.
 * Unlit, alpha-blended, depth-tested.
 *
 * The highlight level arrives in the texture's w: greyed nodes are
 * desaturated and mixed `uGreyMix` toward the background at `uGreyOpacity`;
 * emphasised nodes are drawn `uEmphScale` larger and brighter. In halo mode
 * (the emphasis overlay that feeds selective bloom) only emphasised nodes
 * draw, as a larger soft glow.
 *
 * Instance attributes: aIndex float (render index), aColor vec3 (linear RGB),
 * aSize float (CSS px before emphasis). Quad `position`: x, y ∈ {−1, +1}.
 */
import * as THREE from 'three';

export interface OverviewNodeUniforms {
  uPos: { value: THREE.Texture };
  uTexWidth: { value: number };
  uResolution: { value: THREE.Vector2 };
  uGreyMix: { value: number };
  uGreyOpacity: { value: number };
  uBg: { value: THREE.Color };
  uEmphScale: { value: number };
  uEmphBrighten: { value: number };
  /** 1 = halo overlay: emphasised nodes only, 2.5× size, soft glow. */
  uHalo: { value: number };
}

export function createOverviewNodeUniforms(posTex: THREE.Texture, texWidth: number, halo: boolean): OverviewNodeUniforms {
  return {
    uPos: { value: posTex },
    uTexWidth: { value: texWidth },
    uResolution: { value: new THREE.Vector2(1, 1) },
    uGreyMix: { value: 0.65 },
    uGreyOpacity: { value: 0.1 },
    uBg: { value: new THREE.Color(0, 0, 0) },
    uEmphScale: { value: 1.35 },
    uEmphBrighten: { value: 1.25 },
    uHalo: { value: halo ? 1 : 0 },
  };
}

export function createOverviewNodeGeometry(capacity: number): THREE.InstancedBufferGeometry {
  const geo = new THREE.InstancedBufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(new Float32Array([
    -1, -1, 0, 1, -1, 0, 1, 1, 0, -1, 1, 0,
  ]), 3));
  geo.setIndex([0, 1, 2, 0, 2, 3]);
  const cap = Math.max(1, capacity);
  geo.setAttribute('aIndex', new THREE.InstancedBufferAttribute(new Float32Array(cap), 1));
  geo.setAttribute('aColor', new THREE.InstancedBufferAttribute(new Float32Array(cap * 3), 3));
  geo.setAttribute('aSize', new THREE.InstancedBufferAttribute(new Float32Array(cap), 1));
  geo.instanceCount = 0;
  return geo;
}

/** Halo size multiple (the bloom source is wider than the sprite). */
export const HALO_SCALE = 2.5;

/** The sprite material (TSL; WebGPURenderer compiles it for either backend). */
export async function createTslOverviewNodeMaterial(
  uniforms: OverviewNodeUniforms,
  geometry: THREE.InstancedBufferGeometry,
): Promise<THREE.Material> {
  const W = await import('three/webgpu');
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- TSL node builder types have no stable public typing
  const T = (await import('three/tsl')) as any;
  const {
    Fn, float, int, vec3, vec4, ivec2, uniform, instancedBufferAttribute, positionGeometry,
    cameraViewMatrix, cameraProjectionMatrix, varyingProperty, select, clamp, mix, dot, length, smoothstep,
    Discard, If,
  } = T;
  const halo = uniforms.uHalo.value > 0.5;
  const u = {
    pos: T.texture(uniforms.uPos.value).onRenderUpdate(() => uniforms.uPos.value),
    texWidth: uniform(uniforms.uTexWidth.value).onRenderUpdate(() => uniforms.uTexWidth.value),
    res: uniform(new THREE.Vector2(1, 1)).onRenderUpdate(() => uniforms.uResolution.value),
    greyMix: uniform(0.65).onRenderUpdate(() => uniforms.uGreyMix.value),
    greyOpacity: uniform(0.1).onRenderUpdate(() => uniforms.uGreyOpacity.value),
    bg: uniform(new THREE.Color()).onRenderUpdate(() => uniforms.uBg.value),
    emphScale: uniform(1.35).onRenderUpdate(() => uniforms.uEmphScale.value),
    emphBrighten: uniform(1.25).onRenderUpdate(() => uniforms.uEmphBrighten.value),
  };
  const aIndex = instancedBufferAttribute(geometry.getAttribute('aIndex'));
  const aColor = instancedBufferAttribute(geometry.getAttribute('aColor'));
  const aSize = instancedBufferAttribute(geometry.getAttribute('aSize'));
  const vColor = varyingProperty('vec3', 'vOnColor');
  const vAlpha = varyingProperty('float', 'vOnAlpha');
  const vCorner = varyingProperty('vec2', 'vOnCorner');

  const vertex = Fn(() => {
    const i = int(aIndex.add(0.5));
    const w = int(u.texWidth.add(0.5));
    const p = u.pos.load(ivec2(i.mod(w), i.div(w))).toVar();
    const greyed = p.w.lessThan(0.5);
    const emph = p.w.greaterThan(1.5);
    const clip = cameraProjectionMatrix.mul(cameraViewMatrix).mul(vec4(p.xyz, 1)).toVar();
    const size = aSize.mul(select(emph, u.emphScale, float(1))).toVar();
    const col = vec3(aColor).toVar();
    const alpha = float(1).toVar();
    If(greyed, () => {
      const l = dot(col, vec3(0.2126, 0.7152, 0.0722));
      col.assign(mix(vec3(l), vec3(u.bg), clamp(u.greyMix, 0, 1)));
      alpha.assign(u.greyOpacity);
    }).ElseIf(emph, () => {
      col.mulAssign(u.emphBrighten);
    });
    if (halo) {
      size.mulAssign(HALO_SCALE);
      alpha.assign(select(emph, float(0.6), float(0)));
    }
    clip.xy.addAssign(positionGeometry.xy.mul(size.mul(0.5)).div(u.res.mul(0.5)).mul(clip.w));
    vColor.assign(col);
    vAlpha.assign(alpha);
    vCorner.assign(positionGeometry.xy);
    return select(alpha.greaterThan(0), clip, vec4(2, 2, 2, 1));
  })();

  const fragment = Fn(() => {
    If(vAlpha.lessThanEqual(0), () => { Discard(); });
    const r = length(vCorner);
    If(r.greaterThan(1), () => { Discard(); });
    if (halo) {
      return vec4(vColor, vAlpha.mul(float(1).sub(smoothstep(0, 1, r))));
    }
    const a = vAlpha.mul(float(1).sub(smoothstep(0.85, 1, r)));
    const col = mix(vColor, vColor.mul(0.45), smoothstep(0.62, 0.78, r));
    return vec4(col, a);
  })();

  const material = new W.MeshBasicNodeMaterial({
    transparent: true,
    depthWrite: !halo,
    depthTest: true,
    blending: halo ? THREE.AdditiveBlending : THREE.NormalBlending,
  });
  material.name = halo ? 'OverviewNodeHaloNodeMaterial' : 'OverviewNodeNodeMaterial';
  (material as unknown as { vertexNode: unknown }).vertexNode = vertex;
  (material as unknown as { colorNode: unknown }).colorNode = fragment;
  return material as unknown as THREE.Material;
}

/**
 * Sprite size in CSS px from the encoded size (ADR-2182 D1, "spriteSizePx ×
 * nodeEncoding size"). The encoder's size is computeNodeScale × nodeSize; the
 * smallest knowledge node is computeNodeScale ≈ SPRITE_SIZE_REF, so dividing
 * by nodeSize × SPRITE_SIZE_REF makes the smallest node exactly spriteSizePx
 * and larger nodes proportionally larger, clamped to [MIN, MAX] px.
 */
export const SPRITE_SIZE_REF = 2.5;
export const SPRITE_MIN_PX = 2;
export const SPRITE_MAX_PX = 24;

export function spriteSizePx(encodedSize: number, nodeSize: number, spritePx: number): number {
  const base = spritePx > 0 ? spritePx : 4;
  const gain = nodeSize > 0 ? nodeSize : 1;
  const rel = encodedSize > 0 ? encodedSize / (gain * SPRITE_SIZE_REF) : 1;
  const px = base * Math.max(1, rel);
  return px < SPRITE_MIN_PX ? SPRITE_MIN_PX : px > SPRITE_MAX_PX ? SPRITE_MAX_PX : px;
}
