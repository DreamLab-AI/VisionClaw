/**
 * BotsEdges.tsx
 * The inter-agent collaboration edge layer, drawn as ONE THREE.LineSegments.
 *
 * Replaces the previous per-edge BotsEdgeComponent (one THREE.Line — plus
 * secondary/overload SimpleLines and sphere particles — per edge, i.e. 342+ draw
 * calls for a 19-agent swarm). All inter-agent edges now share a single
 * pre-allocated position+colour BufferAttribute pair, grown by doubling (cf.
 * GlassEdges), updated in place every frame from the live agent positions. Zero
 * per-frame allocation in steady state (only a rare re-alloc on capacity growth).
 *
 * PRESERVED (per-edge, encoded in the vertex colours under additive blending —
 * the bioluminescent idiom shared with AgentTrail / TransientBeams):
 *   - Activity colour: active edges recolour by average token rate
 *     (>20 → #E67E22, >10 → #3498DB, else #2980B9); idle edges use the base
 *     edge colour.
 *   - Activity opacity: base 0.8 active / 0.3 idle plus a token-rate boost,
 *     baked into vertex-colour intensity (additive blend → low intensity reads
 *     as faint/transparent, high as a bright thread).
 *   - Opacity pulsing: high-traffic edges (avgTokenRate > 40 || messageCount >
 *     200) pulse via sin(t*5)*0.3+1, folded into the same intensity.
 *   - Live endpoints: both vertices track their agents' server positions every
 *     frame.
 *
 * SIMPLIFIED (documented — the brief accepts uniform additive glow):
 *   - Organic curved sway tendril → a straight segment. A curve needs N segments
 *     per edge (N× the vertex budget); LineSegments is 2 verts/edge by design.
 *   - Secondary energy channel + overload channel (extra parallel lines) → folded
 *     into the single edge's additive intensity (brighter = higher traffic).
 *   - 4-sphere data-flow particle animation → removed (per-edge meshes were their
 *     own draw calls); flow now reads through the pulsing glow.
 *   - Per-edge line width (was computed but never applied — WebGL clamps
 *     lineWidth to 1 on most platforms) → uniform width.
 */
import React, { useRef, useMemo, useEffect } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import { BotsEdge, BotsAgent } from '../types/BotsTypes';
import { CHAIN_PAYMENT_COLOR, CHAIN_PAYMENT_EDGE_TYPE } from '../chain/chainPayments';

/* R3F maps three.js props onto JSX host elements (args, position, rotation, intensity...);
   these are not DOM properties. react/no-unknown-property is not enforced in this config. */

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested in __tests__/BotsEdges.test.ts) — byte-identical to
// the arithmetic the old per-edge BotsEdgeComponent applied.
// ---------------------------------------------------------------------------

/** First capacity (edges) allocated up front; grows by doubling past this. */
export const EDGE_INITIAL_CAPACITY = 256;

/** Average of two agents' token rates (undefined/missing → 0). */
export function computeAvgTokenRate(
  srcRate: number | undefined,
  tgtRate: number | undefined,
): number {
  return ((srcRate || 0) + (tgtRate || 0)) / 2;
}

/**
 * Per-edge opacity: base (0.8 active / 0.3 idle) plus a token-rate boost, capped
 * at 1. Baked into vertex-colour intensity under additive blending.
 */
export function computeEdgeOpacity(isActive: boolean, avgTokenRate: number): number {
  const baseOpacity = isActive ? 0.8 : 0.3;
  const tokenOpacity = avgTokenRate > 10 ? Math.min(avgTokenRate / 50, 0.4) : 0;
  return Math.min(baseOpacity + tokenOpacity, 1);
}

/** Per-edge hue: idle edges use `baseColor`; active edges bucket by token rate. */
export function computeEdgeColor(
  isActive: boolean,
  avgTokenRate: number,
  baseColor: string,
): string {
  if (!isActive) return baseColor;
  if (avgTokenRate > 20) return '#E67E22';
  if (avgTokenRate > 10) return '#3498DB';
  return '#2980B9';
}

/** True while the edge carried a message within the last 5 s (wall-clock ms). */
export function isEdgeActive(lastMessageTime: number, nowMs: number): boolean {
  return nowMs - lastMessageTime < 5000;
}

/** High-traffic edges pulse; quiet ones hold steady. */
export function shouldEdgePulse(avgTokenRate: number, messageCount: number): boolean {
  return avgTokenRate > 40 || messageCount > 200;
}

/** Pulse envelope sin(t*5)*0.3+1 (t in seconds); 1 when the edge should not pulse. */
export function computePulse(shouldPulse: boolean, elapsedSeconds: number): number {
  return shouldPulse ? Math.sin(elapsedSeconds * 5) * 0.3 + 1 : 1;
}

/**
 * Sidechain payment edges (S5) keep their own colour whatever the token
 * traffic: full intensity once settled, dimmed while unsettled, never pulsing.
 * Returns undefined for any other edge.
 */
export function chainPaymentStyle(
  edge: Pick<BotsEdge, 'type' | 'chainPayment'>,
): { color: string; opacity: number } | undefined {
  if (edge.type !== CHAIN_PAYMENT_EDGE_TYPE) return undefined;
  return { color: CHAIN_PAYMENT_COLOR, opacity: edge.chainPayment?.settled ? 1 : 0.45 };
}

/** Grow-by-double capacity, like GlassEdges — smallest 2^k*current ≥ needed. */
export function growEdgeCapacity(current: number, needed: number): number {
  let cap = Math.max(1, current);
  while (cap < needed) cap *= 2;
  return cap;
}

// ---------------------------------------------------------------------------
// Buffers + per-edge metadata
// ---------------------------------------------------------------------------

interface EdgeLayerBuffers {
  line: THREE.LineSegments;
  geometry: THREE.BufferGeometry;
  material: THREE.LineBasicMaterial;
  /** capacity * 6 floats (2 verts * xyz). Written in place each frame. */
  positions: Float32Array;
  /** capacity * 6 floats (2 verts * rgb). Written in place each frame. */
  colors: Float32Array;
  capacity: number;
}

/**
 * Per-edge static data resolved once on a data change. Colour is time-varying
 * (isActive flips after 5 s), so BOTH candidate colours + opacities are
 * precomputed and the frame loop selects — no string parsing per frame.
 */
interface EdgeMeta {
  srcId: string;
  tgtId: string;
  activeR: number; activeG: number; activeB: number;
  inactiveR: number; inactiveG: number; inactiveB: number;
  activeOpacity: number;
  inactiveOpacity: number;
  shouldPulse: boolean;
  lastMessageTime: number;
}

/** Module-scope scratch — colour hex → linear rgb, no per-edge allocation. */
const _edgeColorScratch = new THREE.Color();

/** Allocate a LineSegments + its position/colour buffers at `capacity` edges. */
function createEdgeBuffers(capacity: number): EdgeLayerBuffers {
  const positions = new Float32Array(capacity * 6);
  const colors = new Float32Array(capacity * 6);
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));
  geometry.setDrawRange(0, 0);
  const material = new THREE.LineBasicMaterial({
    vertexColors: true,
    transparent: true,
    opacity: 1,
    blending: THREE.AdditiveBlending,
    depthWrite: false,
    toneMapped: false,
  });
  const line = new THREE.LineSegments(geometry, material);
  line.frustumCulled = false;
  return { line, geometry, material, positions, colors, capacity };
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export interface BotsEdgesProps {
  /** Inter-agent edges keyed by id. */
  edges: Map<string, BotsEdge>;
  /** Agents keyed by id — supplies token rates for activity colouring. */
  agents: Map<string, BotsAgent>;
  /** Live agent positions (server physics). Read every frame, never mutated. */
  positionsRef: React.MutableRefObject<Map<string, THREE.Vector3>>;
  /** Base (idle) edge colour, e.g. colours.edge. */
  color: string;
}

export const BotsEdges: React.FC<BotsEdgesProps> = ({ edges, agents, positionsRef, color }) => {
  // One set of buffers for the lifetime of the layer — the line object identity
  // never changes (growth replaces the geometry's attributes in place), so the
  // <primitive> below is stable.
  const buffersRef = useRef<EdgeLayerBuffers | null>(null);
  if (!buffersRef.current) buffersRef.current = createEdgeBuffers(EDGE_INITIAL_CAPACITY);

  // Resolve per-edge metadata only when the edge/agent maps or base colour change
  // (BotsVisualization mints fresh Maps per data update) — never per frame.
  const meta = useMemo<EdgeMeta[]>(() => {
    const out: EdgeMeta[] = [];
    edges.forEach((edge) => {
      const src = agents.get(edge.source);
      const tgt = agents.get(edge.target);
      const avg = computeAvgTokenRate(src?.tokenRate, tgt?.tokenRate);

      const chain = chainPaymentStyle(edge);
      if (chain) {
        _edgeColorScratch.set(chain.color);
        const { r, g, b } = _edgeColorScratch;
        out.push({
          srcId: edge.source,
          tgtId: edge.target,
          activeR: r, activeG: g, activeB: b,
          inactiveR: r, inactiveG: g, inactiveB: b,
          activeOpacity: chain.opacity,
          inactiveOpacity: chain.opacity,
          shouldPulse: false,
          lastMessageTime: edge.lastMessageTime,
        });
        return;
      }

      _edgeColorScratch.set(computeEdgeColor(true, avg, color));
      const activeR = _edgeColorScratch.r;
      const activeG = _edgeColorScratch.g;
      const activeB = _edgeColorScratch.b;

      _edgeColorScratch.set(color);
      const inactiveR = _edgeColorScratch.r;
      const inactiveG = _edgeColorScratch.g;
      const inactiveB = _edgeColorScratch.b;

      out.push({
        srcId: edge.source,
        tgtId: edge.target,
        activeR, activeG, activeB,
        inactiveR, inactiveG, inactiveB,
        activeOpacity: computeEdgeOpacity(true, avg),
        inactiveOpacity: computeEdgeOpacity(false, avg),
        shouldPulse: shouldEdgePulse(avg, edge.messageCount),
        lastMessageTime: edge.lastMessageTime,
      });
    });
    return out;
  }, [edges, agents, color]);

  // Latest meta for the frame loop (ref pattern — decouples the useFrame closure
  // from render identity, cf. GlassEdges' glowSettingsRef).
  const metaRef = useRef<EdgeMeta[]>(meta);
  metaRef.current = meta;

  // Dispose GPU resources on unmount (R3F <primitive> never auto-disposes).
  useEffect(() => {
    return () => {
      const b = buffersRef.current;
      if (b) {
        b.geometry.dispose();
        b.material.dispose();
      }
    };
  }, []);

  useFrame((state) => {
    const b = buffersRef.current;
    if (!b) return;
    const metaArr = metaRef.current;
    const edgeCount = metaArr.length;

    // Grow-by-double if the swarm gained edges past capacity. Rare event — the
    // only frame that allocates (matches GlassEdges' reallocate-in-hot-path).
    if (edgeCount > b.capacity) {
      const nextCap = growEdgeCapacity(b.capacity, edgeCount);
      const positions = new Float32Array(nextCap * 6);
      const colors = new Float32Array(nextCap * 6);
      b.geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
      b.geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));
      b.positions = positions;
      b.colors = colors;
      b.capacity = nextCap;
    }

    const positions = b.positions;
    const colors = b.colors;
    const posMap = positionsRef.current;
    const nowMs = Date.now();
    const pulseWave = Math.sin(state.clock.elapsedTime * 5) * 0.3 + 1;

    // Compact write: only edges with both endpoints resolved take a slot, so the
    // draw range never covers a degenerate segment.
    let w = 0;
    for (let i = 0; i < edgeCount; i++) {
      const m = metaArr[i];
      const s = posMap.get(m.srcId);
      const t = posMap.get(m.tgtId);
      if (!s || !t) continue;

      const active = isEdgeActive(m.lastMessageTime, nowMs);
      const opacity = active ? m.activeOpacity : m.inactiveOpacity;
      const pulse = m.shouldPulse ? pulseWave : 1;
      const intensity = opacity * pulse;
      const r = (active ? m.activeR : m.inactiveR) * intensity;
      const g = (active ? m.activeG : m.inactiveG) * intensity;
      const bl = (active ? m.activeB : m.inactiveB) * intensity;

      const p = w * 6;
      positions[p]     = s.x; positions[p + 1] = s.y; positions[p + 2] = s.z;
      positions[p + 3] = t.x; positions[p + 4] = t.y; positions[p + 5] = t.z;
      colors[p]     = r; colors[p + 1] = g; colors[p + 2] = bl;
      colors[p + 3] = r; colors[p + 4] = g; colors[p + 5] = bl;
      w++;
    }

    (b.geometry.attributes.position as THREE.BufferAttribute).needsUpdate = true;
    (b.geometry.attributes.color as THREE.BufferAttribute).needsUpdate = true;
    b.geometry.setDrawRange(0, w * 2);
  });

  return <primitive object={buffersRef.current.line} />;
};

export default BotsEdges;
