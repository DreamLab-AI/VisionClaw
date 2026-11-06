/**
 * The scene environment map for the PBR glass gems and tubes: a dim studio
 * (#181b22) lit by four Lightformer-style panels, rendered once into a cube
 * map and installed as `scene.environment` (the background is untouched).
 *
 * drei's <Environment> renders into a WebGLCubeRenderTarget, which
 * WebGPURenderer does not support: on its WebGL2 backend each face bind fails
 * (GL_INVALID_OPERATION from glFramebufferTexture2D) and the map stays empty.
 * three/webgpu's CubeRenderTarget is the cube target WebGPURenderer draws on
 * either backend; the node materials prefilter it (PMREM) themselves.
 */
import { useEffect } from 'react';
import { useThree } from '@react-three/fiber';
import * as THREE from 'three';
import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('StudioEnvironment');

/** One emissive panel: drei Lightformer's look (unlit, not tone-mapped, colour × intensity, facing the origin). */
interface Panel { intensity: number; position: [number, number, number]; scale: [number, number]; color: string }

/** Light from above-behind, two side strips (cool / warm) and a low fill. */
const PANELS: readonly Panel[] = [
  { intensity: 1.4, position: [0, 6, -9], scale: [12, 12], color: '#cfe0ff' },
  { intensity: 1.0, position: [-7, 1, -2], scale: [12, 3], color: '#a8c4ff' },
  { intensity: 1.0, position: [7, -1, -2], scale: [12, 3], color: '#ffd9b0' },
  { intensity: 0.8, position: [0, -6, 4], scale: [12, 12], color: '#6b7a9c' },
];

const STUDIO_BACKGROUND = '#181b22';

function buildStudio(): { scene: THREE.Scene; dispose: () => void } {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(STUDIO_BACKGROUND);
  const geometry = new THREE.PlaneGeometry(1, 1);
  const materials: THREE.Material[] = [];
  for (const p of PANELS) {
    const material = new THREE.MeshBasicMaterial({
      color: new THREE.Color(p.color).multiplyScalar(p.intensity),
      side: THREE.DoubleSide,
      toneMapped: false,
    });
    materials.push(material);
    const mesh = new THREE.Mesh(geometry, material);
    mesh.position.set(...p.position);
    mesh.scale.set(p.scale[0], p.scale[1], 1);
    mesh.lookAt(0, 0, 0);
    scene.add(mesh);
  }
  return { scene, dispose: () => { geometry.dispose(); for (const m of materials) m.dispose(); } };
}

export function StudioEnvironment({ resolution = 256 }: { resolution?: number }): null {
  const gl = useThree((s) => s.gl);
  const scene = useThree((s) => s.scene);

  useEffect(() => {
    let cancelled = false;
    let target: { texture: THREE.Texture; dispose: () => void } | null = null;
    const studio = buildStudio();
    (async () => {
      try {
        const { CubeRenderTarget } = await import('three/webgpu');
        if (cancelled) return;
        const cube = new CubeRenderTarget(resolution, { type: THREE.HalfFloatType });
        const camera = new THREE.CubeCamera(0.1, 1000, cube as unknown as THREE.WebGLCubeRenderTarget);
        camera.update(gl as unknown as THREE.WebGLRenderer, studio.scene);
        target = cube;
        scene.environment = cube.texture;
      } catch (err) {
        logger.warn('environment map failed to build; PBR materials draw without reflections:', err);
      }
    })();
    return () => {
      cancelled = true;
      if (target && scene.environment === target.texture) scene.environment = null;
      target?.dispose();
      studio.dispose();
    };
  }, [gl, scene, resolution]);

  return null;
}
