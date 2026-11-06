/**
 * Overview edge material (ADR-2181 D1/D2, ADR-2183 D3).
 *
 * Screen-space edge quads: each edge instance is a 4-vertex quad whose two
 * endpoints are fetched from the node position texture
 * (features/graph/overview/nodePositionTexture.ts) by render index, projected,
 * and extruded perpendicular to the on-screen segment by half the edge width
 * in CSS pixels — the technique of three's LineSegments2 / LineMaterial, with
 * per-edge width, on-screen-length alpha fade and grey-out added. Unlit,
 * alpha-blended, no depth write.
 *
 * One TSL material (MeshBasicNodeMaterial with a vertexNode, textureLoad),
 * which WebGPURenderer compiles to WGSL or, on its WebGL2 backend, to GLSL.
 * Per-instance data rides on InstancedBufferAttributes read through
 * `instancedBufferAttribute` (the memory cloud's sprites use the same pattern,
 * cloudSprites.ts). The on-screen length fade is edgeStyle.fadeAlphaWith, the
 * same function the CPU evaluates; the width map mirrors edgeStyle.edgeWidthPx.
 *
 * Instance attributes:
 *   aEdge   vec2  (srcIndex, tgtIndex) render indices
 *   aColor  vec3  linear RGB (edge-type colour)
 *   aWidth  float width in CSS px (edgeWidthPx of the weight)
 * Quad vertex `position`: x = side (−1 | +1), y = endpoint (0 = src, 1 = tgt).
 */
import * as THREE from 'three';
import { fadeAlphaWith, tslScalarOps } from '../../features/graph/overview/edgeStyle';

export interface OverviewEdgeUniformValues {
  posTex: THREE.Texture;
  texWidth: number;
  /** CSS px drawing size of the canvas. */
  resolution: THREE.Vector2;
  dpr: number;
  fadeNearPx: number;
  fadeFarPx: number;
  minOpacity: number;
  opacity: number;
  greyoutOpacity: number;
  /** 1 = dashed (inferred edges). */
  dashed: number;
  dashPx: number;
  gapPx: number;
  /** 1 = use uTint instead of aColor. */
  useTint: number;
  tint: THREE.Color;
  near: number;
}

/** Live uniforms: write `.value`, never replace the object. */
export interface OverviewEdgeUniforms {
  uPos: { value: THREE.Texture };
  uTexWidth: { value: number };
  uResolution: { value: THREE.Vector2 };
  uDpr: { value: number };
  uFadeNear: { value: number };
  uFadeFar: { value: number };
  uMinOpacity: { value: number };
  uOpacity: { value: number };
  uGreyOpacity: { value: number };
  uDashed: { value: number };
  uDash: { value: number };
  uGap: { value: number };
  uUseTint: { value: number };
  uTint: { value: THREE.Color };
  uNear: { value: number };
}

export function createOverviewEdgeUniforms(v: OverviewEdgeUniformValues): OverviewEdgeUniforms {
  return {
    uPos: { value: v.posTex },
    uTexWidth: { value: v.texWidth },
    uResolution: { value: v.resolution },
    uDpr: { value: v.dpr },
    uFadeNear: { value: v.fadeNearPx },
    uFadeFar: { value: v.fadeFarPx },
    uMinOpacity: { value: v.minOpacity },
    uOpacity: { value: v.opacity },
    uGreyOpacity: { value: v.greyoutOpacity },
    uDashed: { value: v.dashed },
    uDash: { value: v.dashPx },
    uGap: { value: v.gapPx },
    uUseTint: { value: v.useTint },
    uTint: { value: v.tint },
    uNear: { value: v.near },
  };
}

/** The 4-vertex quad shared by every edge instance, plus the instance attributes at `capacity`. */
export function createOverviewEdgeGeometry(capacity: number): THREE.InstancedBufferGeometry {
  const geo = new THREE.InstancedBufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(new Float32Array([
    -1, 0, 0, 1, 0, 0, 1, 1, 0, -1, 1, 0,
  ]), 3));
  geo.setIndex([0, 1, 2, 0, 2, 3]);
  const cap = Math.max(1, capacity);
  geo.setAttribute('aEdge', new THREE.InstancedBufferAttribute(new Float32Array(cap * 2), 2));
  geo.setAttribute('aColor', new THREE.InstancedBufferAttribute(new Float32Array(cap * 3), 3));
  geo.setAttribute('aWidth', new THREE.InstancedBufferAttribute(new Float32Array(cap), 1));
  geo.instanceCount = 0;
  return geo;
}

/**
 * Build the material. Async because three/webgpu and three/tsl are loaded on
 * demand (as everywhere else on this path). The uniform objects in `uniforms`
 * are wrapped with `uniform()` nodes that read them each frame, so callers
 * update `.value` and never rebuild.
 */
export async function createTslOverviewEdgeMaterial(
  uniforms: OverviewEdgeUniforms,
  geometry: THREE.InstancedBufferGeometry,
): Promise<THREE.Material> {
  const W = await import('three/webgpu');
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- TSL node builder types have no stable public typing
  const T = (await import('three/tsl')) as any;
  const {
    Fn, float, int, vec2, vec3, vec4, ivec2, uniform, instancedBufferAttribute,
    positionGeometry, cameraViewMatrix, cameraProjectionMatrix, varyingProperty, select, abs, clamp, mod, max, min, length, mix,
    Discard, If,
  } = T;

  const u = {
    pos: T.texture(uniforms.uPos.value).onRenderUpdate(() => uniforms.uPos.value),
    texWidth: uniform(uniforms.uTexWidth.value).onRenderUpdate(() => uniforms.uTexWidth.value),
    res: uniform(new THREE.Vector2()).onRenderUpdate(() => uniforms.uResolution.value),
    dpr: uniform(1).onRenderUpdate(() => uniforms.uDpr.value),
    near: uniform(0.1).onRenderUpdate(() => uniforms.uNear.value),
    fadeNear: uniform(50).onRenderUpdate(() => uniforms.uFadeNear.value),
    fadeFar: uniform(150).onRenderUpdate(() => uniforms.uFadeFar.value),
    minOpacity: uniform(0.25).onRenderUpdate(() => uniforms.uMinOpacity.value),
    opacity: uniform(1).onRenderUpdate(() => uniforms.uOpacity.value),
    grey: uniform(0.1).onRenderUpdate(() => uniforms.uGreyOpacity.value),
    dashed: uniform(0).onRenderUpdate(() => uniforms.uDashed.value),
    dash: uniform(1).onRenderUpdate(() => uniforms.uDash.value),
    gap: uniform(1).onRenderUpdate(() => uniforms.uGap.value),
    useTint: uniform(0).onRenderUpdate(() => uniforms.uUseTint.value),
    tint: uniform(new THREE.Color()).onRenderUpdate(() => uniforms.uTint.value),
  };

  const aEdge = instancedBufferAttribute(geometry.getAttribute('aEdge'));
  const aColor = instancedBufferAttribute(geometry.getAttribute('aColor'));
  const aWidth = instancedBufferAttribute(geometry.getAttribute('aWidth'));

  const fetchNode = (idx: unknown) => {
    const i = int(float(idx).add(0.5));
    const w = int(u.texWidth.add(0.5));
    return u.pos.load(ivec2(i.mod(w), i.div(w)));
  };

  const vAlpha = varyingProperty('float', 'vOvAlpha');
  const vSide = varyingProperty('float', 'vOvSide');
  const vHalf = varyingProperty('float', 'vOvHalf');
  const vAlong = varyingProperty('float', 'vOvAlong');
  const vColor = varyingProperty('vec3', 'vOvColor');

  const vertex = Fn(() => {
    const a = fetchNode(aEdge.x).toVar();
    const b = fetchNode(aEdge.y).toVar();
    const va = cameraViewMatrix.mul(vec4(a.xyz, 1)).toVar();
    const vb = cameraViewMatrix.mul(vec4(b.xyz, 1)).toVar();
    const zn = u.near.negate().mul(1.0001);
    const aBehind = va.z.greaterThan(zn);
    const bBehind = vb.z.greaterThan(zn);
    const out = vec4(2, 2, 2, 1).toVar();
    vAlpha.assign(0);
    vHalf.assign(1);
    vAlong.assign(0);
    If(aBehind.and(bBehind).not(), () => {
      If(aBehind, () => { va.xyz.assign(mix(va.xyz, vb.xyz, zn.sub(va.z).div(vb.z.sub(va.z)))); });
      If(bBehind, () => { vb.xyz.assign(mix(vb.xyz, va.xyz, zn.sub(vb.z).div(va.z.sub(vb.z)))); });
      const ca = cameraProjectionMatrix.mul(va).toVar();
      const cb = cameraProjectionMatrix.mul(vb).toVar();
      const half = u.res.mul(0.5);
      const sa = ca.xy.div(ca.w).mul(half);
      const sb = cb.xy.div(cb.w).mul(half);
      const d = sb.sub(sa).toVar();
      const lenPx = length(d).toVar();
      const dir = select(lenPx.greaterThan(1e-4), d.div(lenPx), vec2(1, 0));
      const nrm = vec2(dir.y.negate(), dir.x);
      const greyed = a.w.lessThan(0.5).or(b.w.lessThan(0.5));
      const emph = greyed.not().and(a.w.greaterThan(1.5).or(b.w.greaterThan(1.5)));
      const width = max(aWidth.add(select(emph, float(1), float(0))), float(1).div(max(u.dpr, 1)));
      const halfW = width.mul(0.5).add(0.5);
      const c = select(positionGeometry.y.lessThan(0.5), ca, cb).toVar();
      c.xy.addAssign(nrm.mul(positionGeometry.x).mul(halfW).div(half).mul(c.w));
      out.assign(c);
      const fade = fadeAlphaWith(tslScalarOps(T), lenPx, u.fadeNear, u.fadeFar, u.minOpacity);
      const alpha = fade.mul(u.opacity).toVar();
      If(greyed, () => { alpha.assign(min(alpha, u.grey)); });
      If(emph, () => { alpha.assign(1); });
      vAlpha.assign(alpha);
      vHalf.assign(halfW);
      vAlong.assign(positionGeometry.y.mul(lenPx));
    });
    vSide.assign(positionGeometry.x);
    vColor.assign(aColor);
    return out;
  })();

  // DoubleSide: the extruded quad's winding flips with the on-screen direction.
  const material = new W.MeshBasicNodeMaterial({ transparent: true, depthWrite: false, depthTest: true, side: THREE.DoubleSide });
  material.name = 'OverviewEdgeNodeMaterial';
  (material as unknown as { vertexNode: unknown }).vertexNode = vertex;
  const fragment = Fn(() => {
    If(vAlpha.lessThanEqual(0), () => { Discard(); });
    If(u.dashed.greaterThan(0.5).and(mod(vAlong, u.dash.add(u.gap)).greaterThan(u.dash)), () => { Discard(); });
    const px = abs(vSide).mul(vHalf);
    const feather = clamp(vHalf.sub(px), 0, 1);
    const col = select(u.useTint.greaterThan(0.5), vec3(u.tint), vColor);
    return vec4(col, vAlpha.mul(feather));
  })();
  // colorNode's alpha is the edge alpha; opacityNode stays materialOpacity (1).
  (material as unknown as { colorNode: unknown }).colorNode = fragment;
  return material as unknown as THREE.Material;
}
