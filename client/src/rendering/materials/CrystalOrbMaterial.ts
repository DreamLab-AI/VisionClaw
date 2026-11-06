import * as THREE from 'three';
import { createLogger } from '../../utils/loggerConfig';

const logger = createLogger('CrystalOrbMaterial');

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

export interface CrystalOrbMaterialResult {
  material: THREE.Material;
  uniforms: {
    time: { value: number };
    glowStrength: { value: number };
    pulseSpeed: { value: number };
  };
  ready: Promise<void>;
}

// ---------------------------------------------------------------------------
// Shared crystal orb material.
// ---------------------------------------------------------------------------

export function createCrystalOrbMaterial(): CrystalOrbMaterialResult {
  const uniforms = { time: { value: 0 }, glowStrength: { value: 1.2 }, pulseSpeed: { value: 0.8 } };

  const material = new THREE.MeshStandardMaterial({
    color: new THREE.Color(0.78, 0.78, 1.0),
    // Translucent but cheap: FrontSide + depthWrite:true bounds the overdraw that
    // 52k double-sided orbs caused, while transparent:true reactivates the Fresnel
    // opacityNode (createTslCrystalOrbMaterial) for the glassy violet orb look.
    roughness: 0.12,
    metalness: 0.0,
    transparent: true,
    opacity: 0.85,
    side: THREE.FrontSide,
    depthWrite: true,
    emissive: new THREE.Color(0.12, 0.12, 0.25),
    emissiveIntensity: 0.3,
    // envMapIntensity scales reflected-environment light (GraphCanvas installs
    // scene.environment, a drei PMREM).
    envMapIntensity: 2.0,
  });

  // TSL ENABLED (r183+) with PBR fallback — the full metadata-driven TSL upgrade
  // (Fresnel base + per-node metadata emissive) is wired from GemNodes.tsx via
  // createTslCrystalOrbMaterial() once metadataTexture and instanceCount are
  // available. The basic Fresnel-only upgrade is skipped here to avoid a
  // double-needsUpdate recompile conflict with the metadata pass (mirrors the
  // GemNodeMaterial factory). PBR + per-frame emissive modulation in GemNodes
  // useFrame remains the active fallback path.
  const ready = Promise.resolve();

  return { material, uniforms, ready };
}

// ---------------------------------------------------------------------------
// TSL metadata-driven material
//
// Mirrors createTslGemMaterial in GemNodeMaterial.ts exactly: reads per-instance
// metadata from the same 2D DataTexture (RGBA float = quality, authority,
// connectionCount, recency), sampled via instanceIndex (NOT an
// InstancedBufferAttribute — that crashes the WebGPU backend with
// drawIndexed(Infinity)). The existing Fresnel look is kept as the base and the
// metadata-driven emissive is ADDED on top so high-authority / high-degree
// ontology orbs glow brighter per-node rather than sharing one breathing value.
// ---------------------------------------------------------------------------
export async function createTslCrystalOrbMaterial(
  material: THREE.MeshStandardMaterial,
  metadataTexture: THREE.DataTexture,
  texWidth: number,
  texHeight: number,
): Promise<boolean> {
  try {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- Three.js TSL module exports are complex node builder types with no stable public API
    const tslMod = await import('three/tsl') as any;
    const {
      float, vec2, vec3,
      mix, pow, sin, add, sub,
      dot, normalize, oneMinus, saturate, fract,
      mod, floor,
      time, instanceIndex,
      normalView, positionView,
      texture: tslTexture,
    } = tslMod;

    // --- Per-instance metadata via 2D DataTexture (row-major grid) ---
    const texW = float(texWidth);
    const texH = float(texHeight);
    const idx = float(instanceIndex);
    const col = mod(idx, texW);
    const row = floor(idx.div(texW));
    const texU = col.add(0.5).div(texW);
    const texV = row.add(0.5).div(texH);
    const meta = tslTexture(metadataTexture, vec2(texU, texV));
    const quality = meta.x;
    const authority = meta.y;
    const connections = meta.z;
    const recency = meta.w;

    // --- Per-instance unique phase ---
    const phase = fract(sin(idx.mul(43758.5453))).mul(6.2831);

    // --- Fresnel rim lighting (the existing crystal-orb look) ---
    const viewDir = normalize(positionView.negate());
    const nDotV = saturate(dot(normalView, viewDir));
    const fresnel = pow(oneMinus(nDotV), float(3.0));

    // --- Authority-driven pulse ---
    const pulseSpeed = mix(float(0.8), float(3.0), authority);
    const pulse = sin(time.mul(pulseSpeed).add(phase)).mul(0.5).add(0.5);

    // --- Quality drives emissive brightness; connections warm the violet base ---
    const qualityBrightness = mix(float(0.3), float(0.8), quality);
    const recencyBoost = mix(float(0.5), float(1.0), recency);
    const warmShift = connections.mul(0.25);

    // Crystal-orb base hue (violet/blue) — matches the static emissive above.
    const baseEmissive = vec3(
      add(float(0.18), warmShift),
      float(0.18),
      sub(float(0.40), warmShift.mul(0.5)),
    );

    // Fresnel base + per-node metadata emissive ADDED on top.
    const fresnelEmissive = vec3(float(0.12), float(0.12), float(0.25)).mul(fresnel);
    const metaEmissive = baseEmissive
      .mul(qualityBrightness)
      .mul(mix(float(0.4), float(1.0), pulse))
      .mul(recencyBoost);
    const emissiveNode = fresnelEmissive.add(metaEmissive);

    // --- Opacity: Fresnel rim + authority-based solidity ---
    const baseAlpha = mix(float(0.55), float(0.85), authority);
    const opacityNode = mix(baseAlpha, float(0.95), fresnel);

    const augmented = material as unknown as TSLNodeProperties;
    augmented.emissiveNode = emissiveNode;
    augmented.opacityNode = opacityNode;
    augmented.needsUpdate = true;

    logger.info('[CrystalOrbMaterial] TSL metadata nodes applied to existing material');
    return true;
  } catch (err) {
    logger.warn('[CrystalOrbMaterial] TSL metadata upgrade failed:', err);
    return false;
  }
}

export function createCrystalOrbGeometry(): THREE.IcosahedronGeometry {
  // Faceted icosahedron (detail=1, 80 tris) instead of a smooth 16×16 sphere
  // (~480 tris). Nodes render a few px wide in dense graphs, so smoothness was
  // invisible cost; at 50k+ instances this is the dominant triangle budget
  // (~27M → ~4M). The flat facets read as crystalline, not lo-fi, at node scale.
  return new THREE.IcosahedronGeometry(0.5, 1);
}
