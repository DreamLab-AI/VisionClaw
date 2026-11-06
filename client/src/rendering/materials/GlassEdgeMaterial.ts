import * as THREE from 'three';
import { isWebGPURenderer } from '../rendererFactory';
import { createLogger } from '../../utils/loggerConfig';

const logger = createLogger('GlassEdgeMaterial');

// ---------------------------------------------------------------------------
// TSL node-augmented material interface
// ---------------------------------------------------------------------------

/** Runtime-augmented material properties added by Three.js TSL (node-based shading). */
interface TSLNodeProperties {
  emissiveNode: unknown;
  opacityNode: unknown;
  needsUpdate: boolean;
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface GlassEdgeMaterialResult {
  material: THREE.Material;
  uniforms: { time: { value: number }; flowSpeed: { value: number } };
  ready: Promise<void>;
}

// ---------------------------------------------------------------------------
// Shared glass edge material — works on both renderers.
// Edges stay thin and subtle; depth-write off so they don't z-fight with nodes.
// ---------------------------------------------------------------------------

export function createGlassEdgeMaterial(baseColor?: string | THREE.Color): GlassEdgeMaterialResult {
  const uniforms = { time: { value: 0 }, flowSpeed: { value: 0.5 } };

  // Base material.color MULTIPLIES the per-edge instanceColor on the GPU. Any
  // non-neutral tint here muddies the semantic edge-type palette (the old
  // #46534f greenish base washed everything toward grey). The base is forced
  // neutral white (1,1,1) so per-edge type colours render TRUE; the opacity
  // slider still has full authority via the materialOpacity TSL node below.
  // `baseColor` is retained only as the emissive/sheen/specular hue accent.
  const resolvedColor = baseColor
    ? (baseColor instanceof THREE.Color ? baseColor : new THREE.Color(baseColor))
    : new THREE.Color(0.35, 0.55, 0.85);
  const neutralBase = new THREE.Color(1, 1, 1);

  const material = new THREE.MeshPhysicalMaterial({
    color: neutralBase,
    ior: 1.5,
    // Transmission masks alpha — keep it ~0 on edges (both renderers) so the
    // opacity/alpha channel has full authority over edge visibility.
    transmission: 0,
    thickness: 0,
    roughness: 0.05,
    metalness: 0.0,
    transparent: true,
    opacity: isWebGPURenderer ? 0.6 : 0.4,
    side: THREE.DoubleSide,
    // 32k transparent tubes overdraw heavily. depthWrite=true causes stacked
    // edges to z-reject and saturate to white; with depthWrite off they blend.
    depthWrite: false,
    // Normal alpha blend — NOT additive (additive blows transparent stacks to white).
    blending: THREE.NormalBlending,
    polygonOffset: true,
    polygonOffsetFactor: 1, // Push edges slightly behind nodes at connection points
    polygonOffsetUnits: 1,
    // Neutral grey emissive (NOT the resolvedColor hue) so the glow follows
    // each tube's per-edge instanceColor intensity rather than re-tinting all
    // edges toward one fixed accent. Toned down so tubes don't self-illuminate
    // to white; the dominant readable signal is the coloured tube.
    emissive: neutralBase.clone().multiplyScalar(isWebGPURenderer ? 0.15 : 0.1),
    emissiveIntensity: isWebGPURenderer ? 0.2 : 0.15,
    // Iridescence/sheen/specular are standard MeshPhysicalMaterial PBR features
    // supported on BOTH backends — they were previously gated to WebGPU for no
    // hard reason, leaving the WebGL edges flat. Applied uniformly now so the two
    // renderers present the same glassy edge response (WebGL parity, task #49).
    iridescence: 0.2,
    iridescenceIOR: 1.3,
    // three r187 deprecated iridescenceThicknessRange in favour of Minimum/Maximum.
    iridescenceThicknessMinimum: 100,
    iridescenceThicknessMaximum: 250,
    sheen: 0.3,
    sheenRoughness: 0.1,
    // Neutral sheen/specular — let the instanceColor define edge hue.
    sheenColor: neutralBase.clone().multiplyScalar(0.7),
    specularIntensity: 0.8,
    specularColor: neutralBase.clone(),
  });

  // WebGL parity (task #49): inject the EXACT same Fresnel emissive + opacity
  // maths the WebGPU TSL nodes apply (below), via onBeforeCompile GLSL. Without
  // this the WebGL edge was flat PBR + a per-frame emissiveIntensity pulse —
  // no grazing-angle rim glow and no Fresnel opacity gain, the visible
  // divergence from the WebGPU edge. Applied synchronously here (not a
  // post-mount effect) so every recreated edge material (capacity growth) gets
  // it. No-op on WebGPU (the TSL `ready` block owns that backend).
  applyGlslEdgeFresnel(material);

  // TSL ENABLED (r183+) with PBR fallback — Fresnel emissive and opacity nodes
  // are applied asynchronously on WebGPU; standard PBR + per-frame emissive
  // modulation in GlassEdges useFrame remains the active fallback path.
  const ready = (async () => {
    if (!isWebGPURenderer) return;
    try {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any -- Three.js TSL module exports are complex node builder types with no stable public API
      const { float, normalize, positionView, normalView, dot, saturate, pow, oneMinus, materialOpacity, materialColor } = await import('three/tsl') as any;
      const viewDir = normalize(positionView.negate());
      const fresnel = pow(oneMinus(saturate(dot(normalView, viewDir))), float(3.0));

      // `materialOpacity` is a LIVE reference node to material.opacity — when
      // GlassEdges sets `mat.opacity = settings.opacity` per update, this node
      // re-reads it every frame (no rebuild, no needsUpdate needed for the value).
      // The slider therefore has full authority: at opacity 0 the product is 0
      // (edges fully invisible); the fresnel rim is a MODULATION scaled by the
      // slider (0.55 base + up to 0.45 fresnel boost), never an additive floor.
      const sliderOpacity = materialOpacity;
      const fresnelGain = float(0.55).add(fresnel.mul(0.45));
      const opacityNode = sliderOpacity.mul(fresnelGain);

      // Emissive in the edge's own hue (live materialColor reference) at a low
      // fresnel-driven intensity — a subtle glass rim, not a white-out.
      const emissiveNode = materialColor.mul(fresnel).mul(float(0.25));

      const augmented = material as unknown as TSLNodeProperties;
      augmented.emissiveNode = emissiveNode;
      augmented.opacityNode = opacityNode;
      // No colorNode: let material.color (driven by graphTypeVisuals.<mode>.edgeColor via GlassEdges colorOverride, ADR-2189)
      // flow through the standard PBR colour path instead of forcing white.
      augmented.needsUpdate = true;
      logger.info('TSL nodes enabled (r183+) — opacityNode bound to live materialOpacity');
    } catch (err) {
      logger.warn('TSL upgrade failed, using PBR fallback:', err);
    }
  })();

  return { material, uniforms, ready };
}

/** Edge material augmented with the idempotency guard for the GLSL injection. */
interface GlslEdgeMaterial extends THREE.MeshPhysicalMaterial {
  userData: { glslEdgeFresnelApplied?: boolean } & Record<string, unknown>;
}

/**
 * WebGL counterpart to the WebGPU TSL edge nodes (createGlassEdgeMaterial's
 * `ready` block). WebGL has no node graph, so this injects the equivalent GLSL
 * into the MeshPhysicalMaterial shader via onBeforeCompile, reproducing the TSL
 * maths exactly:
 *   - emissiveNode  = materialColor * fresnel * 0.25   → a grazing-angle rim glow
 *   - opacityNode   = materialOpacity * (0.55 + fresnel*0.45)
 *
 * The injection lives at <emissivemap_fragment> (after <normal_fragment_begin>,
 * so `normal` is defined; `vViewPosition`, `diffuse` and `diffuseColor` are all
 * in scope). It REPLACES totalEmissiveRadiance — matching WebGPU, where setting
 * emissiveNode overrides the standard emissive entirely (the per-frame
 * emissiveIntensity pulse in GlassEdges useFrame is therefore inert on both
 * backends; the WebGPU edge does not pulse, so neither does this). `diffuse`
 * is the material.color uniform (forced white once per-edge instanceColor is
 * active), mirroring the WebGPU `materialColor` node — the per-edge hue stays in
 * the diffuse/instanceColor path, not the emissive rim. GLSL ES 1.00 native
 * (no GLSL3 forcing — that breaks Three's built-in <opaque_fragment>).
 * Idempotent and a no-op on WebGPU.
 */
export function applyGlslEdgeFresnel(material: THREE.MeshPhysicalMaterial): boolean {
  if (isWebGPURenderer) return false;

  const mat = material as GlslEdgeMaterial;
  if (mat.userData.glslEdgeFresnelApplied) return true;

  const prevOnBeforeCompile = mat.onBeforeCompile;
  mat.onBeforeCompile = (shader, renderer) => {
    if (prevOnBeforeCompile) prevOnBeforeCompile.call(mat, shader, renderer);

    shader.fragmentShader = shader.fragmentShader.replace(
      '#include <emissivemap_fragment>',
      `#include <emissivemap_fragment>
{
  vec3 _evd = normalize(vViewPosition);
  float _endv = clamp(dot(normalize(normal), _evd), 0.0, 1.0);
  float _efres = pow(1.0 - _endv, 3.0);
  totalEmissiveRadiance = diffuse * _efres * 0.25;
  diffuseColor.a *= (0.55 + _efres * 0.45);
}`,
    );
  };

  mat.userData.glslEdgeFresnelApplied = true;
  mat.needsUpdate = true;
  logger.info('GLSL Fresnel emissive + opacity injected (WebGL edge parity)');
  return true;
}

/**
 * Creates a unit-length cylinder geometry for glass tube edges.
 * Stretch to actual edge length via instance/object matrix.
 */
export function createGlassEdgeGeometry(radius: number = 0.03): THREE.CylinderGeometry {
  // 4 radial segments (square prism) instead of 8: edges are hairline-thin at
  // node scale, so the rounder tube was invisible cost across ~26k instances.
  // Open-ended (no caps) — endpoints sit inside the node gems, never seen.
  return new THREE.CylinderGeometry(radius, radius, 1, 4, 1, true);
}
