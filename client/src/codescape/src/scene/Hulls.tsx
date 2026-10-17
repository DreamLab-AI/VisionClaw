/**
 * Community hulls at L0/L1: a 3D convex hull (three's ConvexGeometry) over a
 * bounded sample of each large community's displayed file/aggregate positions,
 * rebuilt about once a second, fading out as symbols open (L2).
 */
import { useFrame } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import { ConvexGeometry } from 'three/examples/jsm/geometries/ConvexGeometry.js';
import { useCs } from '../store';
import { useScene } from './context';
import { tierWeights } from './lod';

const MAX_HULLS = 16;
const MAX_POINTS = 160;

export function Hulls() {
  const { session, frame } = useScene();
  const model = useCs((s) => s.model);
  const isolated = useCs((s) => s.filter.community);
  const group = useMemo(() => new THREE.Group(), []);
  const lastBuild = useRef(-10);
  const materials = useRef(new Map<number, THREE.MeshBasicMaterial>());

  useEffect(() => () => {
    group.children.forEach((c) => (c as THREE.Mesh).geometry.dispose());
    materials.current.forEach((m) => m.dispose());
  }, [group]);

  useFrame((state) => {
    const m = frame.model;
    if (!m || m !== model) return;
    const w = tierWeights(frame.globalZoom);
    const opacity = (w[0] + w[1]) * (1 - w[2]) * (frame.hasFocus ? 0.4 : 1);
    group.visible = opacity > 0.02;
    materials.current.forEach((mat) => (mat.opacity = 0.075 * opacity));
    if (!group.visible || state.clock.elapsedTime - lastBuild.current < 1.0) return;
    lastBuild.current = state.clock.elapsedTime;
    const community = session.pipeline.buffers.community;
    const members = new Map<number, number[]>();
    for (let i = 0; i < m.n; i++) {
      if (m.tier[i] !== 1 || frame.visible[i] === 0) continue;
      const c = community[i];
      if (isolated !== null && c !== isolated) continue;
      let l = members.get(c);
      if (!l) members.set(c, (l = []));
      l.push(i);
    }
    const top = [...members.entries()].filter(([, l]) => l.length >= 4).sort((a, b) => b[1].length - a[1].length).slice(0, MAX_HULLS);
    group.children.forEach((c) => (c as THREE.Mesh).geometry.dispose());
    group.clear();
    for (const [c, list] of top) {
      const step = Math.max(1, Math.floor(list.length / MAX_POINTS));
      const pts: THREE.Vector3[] = [];
      for (let k = 0; k < list.length; k += step) {
        const i = list[k];
        pts.push(new THREE.Vector3(frame.display[i * 3], frame.display[i * 3 + 1], frame.display[i * 3 + 2]));
      }
      let geom: ConvexGeometry;
      try {
        geom = new ConvexGeometry(pts);
      } catch {
        continue; // coplanar / degenerate sample
      }
      let mat = materials.current.get(c);
      if (!mat) {
        const rgb = frame.colour(c);
        mat = new THREE.MeshBasicMaterial({ color: new THREE.Color(rgb[0], rgb[1], rgb[2]), transparent: true, opacity: 0.075 * opacity, depthWrite: false, side: THREE.DoubleSide });
        materials.current.set(c, mat);
      }
      const mesh = new THREE.Mesh(geom, mat);
      mesh.renderOrder = 0;
      mesh.raycast = () => undefined;
      group.add(mesh);
    }
  });

  return <primitive object={group} />;
}
