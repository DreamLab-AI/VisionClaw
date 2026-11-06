/**
 * Both dialects of the overview materials build (ADR-2181/2182). The GLSL is
 * a ShaderMaterial (compiled by the browser, checked here for shape); the TSL
 * material is built for real from three/webgpu + three/tsl, which catches a
 * wrong TSL export or method at test time. Neither runs on a GPU here.
 */
import { describe, it, expect } from 'vitest';
import * as THREE from 'three';
import {
  createOverviewEdgeGeometry, createOverviewEdgeUniforms, createGlslOverviewEdgeMaterial, createTslOverviewEdgeMaterial,
  OVERVIEW_EDGE_VERTEX_GLSL,
} from '../OverviewEdgeMaterial';
import {
  createOverviewNodeGeometry, createOverviewNodeUniforms, createGlslOverviewNodeMaterial, createTslOverviewNodeMaterial,
  OVERVIEW_NODE_VERTEX_GLSL, spriteSizePx, SPRITE_MIN_PX, SPRITE_MAX_PX,
} from '../OverviewNodeMaterial';

const tex = new THREE.DataTexture(new Float32Array(16), 4, 1, THREE.RGBAFormat, THREE.FloatType);
const edgeUniforms = () => createOverviewEdgeUniforms({
  posTex: tex, texWidth: 4, resolution: new THREE.Vector2(800, 600), dpr: 1, fadeNearPx: 50, fadeFarPx: 150,
  minOpacity: 0.25, opacity: 0.6, greyoutOpacity: 0.1, dashed: 0, dashPx: 6, gapPx: 4, useTint: 0,
  tint: new THREE.Color(1, 1, 1), near: 0.1,
});

describe('overview edge material', () => {
  it('geometry: one 4-vertex quad, instance attributes at capacity, nothing drawn yet', () => {
    const g = createOverviewEdgeGeometry(8);
    expect(g.getAttribute('position').count).toBe(4);
    expect(g.getAttribute('aEdge').count).toBe(8);
    expect(g.getAttribute('aColor').itemSize).toBe(3);
    expect(g.instanceCount).toBe(0);
  });
  it('GLSL: GLSL3 ShaderMaterial, unlit, blended, no depth write; never LineSegments/Points', () => {
    const m = createGlslOverviewEdgeMaterial(edgeUniforms());
    expect(m.glslVersion).toBe(THREE.GLSL3);
    expect(m.transparent).toBe(true);
    expect(m.depthWrite).toBe(false);
    expect(OVERVIEW_EDGE_VERTEX_GLSL).toMatch(/texelFetch\(uPos/);
    expect(OVERVIEW_EDGE_VERTEX_GLSL).not.toMatch(/\$\{/);
  });
  it('TSL: builds a node material with a clip-space vertexNode and a colorNode', async () => {
    const g = createOverviewEdgeGeometry(8);
    const m = await createTslOverviewEdgeMaterial(edgeUniforms(), g) as THREE.Material & { isNodeMaterial?: boolean; vertexNode?: unknown; colorNode?: unknown };
    expect(m.isNodeMaterial).toBe(true);
    expect(m.vertexNode).toBeTruthy();
    expect(m.colorNode).toBeTruthy();
    expect(m.transparent).toBe(true);
  });
});

describe('overview node material', () => {
  it('GLSL sprite and halo materials', () => {
    const s = createGlslOverviewNodeMaterial(createOverviewNodeUniforms(tex, 4, false));
    const h = createGlslOverviewNodeMaterial(createOverviewNodeUniforms(tex, 4, true));
    expect(s.depthWrite).toBe(true);
    expect(h.blending).toBe(THREE.AdditiveBlending);
    expect(OVERVIEW_NODE_VERTEX_GLSL).toMatch(/size \*= 2\.50;/);
  });
  it('TSL sprite and halo materials build', async () => {
    for (const halo of [false, true]) {
      const g = createOverviewNodeGeometry(4);
      const m = await createTslOverviewNodeMaterial(createOverviewNodeUniforms(tex, 4, halo), g) as THREE.Material & { vertexNode?: unknown };
      expect(m.vertexNode).toBeTruthy();
    }
  });
  it('sprite size: spriteSizePx for the smallest node, larger nodes larger, clamped', () => {
    expect(spriteSizePx(2.5 * 0.4, 0.4, 4)).toBe(4);
    expect(spriteSizePx(5 * 0.4, 0.4, 4)).toBe(8);
    expect(spriteSizePx(1e6, 1, 4)).toBe(SPRITE_MAX_PX);
    expect(spriteSizePx(0, 1, 1)).toBe(SPRITE_MIN_PX);
  });
});
