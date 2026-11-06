/**
 * Overview node sprites (ADR-2182 D1) and the emphasis halo (D4).
 *
 * One screen-sized sprite per knowledge node, mirroring the knowledge gem
 * mesh instance for instance (GemNodesHandle.getOverviewSource): the same
 * render index and the same final colour (the encoding plus GemNodes'
 * overlays — cycles, SSSP, timeline, heat). Size is spriteSizePx × the
 * encoded size (nodeEncoding.sizes). Centres come from the node position
 * texture on the GPU; the attributes are rewritten only when the colours, the
 * encoding or the sprite size change. The gem LOD (GemNodes) draws gems over
 * the sprites at close zoom.
 *
 * The halo is a second, small batch holding only the emphasised nodes, drawn
 * additively and put on the bloom layer, so selective bloom (bloomScope
 * 'emphasis') blooms exactly the emphasised set.
 */
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import * as THREE from 'three';
import {
  createOverviewNodeGeometry,
  createOverviewNodeUniforms,
  createGlslOverviewNodeMaterial,
  createTslOverviewNodeMaterial,
  spriteSizePx,
  type OverviewNodeUniforms,
} from '../../../rendering/materials/OverviewNodeMaterial';
import { enableBloom } from '../../../rendering/bloomLayer';
import { nodeEncoding } from '../state/nodeEncoding';
import { highlightMask, highlightLevel, HIGHLIGHT_LEVEL_EMPHASISED } from '../state/highlightMask';
import { EMPHASIS_SCALE, backgroundFromSetting } from './highlightVisual';
import { useOverviewMaterial } from './useOverviewMaterial';
import { setOverviewLive } from './overviewLive';
import { useRenderFaultWatch } from './useRenderFaultWatch';
import type { NodePositionTexture } from './nodePositionTexture';
import type { GemNodesHandle } from '../components/GemNodes';

export interface OverviewNodeStyle {
  spriteSizePx: number;
  nodeSize: number;
  greyoutOpacity: number;
  background: string;
  /** Draw the emphasis halo (false under bloomScope 'off'). */
  halo: boolean;
}

interface Props {
  positions: NodePositionTexture;
  sourceRef: React.RefObject<GemNodesHandle | null>;
  style: OverviewNodeStyle;
}

/**
 * The source colours already carry the grey-out mix and the emphasis
 * brightening (GemNodes applies highlightVisualFor in its colour pass), so the
 * sprite shader must not apply them again: grey mix 0 keeps a greyed colour
 * as it is (it is already grey, so the shader's luma step is a no-op) and
 * brighten 1 leaves emphasised colours alone. The shader still owns what only
 * it can do: the greyed alpha and the emphasised size.
 */
function writeUniforms(u: OverviewNodeUniforms, w: number, h: number, style: OverviewNodeStyle): void {
  u.uResolution.value.set(w || 1, h || 1);
  u.uGreyMix.value = 0;
  u.uGreyOpacity.value = style.greyoutOpacity;
  u.uBg.value.setRGB(_bg.r, _bg.g, _bg.b);
  u.uEmphScale.value = EMPHASIS_SCALE;
  u.uEmphBrighten.value = 1;
}

const nextPow2 = (n: number) => 2 ** Math.ceil(Math.log2(Math.max(1, n)));
const _bg = { r: 0, g: 0, b: 0 };

function useSpriteMesh(
  positions: NodePositionTexture, capacity: number, halo: boolean, renderOrder: number,
): { mesh: THREE.Mesh | null; geometry: THREE.InstancedBufferGeometry; uniforms: OverviewNodeUniforms } {
  const geometry = useMemo(() => createOverviewNodeGeometry(capacity), [capacity]);
  useEffect(() => () => geometry.dispose(), [geometry]);
  const uniforms = useMemo(() => createOverviewNodeUniforms(positions.texture, positions.width, halo), [positions, halo]);
  useEffect(() => positions.onReallocate(() => { uniforms.uPos.value = positions.texture; }), [positions, uniforms]);
  const material = useOverviewMaterial(geometry, uniforms, createGlslOverviewNodeMaterial, createTslOverviewNodeMaterial);
  const mesh = useMemo(() => {
    if (!material) return null;
    const m = new THREE.Mesh(geometry, material);
    m.name = halo ? 'overview-node-halo' : 'overview-nodes';
    m.frustumCulled = false;
    m.renderOrder = renderOrder;
    m.raycast = () => {}; // picking stays on the gem mesh (GemNodes' full-population raycast)
    if (halo) enableBloom(m);
    return m;
  }, [geometry, material, halo, renderOrder]);
  return { mesh, geometry, uniforms };
}

export const OverviewNodes: React.FC<Props> = ({ positions, sourceRef, style }) => {
  const { size } = useThree();
  const [capacity, setCapacity] = useState(4096);
  const [haloCapacity, setHaloCapacity] = useState(64);
  const sprites = useSpriteMesh(positions, capacity, false, 2);
  const halo = useSpriteMesh(positions, haloCapacity, true, 3);
  // Gems hide beyond the LOD distance only while these sprites really draw.
  const spritesLive = sprites.mesh !== null;
  useRenderFaultWatch(sprites.mesh, 'nodes');
  useRenderFaultWatch(halo.mesh, 'nodes');
  useEffect(() => {
    setOverviewLive('nodes', spritesLive);
    return () => setOverviewLive('nodes', false);
  }, [spritesLive]);
  const last = useRef({ colorVersion: -1, encVersion: -1, count: -1, spritePx: -1, nodeSize: -1, geo: null as unknown, maskVersion: -1, haloGeo: null as unknown });

  useFrame(() => {
    const src = sourceRef.current?.getOverviewSource() ?? null;
    const count = src ? src.count : 0;
    if (count > capacity) { setCapacity(nextPow2(count)); return; }
    const l = last.current;
    const geo = sprites.geometry;
    if (src && (src.colorVersion !== l.colorVersion || nodeEncoding.version !== l.encVersion || count !== l.count ||
      style.spriteSizePx !== l.spritePx || style.nodeSize !== l.nodeSize || geo !== l.geo)) {
      l.colorVersion = src.colorVersion; l.encVersion = nodeEncoding.version; l.count = count;
      l.spritePx = style.spriteSizePx; l.nodeSize = style.nodeSize; l.geo = geo;
      const aIndex = (geo.getAttribute('aIndex') as THREE.InstancedBufferAttribute);
      const aColor = (geo.getAttribute('aColor') as THREE.InstancedBufferAttribute);
      const aSize = (geo.getAttribute('aSize') as THREE.InstancedBufferAttribute);
      const ia = aIndex.array as Float32Array, ca = aColor.array as Float32Array, sa = aSize.array as Float32Array;
      const sizes = nodeEncoding.sizes;
      for (let i = 0; i < count; i++) {
        const ri = src.renderIndex[i];
        ia[i] = ri;
        ca[i * 3] = src.colors[i * 3]; ca[i * 3 + 1] = src.colors[i * 3 + 1]; ca[i * 3 + 2] = src.colors[i * 3 + 2];
        sa[i] = spriteSizePx(ri < sizes.length ? sizes[ri] : 0, style.nodeSize, style.spriteSizePx);
      }
      aIndex.needsUpdate = true; aColor.needsUpdate = true; aSize.needsUpdate = true;
      geo.instanceCount = count;
      l.maskVersion = -1; // halo follows the colours
    }
    if (!src) geo.instanceCount = 0;

    // Halo: the emphasised subset, rebuilt on mask or colour change.
    const hgeo = halo.geometry;
    if (src && style.halo && (highlightMask.version !== l.maskVersion || hgeo !== l.haloGeo)) {
      l.maskVersion = highlightMask.version; l.haloGeo = hgeo;
      const mask = highlightMask.getMask();
      let n = 0;
      if (highlightMask.presentFlags !== 0) {
        for (let i = 0; i < count; i++) {
          const ri = src.renderIndex[i];
          if (ri < mask.length && highlightLevel(mask[ri]) === HIGHLIGHT_LEVEL_EMPHASISED) n++;
        }
      }
      if (n > haloCapacity) { setHaloCapacity(nextPow2(n)); l.maskVersion = -1; return; }
      const sa = geo.getAttribute('aSize').array as Float32Array;
      const ca = geo.getAttribute('aColor').array as Float32Array;
      const hi = hgeo.getAttribute('aIndex').array as Float32Array;
      const hc = hgeo.getAttribute('aColor').array as Float32Array;
      const hs = hgeo.getAttribute('aSize').array as Float32Array;
      let k = 0;
      if (n > 0) {
        for (let i = 0; i < count && k < n; i++) {
          const ri = src.renderIndex[i];
          if (!(ri < mask.length && highlightLevel(mask[ri]) === HIGHLIGHT_LEVEL_EMPHASISED)) continue;
          hi[k] = ri; hs[k] = sa[i];
          hc[k * 3] = ca[i * 3]; hc[k * 3 + 1] = ca[i * 3 + 1]; hc[k * 3 + 2] = ca[i * 3 + 2];
          k++;
        }
      }
      hgeo.getAttribute('aIndex').needsUpdate = true;
      hgeo.getAttribute('aColor').needsUpdate = true;
      hgeo.getAttribute('aSize').needsUpdate = true;
      hgeo.instanceCount = k;
    }
    if (!style.halo) hgeo.instanceCount = 0;

    backgroundFromSetting(style.background, _bg);
    writeUniforms(sprites.uniforms, size.width, size.height, style);
    writeUniforms(halo.uniforms, size.width, size.height, style);
  }, -0.3);

  return (
    <>
      {sprites.mesh && <primitive object={sprites.mesh} />}
      {halo.mesh && <primitive object={halo.mesh} />}
    </>
  );
};
