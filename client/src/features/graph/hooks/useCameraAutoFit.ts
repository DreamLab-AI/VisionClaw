import { useRef, useEffect, useCallback } from 'react';
import { useThree } from '@react-three/fiber';
import * as THREE from 'three';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('CameraAutoFit');

/**
 * Custom event name dispatched/listened for camera fit-to-view requests.
 * UI components outside the R3F Canvas (e.g. "Reset View" button) dispatch
 * this event; the hook inside the Canvas listens and performs the fit.
 */
export const CAMERA_FIT_EVENT = 'visionflow:camera-fit';

/**
 * Computes the bounding box of a flat Float32Array of [x,y,z,...] positions
 * and adjusts the camera + controls to frame all nodes with padding.
 */
function fitCameraToBounds(
  camera: THREE.PerspectiveCamera,
  controls: { target: THREE.Vector3; update: () => void } | null,
  positions: Float32Array,
  nodeCount: number,
): void {
  if (nodeCount === 0 || positions.length < 3) return;

  const box = new THREE.Box3();
  const point = new THREE.Vector3();

  const count = Math.min(nodeCount, Math.floor(positions.length / 3));
  for (let i = 0; i < count; i++) {
    const i3 = i * 3;
    point.set(positions[i3], positions[i3 + 1], positions[i3 + 2]);
    box.expandByPoint(point);
  }

  // Guard against degenerate bounding box (all nodes at same point)
  const size = box.getSize(new THREE.Vector3());
  const maxDim = Math.max(size.x, size.y, size.z, 1); // floor at 1 to avoid div-by-zero

  const center = box.getCenter(new THREE.Vector3());

  // Compute ideal camera distance from FOV and bounding sphere
  const fov = camera.fov * (Math.PI / 180);
  const halfFov = fov / 2;
  const distance = (maxDim / (2 * Math.tan(halfFov))) * 1.5; // 1.5x padding

  // Position camera looking from a slight elevation angle for better 3D perception
  const cameraOffset = new THREE.Vector3(0, distance * 0.3, distance);
  camera.position.copy(center).add(cameraOffset);
  camera.lookAt(center);
  camera.updateProjectionMatrix();

  if (controls) {
    controls.target.copy(center);
    controls.update();
  }

  logger.debug(
    `Camera auto-fit: center=(${center.x.toFixed(1)}, ${center.y.toFixed(1)}, ${center.z.toFixed(1)}), ` +
    `maxDim=${maxDim.toFixed(1)}, distance=${distance.toFixed(1)}, nodes=${count}`
  );
}

/**
 * Hook that auto-fits the camera to frame all nodes:
 * - Once on the first batch of non-zero position data (initial load)
 * - On explicit request via the CAMERA_FIT_EVENT custom event
 *
 * Returns a `requestFit` callback for imperative use within the R3F tree.
 */
export function useCameraAutoFit(
  nodePositionsRef: React.RefObject<Float32Array | null>,
  nodeCount: number,
): { requestFit: () => void } {
  const { camera, controls } = useThree();
  const hasAutoFittedRef = useRef(false);
  const pendingFitRef = useRef(false);

  const performFit = useCallback(() => {
    const positions = nodePositionsRef.current;
    if (!positions || positions.length === 0 || nodeCount === 0) return;

    if (camera instanceof THREE.PerspectiveCamera) {
      fitCameraToBounds(
        camera,
        controls as { target: THREE.Vector3; update: () => void } | null,
        positions,
        nodeCount,
      );
    }
  }, [camera, controls, nodePositionsRef, nodeCount]);

  // Listen for explicit fit requests from outside the Canvas
  useEffect(() => {
    const handler = () => {
      // Reset the auto-fit flag so the next position update triggers a fit,
      // or fit immediately if positions are already available
      hasAutoFittedRef.current = false;
      pendingFitRef.current = true;
    };

    window.addEventListener(CAMERA_FIT_EVENT, handler);
    return () => window.removeEventListener(CAMERA_FIT_EVENT, handler);
  }, []);

  // Called from useFrame in GraphManager — checks if a fit is needed
  const requestFit = useCallback(() => {
    // Auto-fit on first real position data
    if (!hasAutoFittedRef.current && nodePositionsRef.current && nodeCount > 0) {
      hasAutoFittedRef.current = true;
      performFit();
      return;
    }

    // Explicit fit request (from event or button)
    if (pendingFitRef.current && nodePositionsRef.current && nodeCount > 0) {
      pendingFitRef.current = false;
      hasAutoFittedRef.current = true;
      performFit();
    }
  }, [performFit, nodePositionsRef, nodeCount]);

  return { requestFit };
}
