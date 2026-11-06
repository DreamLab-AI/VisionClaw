import { useRef, useEffect, useCallback } from 'react';
import { useThree } from '@react-three/fiber';
import * as THREE from 'three';
import { createLogger } from '../../../utils/loggerConfig';
import { robustBounds, type RobustBounds } from '../../../utils/robustBounds';
import { useSettingsStore } from '../../../store/settingsStore';
import { useMemoryCloudStore } from '../../visualisation/memoryCloud/memoryCloudInstance';
import {
  sceneFitBounds,
  createSettleTrigger,
  cloudFitKey,
  CLOUD_SETTLE_MS,
  unoccludedRect,
  fitPose,
  fitIncludesCloud,
  normaliseFitTarget,
  type CloudFitInput,
  type FitTarget,
  type Rect,
} from '../utils/sceneFitBounds';

const logger = createLogger('CameraAutoFit');

/**
 * Custom event name dispatched/listened for camera fit-to-view requests.
 * UI components outside the R3F Canvas (e.g. "Reset View" button) dispatch
 * this event; the hook inside the Canvas listens and performs the fit.
 */
export const CAMERA_FIT_EVENT = 'visionclaw:camera-fit';

/** `detail` of a CAMERA_FIT_EVENT. No detail (or no target) = the stored `graphView.fitTarget`. */
export interface CameraFitRequest {
  target?: FitTarget;
}

/**
 * Ask the desktop camera to refit (ADR-2180), from anywhere: a dock Fit button,
 * a keyboard shortcut, a menu. `target` is a one-shot override of the stored
 * `visualisation.graphView.fitTarget`; it does not change the setting (the
 * menus own persisting a chosen scope). Safe outside the Canvas and in SSR.
 */
export function requestFit(target?: FitTarget): void {
  if (typeof window === 'undefined') return;
  const detail: CameraFitRequest = target ? { target: normaliseFitTarget(target) } : {};
  window.dispatchEvent(new CustomEvent<CameraFitRequest>(CAMERA_FIT_EVENT, { detail }));
}

/** The stored fit scope (ADR-2180 D2), defaulting to the knowledge body. */
export function storedFitTarget(): FitTarget {
  return normaliseFitTarget(useSettingsStore.getState().get<string>('visualisation.graphView.fitTarget'));
}

/** attribute marking a DOM overlay the fit keeps the scene out from under */
export const SCENE_OCCLUDER_ATTR = 'data-scene-occluder';

/** canvas-relative rects of the visible overlays marked SCENE_OCCLUDER_ATTR */
function occluderRects(canvas: HTMLElement): Rect[] {
  if (typeof document === 'undefined') return [];
  const c = canvas.getBoundingClientRect();
  const out: Rect[] = [];
  document.querySelectorAll(`[${SCENE_OCCLUDER_ATTR}]`).forEach((el) => {
    const b = el.getBoundingClientRect();
    if (b.width <= 0 || b.height <= 0) return;
    out.push({ left: b.left - c.left, top: b.top - c.top, right: b.right - c.left, bottom: b.bottom - c.top });
  });
  return out;
}

/**
 * Adjusts the camera + controls to frame `bounds` (from `sceneFitBounds`:
 * both graphs and the memory cloud) inside the part of the canvas that no
 * overlay covers (`unoccludedRect`), with the exact sub-frustum fit of
 * `fitPose`. The far plane grows when the fitted scene would cross it.
 */
function fitCameraToBounds(
  camera: THREE.PerspectiveCamera,
  controls: { target: THREE.Vector3; update: () => void } | null,
  canvas: HTMLElement,
  bounds: RobustBounds,
  count: number,
): void {
  const view = { width: canvas.clientWidth || 1, height: canvas.clientHeight || 1 };
  const free = unoccludedRect(view, occluderRects(canvas));
  const aspect = camera.aspect > 0 ? camera.aspect : view.width / view.height;
  const pose = fitPose(bounds, { fovDeg: camera.fov, aspect }, view, free);
  const target = new THREE.Vector3(...pose.target);
  camera.position.set(...pose.position);
  camera.lookAt(target);
  const reach = camera.position.distanceTo(new THREE.Vector3(...bounds.centre)) + 2 * bounds.radius;
  if (camera.far < reach) camera.far = reach;
  camera.updateProjectionMatrix();

  if (controls) {
    controls.target.copy(target);
    controls.update();
  }

  logger.info(
    `Camera auto-fit: centre=(${bounds.centre.map((v) => v.toFixed(1)).join(', ')}), ` +
    `radius=${bounds.radius.toFixed(1)}, free=${free.left.toFixed(0)},${free.top.toFixed(0)}–${free.right.toFixed(0)},${free.bottom.toFixed(0)}, ` +
    `distance=${camera.position.distanceTo(target).toFixed(1)}, nodes=${count}`
  );
}

/** The memory cloud's fit input when it is on and loaded, else null. */
export function cloudFitInput(): CloudFitInput | null {
  const settings = useSettingsStore.getState();
  if (!settings.get<boolean>('visualisation.embeddingCloud.enabled')) return null;
  const snapshot = useMemoryCloudStore.getState().snapshot;
  if (!snapshot) return null;
  const bounds = robustBounds(snapshot.positions, snapshot.count);
  if (!bounds) return null;
  return { bounds, cloudScale: settings.get<number>('visualisation.embeddingCloud.cloudScale') ?? 5 };
}

/**
 * Hook that auto-fits the camera to the scope `graphView.fitTarget` names
 * (ADR-2180; default: the knowledge body):
 * - Once on the first batch of non-zero position data (initial load)
 * - On explicit request via the CAMERA_FIT_EVENT custom event (`requestFit`),
 *   optionally with a one-shot target override
 * - Once when the stored fit scope changes
 * - Under the 'scene' scope only: once the memory cloud's framing input
 *   settles (it turns on, a snapshot loads, cloudScale changes). Under the
 *   other scopes toggling the cloud leaves the camera alone (ADR-2180 D3).
 *
 * Returns `tickFit`, called once per frame from GraphManager's useFrame.
 */
export function useCameraAutoFit(
  nodePositionsRef: React.RefObject<Float32Array | null>,
  nodeCount: number,
): { tickFit: () => void } {
  const { camera, controls, gl } = useThree();
  const hasAutoFittedRef = useRef(false);
  const pendingFitRef = useRef(false);
  // One-shot target from a CAMERA_FIT_EVENT detail; null = the stored scope.
  const pendingTargetRef = useRef<FitTarget | null>(null);
  const fitTarget = normaliseFitTarget(useSettingsStore((s) => s.get<string>('visualisation.graphView.fitTarget')));
  const fitTargetRef = useRef<FitTarget>(fitTarget);
  fitTargetRef.current = fitTarget;
  const prevFitTargetRef = useRef<FitTarget>(fitTarget);
  const cloudEnabled = useSettingsStore((s) => s.get<boolean>('visualisation.embeddingCloud.enabled')) === true;
  const cloudScale = useSettingsStore((s) => s.get<number>('visualisation.embeddingCloud.cloudScale')) ?? 5;
  const snapshotId = useMemoryCloudStore((s) => s.snapshot?.snapshotId);
  const cloudKeyRef = useRef('');
  cloudKeyRef.current = cloudFitKey(cloudEnabled, snapshotId, cloudScale);
  const settleRef = useRef(createSettleTrigger(CLOUD_SETTLE_MS));

  const performFit = useCallback((target: FitTarget) => {
    const positions = nodePositionsRef.current;
    if (!positions || positions.length === 0 || nodeCount === 0) return;
    const count = Math.min(nodeCount, Math.floor(positions.length / 3));
    const bounds = sceneFitBounds(positions, count, fitIncludesCloud(target) ? cloudFitInput() : null, target);
    if (!bounds) return;

    if (camera instanceof THREE.PerspectiveCamera) {
      fitCameraToBounds(
        camera,
        controls as { target: THREE.Vector3; update: () => void } | null,
        gl.domElement,
        bounds,
        count,
      );
    }
  }, [camera, controls, gl, nodePositionsRef, nodeCount]);

  // Listen for explicit fit requests from outside the Canvas
  useEffect(() => {
    const handler = (e: Event) => {
      // Reset the auto-fit flag so the next position update triggers a fit,
      // or fit immediately if positions are already available
      const detail = (e as CustomEvent<CameraFitRequest | undefined>).detail;
      pendingTargetRef.current = detail?.target ? normaliseFitTarget(detail.target) : null;
      hasAutoFittedRef.current = false;
      pendingFitRef.current = true;
    };

    window.addEventListener(CAMERA_FIT_EVENT, handler);
    return () => window.removeEventListener(CAMERA_FIT_EVENT, handler);
  }, []);

  // Called from useFrame in GraphManager — checks if a fit is needed
  const tickFit = useCallback(() => {
    // The settle trigger always sees the key (so its baseline stays current),
    // but only the whole-scene scope refits on it (ADR-2180 D3).
    const settled = settleRef.current.update(cloudKeyRef.current, performance.now());
    if (settled && fitIncludesCloud(fitTargetRef.current)) pendingFitRef.current = true;

    // The user picked another scope: frame it once.
    if (prevFitTargetRef.current !== fitTargetRef.current) {
      prevFitTargetRef.current = fitTargetRef.current;
      pendingFitRef.current = true;
    }

    // Auto-fit on first real position data
    if (!hasAutoFittedRef.current && nodePositionsRef.current && nodeCount > 0) {
      hasAutoFittedRef.current = true;
      pendingFitRef.current = false;
      const target = pendingTargetRef.current ?? fitTargetRef.current;
      pendingTargetRef.current = null;
      performFit(target);
      return;
    }

    // Explicit fit request (from event or button)
    if (pendingFitRef.current && nodePositionsRef.current && nodeCount > 0) {
      pendingFitRef.current = false;
      hasAutoFittedRef.current = true;
      const target = pendingTargetRef.current ?? fitTargetRef.current;
      pendingTargetRef.current = null;
      performFit(target);
    }
  }, [performFit, nodePositionsRef, nodeCount]);

  return { tickFit };
}
