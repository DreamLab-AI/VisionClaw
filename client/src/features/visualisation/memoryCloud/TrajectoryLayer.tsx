/**
 * TrajectoryLayer — draws the route a memory query takes through the local
 * HNSW over the memory-cloud sample.
 *
 * Mounted inside EmbeddingCloudLayer's scaled, rotating group, so every
 * coordinate here is cloud-local (snapshot positions span roughly ±100; the
 * canopy / tree / hyper layouts use a radius of LAYOUT_RADIUS).
 *
 * The look follows the RuVector Explorer (https://github.com/ruvnet/RuVector,
 * docs/explorer/explorer.js, MIT licence) — `drawTopo`, `drawGraph`,
 * `drawPathAnim`, `drawRoot`, `startMorph` — rebuilt for three.js:
 *   - the search tree grows out of the root in discovery order; kept edges
 *     brighter near the root, rejected candidates as faint orange twigs;
 *   - once the tree is in, the winning route is traced as a root → white →
 *     tip gradient tube with a white core and a glow sheath, a comet head
 *     with an 18-segment fading tail, and beads popping (ease-back) at each hop;
 *   - a pulsing ring marks the answer; focus pull dims everything off the route;
 *   - the sidecar's own top-k are ringed in gold, with a mint dot where the
 *     local route agrees; exact top-k misses are ringed red;
 *   - view changes glide every node with `interpolateLayouts`.
 *
 * Bright materials are `toneMapped: false` with colours above 1, so the
 * scene-wide threshold bloom in GemPostProcessing picks the route up while
 * the dimmed tree stays below threshold. Tubes, not drei Line2, because Line2
 * breaks the WebGPU render pass. Every geometry and material is disposed.
 */

import React, { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import { useFrame } from '@react-three/fiber';
import { useMemoryCloudStore } from './memoryCloudInstance';
import {
  beatState,
  directorClock,
  routeChannel,
  LAYOUT_RADIUS,
} from './memoryCloudStore';
import {
  ROUTE_PALETTE,
  GRADIENT_MID,
  TOTAL_DUR,
  GROW_DUR,
  beadScale,
  cometTail,
  ease,
  focusPull,
  hexToRgb01,
  nodeGrowth,
  pointAt,
  revealPhases,
  routeGradient,
  sampleEdge,
  sampleRoute,
  thinRejected,
  clamp01,
} from './routeMath';
import { tubeIndices, tubeVertexCount, writeTube, writeEdgeSegments } from './routeGeometry';
import type { LayoutResult, SearchTreeNode, Vec3 } from '../memoryTrajectory/types';

// ── sizes in cloud-local units ──
const TUBE_R = 0.35;
const NODE_R = 0.45;
const REJECT_R = 0.28;
const BEAD_R = 0.55;
const COMET_R = 0.5;
const RING_R = 1.4;
const MARK_R = 1.7;
const RADIAL = 8;
const TAIL_SEGMENTS = 18;
const MORPH_MS = 850;
const REJECTED_CAP = 1500;
const REPORT_MS = 100;
const EDGE_SAMPLES_KEPT = 6;
const EDGE_SAMPLES_REJECTED = 2;
const ROUTE_SAMPLES = 16;

const C = {
  root: hexToRgb01(ROUTE_PALETTE.root),
  tip: hexToRgb01(ROUTE_PALETTE.tip),
  mint: hexToRgb01(ROUTE_PALETTE.mint),
  edge: hexToRgb01(ROUTE_PALETTE.edge),
  upper: hexToRgb01(ROUTE_PALETTE.upper),
  prune: hexToRgb01(ROUTE_PALETTE.prune),
  sidecar: hexToRgb01(ROUTE_PALETTE.sidecar),
  miss: hexToRgb01(ROUTE_PALETTE.miss),
  node: [0.93, 0.95, 0.98] as Vec3,
  white: [1, 1, 1] as Vec3,
  tail: [1, 244 / 255, 230 / 255] as Vec3,
};

const mul = (c: Vec3, k: number): Vec3 => [c[0] * k, c[1] * k, c[2] * k];

function additive(opts: { vertexColors?: boolean; color?: THREE.ColorRepresentation; opacity?: number; side?: THREE.Side } = {}) {
  return new THREE.MeshBasicMaterial({
    vertexColors: opts.vertexColors ?? false,
    color: opts.color ?? 0xffffff,
    transparent: true,
    opacity: opts.opacity ?? 1,
    blending: THREE.AdditiveBlending,
    depthWrite: false,
    toneMapped: false,
    side: opts.side ?? THREE.FrontSide,
  });
}

/** Tube mesh with a fixed ring capacity whose buffers are rewritten in place. */
class TubeBuffer {
  readonly geometry = new THREE.BufferGeometry();
  readonly pos: Float32Array;
  readonly col: Float32Array;
  constructor(readonly capacity: number) {
    const n = tubeVertexCount(capacity, RADIAL);
    this.pos = new Float32Array(n * 3);
    this.col = new Float32Array(n * 3);
    this.geometry.setAttribute('position', new THREE.BufferAttribute(this.pos, 3).setUsage(THREE.DynamicDrawUsage));
    this.geometry.setAttribute('color', new THREE.BufferAttribute(this.col, 3).setUsage(THREE.DynamicDrawUsage));
    this.geometry.setIndex(new THREE.BufferAttribute(tubeIndices(capacity, RADIAL), 1));
  }
  write(pts: Vec3[], radius: (i: number) => number, colour: (i: number) => Vec3) {
    writeTube(pts, this.capacity, RADIAL, radius, colour, this.pos, this.col);
    this.geometry.attributes.position.needsUpdate = true;
    this.geometry.attributes.color.needsUpdate = true;
    this.geometry.computeBoundingSphere();
  }
  dispose() {
    this.geometry.dispose();
  }
}

/** LineSegments buffer with fixed capacity. */
class SegmentBuffer {
  readonly geometry = new THREE.BufferGeometry();
  readonly pos: Float32Array;
  readonly col: Float32Array;
  constructor(readonly capacity: number) {
    this.pos = new Float32Array(Math.max(1, capacity) * 6);
    this.col = new Float32Array(Math.max(1, capacity) * 6);
    this.geometry.setAttribute('position', new THREE.BufferAttribute(this.pos, 3).setUsage(THREE.DynamicDrawUsage));
    this.geometry.setAttribute('color', new THREE.BufferAttribute(this.col, 3).setUsage(THREE.DynamicDrawUsage));
    this.geometry.setDrawRange(0, 0);
  }
  commit(segments: number) {
    this.geometry.setDrawRange(0, segments * 2);
    this.geometry.attributes.position.needsUpdate = true;
    this.geometry.attributes.color.needsUpdate = true;
    this.geometry.computeBoundingSphere();
  }
  dispose() {
    this.geometry.dispose();
  }
}

interface Marker {
  id: number;
  colour: Vec3;
  /** radius multiplier */
  size: number;
  /** filled dot rather than ring */
  dot: boolean;
  /** shows from the start rather than once the trace begins */
  always: boolean;
}

export interface TrajectoryLayerProps {
  /** snapshot positions (3 * count), used for markers in the space view */
  cloudPositions: ArrayLike<number>;
  glow: number;
  showRejected: boolean;
  reducedMotion: boolean;
}

const TrajectoryLayer: React.FC<TrajectoryLayerProps> = ({ cloudPositions, glow, showRejected, reducedMotion }) => {
  const run = useMemoryCloudStore((s) => s.query.run);
  const layout = useMemoryCloudStore((s) => s.query.layout);
  const prevLayout = useMemoryCloudStore((s) => s.query.prevLayout);
  const morphSeq = useMemoryCloudStore((s) => s.query.morphSeq);
  const querySeq = useMemoryCloudStore((s) => s.query.seq);
  const response = useMemoryCloudStore((s) => s.query.response);
  const view = useMemoryCloudStore((s) => s.view);
  const seekSeq = useMemoryCloudStore((s) => s.playback.seekSeq);
  const trajectory = useMemoryCloudStore((s) => s.trajectory);

  const groupRef = useRef<THREE.Group>(null);
  const elRef = useRef(0);
  const lastReport = useRef(0);
  const morphStart = useRef<number | null>(null);
  const settledKey = useRef<string>('');

  // Adopt explicit seeks (new query, replay, scrub) from the store.
  useEffect(() => {
    elRef.current = useMemoryCloudStore.getState().playback.el;
    settledKey.current = '';
  }, [seekSeq]);

  useEffect(() => {
    morphStart.current = prevLayout && !reducedMotion ? performance.now() : null;
    settledKey.current = '';
  }, [morphSeq, prevLayout, reducedMotion]);

  // ── static structure per run ──
  const structure = useMemo(() => {
    if (!run) return null;
    const list = run.tree.list;
    const visible = thinRejected(list, REJECTED_CAP);
    const keptIdx: number[] = [];
    const rejectedIdx: number[] = [];
    list.forEach((n, k) => {
      if (!visible[k]) return;
      (n.kept ? keptIdx : rejectedIdx).push(k);
    });
    const pathSet = new Set(run.tree.path);
    return { list, keptIdx, rejectedIdx, pathSet, maxDepth: Math.max(1, run.tree.maxDepth) };
  }, [run]);

  // ── markers: sidecar top-k (gold, mint dot on agreement) and exact top-k misses (red) ──
  const markers = useMemo<Marker[]>(() => {
    if (!run) return [];
    const local = new Set(run.result.top);
    const out: Marker[] = [];
    for (const h of response?.sidecar.results ?? []) {
      if (h.sampleIndex === null || h.sampleIndex === undefined) continue;
      out.push({ id: h.sampleIndex, colour: C.sidecar, size: 1.25, dot: false, always: true });
      if (local.has(h.sampleIndex)) out.push({ id: h.sampleIndex, colour: C.mint, size: 0.45, dot: true, always: true });
    }
    for (const t of run.exactTop) {
      out.push({ id: t, colour: local.has(t) ? C.mint : C.miss, size: 0.85, dot: false, always: false });
    }
    return out;
  }, [run, response]);

  // ── GPU resources, rebuilt per run ──
  const res = useMemo(() => {
    if (!structure || !run) return null;
    const keptSegCap = structure.keptIdx.length * EDGE_SAMPLES_KEPT;
    const rejSegCap = structure.rejectedIdx.length * EDGE_SAMPLES_REJECTED;
    const routeCap = Math.max(2, (run.tree.path.length - 1) * ROUTE_SAMPLES + 2);
    const sphere = new THREE.IcosahedronGeometry(1, 1);
    const box = new THREE.BoxGeometry(1, 1, 1);
    const ring = new THREE.RingGeometry(0.82, 1, 48);
    const disc = new THREE.CircleGeometry(1, 24);
    const keptLines = new SegmentBuffer(keptSegCap);
    const rejLines = new SegmentBuffer(rejSegCap);
    const sheathOuter = new TubeBuffer(routeCap);
    const sheathInner = new TubeBuffer(routeCap);
    const body = new TubeBuffer(routeCap);
    const core = new TubeBuffer(routeCap);
    const tail = new TubeBuffer(TAIL_SEGMENTS + 1);
    const lineMat = new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, blending: THREE.AdditiveBlending, depthWrite: false, toneMapped: false });
    const rejLineMat = lineMat.clone();
    const nodeMat = new THREE.MeshBasicMaterial({ toneMapped: false });
    const rejectMat = additive({ opacity: 1 });
    const beadMat = new THREE.MeshBasicMaterial({ toneMapped: false });
    const beadHaloMat = additive({ opacity: 0.35 });
    const sheathOuterMat = additive({ vertexColors: true, opacity: 0.05 });
    const sheathInnerMat = additive({ vertexColors: true, opacity: 0.11 });
    const bodyMat = new THREE.MeshBasicMaterial({ vertexColors: true, toneMapped: false });
    const coreMat = new THREE.MeshBasicMaterial({ vertexColors: true, toneMapped: false });
    const tailMat = additive({ vertexColors: true, opacity: 1 });
    const cometMat = new THREE.MeshBasicMaterial({ color: 0xffffff, toneMapped: false });
    const cometGlowMat = additive({ color: new THREE.Color().setRGB(...C.tip), opacity: 0.6 });
    const answerMat = additive({ color: new THREE.Color().setRGB(...C.tip), side: THREE.DoubleSide });
    const pulseMat = additive({ color: new THREE.Color().setRGB(...C.tip), side: THREE.DoubleSide });
    const rootRingMat = additive({ color: 0xffffff, side: THREE.DoubleSide, opacity: 0.9 });
    const startRingMat = additive({ color: new THREE.Color().setRGB(...C.root), side: THREE.DoubleSide });
    const markerRingMat = additive({ side: THREE.DoubleSide, opacity: 0.95 });
    const diskMat = new THREE.LineBasicMaterial({ color: new THREE.Color().setRGB(...mul(C.upper, 0.5)), transparent: true, blending: THREE.AdditiveBlending, depthWrite: false, toneMapped: false });

    const keptNodes = new THREE.InstancedMesh(sphere, nodeMat, Math.max(1, structure.keptIdx.length));
    const rejectNodes = new THREE.InstancedMesh(box, rejectMat, Math.max(1, structure.rejectedIdx.length));
    const beads = new THREE.InstancedMesh(sphere, beadMat, Math.max(1, run.tree.path.length));
    const beadHalos = new THREE.InstancedMesh(sphere, beadHaloMat, Math.max(1, run.tree.path.length));
    const markerRings = new THREE.InstancedMesh(ring, markerRingMat, Math.max(1, markers.length));
    const markerDots = new THREE.InstancedMesh(disc, markerRingMat, Math.max(1, markers.length));
    for (const m of [keptNodes, rejectNodes, beads, beadHalos, markerRings, markerDots]) {
      m.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
      m.frustumCulled = false;
      m.setColorAt(0, new THREE.Color(0, 0, 0));
    }
    const diskGeom = new THREE.BufferGeometry().setFromPoints(
      Array.from({ length: 129 }, (_, i) => {
        const a = (i / 128) * Math.PI * 2;
        // the hyper layout maps the unit disk to the XZ plane (y = 0) at LAYOUT_RADIUS
        return new THREE.Vector3(Math.cos(a) * LAYOUT_RADIUS, 0, Math.sin(a) * LAYOUT_RADIUS);
      }),
    );

    const objects = {
      keptLines: new THREE.LineSegments(keptLines.geometry, lineMat),
      rejLines: new THREE.LineSegments(rejLines.geometry, rejLineMat),
      sheathOuter: new THREE.Mesh(sheathOuter.geometry, sheathOuterMat),
      sheathInner: new THREE.Mesh(sheathInner.geometry, sheathInnerMat),
      body: new THREE.Mesh(body.geometry, bodyMat),
      core: new THREE.Mesh(core.geometry, coreMat),
      tail: new THREE.Mesh(tail.geometry, tailMat),
      comet: new THREE.Mesh(sphere, cometMat),
      cometGlow: new THREE.Mesh(sphere, cometGlowMat),
      answer: new THREE.Mesh(ring, answerMat),
      pulse: new THREE.Mesh(ring, pulseMat),
      rootRing: new THREE.Mesh(ring, rootRingMat),
      rootCore: new THREE.Mesh(sphere, cometMat),
      startRing: new THREE.Mesh(ring, startRingMat),
      disk: new THREE.Line(diskGeom, diskMat),
      keptNodes,
      rejectNodes,
      beads,
      beadHalos,
      markerRings,
      markerDots,
    };
    for (const o of Object.values(objects)) {
      o.frustumCulled = false;
      o.renderOrder = 10;
    }
    objects.body.renderOrder = 12;
    objects.core.renderOrder = 13;
    objects.tail.renderOrder = 14;
    objects.comet.renderOrder = 15;

    const dispose = () => {
      for (const b of [keptLines, rejLines, sheathOuter, sheathInner, body, core, tail]) b.dispose();
      for (const g of [sphere, box, ring, disc, diskGeom]) g.dispose();
      for (const m of [lineMat, rejLineMat, nodeMat, rejectMat, beadMat, beadHaloMat, sheathOuterMat, sheathInnerMat, bodyMat, coreMat, tailMat, cometMat, cometGlowMat, answerMat, pulseMat, rootRingMat, startRingMat, markerRingMat, diskMat]) m.dispose();
      for (const m of [keptNodes, rejectNodes, beads, beadHalos, markerRings, markerDots]) m.dispose();
    };
    return { keptLines, rejLines, sheathOuter, sheathInner, body, core, tail, objects, dispose };
  }, [structure, run, markers.length]);

  useEffect(() => () => res?.dispose(), [res]);

  // Attach the objects to our group imperatively; they are rebuilt only per run.
  const mounted = !!run && !!layout;
  useEffect(() => {
    const g = groupRef.current;
    if (!g || !res || !mounted) return;
    const objs = Object.values(res.objects);
    g.add(...objs);
    return () => {
      g.remove(...objs);
    };
  }, [res, mounted]);

  useEffect(() => {
    if (!run) {
      routeChannel.pts = [];
      routeChannel.seq++;
    }
  }, [run]);

  // ── per-frame scratch ──
  const scratch = useMemo(
    () => ({
      m: new THREE.Matrix4(),
      q: new THREE.Quaternion(),
      qInv: new THREE.Quaternion(),
      v: new THREE.Vector3(),
      s: new THREE.Vector3(),
      col: new THREE.Color(),
      identity: new THREE.Quaternion(),
      cached: null as null | {
        layout: LayoutResult;
        keptPolys: Vec3[][];
        rejPolys: Vec3[][];
        route: { pts: Vec3[]; knots: number[] };
      },
    }),
    [],
  );

  useEffect(() => {
    scratch.cached = null;
    settledKey.current = '';
  }, [layout, run, scratch, querySeq, showRejected]);

  useFrame(({ camera, clock }, dt) => {
    if (!run || !layout || !structure || !res || !groupRef.current) return;
    const st = useMemoryCloudStore.getState();
    const now = performance.now();

    // ── clock ──
    if (directorClock.active) {
      elRef.current = directorClock.el;
    } else if (st.playback.playing) {
      elRef.current += Math.min(dt, 0.1) * st.playback.speed;
    }
    const el = reducedMotion ? TOTAL_DUR + GROW_DUR : elRef.current;
    const ph = revealPhases(el, reducedMotion);
    if (!directorClock.active && now - lastReport.current > REPORT_MS) {
      lastReport.current = now;
      const playing = st.playback.playing && !(ph.done && el >= TOTAL_DUR + GROW_DUR);
      if (Math.abs(st.playback.el - el) > 1e-3 || playing !== st.playback.playing) st.reportPlayback(el, playing);
    }

    // ── layout (with morph) ──
    let L = layout;
    let morphing = false;
    if (morphStart.current !== null && prevLayout && trajectory) {
      const mf = clamp01((now - morphStart.current) / MORPH_MS);
      if (mf < 1) {
        // interpolateLayouts applies its own cubic ease; one-sided nodes glide from their ancestor
        L = trajectory.interpolateLayouts(prevLayout, layout, mf, run.tree);
        morphing = true;
      } else {
        morphStart.current = null;
      }
    }

    // ── geometry cache: edge polylines and route samples for this layout ──
    let cache = scratch.cached;
    if (!cache || cache.layout !== L) {
      const polysFor = (idx: number[], n: number) =>
        idx.map((k) => {
          const node = structure.list[k];
          if (node.parent < 0) return [] as Vec3[];
          return sampleEdge(L, node.parent, node.id, n) ?? [];
        });
      cache = {
        layout: L,
        keptPolys: polysFor(structure.keptIdx, EDGE_SAMPLES_KEPT),
        rejPolys: showRejected ? polysFor(structure.rejectedIdx, EDGE_SAMPLES_REJECTED) : [],
        route: sampleRoute(run.tree.path, L, ROUTE_SAMPLES),
      };
      scratch.cached = cache;
      if (!morphing) {
        routeChannel.pts = cache.route.pts.map((p) => [p[0], p[1], p[2]] as [number, number, number]);
        routeChannel.seq++;
      }
    }

    const total = structure.list.length;
    const fpull = focusPull(ph.pathT);
    const glowK = glow * (beatState.on ? 1 + 0.2 * beatState.pulse : 1);
    const pts = cache.route.pts;
    const last = pts.length - 1;
    const head = ph.pathT < 1 ? ease.io(ph.pathT) * last : last;

    // Group-local billboard rotation: inverse(group world rotation) · camera.
    groupRef.current.getWorldQuaternion(scratch.qInv).invert();
    scratch.q.copy(scratch.qInv).multiply(camera.quaternion);

    const settled = ph.done && el >= TOTAL_DUR + GROW_DUR && !morphing;
    const key = `${settled}|${showRejected}|${glow}|${view}`;
    const redrawTree = !settled || settledKey.current !== key;
    if (settled) settledKey.current = key;

    if (redrawTree) {
      // ── edges: grow out of the parent; depth-graded; focus pull dims off-route ──
      const dimOff = 1 - 0.55 * fpull;
      const partial = (poly: Vec3[], g: number): Vec3[] => {
        if (g >= 1 || poly.length < 2) return poly;
        const f = ease.out(g) * (poly.length - 1);
        const out = poly.slice(0, Math.floor(f) + 1);
        out.push(pointAt(poly, f));
        return out;
      };
      const keptDrawn: Vec3[][] = [];
      const keptColours: Vec3[] = [];
      structure.keptIdx.forEach((k, e) => {
        const g = k === 0 ? 1 : nodeGrowth(k, total, el);
        if (g <= 0) return;
        const node = structure.list[k];
        const poly = cache!.keptPolys[e];
        if (poly.length < 2) return;
        keptDrawn.push(partial(poly, g));
        const depthF = node.depth / structure.maxDepth;
        let c: Vec3;
        if (node.learned) c = mul(C.mint, 0.9);
        else if (node.layer > 0) c = mul(C.upper, 0.7 - 0.3 * depthF);
        else if (g < 1) c = mul(C.edge, 0.8);
        else if (structure.pathSet.has(node.id)) c = mul(C.edge, 0.55 - 0.3 * depthF);
        else c = mul(C.edge, (0.55 - 0.3 * depthF) * dimOff);
        keptColours.push(c);
      });
      res.keptLines.commit(writeEdgeSegments(keptDrawn, (e) => keptColours[e], res.keptLines.pos, res.keptLines.col));

      if (showRejected) {
        const rejDrawn: Vec3[][] = [];
        structure.rejectedIdx.forEach((k, e) => {
          const g = nodeGrowth(k, total, el);
          if (g <= 0) return;
          const poly = cache!.rejPolys[e];
          if (poly && poly.length >= 2) rejDrawn.push(partial(poly, g));
        });
        const rc = mul(C.prune, 0.16 * dimOff);
        res.rejLines.commit(writeEdgeSegments(rejDrawn, () => rc, res.rejLines.pos, res.rejLines.col));
      } else {
        res.rejLines.commit(0);
      }

      // ── nodes: pop in with ease-back; hubs larger; route nodes tint to tip under focus pull ──
      const kn = res.objects.keptNodes;
      structure.keptIdx.forEach((k, i) => {
        const node: SearchTreeNode = structure.list[k];
        const p = L.positions.get(node.id);
        const g = k === 0 ? 1 : nodeGrowth(k, total, el);
        const sc = p && g > 0 ? Math.max(0, ease.back(g)) : 0;
        const hub = Math.log2((node.subtreeSize || 1) + 1);
        const r = NODE_R * (node.children.length ? 1 + hub * 0.35 : 0.85) * sc;
        scratch.v.set(p?.[0] ?? 0, p?.[1] ?? 0, p?.[2] ?? 0);
        scratch.s.setScalar(Math.max(r, 1e-5));
        scratch.m.compose(scratch.v, scratch.identity, scratch.s);
        kn.setMatrixAt(i, scratch.m);
        const flash = 1 - g;
        let c: Vec3 = node.learned ? C.mint : node.layer > 0 ? C.upper : C.node;
        if (structure.pathSet.has(node.id) && fpull > 0) {
          const f = fpull / 0.6;
          c = [c[0] + (C.tip[0] - c[0]) * f, c[1] + (C.tip[1] - c[1]) * f, c[2] + (C.tip[2] - c[2]) * f];
        }
        const bright = (structure.pathSet.has(node.id) ? 1 : 1 - 0.45 * fpull) * (1 + flash * 1.5);
        kn.setColorAt(i, scratch.col.setRGB(c[0] * bright, c[1] * bright, c[2] * bright));
      });
      kn.count = structure.keptIdx.length;
      kn.instanceMatrix.needsUpdate = true;
      if (kn.instanceColor) kn.instanceColor.needsUpdate = true;

      const rn = res.objects.rejectNodes;
      rn.visible = showRejected;
      if (showRejected) {
        structure.rejectedIdx.forEach((k, i) => {
          const node = structure.list[k];
          const p = L.positions.get(node.id);
          const g = nodeGrowth(k, total, el);
          const flicker = g < 1 && !reducedMotion ? 0.5 + 0.5 * Math.sin(now / 40 + node.id) : 1;
          const s = p && g > 0 ? REJECT_R * (0.6 + 0.4 * Math.max(0, ease.back(g))) : 1e-5;
          scratch.v.set(p?.[0] ?? 0, p?.[1] ?? 0, p?.[2] ?? 0);
          scratch.s.setScalar(s);
          scratch.m.compose(scratch.v, scratch.q, scratch.s);
          rn.setMatrixAt(i, scratch.m);
          const a = 0.55 * g * flicker * (1 - 0.2 * (fpull / 0.6));
          rn.setColorAt(i, scratch.col.setRGB(C.prune[0] * a, C.prune[1] * a, C.prune[2] * a));
        });
        rn.count = structure.rejectedIdx.length;
        rn.instanceMatrix.needsUpdate = true;
        if (rn.instanceColor) rn.instanceColor.needsUpdate = true;
      }
    }

    // ── route ribbon, comet, beads, answer ──
    const showPath = ph.pathT > 0 && pts.length >= 2;
    const o = res.objects;
    for (const m of [o.sheathOuter, o.sheathInner, o.body, o.core, o.beads, o.beadHalos]) m.visible = showPath;
    if (showPath && (redrawTree || beatState.on)) {
      const hi = Math.floor(head);
      const drawn = pts.slice(0, hi + 1);
      if (hi < last) drawn.push(pointAt(pts, head));
      const u = (i: number) => (drawn.length > 1 ? i / (pts.length - 1) : 0);
      const bodyK = 0.85 + 0.55 * glowK;
      res.sheathOuter.write(drawn, () => TUBE_R * 7, () => mul(C.tip, Math.min(1.5, glowK)));
      res.sheathInner.write(drawn, () => TUBE_R * 3.6, () => mul(C.tip, Math.min(1.5, glowK)));
      res.body.write(drawn, () => TUBE_R, (i) => mul(routeGradient(u(i)), bodyK));
      res.core.write(drawn, () => TUBE_R * 0.38, () => mul(C.white, 0.9 + 0.5 * glowK));

      // beads pop as the head passes each hop
      const knots = cache.route.knots;
      knots.forEach((ki, j) => {
        const sc = j === 0 ? 0 : beadScale(head, ki);
        const p = pts[ki];
        scratch.v.set(p[0], p[1], p[2]);
        const c = ki / last < GRADIENT_MID ? C.root : C.tip;
        scratch.s.setScalar(Math.max(1e-5, BEAD_R * sc));
        scratch.m.compose(scratch.v, scratch.identity, scratch.s);
        o.beads.setMatrixAt(j, scratch.m);
        o.beads.setColorAt(j, scratch.col.setRGB(1.2 + 0.4 * glowK, 1.2 + 0.4 * glowK, 1.2 + 0.4 * glowK));
        scratch.s.setScalar(Math.max(1e-5, BEAD_R * 2.6 * sc * Math.min(1.5, glowK)));
        scratch.m.compose(scratch.v, scratch.identity, scratch.s);
        o.beadHalos.setMatrixAt(j, scratch.m);
        o.beadHalos.setColorAt(j, scratch.col.setRGB(c[0], c[1], c[2]));
      });
      o.beads.count = knots.length;
      o.beadHalos.count = knots.length;
      for (const m of [o.beads, o.beadHalos]) {
        m.instanceMatrix.needsUpdate = true;
        if (m.instanceColor) m.instanceColor.needsUpdate = true;
      }
    }

    // comet: travels with the head while tracing; loops gently once converged
    let cometAt: number | null = null;
    let cometSize = 1;
    if (showPath && ph.pathT < 1) {
      cometAt = head + (beatState.on ? 0.015 * beatState.pulse * last : 0);
    } else if (showPath && !reducedMotion) {
      cometAt = ((clock.elapsedTime / 2.8) % 1) * last;
      cometSize = 0.7;
    }
    o.comet.visible = o.cometGlow.visible = o.tail.visible = cometAt !== null;
    if (cometAt !== null) {
      const hp = pointAt(pts, Math.min(last, cometAt));
      const pulse = beatState.on ? 1 + 0.35 * beatState.pulse : 1;
      o.comet.position.set(hp[0], hp[1], hp[2]);
      o.comet.scale.setScalar(COMET_R * cometSize * pulse);
      o.cometGlow.position.copy(o.comet.position);
      o.cometGlow.scale.setScalar(COMET_R * 5 * cometSize * Math.min(1.6, glowK) * pulse);
      (o.comet.material as THREE.MeshBasicMaterial).color.setScalar(1.5 + glowK);
      const segs = cometTail(pts, Math.min(last, cometAt), TAIL_SEGMENTS);
      if (segs.length) {
        // back of the tail first, head last
        const tailPts: Vec3[] = [segs[segs.length - 1].a, ...segs.slice().reverse().map((s) => s.b)];
        const widths = [0, ...segs.slice().reverse().map((s) => s.width)];
        const alphas = [0, ...segs.slice().reverse().map((s) => s.alpha)];
        res.tail.write(tailPts, (i) => COMET_R * 0.9 * cometSize * widths[i], (i) => mul(C.tail, alphas[i] * (0.8 + 0.4 * glowK)));
      } else {
        res.tail.write([], () => 0, () => C.white);
      }
    }

    // answer: static ring once converged, plus a pulsing ring (beat-locked when music plays)
    const answerOn = ph.pathT >= 1 && pts.length >= 2;
    o.answer.visible = answerOn;
    o.pulse.visible = answerOn && !reducedMotion;
    if (answerOn) {
      const tp = pts[last];
      o.answer.position.set(tp[0], tp[1], tp[2]);
      o.answer.quaternion.copy(scratch.q);
      o.answer.scale.setScalar(RING_R);
      (o.answer.material as THREE.MeshBasicMaterial).color.setRGB(...mul(C.tip, 1 + 0.6 * glowK));
      const pr = beatState.on ? beatState.phase : (clock.elapsedTime / 0.9) % 1;
      o.pulse.position.copy(o.answer.position);
      o.pulse.quaternion.copy(scratch.q);
      o.pulse.scale.setScalar(RING_R * (1 + pr * 2.7));
      (o.pulse.material as THREE.MeshBasicMaterial).opacity = 1 - pr;
    }

    // root: white ring + core, and an expanding start ring for the first 1.4 s
    const rp = L.positions.get(run.tree.root);
    o.rootRing.visible = o.rootCore.visible = !!rp;
    o.startRing.visible = !!rp && el < 1.4 && !reducedMotion;
    if (rp) {
      o.rootRing.position.set(rp[0], rp[1], rp[2]);
      o.rootRing.quaternion.copy(scratch.q);
      const rpulse = beatState.on ? 1 + 0.12 * beatState.bar : reducedMotion ? 1 : 1 + 0.06 * Math.sin(now / 500);
      o.rootRing.scale.setScalar(1.8 * rpulse);
      o.rootCore.position.copy(o.rootRing.position);
      o.rootCore.scale.setScalar(0.8);
      if (o.startRing.visible) {
        const f = el / 1.4;
        o.startRing.position.copy(o.rootRing.position);
        o.startRing.quaternion.copy(scratch.q);
        o.startRing.scale.setScalar(2 + ease.out(f) * LAYOUT_RADIUS * 0.35);
        (o.startRing.material as THREE.MeshBasicMaterial).opacity = 0.7 * (1 - f);
      }
    }

    // markers: sidecar top-k always; exact top-k once the trace begins
    let mi = 0;
    let di = 0;
    const markAlpha = clamp01(ph.pathT * 2);
    for (const mk of markers) {
      const p = L.positions.get(mk.id) ??
        (view === 'space' && mk.id * 3 + 2 < cloudPositions.length
          ? ([cloudPositions[mk.id * 3], cloudPositions[mk.id * 3 + 1], cloudPositions[mk.id * 3 + 2]] as Vec3)
          : null);
      if (!p) continue;
      const a = mk.always ? 1 : markAlpha;
      if (a <= 0) continue;
      scratch.v.set(p[0], p[1], p[2]);
      scratch.s.setScalar(MARK_R * mk.size);
      scratch.m.compose(scratch.v, scratch.q, scratch.s);
      const target = mk.dot ? o.markerDots : o.markerRings;
      const idx = mk.dot ? di++ : mi++;
      target.setMatrixAt(idx, scratch.m);
      target.setColorAt(idx, scratch.col.setRGB(mk.colour[0] * a, mk.colour[1] * a, mk.colour[2] * a));
    }
    o.markerRings.count = mi;
    o.markerDots.count = di;
    for (const m of [o.markerRings, o.markerDots]) {
      m.instanceMatrix.needsUpdate = true;
      if (m.instanceColor) m.instanceColor.needsUpdate = true;
    }

    // Poincaré disk boundary in the hyper view
    o.disk.visible = view === 'hyper' && !morphing;
  });

  if (!run || !layout) return null;
  return <group ref={groupRef} name="memory-trajectory-layer" />;
};

export default TrajectoryLayer;
