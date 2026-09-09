import * as THREE from 'three';
import { isWebGPURenderer } from '../rendererFactory';
import { createLogger } from '../../utils/loggerConfig';

const logger = createLogger('AgentCapsuleMaterial');

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

export interface AgentCapsuleMaterialResult {
  material: THREE.Material;
  uniforms: {
    time: { value: number };
    glowStrength: { value: number };
    activityLevel: { value: number };
  };
  ready: Promise<void>;
}

// ---------------------------------------------------------------------------
// Shared agent capsule material — works on both renderers.
// ---------------------------------------------------------------------------

export function createAgentCapsuleMaterial(): AgentCapsuleMaterialResult {
  const uniforms = { time: { value: 0 }, glowStrength: { value: 1.0 }, activityLevel: { value: 1.0 } };

  const material = new THREE.MeshStandardMaterial({
    color: new THREE.Color(0.82, 0.95, 0.86),
    // Opaque + single-sided: agents were already near-opaque; drop blend + back faces.
    roughness: 0.15,
    metalness: 0.0,
    transparent: true,
    opacity: 1.0,
    side: THREE.FrontSide,
    depthWrite: true,
    emissive: new THREE.Color(0.08, 0.4, 0.2),
    emissiveIntensity: 0.25,
    // envMapIntensity scales reflected-environment light. GraphCanvas installs
    // scene.environment (drei PMREM) on BOTH backends, so the WebGL capsule now
    // reflects the same as WebGPU (parity, task #50). Previously gated to WebGPU.
    envMapIntensity: 1.8,
  });

  // TSL ENABLED (r183+) with PBR fallback — the full metadata-driven TSL upgrade
  // (Fresnel base + per-node metadata emissive) is wired from GemNodes.tsx via
  // createTslAgentCapsuleMaterial() once metadataTexture and instanceCount are
  // available. The basic Fresnel-only upgrade is skipped here to avoid a
  // double-needsUpdate recompile conflict with the metadata pass (mirrors the
  // GemNodeMaterial factory). PBR + per-frame emissive modulation in GemNodes
  // useFrame remains the active fallback path.
  const ready = Promise.resolve();

  return { material, uniforms, ready };
}

// ---------------------------------------------------------------------------
// TSL metadata-driven material (WebGPU only)
//
// Mirrors createTslGemMaterial in GemNodeMaterial.ts: reads per-instance
// metadata from the same 2D DataTexture (RGBA float = quality, authority,
// connectionCount, recency) via instanceIndex (NOT an InstancedBufferAttribute).
// Keeps the existing Fresnel bioluminescent look as the base and ADDS a
// per-node metadata emissive so high-authority / high-degree agent capsules glow
// brighter individually rather than sharing one breathing value.
// ---------------------------------------------------------------------------
export async function createTslAgentCapsuleMaterial(
  material: THREE.MeshStandardMaterial,
  metadataTexture: THREE.DataTexture,
  texWidth: number,
  texHeight: number,
): Promise<boolean> {
  if (!isWebGPURenderer) return false;

  try {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- Three.js TSL module exports are complex node builder types with no stable public API
    const tslMod = await import('three/tsl') as any;
    const {
      float, vec2, vec3,
      mix, pow, sin, add, sub, max, step,
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
    // meta.w is REINTERPRETED for the agent capsule population: GemNodes packs a
    // status-derived activity scalar here (idle 0.15 … active/busy 1.0, error ~0.9)
    // instead of the recency value it carries for the gem/ontology populations.
    // Safe because GemNodes renders one population per mesh, so this texture is
    // exclusively agent metadata. NearestFilter float texels preserve the exact
    // packed value, letting the error band isolate cleanly below.
    const activity = meta.w;

    // --- Per-instance unique phase ---
    const phase = fract(sin(idx.mul(43758.5453))).mul(6.2831);

    // --- Fresnel rim lighting — sprite-like halo for embodied agents ---
    const viewDir = normalize(positionView.negate());
    const nDotV = saturate(dot(normalView, viewDir));
    const fresnel = pow(oneMinus(nDotV), float(3.0));
    const haloRim = pow(oneMinus(nDotV), float(1.5));

    // --- Pulse driven by whichever is stronger: KG authority or live activity ---
    // so an idle swarm rests (slow pulse) and an active one works (fast pulse).
    const pulseSpeed = mix(float(1.0), float(3.5), max(authority, activity));
    const pulse = sin(time.mul(pulseSpeed).add(phase)).mul(0.5).add(0.5);

    // --- Quality drives emissive brightness; connections warm the green base ---
    const qualityBrightness = mix(float(0.3), float(0.8), quality);
    const activityBoost = mix(float(0.5), float(1.0), activity);
    const warmShift = connections.mul(0.25);

    // Agent base hue (bioluminescent green) — matches the static emissive above.
    const baseEmissive = vec3(
      add(float(0.08), warmShift),
      float(0.40),
      sub(float(0.20), warmShift.mul(0.3)),
    );

    const fresnelEmissive = vec3(float(0.08), float(0.4), float(0.2)).mul(fresnel);
    const haloEmissive = vec3(float(0.12), float(0.5), float(0.3)).mul(haloRim).mul(activityBoost);
    const metaEmissive = baseEmissive
      .mul(qualityBrightness)
      .mul(mix(float(0.4), float(1.0), pulse))
      .mul(activityBoost);

    // Error tint: the packed error activity (~0.9) sits in a tight band below
    // active/busy (1.0). Gate on low authority so a genuine high-authority agent
    // never trips the distress red (avoids false positives).
    const errorBand = step(float(0.85), activity).mul(oneMinus(step(float(0.95), activity)));
    const notAuthoritative = oneMinus(step(float(0.05), authority));
    const errorFlag = errorBand.mul(notAuthoritative);
    const errorTint = vec3(float(0.7), float(0.0), float(0.0)).mul(errorFlag);

    const emissiveNode = fresnelEmissive.add(haloEmissive).add(metaEmissive).add(errorTint);

    // --- Opacity: Fresnel rim + authority-based solidity; done agents fade ---
    // A very low activity (≤0.1) flags a "done" agent (no recent work-target
    // beam); these fade to 0.3 alpha so they remain visible but recessive.
    const doneFlag = oneMinus(step(float(0.1), activity));
    const workingAlpha = mix(float(0.5), float(0.9), authority);
    const baseAlpha = mix(workingAlpha, float(0.3), doneFlag);
    const opacityNode = mix(baseAlpha, float(0.95), fresnel);

    const augmented = material as unknown as TSLNodeProperties;
    augmented.emissiveNode = emissiveNode;
    augmented.opacityNode = opacityNode;
    augmented.needsUpdate = true;

    logger.info('[AgentCapsuleMaterial] TSL metadata nodes applied to existing material');
    return true;
  } catch (err) {
    logger.warn('[AgentCapsuleMaterial] TSL metadata upgrade failed:', err);
    return false;
  }
}

export function createAgentCapsuleGeometry(): THREE.CapsuleGeometry {
  // Low-poly caps/radials (4/8) — smooth tessellation is invisible at node scale.
  return new THREE.CapsuleGeometry(0.3, 0.6, 4, 8);
}
