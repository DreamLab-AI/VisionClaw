/**
 * OrbitControls with fly-to on focus/overview cues, and the global tier
 * (with hysteresis) reported to the store, which drives the server View.
 */
import { OrbitControls } from '@react-three/drei';
import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useRef, type ComponentRef } from 'react';
import * as THREE from 'three';
import { dispatch, useCs } from '../store';
import { useScene } from './context';
import { selectTier } from './lod';

interface Flight {
  fromPos: THREE.Vector3;
  toPos: THREE.Vector3;
  fromTarget: THREE.Vector3;
  toTarget: THREE.Vector3;
  t: number;
  duration: number;
}

const ease = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);

export function CameraRig() {
  const { session, frame } = useScene();
  const controls = useRef<ComponentRef<typeof OrbitControls> | null>(null);
  const { camera } = useThree();
  const cue = useCs((s) => s.cameraCue);
  const flight = useRef<Flight | null>(null);
  const tier = useRef(session.store.getState().currentTier);
  const initialised = useRef(false);

  useEffect(() => {
    const c = controls.current;
    if (!c) return;
    const s = session.store.getState();
    const R = frame.sceneRadius;
    const curTarget = c.target.clone();
    const dir = camera.position.clone().sub(curTarget);
    const dist = dir.length() || R;
    dir.normalize();
    let toTarget = curTarget.clone();
    let toDist = dist;
    if (cue.kind === 'overview') {
      toTarget = frame.centre.clone();
      toDist = R * 2.8;
    } else if (cue.kind === 'zoom_in') toDist = dist * 0.55;
    else if (cue.kind === 'zoom_out') toDist = dist * 1.8;
    else if (cue.kind === 'focus') {
      const m = frame.model;
      const i = s.selection !== null && m ? m.index.get(s.selection) : undefined;
      if (i === undefined) return;
      toTarget = new THREE.Vector3(frame.display[i * 3], frame.display[i * 3 + 1], frame.display[i * 3 + 2]);
      const tierIdx = m!.tier[i];
      toDist = R * (tierIdx === 0 ? 0.9 : tierIdx === 1 ? 0.35 : 0.14);
    }
    flight.current = {
      fromPos: camera.position.clone(),
      toPos: toTarget.clone().add(dir.multiplyScalar(toDist)),
      fromTarget: curTarget,
      toTarget,
      t: 0,
      duration: 0.8,
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cue.n]);

  useFrame((_, dt) => {
    const c = controls.current;
    if (!c) return;
    const cam = camera as THREE.PerspectiveCamera;
    const R = frame.sceneRadius;
    if (!initialised.current && frame.model) {
      initialised.current = true;
      c.target.copy(frame.centre);
      cam.position.copy(frame.centre).add(new THREE.Vector3(0.35, 0.45, 1).normalize().multiplyScalar(R * 2.8));
    }
    if (Math.abs(cam.far - R * 30) > R) {
      cam.near = Math.max(0.05, R * 0.0005);
      cam.far = R * 30;
      cam.updateProjectionMatrix();
      c.maxDistance = R * 12;
      c.minDistance = R * 0.005;
    }
    const f = flight.current;
    if (f) {
      f.t = Math.min(1, f.t + dt / f.duration);
      const k = ease(f.t);
      cam.position.lerpVectors(f.fromPos, f.toPos, k);
      c.target.lerpVectors(f.fromTarget, f.toTarget, k);
      if (f.t >= 1) flight.current = null;
    }
    c.update();
    const next = selectTier(cam.position.distanceTo(c.target), R, tier.current);
    if (next !== tier.current) {
      tier.current = next;
      dispatch({ type: 'current_tier', tier: next });
    }
  });

  return <OrbitControls ref={controls as never} makeDefault enableDamping dampingFactor={0.12} rotateSpeed={0.6} zoomSpeed={0.9} />;
}
