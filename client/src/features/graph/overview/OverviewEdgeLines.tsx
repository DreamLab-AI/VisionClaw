/**
 * One batch of screen-space overview edges (ADR-2181 D1/D2): an instanced
 * quad per edge, endpoints fetched on the GPU from the node position texture.
 * The instance attributes (endpoint indices, colour, width) are written only
 * when `data` or the width settings change, never per frame; per frame only a
 * handful of uniforms (resolution, near plane) are refreshed.
 *
 * Used for the asserted overview edges (OverviewLayer) and, dashed and tinted,
 * for the inferred edges (InferredEdges).
 */
import React, { useEffect, useMemo } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import * as THREE from 'three';
import {
  createOverviewEdgeGeometry,
  createOverviewEdgeUniforms,
  createTslOverviewEdgeMaterial,
} from '../../../rendering/materials/OverviewEdgeMaterial';
import { edgeWidthPx } from './edgeStyle';
import { useOverviewMaterial } from './useOverviewMaterial';
import { buildFault, isOverviewFaulted, overviewPositionsReady, setOverviewLive } from './overviewLive';
import { useRenderFaultWatch } from './useRenderFaultWatch';
import type { NodePositionTexture } from './nodePositionTexture';

export interface OverviewEdgeData {
  /** Edges to draw. */
  count: number;
  /** Render index of each edge's source / target (length ≥ count after `pick`). */
  src: ArrayLike<number>;
  tgt: ArrayLike<number>;
  /** Linear RGB, stride 3; omitted → the `tint`. */
  colors?: ArrayLike<number> | null;
  /** Edge weights; omitted → widthPx. */
  weights?: ArrayLike<number> | null;
  /** Optional subset of indices into src/tgt (ADR-2181 D4 truncation); omitted → 0..count−1. */
  pick?: ArrayLike<number> | null;
}

export interface OverviewEdgeStyle {
  widthPx: number;
  maxWidthPx: number;
  fadeNearPx: number;
  fadeFarPx: number;
  minOpacity: number;
  opacity: number;
  greyoutOpacity: number;
  dashed?: boolean;
  dashPx?: number;
  gapPx?: number;
  tint?: string | null;
}

interface Props {
  positions: NodePositionTexture;
  data: OverviewEdgeData | null;
  style: OverviewEdgeStyle;
  visible: boolean;
  renderOrder?: number;
  name?: string;
  /** Publish this batch as the live overview edges (the asserted batch only). */
  publishLive?: boolean;
}

export const OverviewEdgeLines: React.FC<Props> = ({ positions, data, style, visible, renderOrder = 1, name = 'overview-edges', publishLive = false }) => {
  const { size, gl } = useThree();
  const capacity = data ? Math.max(1, 2 ** Math.ceil(Math.log2(Math.max(1, data.count)))) : 1;
  const geometry = useMemo(() => createOverviewEdgeGeometry(capacity), [capacity]);
  useEffect(() => () => geometry.dispose(), [geometry]);

  const uniforms = useMemo(() => createOverviewEdgeUniforms({
    posTex: positions.texture,
    texWidth: positions.width,
    resolution: new THREE.Vector2(1, 1),
    dpr: 1,
    fadeNearPx: style.fadeNearPx,
    fadeFarPx: style.fadeFarPx,
    minOpacity: style.minOpacity,
    opacity: style.opacity,
    greyoutOpacity: style.greyoutOpacity,
    dashed: style.dashed ? 1 : 0,
    dashPx: style.dashPx ?? 6,
    gapPx: style.gapPx ?? 4,
    useTint: style.tint && !data?.colors ? 1 : 0,
    tint: new THREE.Color(style.tint ?? '#ffffff'),
    near: 0.1,
    // Uniform objects are created once per texture; settings are written into them below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }), [positions]);
  useEffect(() => positions.onReallocate(() => { uniforms.uPos.value = positions.texture; }), [positions, uniforms]);

  const faultLayer = publishLive ? 'edges' : 'inferred';
  const material = useOverviewMaterial(geometry, uniforms, createTslOverviewEdgeMaterial, buildFault(faultLayer));

  const mesh = useMemo(() => {
    if (!material) return null;
    const m = new THREE.Mesh(geometry, material);
    m.name = name;
    m.frustumCulled = false;
    m.renderOrder = renderOrder;
    m.raycast = () => {};
    return m;
  }, [geometry, material, name, renderOrder]);

  // ADR-2181 D4: a faulting draw hides this batch and is reported, never skipped silently.
  useRenderFaultWatch(mesh, faultLayer);
  // Tubes shrink to close zoom only while these edges really draw.
  const live = publishLive && mesh !== null && visible;
  useEffect(() => {
    if (!publishLive) return;
    setOverviewLive('edges', live);
    return () => setOverviewLive('edges', false);
  }, [publishLive, live]);

  // Instance attributes: only when the data or the width mapping changes.
  useEffect(() => {
    if (!data) { geometry.instanceCount = 0; return; }
    const aEdge = geometry.getAttribute('aEdge') as THREE.InstancedBufferAttribute;
    const aColor = geometry.getAttribute('aColor') as THREE.InstancedBufferAttribute;
    const aWidth = geometry.getAttribute('aWidth') as THREE.InstancedBufferAttribute;
    const e = aEdge.array as Float32Array;
    const c = aColor.array as Float32Array;
    const w = aWidth.array as Float32Array;
    const n = Math.min(data.count, aWidth.count);
    for (let k = 0; k < n; k++) {
      const i = data.pick ? data.pick[k] : k;
      e[k * 2] = data.src[i];
      e[k * 2 + 1] = data.tgt[i];
      if (data.colors) {
        c[k * 3] = data.colors[i * 3];
        c[k * 3 + 1] = data.colors[i * 3 + 1];
        c[k * 3 + 2] = data.colors[i * 3 + 2];
      } else {
        c[k * 3] = 1; c[k * 3 + 1] = 1; c[k * 3 + 2] = 1;
      }
      w[k] = edgeWidthPx(data.weights ? data.weights[i] : 1, style.widthPx, style.maxWidthPx);
    }
    aEdge.needsUpdate = true;
    aColor.needsUpdate = true;
    aWidth.needsUpdate = true;
    geometry.instanceCount = n;
  }, [data, geometry, style.widthPx, style.maxWidthPx]);

  useFrame(({ camera }) => {
    if (!mesh) return;
    mesh.visible = visible && geometry.instanceCount > 0 && overviewPositionsReady() && !isOverviewFaulted(faultLayer);
    if (!mesh.visible) return;
    uniforms.uResolution.value.set(size.width || 1, size.height || 1);
    uniforms.uDpr.value = gl.getPixelRatio();
    uniforms.uNear.value = (camera as THREE.PerspectiveCamera).near ?? 0.1;
    uniforms.uFadeNear.value = style.fadeNearPx;
    uniforms.uFadeFar.value = style.fadeFarPx;
    uniforms.uMinOpacity.value = style.minOpacity;
    uniforms.uOpacity.value = style.opacity;
    uniforms.uGreyOpacity.value = style.greyoutOpacity;
    uniforms.uDashed.value = style.dashed ? 1 : 0;
    uniforms.uUseTint.value = style.tint && !data?.colors ? 1 : 0;
  });

  return mesh ? <primitive object={mesh} /> : null;
};
