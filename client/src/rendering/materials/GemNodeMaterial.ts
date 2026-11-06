import * as THREE from 'three';
import { createLogger } from '../../utils/loggerConfig';

const logger = createLogger('GemNodeMaterial');

// ---------------------------------------------------------------------------
// TSL node-augmented material interface
// ---------------------------------------------------------------------------

/** Runtime-augmented material properties added by Three.js TSL (node-based shading). */
interface TSLNodeProperties {
  emissiveNode: unknown;
  opacityNode: unknown;
  positionNode: unknown;
  needsUpdate: boolean;
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface GemMaterialResult {
  material: THREE.Material;
  uniforms: {
    time: { value: number };
    glowStrength: { value: number };
  };
  ready: Promise<void>;
}

// ---------------------------------------------------------------------------
// Base gem material — MeshStandardMaterial.
// Cheaper fragment shader than MeshPhysical (no clearcoat/sheen/iridescence);
// per-node glow comes from emissive + the TSL emissiveNode below.
// This is always available as the synchronous fallback.
// ---------------------------------------------------------------------------

export function createGemNodeMaterial(): GemMaterialResult {
  const uniforms = { time: { value: 0 }, glowStrength: { value: 1.5 } };

  // Mid-tone crystalline base so iridescence has reflected light to modulate.
  // Emissive kept subtle — the per-instance TSL emissiveNode provides the glow.
  const material = new THREE.MeshStandardMaterial({
    color: new THREE.Color(0.35, 0.38, 0.5),
    // Translucent but cheap: FrontSide (no backface fragment doubling) +
    // depthWrite:true (depth-test rejects occluded fragments, bounding overdraw).
    // transparent:true reactivates the Fresnel opacityNode (createTslGemMaterial)
    // for the glassy crystalline rim. This is the middle ground between the
    // original DoubleSide+depthWrite:false glass (correct blend, full overdraw)
    // and fully opaque — restores the look without the overdraw that tanked fps.
    roughness: 0.08,
    metalness: 0.15,
    transparent: true,
    opacity: 0.85,
    side: THREE.FrontSide,
    depthWrite: true,
    emissive: new THREE.Color(0.03, 0.04, 0.1),
    emissiveIntensity: 0.4,
    // envMapIntensity scales reflected-environment light (GraphCanvas installs
    // scene.environment, a drei PMREM).
    envMapIntensity: 1.8,
  });

  // TSL ENABLED (r183+) with PBR fallback — the full metadata-driven TSL upgrade
  // is wired from GemNodes.tsx via createTslGemMaterial() once metadataTexture and
  // instanceCount are available. The basic Fresnel-only upgrade is skipped here to
  // avoid a double-needsUpdate conflict with the metadata pass.
  const ready = Promise.resolve();

  return { material, uniforms, ready };
}

// ---------------------------------------------------------------------------
// TSL metadata-driven material
//
// Reads per-instance metadata from a 2D DataTexture (WxH grid, RGBA, Float):
//   .r = quality   (0-1)  → emissive glow brightness
//   .g = authority  (0-1)  → pulse speed + base opacity
//   .b = connections (0-1) → emissive warmth (blue → orange)
//   .a = recency   (0-1)  → overall vibrancy
//
// Texture sampled via instanceIndex — avoids InstancedBufferAttribute which
// causes drawIndexed(Infinity) crash in WebGPU backend.
// instanceColor is read for per-node tinting.
// No backdropNode/viewportSharedTexture — avoids the transmission crash.
// ---------------------------------------------------------------------------

/**
 * Augment an existing MeshStandardMaterial with TSL metadata-driven emissive
 * and opacity nodes.  This follows the same pattern as GlassEdgeMaterial:
 * add TSL nodes to the EXISTING material rather than creating a new
 * MeshPhysicalNodeMaterial — the latter silently fails with InstancedMesh
 * on WebGPU.
 *
 * Per-instance color is handled by the standard material's native
 * instanceColor support (setColorAt) — no colorNode override needed.
 */
export async function createTslGemMaterial(
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
    // The metadata is packed into a texWidth×texHeight grid (width capped well
    // under the WebGPU 8192 limit). Instance i lives at texel (i % W, i / W);
    // sample at the texel centre with NearestFilter.
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
    const rawIndex = float(instanceIndex);
    const phase = fract(sin(rawIndex.mul(43758.5453))).mul(6.2831);

    // --- Fresnel rim lighting ---
    const viewDir = normalize(positionView.negate());
    const nDotV = saturate(dot(normalView, viewDir));
    const fresnel = pow(oneMinus(nDotV), float(3.0));

    // --- Authority-driven pulse ---
    const pulseSpeed = mix(float(0.8), float(3.0), authority);
    const pulse = sin(time.mul(pulseSpeed).add(phase)).mul(0.5).add(0.5);

    // --- Quality drives emissive brightness ---
    const qualityBrightness = mix(float(0.3), float(0.8), quality);
    const recencyBoost = mix(float(0.5), float(1.0), recency);
    const warmShift = connections.mul(0.25);

    const baseEmissive = vec3(
      add(float(0.25), warmShift),
      float(0.30),
      sub(float(0.50), warmShift.mul(0.5)),
    );

    const emissiveNode = baseEmissive
      .mul(qualityBrightness)
      .mul(mix(float(0.4), float(1.0), pulse))
      .mul(recencyBoost);

    // --- Opacity: Fresnel rim + authority-based solidity ---
    const baseAlpha = mix(float(0.55), float(0.85), authority);
    const opacityNode = mix(baseAlpha, float(0.95), fresnel);

    // Add TSL nodes to the EXISTING material (GlassEdges pattern).
    // Do NOT set colorNode — the standard material reads instanceColor natively.
    const augmented = material as unknown as TSLNodeProperties;
    augmented.emissiveNode = emissiveNode;
    augmented.opacityNode = opacityNode;
    augmented.needsUpdate = true;

    logger.info('[GemNodeMaterial] TSL metadata nodes applied to existing material');
    return true;
  } catch (err) {
    logger.warn('[GemNodeMaterial] TSL metadata upgrade failed:', err);
    return false;
  }
}

// ---------------------------------------------------------------------------
// TSL GPU instance transform
//
// Task #54: move the per-instance position + uniform scale off the CPU. Instead
// of composing a mat4 on the CPU every frame (makeScale + setPosition +
// setMatrixAt), the transform is sampled on the GPU from a per-instance
// DataTexture (RGBA float = x, y, z, scale) via instanceIndex — the SAME
// safe sampling path used for the metadata texture. An InstancedBufferAttribute
// is deliberately NOT used here: it crashes the WebGPU backend with
// drawIndexed(Infinity) on this InstancedMesh path (see the metadata note above).
//
// positionNode overrides the local vertex position: positionLocal * scale + pos.
// The mesh's instanceMatrix is left identity and its modelMatrix is identity
// (scene-added, untransformed), so object space == world space and the result
// matches the old CPU matrix exactly. The CPU per-frame cost collapses to a
// single contiguous typed-array write + needsUpdate, with no mat4 math.
// ---------------------------------------------------------------------------

/**
 * Augment a MeshStandardMaterial so the GPU derives each instance's transform
 * (position + uniform scale) from a transform DataTexture sampled by
 * instanceIndex.
 *
 * @param transformTexture RGBA float texture, texel i = (x, y, z, scale).
 * @param texWidth  texture width (texels)
 * @param texHeight texture height (texels)
 * @returns true if the positionNode was applied.
 */
export async function applyTslInstanceTransform(
  material: THREE.MeshStandardMaterial,
  transformTexture: THREE.DataTexture,
  texWidth: number,
  texHeight: number,
): Promise<boolean> {
  try {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- Three.js TSL module exports are complex node builder types with no stable public API
    const tslMod = await import('three/tsl') as any;
    const {
      float, vec2,
      mod, floor,
      instanceIndex, positionLocal,
      texture: tslTexture,
    } = tslMod;

    // Sample the transform texel for this instance (same row-major grid layout
    // as the metadata texture: texel (i % W, i / W), centre-sampled, Nearest).
    const texW = float(texWidth);
    const texH = float(texHeight);
    const idx = float(instanceIndex);
    const col = mod(idx, texW);
    const row = floor(idx.div(texW));
    const texU = col.add(0.5).div(texW);
    const texV = row.add(0.5).div(texH);
    const xform = tslTexture(transformTexture, vec2(texU, texV));
    const instPos = xform.xyz;       // world position
    const instScale = xform.w;       // uniform scale

    // Local vertex scaled then translated into world space. Mirrors the old
    // CPU mat4 (makeScale(s,s,s); setPosition(x,y,z)) with identity model+inst.
    const augmented = material as unknown as TSLNodeProperties;
    augmented.positionNode = positionLocal.mul(instScale).add(instPos);
    augmented.needsUpdate = true;

    logger.info('[GemNodeMaterial] TSL GPU instance transform applied');
    return true;
  } catch (err) {
    logger.warn('[GemNodeMaterial] TSL instance-transform upgrade failed:', err);
    return false;
  }
}

// ---------------------------------------------------------------------------
// Disposal helper
// ---------------------------------------------------------------------------

export function disposeGemMaterial(result: GemMaterialResult): void {
  if (result.material) {
    result.material.dispose();
  }
}

// ---------------------------------------------------------------------------
// Geometry helper — faceted gem (detail=1, 80 tris; detail=2 was 320 and
// invisible at node pixel scale)
// ---------------------------------------------------------------------------

export function createGemGeometry(): THREE.IcosahedronGeometry {
  return new THREE.IcosahedronGeometry(0.5, 1);
}
