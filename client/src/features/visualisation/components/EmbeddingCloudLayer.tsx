/**
 * EmbeddingCloudLayer — the live RuVector memory cloud.
 *
 * Points come from `GET /api/memory-cloud` (a stratified, PCA-projected sample
 * of the sidecar's real embeddings) through the memory explorer store; the
 * retired static `/embedding-cloud.json` is gone. `memory_flash` events light
 * the flashed entry when it is in the sample, a few rows of its namespace
 * when only the namespace is, and nothing otherwise — unmatched flashes are
 * counted for the explorer HUD rather than landing on a random point.
 *
 * The cloud frames itself on the graph (cloudFrame.ts): an outer group sits at
 * the graph's robust centre and scales the cloud's robust radius to the
 * graph's (times cloudScale / 5); an inner group recentres the cloud on its
 * dense core. Points draw as round sprites. With Graph Separation > 0 the
 * cloud glides to the memory vertex of the separated-layout triangle
 * (ADR-2135), sized to one graph; route, beads, bursts and the camera rig
 * live inside the groups, so they follow it.
 *
 * While a query route is shown the cloud stops rotating, points off the route
 * dim (focus pull), and TrajectoryLayer draws the route inside the inner group
 * so it scales and turns with the cloud. MemoryCameraRig flies the camera to a
 * point on request, frames each new route, and runs the cinematic director.
 */

import React, { useEffect, useRef, useMemo, useState, useCallback } from 'react';
import * as THREE from 'three';
import { useFrame } from '@react-three/fiber';
import { Html } from '@react-three/drei';
import { useSettingsStore } from '@/store/settingsStore';
import { useWebSocketStore } from '@/store/websocketStore';
import type { EmbeddingCloudSettings } from '../../settings/config/settings';
import {
  memoryActionProfile,
  semanticBurstColor,
  type BurstProfile,
} from '../semanticEncoding';
import { useMemoryCloudStore } from '../memoryCloud/memoryCloudInstance';
import { buildCloudColours, buildIndexMaps, resolveFlashTargets, applyFocusDim, burstFrame, type IndexMaps } from '../memoryCloud/cloudData';
import { directorClock } from '../memoryCloud/memoryCloudStore';
import TrajectoryLayer from '../memoryCloud/TrajectoryLayer';
import MemoryCameraRig from '../memoryCloud/MemoryCameraRig';
import { useReducedMotion } from '../memoryCloud/useReducedMotion';
import { cloudPlacement, cloudPointSize, discSpritePixels, graphBoundsFor } from '../memoryCloud/cloudFrame';
import { robustBounds, type RobustBounds } from '@/utils/robustBounds';
import { sharedNodePositions, sharedNodeIdToIndexMap } from '../../graph/contexts/NodePositionContext';
import { graphDataManager } from '../../graph/managers/graphDataManager';

interface EmbeddingCloudProps {
  enabled: boolean;
}

interface MemoryFlashEvent {
  key: string;
  namespace: string;
  action: string;
  timestamp: number;
}

// ── Flash burst config ──
// Per-burst scale/duration/motion now come from the memory-action profile in
// semanticEncoding; only the pool size and neutral init colour live here.
const BURST_POOL_SIZE = 64;       // max concurrent bursts
const BURST_INIT_COLOR = 0x9ad6ff; // neutral colour for idle pool meshes
// Seconds between concentric rings of a multi-ring burst (search/store ripple).
const RING_STAGGER = 0.14;
/** seconds for the focus-pull dimming to fade in or out */
const DIM_FADE = 0.6;
/** seconds between re-reads of the graph's extent while physics settles */
const GRAPH_BOUNDS_EVERY = 1;
/** seconds for the cloud to glide to a new placement */
const PLACE_GLIDE = 0.8;
const SPRITE_SIZE = 64;

/**
 * The graph's robust bounds: the live position buffer while physics runs,
 * else the positions in the cached graph data (no binary stream yet, or the
 * socket is down). Null when there is no graph. With the layout separated
 * the positions are folded into one graph's frame first (graphBoundsFor).
 */
function readGraphBounds(separation: number): RobustBounds | null {
  const n = sharedNodeIdToIndexMap.size;
  if (sharedNodePositions && n > 0) {
    const b = graphBoundsFor(sharedNodePositions, n, separation);
    if (b) return b;
  }
  const nodes = graphDataManager.getLastGraphData()?.nodes;
  if (!nodes?.length) return null;
  const flat = new Float32Array(nodes.length * 3);
  let k = 0;
  for (const node of nodes) {
    const p = node.position;
    if (!p) continue;
    flat[k++] = p.x;
    flat[k++] = p.y;
    flat[k++] = p.z;
  }
  return graphBoundsFor(flat, k / 3, separation);
}

interface BurstSlot {
  mesh: THREE.Mesh;
  startTime: number;
  active: boolean;
  delay: number;         // seconds before this (concentric) ring begins animating
  profile: BurstProfile; // semantic motion/scale/duration for this burst
}

const statusBox = (colour: string, background: string): React.CSSProperties => ({
  background,
  color: colour,
  padding: '8px 16px',
  borderRadius: 6,
  fontSize: 12,
  fontFamily: 'monospace',
  whiteSpace: 'nowrap',
});

const EmbeddingCloudLayer: React.FC<EmbeddingCloudProps> = ({ enabled }) => {
  /** outer: graph centre, scale, rotation */
  const placeRef = useRef<THREE.Group>(null);
  /** inner: cloud-local frame (recentred on the dense core); the route and rig use this */
  const groupRef = useRef<THREE.Group>(null);
  const placeState = useRef({ graph: null as RobustBounds | null, sinceRead: Infinity, placed: false, separation: 0 });
  const pointsRef = useRef<THREE.Points>(null);
  const [hovered, setHovered] = useState<{ index: number; point: THREE.Vector3 } | null>(null);
  const reducedMotion = useReducedMotion();

  const snapshot = useMemoryCloudStore((s) => s.snapshot);
  const status = useMemoryCloudStore((s) => s.status);
  const loadError = useMemoryCloudStore((s) => s.error);
  const run = useMemoryCloudStore((s) => s.query.run);
  const response = useMemoryCloudStore((s) => s.query.response);
  const cinematicActive = useMemoryCloudStore((s) => s.cinematic.active);

  // Index maps for flash targeting (rebuilt per snapshot)
  const maps = useRef<IndexMaps>({ byKey: new Map(), byNamespace: new Map() });

  // Burst effect pool
  const burstPool = useRef<BurstSlot[]>([]);
  const burstGroupRef = useRef<THREE.Group>(null);
  const burstNextSlot = useRef(0);

  const settings = useSettingsStore(
    s => s.settings?.visualisation?.embeddingCloud,
  ) as EmbeddingCloudSettings | undefined;

  const pointSize = settings?.pointSize ?? 7.5;
  const opacity = settings?.opacity ?? 0.6;
  const colorBy = settings?.colorBy ?? 'namespace';
  const rotationSpeed = settings?.rotationSpeed ?? 0.0005;
  const maxPoints = settings?.maxPoints ?? 50000;
  const cloudScale = settings?.cloudScale ?? 5.0;
  // Graph Separation (ADR-2135): the cloud takes the triangle's memory vertex.
  const rawSeparation = useSettingsStore(
    s => s.settings?.visualisation?.graphs?.knowledge?.physics?.graphSeparationX,
  );
  const separation = typeof rawSeparation === 'number' && Number.isFinite(rawSeparation) ? rawSeparation : 0;
  const routeGlow = settings?.routeGlow ?? 1.2;
  const showRejected = settings?.showRejected ?? true;
  const dimOffRoute = settings?.dimOffRoute ?? 0.75;
  const trajectoryView = settings?.trajectoryView ?? 'canopy';
  const playbackSpeed = settings?.playbackSpeed ?? 1;
  const learningEnabled = settings?.learningEnabled ?? true;
  const learningTargetRecall = settings?.learningTargetRecall ?? 0.9;
  const learningRate = settings?.learningRate ?? 0.2;

  // Load the live snapshot when the cloud is switched on.
  useEffect(() => {
    if (!enabled) return;
    const st = useMemoryCloudStore.getState();
    if (st.status === 'idle' || (st.status === 'error' && !st.snapshot)) void st.loadSnapshot();
  }, [enabled]);

  // Settings are the persisted source for the explorer's view, speed and learning.
  useEffect(() => { useMemoryCloudStore.getState().setView(trajectoryView); }, [trajectoryView]);
  useEffect(() => { useMemoryCloudStore.getState().setSpeed(playbackSpeed); }, [playbackSpeed]);
  useEffect(() => {
    useMemoryCloudStore.getState().setLearning({
      enabled: learningEnabled,
      targetRecall: learningTargetRecall,
      learningRate,
    });
  }, [learningEnabled, learningTargetRecall, learningRate]);

  useEffect(() => {
    maps.current = snapshot ? buildIndexMaps(snapshot.metadata) : { byKey: new Map(), byNamespace: new Map() };
    setHovered(null);
  }, [snapshot]);

  // Base colours per mode; the displayed buffer is base × focus dimming.
  const baseColours = useMemo(
    () => (snapshot ? buildCloudColours(snapshot, colorBy) : null),
    [snapshot, colorBy],
  );

  // Build point cloud geometry. The engine indexes every row, so rows past
  // maxPoints are hidden with the draw range rather than dropped.
  const geometry = useMemo(() => {
    if (!snapshot || !baseColours) return null;
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.Float32BufferAttribute(new Float32Array(snapshot.positions), 3));
    geo.setAttribute('color', new THREE.Float32BufferAttribute(new Float32Array(baseColours), 3));
    geo.setDrawRange(0, Math.min(snapshot.count, maxPoints));
    return geo;
  }, [snapshot, baseColours, maxPoints]);

  useEffect(() => () => geometry?.dispose(), [geometry]);

  // the cloud's own robust frame, cloud-local
  const cloudBounds = useMemo(
    () => (snapshot ? robustBounds(snapshot.positions, snapshot.count) : null),
    [snapshot],
  );
  useEffect(() => {
    placeState.current.sinceRead = Infinity;
    placeState.current.placed = false;
  }, [cloudBounds, cloudScale]);

  // round point sprite: a white disc, so vertex colours and size still apply
  const sprite = useMemo(() => {
    const t = new THREE.DataTexture(discSpritePixels(SPRITE_SIZE), SPRITE_SIZE, SPRITE_SIZE, THREE.RGBAFormat);
    t.magFilter = THREE.LinearFilter;
    t.minFilter = THREE.LinearFilter;
    t.generateMipmaps = false;
    t.needsUpdate = true;
    return t;
  }, []);
  useEffect(() => () => sprite.dispose(), [sprite]);

  // Rows that stay lit while a query is shown: the search tree, both top-k sets
  // and every sampled sidecar hit.
  const focusSet = useMemo(() => {
    if (!run) return null;
    const keep = new Set<number>();
    for (const n of run.tree.list) if (n.kept) keep.add(n.id);
    for (const i of run.result.top) keep.add(i);
    for (const i of run.exactTop) keep.add(i);
    for (const h of response?.sidecar.results ?? []) if (h.sampleIndex !== null) keep.add(h.sampleIndex);
    return keep;
  }, [run, response]);

  const dimRef = useRef({ current: 0, applied: -1, set: null as Set<number> | null });

  // Initialise burst pool (ring meshes that expand + fade)
  useEffect(() => {
    if (!enabled || !snapshot) return;
    const geo = new THREE.RingGeometry(0.8, 1.0, 32);
    const pool: BurstSlot[] = [];
    for (let i = 0; i < BURST_POOL_SIZE; i++) {
      const mat = new THREE.MeshBasicMaterial({
        color: BURST_INIT_COLOR,
        transparent: true,
        opacity: 0,
        side: THREE.DoubleSide,
        depthWrite: false,
        blending: THREE.AdditiveBlending,
      });
      const mesh = new THREE.Mesh(geo, mat);
      mesh.visible = false;
      mesh.renderOrder = 999;
      pool.push({ mesh, startTime: 0, active: false, delay: 0, profile: memoryActionProfile('access') });
    }
    burstPool.current = pool;
    const group = burstGroupRef.current;
    if (group) {
      while (group.children.length > 0) group.remove(group.children[0]);
      pool.forEach(s => group.add(s.mesh));
    }
    return () => {
      if (group) pool.forEach(s => group.remove(s.mesh));
      pool.forEach(s => (s.mesh.material as THREE.MeshBasicMaterial).dispose());
      geo.dispose();
      burstPool.current = [];
    };
  }, [enabled, snapshot]);

  // Spawn a burst at a cloud-local position, coloured + animated by the memory
  // action verb (store/retrieve/search/list/delete) and tinted by namespace.
  // High-weight verbs ripple as `profile.rings` staggered concentric rings.
  const spawnBurst = useCallback((localPos: THREE.Vector3, action?: string, namespace?: string) => {
    const pool = burstPool.current;
    if (pool.length === 0) return;
    const profile = memoryActionProfile(action);
    const now = performance.now();
    for (let r = 0; r < profile.rings; r++) {
      const slot = pool[burstNextSlot.current % pool.length];
      burstNextSlot.current++;
      slot.profile = profile;
      slot.delay = r * RING_STAGGER;
      slot.mesh.position.copy(localPos);
      slot.mesh.scale.setScalar(profile.motion === 'implode' ? profile.maxScale : 0.01);
      slot.mesh.visible = false;
      const mat = slot.mesh.material as THREE.MeshBasicMaterial;
      mat.opacity = 1.0;
      semanticBurstColor(mat.color, action, namespace);
      slot.startTime = now;
      slot.active = true;
    }
  }, []);

  // memory_flash → real keys, then namespace stand-ins, else counted as unmatched.
  useEffect(() => {
    if (!enabled || !snapshot) return;
    const positions = snapshot.positions;
    const limit = Math.min(snapshot.count, maxPoints);
    const unsubscribe = useWebSocketStore.getState().on('memoryFlash', (raw: unknown) => {
      const event = raw as MemoryFlashEvent;
      if (!event?.key && !event?.namespace) return;
      const { indices, match } = resolveFlashTargets(event, maps.current);
      useMemoryCloudStore.getState().recordFlash(match);
      for (const idx of indices) {
        if (idx >= limit) continue;
        spawnBurst(new THREE.Vector3(positions[idx * 3], positions[idx * 3 + 1], positions[idx * 3 + 2]), event.action, event.namespace);
      }
    });
    return unsubscribe;
  }, [enabled, snapshot, maxPoints, spawnBurst]);

  // Per-frame: rotation (paused while a route is shown), focus dimming, bursts.
  useFrame(({ camera }, dt) => {
    // placement: frame the cloud on the graph, re-reading its extent at 1 Hz
    const ps = placeState.current;
    const outer = placeRef.current;
    if (outer) {
      ps.sinceRead += dt;
      // a slider move re-reads at once: the fold depends on the separation
      if (ps.sinceRead >= GRAPH_BOUNDS_EVERY || ps.separation !== separation) {
        ps.sinceRead = 0;
        ps.separation = separation;
        ps.graph = readGraphBounds(separation);
      }
      const place = cloudPlacement(cloudBounds, ps.graph, cloudScale, separation);
      // glide while physics settles; snap on first placement or under reduced motion
      const f = !ps.placed || reducedMotion ? 1 : Math.min(1, dt / PLACE_GLIDE);
      outer.position.x += (place.position[0] - outer.position.x) * f;
      outer.position.y += (place.position[1] - outer.position.y) * f;
      outer.position.z += (place.position[2] - outer.position.z) * f;
      outer.scale.setScalar(outer.scale.x + (place.scale - outer.scale.x) * f);
      groupRef.current?.position.set(...place.offset);
      ps.placed = true;
      const mat = pointsRef.current?.material as THREE.PointsMaterial | undefined;
      if (mat) mat.size = cloudPointSize(pointSize, outer.scale.x);
    }

    const routeShown = !!run;
    if (outer && rotationSpeed > 0 && !routeShown && !cinematicActive && !directorClock.active && !reducedMotion) {
      outer.rotation.y += rotationSpeed;
    }

    // focus pull: fade the dim amount in/out and rewrite colours only while it moves
    const d = dimRef.current;
    if (focusSet) d.set = focusSet;
    const goal = focusSet ? dimOffRoute : 0;
    const step = reducedMotion ? 1 : Math.min(1, dt / DIM_FADE);
    d.current += (goal - d.current) * (Math.abs(goal - d.current) < 0.005 ? 1 : step);
    if (geometry && baseColours && (Math.abs(d.current - d.applied) > 0.002 || d.applied < 0)) {
      const attr = geometry.getAttribute('color') as THREE.BufferAttribute;
      applyFocusDim(baseColours, attr.array as Float32Array, d.set, d.current);
      attr.needsUpdate = true;
      d.applied = d.current;
      if (!focusSet && d.current === 0) d.set = null;
    }

    // Animate burst rings: expand/implode + fade + billboard, per-slot semantic
    const now = performance.now();
    for (const slot of burstPool.current) {
      if (!slot.active) continue;
      const { duration } = slot.profile;
      const elapsed = (now - slot.startTime) / 1000 - slot.delay;
      if (elapsed < 0) {
        slot.mesh.visible = false;
        continue;
      }
      if (elapsed >= duration) {
        slot.active = false;
        slot.mesh.visible = false;
        continue;
      }
      slot.mesh.visible = true;
      const f = burstFrame(elapsed / duration, slot.profile, reducedMotion);
      slot.mesh.scale.setScalar(f.scale);
      (slot.mesh.material as THREE.MeshBasicMaterial).opacity = f.alpha;
      slot.mesh.quaternion.copy(camera.quaternion);
    }
  });

  // Colours change when the base changes: force a rewrite next frame.
  useEffect(() => { dimRef.current.applied = -1; }, [baseColours, geometry]);

  const onPointerMove = useCallback(
    (e: THREE.Event & { index?: number; point?: THREE.Vector3 }) => {
      if (e.index != null && e.point && snapshot) {
        setHovered({ index: e.index, point: e.point.clone() });
      }
    },
    [snapshot],
  );

  const onPointerOut = useCallback(() => setHovered(null), []);

  if (!enabled) return null;

  if (!snapshot && (status === 'loading' || status === 'idle')) {
    return (
      <Html center>
        <div style={statusBox('#7ef0cf', 'rgba(0,0,0,0.7)')}>Loading memory cloud…</div>
      </Html>
    );
  }

  if (!snapshot && status === 'error') {
    return (
      <Html center>
        <div style={statusBox('#ff6666', 'rgba(80,0,0,0.7)')}>Memory cloud unavailable: {loadError}</div>
      </Html>
    );
  }

  if (!snapshot || !geometry) return null;

  const hoveredMeta = hovered ? snapshot.metadata[hovered.index] : undefined;
  const worldHover = hovered && groupRef.current ? groupRef.current.worldToLocal(hovered.point.clone()) : null;

  return (
    <group ref={placeRef} name="embedding-cloud-layer">
    <group ref={groupRef} name="embedding-cloud-local">
      <points
        ref={pointsRef}
        geometry={geometry}
        onPointerMove={onPointerMove}
        onPointerOut={onPointerOut}
      >
        <pointsMaterial
          size={cloudPointSize(pointSize, placeRef.current?.scale.x ?? cloudScale)}
          opacity={opacity}
          map={sprite}
          alphaTest={0.02}
          transparent
          vertexColors
          sizeAttenuation
          depthWrite={false}
        />
      </points>
      {/* Burst ring pool — lives inside the cloud group so it scales/rotates with it */}
      <group ref={burstGroupRef} />
      <TrajectoryLayer
        cloudPositions={snapshot.positions}
        glow={routeGlow}
        showRejected={showRejected}
        reducedMotion={reducedMotion}
      />
      <MemoryCameraRig cloudGroup={groupRef} positions={snapshot.positions} />
      {hoveredMeta && worldHover && (
        <Html position={worldHover} center style={{ pointerEvents: 'none' }}>
          <div
            style={{
              background: 'rgba(0,0,0,0.85)',
              color: '#fff',
              padding: '4px 8px',
              borderRadius: 4,
              fontSize: 11,
              whiteSpace: 'nowrap',
              maxWidth: 300,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
          >
            <div><b>{hoveredMeta.key}</b></div>
            <div style={{ opacity: 0.7 }}>
              {hoveredMeta.namespace} / {hoveredMeta.sourceType}
              {hoveredMeta.updatedAt ? ` · ${new Date(hoveredMeta.updatedAt).toLocaleDateString('en-GB')}` : ''}
            </div>
          </div>
        </Html>
      )}
    </group>
    </group>
  );
};

export default EmbeddingCloudLayer;
