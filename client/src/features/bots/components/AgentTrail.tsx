/**
 * AgentTrail.tsx
 * A fading motion-trail ribbon behind a single agent body — "watch the swarm move".
 *
 * The trail source is the agent's marker position, the one the marker is drawn at and its beams start
 * from (`agentMarkerPosition`, the marker pass's map: ADR-2142 / ADR-2135).
 * This layer is mounted as a SIBLING of the moving agent group, so its
 * vertices live in the same parent (world) space and stay put while the agent flies
 * on — a genuine trail, not a rigid attachment.
 *
 * Sampling is time-gated (~120ms, not per-frame) and displacement-gated: an idle
 * agent (displacement < epsilon) grows no trail, and a moving one decays its tail by
 * ring-buffer rotation (the fixed-length buffer overwrites the oldest sample).
 *
 * Rendering follows the repo idioms (cf. TransientBeamsLayer, AgentCollabEdges): a single
 * THREE.Line whose position + colour attributes are pre-allocated once and mutated in
 * place — zero per-frame allocation. The oldest→newest fade is baked into the vertex
 * colours under additive blending (the bioluminescent idiom shared with the agent
 * overlay), so a fading vertex tends to black = transparent, exactly the
 * opacity-envelope shape TransientBeamsLayer uses for its beams.
 */
import React, { useMemo, useRef, useEffect, useLayoutEffect } from 'react';
import * as THREE from 'three';
import { agentMarkerPosition } from '../agentMarkerPass';
import { useFrame } from '@react-three/fiber';

/* R3F maps three.js props onto JSX host elements (args, position, rotation, intensity...);
   these are not DOM properties. react/no-unknown-property is not enforced in this config. */

// ---------------------------------------------------------------------------
// Pure ring-buffer + sampling helpers (unit-tested in AgentTrail.test.ts)
// ---------------------------------------------------------------------------

export interface Vec3Like {
  x: number;
  y: number;
  z: number;
}

/** A fixed-capacity circular buffer of sampled positions (x,y,z interleaved). */
export interface TrailRing {
  /** capacity * 3 floats, written circularly at `head`. */
  positions: Float32Array;
  capacity: number;
  /** Number of valid samples held (0..capacity). */
  count: number;
  /** Index of the next write slot (0..capacity-1). */
  head: number;
}

/** Trail length bounds — mirrored by the Agents-group `trailLength` slider. */
export const TRAIL_MIN_LENGTH = 8;
export const TRAIL_MAX_LENGTH = 48;
export const TRAIL_DEFAULT_LENGTH = 24;

/** Sample cadence in ms — deliberately coarse so the trail is cheap, not per-frame. */
export const TRAIL_SAMPLE_MS = 120;
/** Minimum inter-sample displacement (world units) below which the agent is "idle". */
export const TRAIL_EPSILON = 0.05;
/** Peak vertex intensity (newest end) under additive blending. */
export const TRAIL_MAX_INTENSITY = 0.85;

/** Allocate an empty ring of the given capacity (clamped to ≥1). */
export function createTrailRing(capacity: number): TrailRing {
  const cap = Math.max(1, capacity | 0);
  return { positions: new Float32Array(cap * 3), capacity: cap, count: 0, head: 0 };
}

/**
 * Sampling gate: true when there is no previous sample (seed the trail) or the agent
 * has moved at least `epsilon` since the last sample. Compares squared distance to
 * avoid a sqrt in the hot path.
 */
export function shouldSample(
  prev: Vec3Like | null | undefined,
  next: Vec3Like,
  epsilon: number,
): boolean {
  if (!prev) return true;
  const dx = next.x - prev.x;
  const dy = next.y - prev.y;
  const dz = next.z - prev.z;
  return dx * dx + dy * dy + dz * dz >= epsilon * epsilon;
}

/**
 * Push one sample into the ring. Writes at `head`, advances it modulo capacity, and
 * grows `count` until the buffer is full — thereafter every push overwrites the
 * oldest sample (the tail decays by rotation). Zero allocation.
 */
export function pushSample(ring: TrailRing, x: number, y: number, z: number): void {
  const base = ring.head * 3;
  ring.positions[base] = x;
  ring.positions[base + 1] = y;
  ring.positions[base + 2] = z;
  ring.head = (ring.head + 1) % ring.capacity;
  if (ring.count < ring.capacity) ring.count += 1;
}

/**
 * Copy the ring's live samples into `out` in oldest→newest order and return the
 * count. `out` must hold at least `count * 3` floats.
 */
export function copyOrdered(ring: TrailRing, out: Float32Array): number {
  const { count, capacity, head, positions } = ring;
  // When full, the oldest sample sits at `head` (next to be overwritten); before the
  // buffer fills it starts at 0.
  const start = count < capacity ? 0 : head;
  for (let i = 0; i < count; i++) {
    const src = ((start + i) % capacity) * 3;
    const dst = i * 3;
    out[dst] = positions[src];
    out[dst + 1] = positions[src + 1];
    out[dst + 2] = positions[src + 2];
  }
  return count;
}

/**
 * Oldest→newest fade envelope over t∈[0,1] (0 = tail, 1 = head, nearest the agent).
 * Quadratic ease so the tail fades faster than the head — the same fade-curve shape as
 * TransientBeamsLayer's opacity envelope, applied monotonically for a trail.
 */
export function trailFade(t: number): number {
  if (t <= 0) return 0;
  if (t >= 1) return 1;
  return t * t;
}

/** Paint `count` vertices oldest→newest with the base colour scaled by the fade. */
function writeTrailColors(
  out: Float32Array,
  count: number,
  color: THREE.Color,
  maxIntensity: number,
): void {
  for (let i = 0; i < count; i++) {
    const t = count > 1 ? i / (count - 1) : 1;
    const f = trailFade(t) * maxIntensity;
    const o = i * 3;
    out[o] = color.r * f;
    out[o + 1] = color.g * f;
    out[o + 2] = color.b * f;
  }
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

/** The point a trail samples for an agent: its marker position, and nothing else. */
export function trailSamplePoint(wireId: number | string, out: { set(x: number, y: number, z: number): unknown }): boolean {
  return agentMarkerPosition(wireId, out);
}

/** Scratch for the per-frame sample; the trail copies out of it, so one is shared. */
const _sample = new THREE.Vector3();

export interface AgentTrailProps {
  /** The agent's wire id; its marker position is sampled each frame (see `trailSamplePoint`). */
  wireId: number;
  /** Trail colour — the agent's status colour (with swarm tint already applied when enabled). */
  color: string;
  /** Ring capacity (samples retained); clamped to [TRAIL_MIN_LENGTH, TRAIL_MAX_LENGTH]. */
  length?: number;
  /** Sample cadence (ms). */
  sampleIntervalMs?: number;
  /** Idle threshold (world units) below which no sample is taken. */
  epsilon?: number;
  /** Peak vertex intensity at the head. */
  maxIntensity?: number;
}

export const AgentTrail: React.FC<AgentTrailProps> = ({
  wireId,
  color,
  length = TRAIL_DEFAULT_LENGTH,
  sampleIntervalMs = TRAIL_SAMPLE_MS,
  epsilon = TRAIL_EPSILON,
  maxIntensity = TRAIL_MAX_INTENSITY,
}) => {
  const capacity = Math.max(TRAIL_MIN_LENGTH, Math.min(length | 0, TRAIL_MAX_LENGTH));

  // Per-agent trail object, rebuilt only when the capacity setting changes. Geometry,
  // material and both attribute arrays are allocated here and mutated in place inside
  // useFrame — no per-frame allocation. Built imperatively (not via the ambiguous
  // R3F `<line>` intrinsic, which collides with SVGLineElement typing) and mounted
  // through `<primitive>`.
  const trail = useMemo(() => {
    const positions = new Float32Array(capacity * 3);
    const colors = new Float32Array(capacity * 3);
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
    const line = new THREE.Line(geometry, material);
    line.frustumCulled = false;
    return { line, geometry, material, positions, colors, ring: createTrailRing(capacity) };
  }, [capacity]);

  // Dispose the GPU resources of the *previous* trail when capacity changes / on unmount.
  useEffect(
    () => () => {
      trail.geometry.dispose();
      trail.material.dispose();
    },
    [trail],
  );

  const baseColor = useMemo(() => new THREE.Color(color), [color]);
  const lastSampleRef = useRef(-Infinity);
  const prevSampleRef = useRef(new THREE.Vector3());
  const hasSampleRef = useRef(false);
  const countRef = useRef(0);

  // Reset the sampling gate whenever the buffers are rebuilt (capacity change) so the
  // fresh, empty trail re-seeds at the agent's current position immediately.
  useLayoutEffect(() => {
    hasSampleRef.current = false;
    lastSampleRef.current = -Infinity;
    countRef.current = 0;
  }, [trail]);

  // Recolour the live vertices when the status colour (or its swarm tint) changes,
  // without waiting for the next positional sample.
  useEffect(() => {
    writeTrailColors(trail.colors, countRef.current, baseColor, maxIntensity);
    (trail.geometry.attributes.color as THREE.BufferAttribute).needsUpdate = true;
  }, [baseColor, trail, maxIntensity]);

  useFrame((state) => {
    const now = state.clock.elapsedTime;
    if (now - lastSampleRef.current < sampleIntervalMs / 1000) return;

    const p = _sample;
    if (!trailSamplePoint(wireId, p)) return;
    const prev = hasSampleRef.current ? prevSampleRef.current : null;
    // Idle agents grow no trail; only a genuine move seeds/extends the buffer.
    if (!shouldSample(prev, p, epsilon)) return;

    lastSampleRef.current = now;
    prevSampleRef.current.copy(p);
    hasSampleRef.current = true;

    pushSample(trail.ring, p.x, p.y, p.z);
    const count = copyOrdered(trail.ring, trail.positions);
    countRef.current = count;
    writeTrailColors(trail.colors, count, baseColor, maxIntensity);

    (trail.geometry.attributes.position as THREE.BufferAttribute).needsUpdate = true;
    (trail.geometry.attributes.color as THREE.BufferAttribute).needsUpdate = true;
    trail.geometry.setDrawRange(0, count);
  });

  return <primitive object={trail.line} />;
};

export default AgentTrail;
