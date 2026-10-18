/**
 * All nodes as ONE InstancedMesh of camera-facing quads with SDF-sphere
 * shading. Instance attributes alias the FrameState arrays where possible
 * (positions are zero-copy), so a frame costs one buffer upload.
 */
import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import { useCs, dispatch } from '../store';
import { useScene } from './context';
import { NodePicker } from './Picking';
import { NODE_FRAGMENT, NODE_VERTEX } from './shaders';

export const BG = new THREE.Color('#0b0e14');

function baseSize(tier: number, importance: number, sizeNorm: number): number {
  if (tier === 0) return 9 + 12 * Math.sqrt(sizeNorm);
  if (tier === 1) return 3 + 3 * importance;
  return 1.1 + 2.6 * importance;
}

/** Emissive strength from `?emissive=` (0..1), default 0.7. */
function readEmissive(): number {
  try {
    const raw = new URLSearchParams(window.location.search).get('emissive');
    if (raw === null) return 0.7;
    const v = Number(raw);
    return Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 0.7;
  } catch {
    return 0.7;
  }
}

export function Nodes() {
  const { session, frame } = useScene();
  const model = useCs((s) => s.model);
  const { gl, camera, size } = useThree();
  const lastVersion = useRef(-1);
  const lastColourFrame = useRef(0);

  const uniforms = useMemo(
    () => ({
      uScale: { value: 1 },
      uMinPx: { value: 1.6 },
      uViewportH: { value: 1080 },
      uProjScale: { value: 1000 },
      uBg: { value: new THREE.Vector3(BG.r, BG.g, BG.b) },
      uTime: { value: 0 },
      // Self-lit community colour (operator request 2026-10-18); override with ?emissive=0..1.
      uEmissive: { value: readEmissive() },
    }),
    [],
  );
  const material = useMemo(
    () => new THREE.ShaderMaterial({ vertexShader: NODE_VERTEX, fragmentShader: NODE_FRAGMENT, uniforms, depthWrite: true, depthTest: true, transparent: false }),
    [uniforms],
  );
  const picker = useMemo(() => new NodePicker(uniforms), [uniforms]);

  // Geometry with instance attributes, rebuilt when the index space changes.
  const geometry = useMemo(() => {
    const g = new THREE.BufferGeometry();
    const quad = new THREE.PlaneGeometry(1, 1);
    g.index = quad.index;
    g.setAttribute('position', quad.getAttribute('position'));
    const n = Math.max(1, model?.n ?? 1);
    g.setAttribute('iPos', new THREE.InstancedBufferAttribute(new Float32Array(n * 3), 3).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('iColor', new THREE.InstancedBufferAttribute(new Float32Array(n * 3), 3).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('iParams', new THREE.InstancedBufferAttribute(new Float32Array(n * 4), 4).setUsage(THREE.DynamicDrawUsage));
    g.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1e9);
    lastVersion.current = -1;
    return g;
  }, [model]);

  const maxSize = useMemo(() => {
    let mx = 1;
    if (model) for (let i = 0; i < model.n; i++) if (model.tier[i] === 0 && model.snapshot.nodes[i].size > mx) mx = model.snapshot.nodes[i].size;
    return mx;
  }, [model]);

  useEffect(() => {
    picker.attach(geometry, model?.n ?? 0);
    return () => geometry.dispose();
  }, [geometry, picker, model]);

  useEffect(() => () => {
    material.dispose();
    picker.dispose();
  }, [material, picker]);

  // Pointer interaction via GPU picking.
  useEffect(() => {
    const el = gl.domElement;
    let down: { x: number; y: number } | null = null;
    let lastHover = 0;
    const local = (e: PointerEvent | MouseEvent) => {
      const r = el.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top, w: r.width, h: r.height };
    };
    const pickAt = (e: PointerEvent | MouseEvent) => {
      const p = local(e);
      const i = picker.pick(gl, camera as THREE.PerspectiveCamera, p.x, p.y, p.w, p.h);
      const m = session.store.getState().model;
      if (!m || i < 0 || i >= m.n || frame.weight[i] <= 0.05) return null;
      return m.ids[i];
    };
    const onMove = (e: PointerEvent) => {
      if (e.buttons !== 0) return;
      const now = performance.now();
      if (now - lastHover < 50) return;
      lastHover = now;
      dispatch({ type: 'hover', id: pickAt(e) });
    };
    const onDown = (e: PointerEvent) => {
      down = { x: e.clientX, y: e.clientY };
    };
    const onUp = (e: PointerEvent) => {
      if (!down || Math.hypot(e.clientX - down.x, e.clientY - down.y) > 4 || e.button !== 0) {
        down = null;
        return;
      }
      down = null;
      const id = pickAt(e);
      if (id !== null) dispatch({ type: 'select', id, fly: false });
    };
    const onDbl = (e: MouseEvent) => {
      const id = pickAt(e);
      if (id !== null) dispatch({ type: 'select', id, fly: true });
    };
    const onLeave = () => dispatch({ type: 'hover', id: null });
    el.addEventListener('pointermove', onMove);
    el.addEventListener('pointerdown', onDown);
    el.addEventListener('pointerup', onUp);
    el.addEventListener('dblclick', onDbl);
    el.addEventListener('pointerleave', onLeave);
    return () => {
      el.removeEventListener('pointermove', onMove);
      el.removeEventListener('pointerdown', onDown);
      el.removeEventListener('pointerup', onUp);
      el.removeEventListener('dblclick', onDbl);
      el.removeEventListener('pointerleave', onLeave);
    };
  }, [gl, camera, picker, session, frame]);

  useFrame((state) => {
    const m = frame.model;
    if (!m || m !== model) return;
    const cam = camera as THREE.PerspectiveCamera;
    uniforms.uTime.value = state.clock.elapsedTime;
    uniforms.uScale.value = frame.sceneRadius / 400;
    uniforms.uViewportH.value = size.height;
    uniforms.uProjScale.value = size.height / 2 / Math.tan((cam.fov * Math.PI) / 360);
    const n = m.n;
    const buffers = session.pipeline.buffers;
    const colAttr = geometry.getAttribute('iColor') as THREE.InstancedBufferAttribute;
    // Colours follow the live (stable-matched) community; refresh twice a second.
    if (state.clock.elapsedTime - lastColourFrame.current > 0.5 || lastVersion.current < 0) {
      lastColourFrame.current = state.clock.elapsedTime;
      const col = colAttr.array as Float32Array;
      for (let i = 0; i < n; i++) {
        const c = frame.colour(m.tier[i] === 0 ? m.community[i] : buffers.community[i]);
        col[i * 3] = c[0];
        col[i * 3 + 1] = c[1];
        col[i * 3 + 2] = c[2];
      }
      colAttr.needsUpdate = true;
    }
    if (frame.version === lastVersion.current) return;
    lastVersion.current = frame.version;
    const posAttr = geometry.getAttribute('iPos') as THREE.InstancedBufferAttribute;
    (posAttr.array as Float32Array).set(frame.display.subarray(0, n * 3));
    posAttr.needsUpdate = true;
    const parAttr = geometry.getAttribute('iParams') as THREE.InstancedBufferAttribute;
    const par = parAttr.array as Float32Array;
    for (let i = 0; i < n; i++) {
      const w = frame.weight[i];
      const imp = buffers.centrality[i] || m.importance[i];
      const sz = w > 0.01 ? baseSize(m.tier[i], imp, m.snapshot.nodes[i].size / maxSize) * (0.25 + 0.75 * w) : 0;
      par[i * 4] = sz;
      par[i * 4 + 1] = frame.heat[i];
      par[i * 4 + 2] = frame.alpha[i];
      par[i * 4 + 3] = frame.flags[i];
    }
    parAttr.needsUpdate = true;
  });

  if (!model || model.n === 0) return null;
  return <instancedMesh key={model.n} args={[geometry, material, model.n]} frustumCulled={false} raycast={() => null} />;
}
