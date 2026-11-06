/**
 * Build an overview TSL material for its geometry (async: three/webgpu and
 * three/tsl load on demand). Rebuilt when the geometry is replaced (the
 * material binds its instance attributes at build time). Disposes what it
 * built. A build that fails is a fault of the layer (ADR-2181 D4), reported
 * through `onError` so the layer can hand over to the gems / tubes.
 */
import { useEffect, useState } from 'react';
import type * as THREE from 'three';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('OverviewMaterial');

export function useOverviewMaterial<U>(
  geometry: THREE.InstancedBufferGeometry,
  uniforms: U,
  build: (u: U, g: THREE.InstancedBufferGeometry) => Promise<THREE.Material>,
  onError?: (err: unknown) => void,
): THREE.Material | null {
  // The material is paired with the geometry it was built for: it binds that
  // geometry's attribute objects, so a new geometry (capacity growth) must
  // never draw with the previous material, not even for the frame before the
  // rebuild resolves.
  const [built, setBuilt] = useState<{ material: THREE.Material; geometry: THREE.InstancedBufferGeometry } | null>(null);
  useEffect(() => {
    let cancelled = false;
    let made: THREE.Material | null = null;
    build(uniforms, geometry).then((m) => {
      if (cancelled) { m.dispose(); return; }
      made = m;
      setBuilt({ material: m, geometry });
    }).catch((err) => {
      if (cancelled) return;
      logger.warn('overview material failed to build:', err);
      onError?.(err);
    });
    return () => {
      cancelled = true;
      made?.dispose();
      setBuilt(null);
    };
    // onError is read at failure time; a new callback identity must not rebuild.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [geometry, uniforms, build]);
  return built && built.geometry === geometry ? built.material : null;
}
