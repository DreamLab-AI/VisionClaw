/**
 * WasmSceneEffects
 *
 * React Three Fiber component that renders WASM-driven ambient background
 * effects for the knowledge graph visualization:
 *
 * 1. Particle field: drifting points with noise-based motion (WASM simulation)
 * 2. Energy wisps: larger hue-shifting glow orbs (WASM simulation)
 * 3. Atmosphere plane: procedural nebula texture as far background (WASM)
 * 4. JS fallback: if WASM fails to load, renders lightweight JS particles
 *
 * Rendering uses InstancedMesh + MeshBasicMaterial (WebGPURenderer draws no
 * raw-GLSL ShaderMaterial or sized Points). The GLSL fog plane that only
 * WebGLRenderer drew is gone; the WASM atmosphere plane is the background.
 *
 * Performance contract:
 *   - 3 draw calls maximum (particles + wisps + atmosphere)
 *   - All Float32Arrays pre-allocated in useMemo
 *   - Zero per-frame GC pressure (reused typed array views from WASM)
 *   - InstancedMesh rendering: proven pattern from GemNodes/GlassEdges
 */

import React, { useMemo, useRef, useEffect } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import * as THREE from 'three';
import { useWasmSceneEffects } from '../../../hooks/useWasmSceneEffects';
import { useSettingsStore } from '../../../store/settingsStore';
import type { SceneEffectsSettings } from '../../settings/config/settings';

// Pre-allocated temp objects (avoids per-frame GC).
// INVARIANT: These are shared across all sub-components and are safe ONLY
// because R3F useFrame callbacks execute synchronously on the main thread
// in a single requestAnimationFrame tick. Do NOT use in useEffect, callbacks,
// or any async context — allocate locals instead.
const _tempAtmDir = new THREE.Vector3();
const _tmpMat4 = new THREE.Matrix4();
const _tmpPos = new THREE.Vector3();
const _tmpScale = new THREE.Vector3();
const _tmpColor = new THREE.Color();
const _identityQuat = new THREE.Quaternion();
// Module-scope reusable HSL object (avoid per-frame GC pressure)
const _tmpHsl = { h: 0, s: 0, l: 0 };

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------
export interface WasmSceneEffectsProps {
  enabled?: boolean;
  /** Number of ambient particles (default 256, max 512). */
  particleCount?: number;
  /** Number of energy wisps (default 48, max 128). */
  wispCount?: number;
  /** Whether energy wisps are enabled (default true). */
  wispsEnabled?: boolean;
  /** Wisp drift speed multiplier (default 1.0). */
  wispDriftSpeed?: number;
  /** Atmosphere texture resolution (default 128). */
  atmosphereResolution?: number;
  /** Whether atmosphere/fog is enabled (default true). */
  atmosphereEnabled?: boolean;
  /** Overall intensity 0-1 (maps to opacity). */
  intensity?: number;
  /** Particle drift speed multiplier (0-2, default 0.5). Reserved for future WASM bridge support. */
  particleDrift?: number;
  /** Particle base color as CSS hex/rgb string (default '#6680E6'). */
  particleColor?: string;
  /** Wisp base color as CSS hex/rgb string (default '#668FCC'). */
  wispColor?: string;
}

// ---------------------------------------------------------------------------
// Shared constants
// ---------------------------------------------------------------------------
// Fallback constants aligned with DEFAULT_SCENE_EFFECTS in settingsApi.ts.
// particleCount default is 128 (settings), FALLBACK_COUNT kept at 128 to match.
const FALLBACK_COUNT = 128;
const FALLBACK_RADIUS = 120;
const FALLBACK_DRIFT = 0.15;
// Wisp radius matches the spatial scale of WASM wisp placement
const WISP_RADIUS = 80;
// Default colors used when no settings override is provided
const DEFAULT_PARTICLE_COLOR = '#6680E6';
const DEFAULT_WISP_COLOR = '#668FCC';

function hashNoise(x: number, y: number, seed: number): number {
  let h = (seed * 374761393 + x * 668265263 + y * 1274126177) | 0;
  h = ((h ^ (h >> 13)) * 1103515245) | 0;
  return ((h & 0x7fffffff) / 0x7fffffff) * 2 - 1;
}

// ---------------------------------------------------------------------------
// WASM-powered particles (InstancedMesh)
//
// WASM computes positions, opacities, sizes via Rust noise simulation.
// This component maps WASM data to InstancedMesh matrices + instance colors.
// Opacity is baked into color brightness (additive blending: dimmer = fainter).
// ---------------------------------------------------------------------------
interface WasmParticleInstancesProps {
  particles: NonNullable<ReturnType<typeof useWasmSceneEffects>['particles']>;
  update: ReturnType<typeof useWasmSceneEffects>['update'];
  opacity: number;
  count: number;
  /** Base particle color as CSS string (default from settings). */
  color?: string;
}

const WasmParticleInstances: React.FC<WasmParticleInstancesProps> = ({
  particles,
  update,
  opacity,
  count,
  color,
}) => {
  const meshRef = useRef<THREE.InstancedMesh | null>(null);
  const baseColor = useMemo(() => new THREE.Color(color || DEFAULT_PARTICLE_COLOR), [color]);

  const mesh = useMemo(() => {
    const geo = new THREE.IcosahedronGeometry(0.15, 0);
    const mat = new THREE.MeshBasicMaterial({
      transparent: true,
      opacity: 0.6,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.FrontSide,
    });

    const m = new THREE.InstancedMesh(geo, mat, count);
    m.frustumCulled = false;
    m.count = count;

    for (let i = 0; i < count; i++) {
      _tmpMat4.makeTranslation(0, 0, 0);
      m.setMatrixAt(i, _tmpMat4);
      m.setColorAt(i, baseColor);
    }
    m.instanceMatrix.needsUpdate = true;
    if (m.instanceColor) m.instanceColor.needsUpdate = true;

    meshRef.current = m;
    return m;
    // Deps intentionally narrowed; react-hooks/exhaustive-deps is not enforced in this config.
  }, []);

  // Dispose GPU resources on unmount (geometry, material, InstancedMesh buffers)
  useEffect(() => () => {
    mesh.geometry.dispose();
    (mesh.material as THREE.Material).dispose();
    mesh.dispose();
  }, [mesh]);

  useFrame(({ camera }, delta) => {
    const m = meshRef.current;
    if (!m || particles.isDisposed) return;

    const dt = Math.min(delta, 0.05);
    const cam = camera.position;
    update(dt, cam.x, cam.y, cam.z);

    const wasmPositions = particles.getPositions();
    const wasmOpacities = particles.getOpacities();
    const wasmSizes = particles.getSizes();
    const colorArray = m.instanceColor?.array as Float32Array | undefined;
    // Clamp to allocated buffer size to prevent overrun if count prop > mesh capacity
    const renderCount = Math.min(count, m.count);

    for (let i = 0; i < renderCount; i++) {
      const i3 = i * 3;
      _tmpPos.set(wasmPositions[i3], wasmPositions[i3 + 1], wasmPositions[i3 + 2]);
      const s = wasmSizes[i] * 0.12;
      _tmpScale.set(s, s, s);
      _tmpMat4.compose(_tmpPos, _identityQuat, _tmpScale);
      m.setMatrixAt(i, _tmpMat4);

      if (colorArray) {
        const brightness = wasmOpacities[i] * opacity;
        colorArray[i3] = baseColor.r * brightness;
        colorArray[i3 + 1] = baseColor.g * brightness;
        colorArray[i3 + 2] = baseColor.b * brightness;
      }
    }

    m.instanceMatrix.needsUpdate = true;
    if (m.instanceColor) m.instanceColor.needsUpdate = true;
  });

  return <primitive object={mesh} />;
};

// ---------------------------------------------------------------------------
// WASM-powered energy wisps (InstancedMesh)
//
// WASM computes positions, opacities, sizes, hues. This component maps
// that data to InstancedMesh matrices and hue-shifted instance colors.
// ---------------------------------------------------------------------------
interface WasmWispInstancesProps {
  wisps: NonNullable<ReturnType<typeof useWasmSceneEffects>['wisps']>;
  opacity: number;
  count: number;
  /** Base wisp color as CSS string (default from settings). */
  color?: string;
}

const WasmWispInstances: React.FC<WasmWispInstancesProps> = ({
  wisps,
  opacity,
  count,
  color,
}) => {
  const meshRef = useRef<THREE.InstancedMesh | null>(null);
  const wispBaseColor = useMemo(() => new THREE.Color(color || DEFAULT_WISP_COLOR), [color]);

  const mesh = useMemo(() => {
    const geo = new THREE.IcosahedronGeometry(0.3, 0);
    const mat = new THREE.MeshBasicMaterial({
      transparent: true,
      opacity: 0.7,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.FrontSide,
    });

    const m = new THREE.InstancedMesh(geo, mat, count);
    m.frustumCulled = false;
    m.count = count;

    _tmpColor.copy(wispBaseColor);
    for (let i = 0; i < count; i++) {
      _tmpMat4.makeTranslation(0, 0, 0);
      m.setMatrixAt(i, _tmpMat4);
      m.setColorAt(i, _tmpColor);
    }
    m.instanceMatrix.needsUpdate = true;
    if (m.instanceColor) m.instanceColor.needsUpdate = true;

    meshRef.current = m;
    return m;
    // Deps intentionally narrowed; react-hooks/exhaustive-deps is not enforced in this config.
  }, []);

  // Dispose GPU resources on unmount
  useEffect(() => () => {
    mesh.geometry.dispose();
    (mesh.material as THREE.Material).dispose();
    mesh.dispose();
  }, [mesh]);

  // NOTE: This component does NOT call update() — simulation is ticked by
  // WasmParticleInstances which always mounts when WASM is ready. This is
  // intentional to avoid double-ticking. If particles are ever made optional,
  // wisps must call update() themselves.
  useFrame(() => {
    const m = meshRef.current;
    if (!m || wisps.isDisposed) return;

    const wasmPositions = wisps.getPositions();
    const wasmOpacities = wisps.getOpacities();
    const wasmSizes = wisps.getSizes();
    const wasmHues = wisps.getHues();
    const colorArray = m.instanceColor?.array as Float32Array | undefined;
    // Clamp to allocated buffer size to prevent overrun if count prop > mesh capacity
    const renderCount = Math.min(count, m.count);

    for (let i = 0; i < renderCount; i++) {
      const i3 = i * 3;
      _tmpPos.set(wasmPositions[i3], wasmPositions[i3 + 1], wasmPositions[i3 + 2]);
      const s = wasmSizes[i] * 0.2;
      _tmpScale.set(s, s, s);
      _tmpMat4.compose(_tmpPos, _identityQuat, _tmpScale);
      m.setMatrixAt(i, _tmpMat4);

      if (colorArray) {
        // Derive base HSL from configured wisp color, shift hue per WASM output
        wispBaseColor.getHSL(_tmpHsl);
        const hue = wasmHues[i] * 0.3 + _tmpHsl.h;
        _tmpColor.setHSL(hue, _tmpHsl.s, _tmpHsl.l);
        const brightness = wasmOpacities[i] * opacity;
        colorArray[i3] = _tmpColor.r * brightness;
        colorArray[i3 + 1] = _tmpColor.g * brightness;
        colorArray[i3 + 2] = _tmpColor.b * brightness;
      }
    }

    m.instanceMatrix.needsUpdate = true;
    if (m.instanceColor) m.instanceColor.needsUpdate = true;
  });

  return <primitive object={mesh} />;
};

// ---------------------------------------------------------------------------
// WASM-powered atmosphere (MeshBasicMaterial + DataTexture — already compat)
// ---------------------------------------------------------------------------
interface WasmAtmosphereProps {
  atmosphere: NonNullable<ReturnType<typeof useWasmSceneEffects>['atmosphere']>;
  opacity: number;
}

const WasmAtmosphereBackground: React.FC<WasmAtmosphereProps> = ({
  atmosphere,
  opacity,
}) => {
  const meshRef = useRef<THREE.Mesh>(null);
  const { camera } = useThree();

  const texture = useMemo(() => {
    const w = atmosphere.width;
    const h = atmosphere.height;
    const tex = new THREE.DataTexture(
      new Uint8Array(w * h * 4),
      w, h,
      THREE.RGBAFormat,
    );
    tex.minFilter = THREE.LinearFilter;
    tex.magFilter = THREE.LinearFilter;
    tex.wrapS = THREE.ClampToEdgeWrapping;
    tex.wrapT = THREE.ClampToEdgeWrapping;
    tex.needsUpdate = true;
    return tex;
  }, [atmosphere]);

  useEffect(() => () => { texture.dispose(); }, [texture]);

  useFrame((_state, delta) => {
    if (atmosphere.isDisposed) return;
    const dt = Math.min(delta, 0.05);
    atmosphere.update(dt);

    const wasmPixels = atmosphere.getPixels();
    const texData = texture.image.data as Uint8Array;
    texData.set(wasmPixels);
    texture.needsUpdate = true;

    const mesh = meshRef.current;
    if (mesh) {
      _tempAtmDir.set(0, 0, -1).applyQuaternion(camera.quaternion);
      mesh.position.copy(camera.position).add(_tempAtmDir.multiplyScalar(90));
      mesh.quaternion.copy(camera.quaternion);
    }
  });

  return (
    <mesh ref={meshRef} renderOrder={-20} frustumCulled={false}>
      <planeGeometry args={[200, 200]} />
      <meshBasicMaterial
        map={texture}
        transparent
        opacity={opacity * 0.6}
        depthWrite={false}
        side={THREE.FrontSide}
        blending={THREE.AdditiveBlending}
      />
    </mesh>
  );
};

// ---------------------------------------------------------------------------
// JS Fallback: particles (InstancedMesh — no WASM needed)
// ---------------------------------------------------------------------------
const FallbackParticles: React.FC<{ opacity: number }> = React.memo(({ opacity }) => {
  const meshRef = useRef<THREE.InstancedMesh | null>(null);

  const { mesh, basePositions, baseSpeeds } = useMemo(() => {
    const geo = new THREE.IcosahedronGeometry(0.4, 0);
    const mat = new THREE.MeshBasicMaterial({
      transparent: true,
      opacity: Math.min(opacity, 0.6),
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.FrontSide,
    });

    const m = new THREE.InstancedMesh(geo, mat, FALLBACK_COUNT);
    m.frustumCulled = false;

    const c1 = new THREE.Color('#1a1a4e');
    const c2 = new THREE.Color('#c8d8ff');

    const pos = new Float32Array(FALLBACK_COUNT * 3);
    const spd = new Float32Array(FALLBACK_COUNT * 3);

    for (let i = 0; i < FALLBACK_COUNT; i++) {
      const phi = hashNoise(i, 0, 42) * Math.PI;
      const theta = hashNoise(i, 1, 42) * Math.PI * 2;
      const r = (0.3 + 0.7 * Math.abs(hashNoise(i, 2, 42))) * FALLBACK_RADIUS;
      const x = r * Math.sin(phi) * Math.cos(theta);
      const y = r * Math.sin(phi) * Math.sin(theta);
      const z = r * Math.cos(phi);
      pos[i * 3] = x;
      pos[i * 3 + 1] = y;
      pos[i * 3 + 2] = z;

      _tmpMat4.makeTranslation(x, y, z);
      m.setMatrixAt(i, _tmpMat4);

      const t = Math.abs(hashNoise(i, 4, 42));
      _tmpColor.copy(c1).lerp(c2, t);
      m.setColorAt(i, _tmpColor);

      spd[i * 3] = 0.5 + Math.abs(hashNoise(i, 5, 42));
      spd[i * 3 + 1] = 0.5 + Math.abs(hashNoise(i, 6, 42));
      spd[i * 3 + 2] = 0.5 + Math.abs(hashNoise(i, 7, 42));
    }

    m.instanceMatrix.needsUpdate = true;
    if (m.instanceColor) m.instanceColor.needsUpdate = true;
    m.count = FALLBACK_COUNT;

    meshRef.current = m;
    return { mesh: m, basePositions: pos, baseSpeeds: spd };
    // Deps intentionally narrowed; react-hooks/exhaustive-deps is not enforced in this config.
  }, []);

  // Dispose GPU resources on unmount
  useEffect(() => () => {
    mesh.geometry.dispose();
    (mesh.material as THREE.Material).dispose();
    mesh.dispose();
  }, [mesh]);

  useEffect(() => {
    const mat = mesh.material as THREE.MeshBasicMaterial;
    mat.opacity = Math.min(opacity, 0.6);
    mat.needsUpdate = true;
  }, [opacity, mesh]);

  useFrame(({ clock }) => {
    const m = meshRef.current;
    if (!m) return;
    const t = clock.elapsedTime * FALLBACK_DRIFT;

    for (let i = 0; i < FALLBACK_COUNT; i++) {
      const i3 = i * 3;
      const x = basePositions[i3] + Math.sin(t * baseSpeeds[i3] + i * 0.7) * 2.0;
      const y = basePositions[i3 + 1] + Math.sin(t * baseSpeeds[i3 + 1] + i * 1.3) * 1.5;
      const z = basePositions[i3 + 2] + Math.cos(t * baseSpeeds[i3 + 2] + i * 0.9) * 2.0;
      _tmpMat4.makeTranslation(x, y, z);
      m.setMatrixAt(i, _tmpMat4);
    }
    m.instanceMatrix.needsUpdate = true;
  });

  return <primitive object={mesh} />;
});
FallbackParticles.displayName = 'FallbackParticles';

// ---------------------------------------------------------------------------
// JS Fallback: energy wisps (InstancedMesh — no WASM needed)
// ---------------------------------------------------------------------------
const FallbackWisps: React.FC<{ opacity: number; count: number }> = React.memo(({ opacity, count }) => {
  const meshRef = useRef<THREE.InstancedMesh | null>(null);
  const safeCount = Math.min(count, 128);

  const { mesh, basePositions, speeds } = useMemo(() => {
    const geo = new THREE.IcosahedronGeometry(0.8, 0);
    const mat = new THREE.MeshBasicMaterial({
      transparent: true,
      opacity: Math.min(opacity, 0.7),
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.FrontSide,
    });

    const m = new THREE.InstancedMesh(geo, mat, safeCount);
    m.frustumCulled = false;

    const pos = new Float32Array(safeCount * 3);
    const spd = new Float32Array(safeCount * 3);

    for (let i = 0; i < safeCount; i++) {
      const phi = hashNoise(i, 10, 77) * Math.PI;
      const theta = hashNoise(i, 11, 77) * Math.PI * 2;
      const r = (0.4 + 0.6 * Math.abs(hashNoise(i, 12, 77))) * WISP_RADIUS;
      const x = r * Math.sin(phi) * Math.cos(theta);
      const y = r * Math.sin(phi) * Math.sin(theta);
      const z = r * Math.cos(phi);
      pos[i * 3] = x;
      pos[i * 3 + 1] = y;
      pos[i * 3 + 2] = z;

      _tmpMat4.makeTranslation(x, y, z);
      m.setMatrixAt(i, _tmpMat4);

      const hue = 0.55 + hashNoise(i, 13, 77) * 0.15;
      _tmpColor.setHSL(hue, 0.7, 0.6);
      m.setColorAt(i, _tmpColor);

      spd[i * 3] = 0.3 + Math.abs(hashNoise(i, 14, 77)) * 0.5;
      spd[i * 3 + 1] = 0.3 + Math.abs(hashNoise(i, 15, 77)) * 0.5;
      spd[i * 3 + 2] = 0.3 + Math.abs(hashNoise(i, 16, 77)) * 0.5;
    }

    m.instanceMatrix.needsUpdate = true;
    if (m.instanceColor) m.instanceColor.needsUpdate = true;
    m.count = safeCount;

    meshRef.current = m;
    return { mesh: m, basePositions: pos, speeds: spd };
    // Deps intentionally narrowed; react-hooks/exhaustive-deps is not enforced in this config.
  }, []);

  // Dispose GPU resources on unmount
  useEffect(() => () => {
    mesh.geometry.dispose();
    (mesh.material as THREE.Material).dispose();
    mesh.dispose();
  }, [mesh]);

  useEffect(() => {
    const mat = mesh.material as THREE.MeshBasicMaterial;
    mat.opacity = Math.min(opacity, 0.7);
    mat.needsUpdate = true;
  }, [opacity, mesh]);

  useFrame(({ clock }) => {
    const m = meshRef.current;
    if (!m) return;
    const t = clock.elapsedTime * 0.1;

    for (let i = 0; i < safeCount; i++) {
      const i3 = i * 3;
      const x = basePositions[i3] + Math.sin(t * speeds[i3] + i * 1.1) * 5.0;
      const y = basePositions[i3 + 1] + Math.sin(t * speeds[i3 + 1] + i * 0.7) * 4.0;
      const z = basePositions[i3 + 2] + Math.cos(t * speeds[i3 + 2] + i * 1.3) * 5.0;
      _tmpMat4.makeTranslation(x, y, z);
      m.setMatrixAt(i, _tmpMat4);
    }
    m.instanceMatrix.needsUpdate = true;
  });

  return <primitive object={mesh} />;
});
FallbackWisps.displayName = 'FallbackWisps';

// ---------------------------------------------------------------------------
// Main component — NO WebGPU gate: WASM simulation drives InstancedMesh
// rendering on all backends. Fallback only used when WASM fails to load.
// ---------------------------------------------------------------------------
const WasmSceneEffects: React.FC<WasmSceneEffectsProps> = ({
  enabled = true,
  particleCount = 256,
  wispCount = 48,
  wispsEnabled = true,
  wispDriftSpeed = 1.0,
  atmosphereResolution = 128,
  atmosphereEnabled = true,
  intensity = 0.6,
  particleDrift: _particleDrift = 0.5,
  particleColor: particleColorProp,
  wispColor: wispColorProp,
}) => {
  // Read scene effects settings from the store for values not passed as props
  const storeSettings = useSettingsStore(s => s.get<SceneEffectsSettings>('visualisation.sceneEffects'));
  const resolvedParticleColor = particleColorProp || storeSettings?.particleColor || DEFAULT_PARTICLE_COLOR;
  const resolvedWispColor = wispColorProp || storeSettings?.wispColor || DEFAULT_WISP_COLOR;

  const { ready, failed, particles, atmosphere, wisps, update } = useWasmSceneEffects({
    particleCount,
    wispCount: wispsEnabled ? wispCount : 0,
    atmosphereWidth: atmosphereResolution,
    atmosphereHeight: atmosphereResolution,
    enabled,
  });

  // Apply drift speed setting to wisps
  useEffect(() => {
    if (wisps) {
      wisps.setDriftSpeed(wispDriftSpeed);
    }
  }, [wisps, wispDriftSpeed]);

  if (!enabled) return null;

  const clamped = Math.max(0, Math.min(1, intensity));

  // WASM path: Rust noise simulation -> InstancedMesh rendering
  if (ready && particles) {
    return (
      <group name="wasm-scene-effects">
        <WasmParticleInstances
          particles={particles}
          update={update}
          opacity={clamped}
          count={particleCount}
          color={resolvedParticleColor}
        />
        {atmosphereEnabled && atmosphere && (
          <WasmAtmosphereBackground
            atmosphere={atmosphere}
            opacity={clamped}
          />
        )}
        {wispsEnabled && wisps && (
          <WasmWispInstances
            wisps={wisps}
            opacity={clamped}
            count={wispCount}
            color={resolvedWispColor}
          />
        )}
      </group>
    );
  }

  // JS fallback: only when WASM fails to load (not renderer-dependent)
  if (failed || !ready) {
    const particleOpacity = 0.15 + clamped * 0.35;
    const wispOpacity = 0.2 + clamped * 0.4;

    return (
      <group name="wasm-scene-effects-fallback" renderOrder={-1}>
        <FallbackParticles opacity={particleOpacity} />
        {wispsEnabled && <FallbackWisps opacity={wispOpacity} count={wispCount} />}
      </group>
    );
  }

  return null;
};

export default WasmSceneEffects;
