/**
 * SpacePilot → camera, applied from inside `CameraRig`'s frame loop so the
 * puck and the fly-to animation never fight (the puck wins and cancels it).
 *
 * `useSpacePilotMotion` returns a per-frame function the rig calls just before
 * `controls.update()`. It also binds the puck's buttons to the same actions the
 * palette and voice grammar expose:
 *
 *   1 Overview · 2 Zoom in · 3 Zoom out · 4 Back · 5 Tier auto · 6 Tier L0 ·
 *   7 Tier L1 · 8 Tier L2 · 9 Clear selection · 10 Toggle hide tests
 */
import { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import { dispatch } from '../store';
import { cameraDelta, DEFAULT_MOTION, isIdle, type MotionConfig } from './spaceMotion';
import { spacePilot } from './spacePilot';

const BUTTON_ACTIONS: Record<number, () => void> = {
  1: () => dispatch({ type: 'camera', kind: 'overview' }),
  2: () => dispatch({ type: 'camera', kind: 'zoom_in' }),
  3: () => dispatch({ type: 'camera', kind: 'zoom_out' }),
  4: () => dispatch({ type: 'back' }),
  5: () => dispatch({ type: 'tier_mode', mode: 'auto' }),
  6: () => dispatch({ type: 'tier_mode', mode: 'l0' }),
  7: () => dispatch({ type: 'tier_mode', mode: 'l1' }),
  8: () => dispatch({ type: 'tier_mode', mode: 'l2' }),
  9: () => dispatch({ type: 'select', id: null }),
  10: () => dispatch({ type: 'toggle_hide_tests' }),
};

/** Bind puck buttons to store actions for the lifetime of the component. */
export function useSpacePilotButtons(): void {
  useEffect(() => {
    const dev = spacePilot();
    dev.onButton = (i) => BUTTON_ACTIONS[i]?.();
    return () => {
      dev.onButton = () => {};
    };
  }, []);
}

export interface OrbitLike {
  target: THREE.Vector3;
  minDistance: number;
  maxDistance: number;
}

/**
 * Per-frame puck application. Returns `true` when the puck moved the camera
 * this frame, so the caller can drop any fly-to in progress.
 */
export function useSpacePilotMotion(motion?: Partial<MotionConfig>) {
  const cfg = useMemo<MotionConfig>(() => ({ ...DEFAULT_MOTION, ...motion }), [motion]);
  const offset = useRef(new THREE.Vector3());
  const spherical = useRef(new THREE.Spherical());
  const right = useRef(new THREE.Vector3());
  const up = useRef(new THREE.Vector3());

  return (cam: THREE.PerspectiveCamera, c: OrbitLike, dt: number): boolean => {
    const dev = spacePilot();
    if (dev.state !== 'connected') return false;
    const s = dev.sample;
    if (isIdle(s, cfg)) return false;
    const d = cameraDelta(s, dt, cfg);
    applyDelta(cam, c, d, offset.current, spherical.current, right.current, up.current);
    return true;
  };
}

/** Apply a delta to a camera orbiting `c.target`. Exported for tests. */
export function applyDelta(
  cam: THREE.PerspectiveCamera,
  c: OrbitLike,
  d: ReturnType<typeof cameraDelta>,
  offset = new THREE.Vector3(),
  spherical = new THREE.Spherical(),
  right = new THREE.Vector3(),
  up = new THREE.Vector3(),
): void {
  offset.copy(cam.position).sub(c.target);
  spherical.setFromVector3(offset);
  spherical.theta += d.azimuth;
  spherical.phi = THREE.MathUtils.clamp(spherical.phi - d.elevation, 0.05, Math.PI - 0.05);
  spherical.radius = THREE.MathUtils.clamp(spherical.radius * d.dollyFactor, c.minDistance || 0.01, c.maxDistance || Infinity);
  spherical.makeSafe();
  offset.setFromSpherical(spherical);

  if (d.panRight !== 0 || d.panUp !== 0) {
    const dist = spherical.radius;
    cam.updateMatrix();
    right.setFromMatrixColumn(cam.matrix, 0).multiplyScalar(d.panRight * dist);
    up.setFromMatrixColumn(cam.matrix, 1).multiplyScalar(d.panUp * dist);
    c.target.add(right).add(up);
  }
  cam.position.copy(c.target).add(offset);
  cam.lookAt(c.target);
}
