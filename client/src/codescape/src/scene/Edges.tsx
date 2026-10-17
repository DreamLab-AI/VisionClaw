/**
 * Tiered edges, never all at once (decision record §10; no FDEB):
 *  - tier 0: one quadratic-Bézier ribbon per community pair, width by edge count;
 *  - tier 1: file-level imports;
 *  - tier 2: calls inside the focus k-hop neighbourhood (≤ 20k segments) plus query highlights.
 * Tiers 1+2 share ONE LineSegments whose draw range follows the live segment count.
 */
import { useFrame } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import type { GraphModel } from '../graph';
import { useCs } from '../store';
import { useScene } from './context';
import { tierWeights } from './lod';

export const MAX_CALL_SEGMENTS = 20_000;
export const MAX_IMPORT_SEGMENTS = 30_000;
const CURVE_STEPS = 16;
const MAX_PAIRS = 400;

export interface CommunityPair {
  a: number;
  b: number;
  count: number;
}

/** Count cross-community dependency edges (calls + imports) per unordered community pair, top `max`. */
export function communityPairs(m: GraphModel, community: ArrayLike<number>, max = MAX_PAIRS): CommunityPair[] {
  const counts = new Map<number, CommunityPair>();
  const add = (s: number, t: number) => {
    const ca = community[s];
    const cb = community[t];
    if (ca === cb) return;
    const a = Math.min(ca, cb);
    const b = Math.max(ca, cb);
    const key = a * 0x100000 + b;
    const cur = counts.get(key);
    if (cur) cur.count++;
    else counts.set(key, { a, b, count: 1 });
  };
  for (let k = 0; k < m.calls.length; k += 2) add(m.calls[k], m.calls[k + 1]);
  for (let k = 0; k < m.fileImports.length; k += 2) add(m.fileImports[k], m.fileImports[k + 1]);
  return [...counts.values()].sort((x, y) => y.count - x.count).slice(0, max);
}

const RIBBON_VERTEX = /* glsl */ `
attribute vec3 aNext;
attribute float aSide;
attribute float aWidth;
attribute vec4 aColor;
uniform vec2 uViewport;
uniform float uOpacity;
varying vec4 vColor;
varying float vSide;
void main() {
  vec4 a = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  vec4 b = projectionMatrix * modelViewMatrix * vec4(aNext, 1.0);
  vec2 sa = a.xy / a.w * uViewport * 0.5;
  vec2 sb = b.xy / b.w * uViewport * 0.5;
  vec2 dir = normalize(sb - sa + vec2(1e-5, 0.0));
  vec2 nrm = vec2(-dir.y, dir.x);
  a.xy += nrm * aSide * aWidth * 0.5 / (uViewport * 0.5) * a.w;
  gl_Position = a;
  vColor = vec4(aColor.rgb, aColor.a * uOpacity);
  vSide = aSide;
}
`;

const RIBBON_FRAGMENT = /* glsl */ `
varying vec4 vColor;
varying float vSide;
void main() {
  float edge = 1.0 - smoothstep(0.55, 1.0, abs(vSide));
  gl_FragColor = vec4(vColor.rgb, vColor.a * edge);
}
`;

const LINE_VERTEX = /* glsl */ `
attribute vec4 aColor;
varying vec4 vColor;
void main() {
  vColor = aColor;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`;
const LINE_FRAGMENT = /* glsl */ `
varying vec4 vColor;
void main() { gl_FragColor = vColor; }
`;

function CommunityRibbons() {
  const { session, frame } = useScene();
  const model = useCs((s) => s.model);
  const pairsRef = useRef<CommunityPair[]>([]);
  const lastBuild = useRef(-1);
  const lastFill = useRef(-1);
  const material = useMemo(
    () =>
      new THREE.ShaderMaterial({
        vertexShader: RIBBON_VERTEX,
        fragmentShader: RIBBON_FRAGMENT,
        uniforms: { uViewport: { value: new THREE.Vector2(1920, 1080) }, uOpacity: { value: 1 } },
        transparent: true,
        depthWrite: false,
        blending: THREE.AdditiveBlending,
      }),
    [],
  );
  // Two triangles per curve step: vertices (a,-1)(a,+1)(b,-1)(b,+1), indexed.
  const geometry = useMemo(() => {
    const g = new THREE.BufferGeometry();
    const verts = MAX_PAIRS * CURVE_STEPS * 4;
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(verts * 3), 3).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('aNext', new THREE.BufferAttribute(new Float32Array(verts * 3), 3).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('aSide', new THREE.BufferAttribute(new Float32Array(verts), 1));
    g.setAttribute('aWidth', new THREE.BufferAttribute(new Float32Array(verts), 1).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('aColor', new THREE.BufferAttribute(new Float32Array(verts * 4), 4).setUsage(THREE.DynamicDrawUsage));
    const side = g.getAttribute('aSide').array as Float32Array;
    const idx = new Uint32Array(MAX_PAIRS * CURVE_STEPS * 6);
    for (let q = 0; q < MAX_PAIRS * CURVE_STEPS; q++) {
      const v = q * 4;
      side[v] = -1;
      side[v + 1] = 1;
      side[v + 2] = -1;
      side[v + 3] = 1;
      idx.set([v, v + 1, v + 2, v + 1, v + 3, v + 2], q * 6);
    }
    g.setIndex(new THREE.BufferAttribute(idx, 1));
    g.setDrawRange(0, 0);
    g.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1e9);
    return g;
  }, []);
  useEffect(() => () => {
    geometry.dispose();
    material.dispose();
  }, [geometry, material]);

  useFrame((state) => {
    const m = frame.model;
    if (!m || m !== model) return;
    (material.uniforms.uViewport.value as THREE.Vector2).set(state.size.width, state.size.height);
    const w = tierWeights(frame.globalZoom);
    const opacity = (w[0] + 0.35 * w[1]) * (1 - w[2]) * (frame.hasFocus ? 0.35 : 1);
    material.uniforms.uOpacity.value = opacity;
    if (opacity < 0.01) {
      geometry.setDrawRange(0, 0);
      return;
    }
    const buffers = session.pipeline.buffers;
    // Rebuild the pair list when the model changes or every ~3 s (communities are recomputed server-side).
    const now = state.clock.elapsedTime;
    if (frame.modelVersion !== lastBuild.current || now - (lastFill.current < 0 ? 0 : lastFill.current) > 3) {
      lastBuild.current = frame.modelVersion;
      lastFill.current = now;
      pairsRef.current = communityPairs(m, buffers.community.subarray(0, m.n));
    }
    if (frame.version % 4 !== 0 && geometry.drawRange.count > 0) return;
    // Community centroids from displayed positions (filters respected).
    const cent = new Map<number, [number, number, number, number]>();
    for (let i = 0; i < m.n; i++) {
      if (m.tier[i] === 0 || frame.visible[i] === 0) continue;
      const c = buffers.community[i];
      let e = cent.get(c);
      if (!e) cent.set(c, (e = [0, 0, 0, 0]));
      e[0] += frame.display[i * 3];
      e[1] += frame.display[i * 3 + 1];
      e[2] += frame.display[i * 3 + 2];
      e[3]++;
    }
    const pos = geometry.getAttribute('position').array as Float32Array;
    const nxt = geometry.getAttribute('aNext').array as Float32Array;
    const wid = geometry.getAttribute('aWidth').array as Float32Array;
    const col = geometry.getAttribute('aColor').array as Float32Array;
    const pairs = pairsRef.current;
    const maxCount = pairs.length ? pairs[0].count : 1;
    const cx = frame.centre.x;
    const cy = frame.centre.y;
    const cz = frame.centre.z;
    const p = new THREE.Vector3();
    const q = new THREE.Vector3();
    const A = new THREE.Vector3();
    const B = new THREE.Vector3();
    const C = new THREE.Vector3();
    const curve = new THREE.QuadraticBezierCurve3(A, C, B);
    let quads = 0;
    for (const pr of pairs) {
      const ea = cent.get(pr.a);
      const eb = cent.get(pr.b);
      if (!ea || !eb || !ea[3] || !eb[3]) continue;
      A.set(ea[0] / ea[3], ea[1] / ea[3], ea[2] / ea[3]);
      B.set(eb[0] / eb[3], eb[1] / eb[3], eb[2] / eb[3]);
      // Control point pulled toward the centre: curves bundle visually through the core, no FDEB.
      C.set((A.x + B.x) * 0.5 * 0.45 + cx * 0.55, (A.y + B.y) * 0.5 * 0.45 + cy * 0.55, (A.z + B.z) * 0.5 * 0.45 + cz * 0.55);
      const width = 1.2 + 9 * Math.sqrt(pr.count / maxCount);
      const ca = frame.colour(pr.a);
      const cb = frame.colour(pr.b);
      const alpha = 0.18 + 0.5 * Math.sqrt(pr.count / maxCount);
      for (let s = 0; s < CURVE_STEPS; s++) {
        const t0 = s / CURVE_STEPS;
        const t1 = (s + 1) / CURVE_STEPS;
        curve.getPoint(t0, p);
        curve.getPoint(t1, q);
        const v = quads * 4;
        for (let k = 0; k < 4; k++) {
          const from = k < 2 ? p : q;
          // The far end's "next" point mirrors so the perpendicular stays consistent.
          const to = k < 2 ? q : p;
          const sign = k < 2 ? 1 : -1;
          pos[(v + k) * 3] = from.x;
          pos[(v + k) * 3 + 1] = from.y;
          pos[(v + k) * 3 + 2] = from.z;
          nxt[(v + k) * 3] = from.x + (to.x - from.x) * sign;
          nxt[(v + k) * 3 + 1] = from.y + (to.y - from.y) * sign;
          nxt[(v + k) * 3 + 2] = from.z + (to.z - from.z) * sign;
          wid[v + k] = width;
          const tt = k < 2 ? t0 : t1;
          col[(v + k) * 4] = ca[0] + (cb[0] - ca[0]) * tt;
          col[(v + k) * 4 + 1] = ca[1] + (cb[1] - ca[1]) * tt;
          col[(v + k) * 4 + 2] = ca[2] + (cb[2] - ca[2]) * tt;
          col[(v + k) * 4 + 3] = alpha * (0.55 + 0.45 * Math.sin(Math.PI * tt));
        }
        quads++;
      }
    }
    for (const name of ['position', 'aNext', 'aWidth', 'aColor']) geometry.getAttribute(name).needsUpdate = true;
    geometry.setDrawRange(0, quads * 6);
  });

  return <mesh geometry={geometry} material={material} frustumCulled={false} renderOrder={2} raycast={() => null} />;
}

function DetailLines() {
  const { session, frame } = useScene();
  const model = useCs((s) => s.model);
  const highlight = useCs((s) => s.highlight);
  const material = useMemo(
    () => new THREE.ShaderMaterial({ vertexShader: LINE_VERTEX, fragmentShader: LINE_FRAGMENT, transparent: true, depthWrite: false }),
    [],
  );
  const geometry = useMemo(() => {
    const g = new THREE.BufferGeometry();
    const segs = MAX_CALL_SEGMENTS + MAX_IMPORT_SEGMENTS + 512;
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(segs * 6), 3).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('aColor', new THREE.BufferAttribute(new Float32Array(segs * 8), 4).setUsage(THREE.DynamicDrawUsage));
    g.setDrawRange(0, 0);
    g.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1e9);
    return g;
  }, []);
  useEffect(() => () => {
    geometry.dispose();
    material.dispose();
  }, [geometry, material]);

  // Segment lists (index pairs + tier) rebuilt when the focus or model changes.
  const segsRef = useRef<{ pairs: Uint32Array; tier: Uint8Array; count: number }>({ pairs: new Uint32Array(0), tier: new Uint8Array(0), count: 0 });
  const builtFor = useRef('');
  const lastVersion = useRef(-1);

  useFrame(() => {
    const m = frame.model;
    if (!m || m !== model) return;
    const key = `${frame.modelVersion}|${frame.selectedIndex}|${frame.hop.size}|${highlight?.command ?? ''}|${highlight?.edgePairs.length ?? 0}`;
    if (key !== builtFor.current) {
      builtFor.current = key;
      const pairs: number[] = [];
      const tier: number[] = [];
      const imports = Math.min(m.fileImports.length / 2, MAX_IMPORT_SEGMENTS);
      for (let k = 0; k < imports; k++) {
        pairs.push(m.fileImports[k * 2], m.fileImports[k * 2 + 1]);
        tier.push(1);
      }
      if (frame.hasFocus) {
        let calls = 0;
        for (let k = 0; k < m.calls.length && calls < MAX_CALL_SEGMENTS; k += 2) {
          const s = m.calls[k];
          const t = m.calls[k + 1];
          if (frame.hop.has(s) && frame.hop.has(t)) {
            pairs.push(s, t);
            tier.push(2);
            calls++;
          }
        }
      }
      if (highlight) {
        for (const [a, b] of highlight.edgePairs.slice(0, 512)) {
          const s = m.index.get(a);
          const t = m.index.get(b);
          if (s !== undefined && t !== undefined) {
            pairs.push(s, t);
            tier.push(3);
          }
        }
        for (let k = 1; k < highlight.path.length; k++) {
          const s = m.index.get(highlight.path[k - 1]);
          const t = m.index.get(highlight.path[k]);
          if (s !== undefined && t !== undefined) {
            pairs.push(s, t);
            tier.push(3);
          }
        }
      }
      segsRef.current = { pairs: Uint32Array.from(pairs), tier: Uint8Array.from(tier), count: tier.length };
      lastVersion.current = -1;
    }
    if (frame.version === lastVersion.current) return;
    lastVersion.current = frame.version;
    const w = tierWeights(frame.globalZoom);
    const importOpacity = Math.max(w[1], frame.hasFocus ? 0.5 : 0) * (1 - 0.6 * w[2]) * 0.55;
    const pos = geometry.getAttribute('position').array as Float32Array;
    const col = geometry.getAttribute('aColor').array as Float32Array;
    const { pairs, tier, count } = segsRef.current;
    const community = session.pipeline.buffers.community;
    let out = 0;
    for (let k = 0; k < count; k++) {
      const s = pairs[k * 2];
      const t = pairs[k * 2 + 1];
      const ws = frame.weight[s];
      const wt = frame.weight[t];
      const tr = tier[k];
      if (tr !== 3 && (ws < 0.05 || wt < 0.05)) continue;
      let alpha: number;
      if (tr === 1) {
        alpha = importOpacity * Math.min(ws, wt);
        if (frame.hasFocus && !(frame.hop.has(s) || frame.hop.has(t))) alpha *= 0.15;
      } else if (tr === 2) {
        const hs = frame.hop.get(s) ?? 9;
        const ht = frame.hop.get(t) ?? 9;
        alpha = 0.85 / (1 + Math.max(hs, ht) * 0.6);
      } else alpha = 1;
      if (alpha < 0.01) continue;
      const cs = frame.colour(community[s]);
      const ct = frame.colour(community[t]);
      const o = out * 6;
      pos[o] = frame.display[s * 3];
      pos[o + 1] = frame.display[s * 3 + 1];
      pos[o + 2] = frame.display[s * 3 + 2];
      pos[o + 3] = frame.display[t * 3];
      pos[o + 4] = frame.display[t * 3 + 1];
      pos[o + 5] = frame.display[t * 3 + 2];
      const c = out * 8;
      if (tr === 3) {
        col.set([1, 0.9, 0.5, 1, 1, 0.9, 0.5, 1], c);
      } else {
        col[c] = cs[0];
        col[c + 1] = cs[1];
        col[c + 2] = cs[2];
        col[c + 3] = alpha;
        col[c + 4] = ct[0];
        col[c + 5] = ct[1];
        col[c + 6] = ct[2];
        col[c + 7] = alpha * 0.6;
      }
      out++;
    }
    const pa = geometry.getAttribute('position') as THREE.BufferAttribute;
    const ca = geometry.getAttribute('aColor') as THREE.BufferAttribute;
    pa.clearUpdateRanges();
    pa.addUpdateRange(0, out * 6);
    pa.needsUpdate = true;
    ca.clearUpdateRanges();
    ca.addUpdateRange(0, out * 8);
    ca.needsUpdate = true;
    geometry.setDrawRange(0, out * 2);
  });

  return <lineSegments geometry={geometry} material={material} frustumCulled={false} renderOrder={1} raycast={() => null} />;
}

export function Edges() {
  return (
    <>
      <CommunityRibbons />
      <DetailLines />
    </>
  );
}
