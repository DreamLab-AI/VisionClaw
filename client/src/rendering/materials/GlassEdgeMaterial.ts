import * as THREE from 'three';
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
// Glass edge material.
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
    // Transmission masks alpha — keep it ~0 on edges so the
    // opacity/alpha channel has full authority over edge visibility.
    transmission: 0,
    thickness: 0,
    roughness: 0.05,
    metalness: 0.0,
    transparent: true,
    opacity: 0.6,
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
    emissive: neutralBase.clone().multiplyScalar(0.15),
    emissiveIntensity: 0.2,
    // Iridescence/sheen/specular: standard MeshPhysicalMaterial PBR features.
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

  // Fresnel emissive and opacity TSL nodes, applied asynchronously; until they
  // land the edge draws as plain PBR.
  const ready = (async () => {
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
