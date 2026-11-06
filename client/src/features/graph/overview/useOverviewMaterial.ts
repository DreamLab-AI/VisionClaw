/**
 * Build an overview material for the live renderer: GLSL ShaderMaterial on
 * WebGLRenderer, TSL node material on WebGPURenderer (async import). Rebuilt
 * when the geometry is replaced (the TSL material binds its instance
 * attributes at build time). Disposes what it built.
 */
import { useEffect, useState } from 'react';
import type * as THREE from 'three';
import { isWebGPURenderer } from '../../../rendering/rendererFactory';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('OverviewMaterial');

export function useOverviewMaterial<U>(
  geometry: THREE.InstancedBufferGeometry,
  uniforms: U,
  glsl: (u: U) => THREE.Material,
  tsl: (u: U, g: THREE.InstancedBufferGeometry) => Promise<THREE.Material>,
): THREE.Material | null {
  const [material, setMaterial] = useState<THREE.Material | null>(null);
  useEffect(() => {
    let cancelled = false;
    let built: THREE.Material | null = null;
    if (isWebGPURenderer) {
      tsl(uniforms, geometry).then((m) => {
        if (cancelled) { m.dispose(); return; }
        built = m;
        setMaterial(m);
      }).catch((err) => logger.warn('overview TSL material failed to build:', err));
    } else {
      built = glsl(uniforms);
      setMaterial(built);
    }
    return () => {
      cancelled = true;
      built?.dispose();
      setMaterial(null);
    };
  }, [geometry, uniforms, glsl, tsl]);
  return material;
}
