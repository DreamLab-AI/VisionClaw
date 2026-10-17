/** The R3F canvas: frame driver (derived state first), then every layer. */
import { Canvas, useFrame, useThree } from '@react-three/fiber';
import { useMemo, useRef } from 'react';
import * as THREE from 'three';
import type { Session } from '../session';
import { dispatch } from '../store';
import { CameraRig } from './Camera';
import { SceneContext, useScene } from './context';
import { Edges } from './Edges';
import { FrameState } from './frameState';
import { Hulls } from './Hulls';
import { Labels } from './Labels';
import { BG, Nodes } from './Nodes';
import { Presence } from './Presence';

function FrameDriver() {
  const { session, frame } = useScene();
  const { camera, size } = useThree();
  const fps = useRef({ ema: 60, last: 0 });
  // Negative priority: runs before every layer's useFrame without taking over rendering.
  useFrame((state, dt) => {
    const controls = state.controls as unknown as { target?: THREE.Vector3 } | null;
    const target = controls?.target ?? frame.centre;
    frame.update(session.store.getState(), session.pipeline.buffers, camera as THREE.PerspectiveCamera, size.height, Math.min(dt, 0.1), target);
    session.sceneRadius = frame.sceneRadius;
    if (dt > 0) fps.current.ema = fps.current.ema * 0.92 + (1 / dt) * 0.08;
    if (state.clock.elapsedTime - fps.current.last > 0.5) {
      fps.current.last = state.clock.elapsedTime;
      dispatch({ type: 'hud', patch: { fps: Math.round(fps.current.ema) } });
    }
  }, -1);
  return null;
}

export function SceneView({ session }: { session: Session }) {
  const frame = useMemo(() => new FrameState(), []);
  const ctx = useMemo(() => ({ session, frame }), [session, frame]);
  return (
    <Canvas
      className="cs-canvas"
      dpr={[1, 2]}
      gl={{ antialias: true, powerPreference: 'high-performance', alpha: false }}
      camera={{ fov: 50, near: 0.5, far: 20000, position: [0, 0, 1200] }}
      onCreated={({ gl, scene }) => {
        gl.setClearColor(BG, 1);
        scene.background = BG.clone();
      }}
      frameloop="always"
    >
      <SceneContext.Provider value={ctx}>
        <FrameDriver />
        <CameraRig />
        <Hulls />
        <Edges />
        <Nodes />
        <Presence />
        <Labels />
      </SceneContext.Provider>
    </Canvas>
  );
}
