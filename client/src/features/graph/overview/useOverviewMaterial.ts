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
  // The material is paired with the geometry it was built for: a TSL material
  // binds that geometry's attribute objects, so a new geometry (capacity
  // growth) must never draw with the previous material, not even for the
  // frame before the rebuild resolves.
  const [built, setBuilt] = useState<{ material: THREE.Material; geometry: THREE.InstancedBufferGeometry } | null>(null);
  useEffect(() => {
    let cancelled = false;
    let made: THREE.Material | null = null;
    if (isWebGPURenderer) {
      tsl(uniforms, geometry).then((m) => {
        if (cancelled) { m.dispose(); return; }
        made = m;
        setBuilt({ material: m, geometry });
      }).catch((err) => logger.warn('overview TSL material failed to build:', err));
    } else {
      made = glsl(uniforms);
      setBuilt({ material: made, geometry });
    }
    return () => {
      cancelled = true;
      made?.dispose();
      setBuilt(null);
    };
  }, [geometry, uniforms, glsl, tsl]);
  return built && built.geometry === geometry ? built.material : null;
}
