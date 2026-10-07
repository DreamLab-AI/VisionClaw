import { useRef, useEffect, useCallback } from 'react';
import { useThree } from '@react-three/fiber';
import * as THREE from 'three';
import { createLogger } from '../../../utils/loggerConfig';
import { robustBounds } from '../../../utils/robustBounds';

const logger = createLogger('CameraAutoFit');

/**
 * Custom event name dispatched/listened for camera fit-to-view requests.
 * UI components outside the R3F Canvas (e.g. "Reset View" button) dispatch
 * this event; the hook inside the Canvas listens and performs the fit.
 */
export const CAMERA_FIT_EVENT = 'visionclaw:camera-fit';

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

  // Percentile bounds: a raw min/max box would fit the layout's far outliers
  // (the same trap the XR adaptive fit hit). See utils/robustBounds.
  const bounds = robustBounds(positions, nodeCount);
  if (!bounds) return;
  const center = new THREE.Vector3(...bounds.centre);
  const count = Math.min(nodeCount, Math.floor(positions.length / 3));

  // Fit the graph's bounding SPHERE against BOTH frustum constraints. Using only
  // maxDim + vertical FOV clips wide graphs on tall (aspect < 1) canvases — the
  // horizontal FOV is the tighter limit there. Derive the horizontal half-FOV
  // from the vertical one and the camera aspect, then back off far enough that
  // the bounding sphere fits inside whichever constraint is tighter.
  const radius = bounds.radius; // sphere radius, floored at 1
  const halfFovV = (camera.fov * (Math.PI / 180)) / 2;
  const aspect = camera.aspect > 0 ? camera.aspect : 1;
  const halfFovH = Math.atan(Math.tan(halfFovV) * aspect);
  const distV = radius / Math.sin(halfFovV);
  const distH = radius / Math.sin(halfFovH);
  const distance = Math.max(distV, distH) * 1.2; // 1.2x padding around the sphere fit

  // Position camera looking from a slight elevation angle for better 3D perception
  const cameraOffset = new THREE.Vector3(0, distance * 0.3, distance);
  camera.position.copy(center).add(cameraOffset);
  camera.lookAt(center);
  camera.updateProjectionMatrix();

  if (controls) {
    controls.target.copy(center);
    controls.update();
  }

  logger.info(
    `Camera auto-fit: center=(${center.x.toFixed(1)}, ${center.y.toFixed(1)}, ${center.z.toFixed(1)}), ` +
    `radius=${radius.toFixed(1)}, aspect=${aspect.toFixed(2)}, distance=${distance.toFixed(1)}, nodes=${count}`
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
