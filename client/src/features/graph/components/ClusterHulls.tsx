import { getDomainColor, DEFAULT_DOMAIN_COLOR as DEFAULT_COLOR } from '../utils/domainColors';
import React, { useRef, useMemo, useEffect, useState } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import { ConvexGeometry } from 'three/examples/jsm/geometries/ConvexGeometry.js';
import {
  nodeAnalyticsStore,
  ANALYTICS_STRIDE,
  ANALYTICS_CLUSTER_OFFSET,
  ANALYTICS_COMMUNITY_OFFSET,
} from '../../analytics/store/nodeAnalyticsStore';

// ============================================================================
// Types
// ============================================================================

interface ClusterHullsProps {
  nodes: Array<{
    id: string;
    label?: string;
    metadata?: any;
    position?: { x: number; y: number; z: number };
  }>;
  nodePositionsRef: React.RefObject<Float32Array | null>;
  nodeIdToIndexMap: Map<string, number>;
  settings: any;
}

// ============================================================================
// Constants
// ============================================================================

const MIN_CLUSTER_SIZE = 4;
const TICK_INTERVAL = 30;
// GPU Louvain on this graph yields ~2000 communities (mostly singletons/tiny);
// drawing every cluster >= MIN_CLUSTER_SIZE renders 800+ overlapping translucent
// hulls — opaque mush, not inspectable. Cap to the N largest communities so
// distinct regions stay legible in 3D. Overridable via clusterHulls.maxHulls.
const DEFAULT_MAX_HULLS = 32;

// Spatial-clustering target bucket count when no server cluster_id exists.
// Over-provisioned vs maxHulls so the N largest occupied cells are dense and
// well-separated; the long tail of sparse cells is dropped by MIN_CLUSTER_SIZE.
const SPATIAL_TARGET_CELLS = 96;

// ============================================================================
// Helpers
// ============================================================================

// ADR-031 D6: the label/domain cluster-key fabrication heuristic was removed.
// Hulls now group strictly by server-provided cluster_id; unclustered nodes
// get no hull. The numeric cluster palette below colours the server clusters.

// GPU cluster palette for numeric cluster IDs
const GPU_CLUSTER_COLORS = [
  '#4FC3F7', '#81C784', '#FFB74D', '#CE93D8', '#FFD54F',
  '#EF5350', '#4DB6AC', '#FF7043', '#78909C', '#AED581',
  '#F48FB1', '#80DEEA', '#FFCC80', '#B39DDB', '#A5D6A7',
  '#90CAF9', '#FFAB91', '#80CBC4', '#FFF176', '#E6EE9C',
];

function getDomainHullColor(domain: string): string {
  // Handle GPU cluster IDs (cluster-1, cluster-2, etc.)
  const clusterMatch = domain.match(/^cluster-(\d+)$/);
  if (clusterMatch) {
    const idx = parseInt(clusterMatch[1], 10) % GPU_CLUSTER_COLORS.length;
    return GPU_CLUSTER_COLORS[idx];
  }
  return getDomainColor(domain);
}

// ADR-031 D6: Louvain community hull colour. Matches GemNodes' community node
// hue (multiply-by-83 mod 360, HSL 0.65/0.5) so a hull and its member nodes
// read as the same region.
const _communityHullColor = new THREE.Color();
function getCommunityHullColor(communityId: number): string {
  const hue = ((communityId * 83) % 360) / 360;
  return `#${_communityHullColor.setHSL(hue, 0.65, 0.5).getHexString()}`;
}

interface ClusterPoint {
  id: string;
  x: number;
  y: number;
  z: number;
}

/**
 * Spatial grid clustering over the live node positions.
 *
 * When no server-side cluster_id is available, semantic-domain grouping is
 * useless for hulls: a domain (e.g. "AI") is a global tag whose members are
 * scattered across the entire disc, so its convex hull blankets the whole
 * population. The force-directed layout, however, already places densely
 * connected nodes near each other — spatial proximity IS the community
 * structure the user sees. Bucketing positions into a grid yields disjoint,
 * tight, non-overlapping clusters by construction.
 *
 * Grid resolution per axis is proportional to that axis's extent so an
 * anisotropic population (the disc spans z far wider than x/y) is subdivided
 * evenly in world space rather than by index, targeting ~targetCells buckets.
 */
function buildSpatialClusters(
  pts: ClusterPoint[],
  targetCells: number,
): Map<string, string[]> {
  const map = new Map<string, string[]>();
  if (pts.length === 0) return map;

  let minX = Infinity, minY = Infinity, minZ = Infinity;
  let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
  for (const p of pts) {
    if (p.x < minX) minX = p.x; if (p.x > maxX) maxX = p.x;
    if (p.y < minY) minY = p.y; if (p.y > maxY) maxY = p.y;
    if (p.z < minZ) minZ = p.z; if (p.z > maxZ) maxZ = p.z;
  }
  const ex = Math.max(maxX - minX, 1e-3);
  const ey = Math.max(maxY - minY, 1e-3);
  const ez = Math.max(maxZ - minZ, 1e-3);

  // Choose k so nx*ny*nz ≈ targetCells, with each nAxis ∝ axis extent.
  const k = Math.cbrt(targetCells / (ex * ey * ez));
  const nx = Math.max(1, Math.round(k * ex));
  const ny = Math.max(1, Math.round(k * ey));
  const nz = Math.max(1, Math.round(k * ez));
  const cellX = ex / nx;
  const cellY = ey / ny;
  const cellZ = ez / nz;

  for (const p of pts) {
    const ix = Math.min(nx - 1, Math.floor((p.x - minX) / cellX));
    const iy = Math.min(ny - 1, Math.floor((p.y - minY) / cellY));
    const iz = Math.min(nz - 1, Math.floor((p.z - minZ) / cellZ));
    const key = `sp-${ix}-${iy}-${iz}`;
    let arr = map.get(key);
    if (!arr) { arr = []; map.set(key, arr); }
    arr.push(p.id);
  }
  return map;
}

/**
 * Given a set of 3D points, compute padded points offset from centroid,
 * then return a ConvexGeometry. Returns null if fewer than MIN_CLUSTER_SIZE
 * valid points or the set stays degenerate.
 */
function buildHullGeometry(
  points: THREE.Vector3[],
  padding: number,
  slabThickness: number,
): ConvexGeometry | null {
  if (points.length < MIN_CLUSTER_SIZE) return null;

  // Centroid + radial padding (push points out so the hull breathes past nodes).
  const centroid = new THREE.Vector3();
  for (const p of points) centroid.add(p);
  centroid.divideScalar(points.length);

  const padded: THREE.Vector3[] = [];
  let minX = Infinity, minY = Infinity, minZ = Infinity;
  let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
  for (const p of points) {
    const q = new THREE.Vector3()
      .subVectors(p, centroid)
      .multiplyScalar(1 + padding)
      .add(centroid);
    padded.push(q);
    if (q.x < minX) minX = q.x; if (q.x > maxX) maxX = q.x;
    if (q.y < minY) minY = q.y; if (q.y > maxY) maxY = q.y;
    if (q.z < minZ) minZ = q.z; if (q.z > maxZ) maxZ = q.z;
  }

  // ConvexHull of a near-coplanar / near-collinear set is degenerate (throws or
  // yields a zero-volume sliver). Detect the THINNEST axis and, only if it's
  // flat relative to the other two, extrude along it by slabThickness. This
  // generalises the old z-only slab — the co-planar disc projection can leave
  // any axis as the degenerate one depending on view, not just z.
  const exts: Array<['x' | 'y' | 'z', number]> = [
    ['x', maxX - minX],
    ['y', maxY - minY],
    ['z', maxZ - minZ],
  ];
  exts.sort((a, b) => a[1] - b[1]);
  const [thinAxis, thinExt] = exts[0];
  const fatExt = exts[2][1];

  let hullPts = padded;
  if (thinExt < 0.05 * fatExt) {
    hullPts = [];
    for (const q of padded) {
      const a = q.clone();
      const b = q.clone();
      a[thinAxis] += slabThickness;
      b[thinAxis] -= slabThickness;
      hullPts.push(a, b);
    }
  }

  try {
    return new ConvexGeometry(hullPts);
  } catch {
    return null;
  }
}

// ============================================================================
// Component
// ============================================================================

export const ClusterHulls: React.FC<ClusterHullsProps> = ({
  nodes,
  nodePositionsRef,
  nodeIdToIndexMap,
  settings,
}) => {
  const groupRef = useRef<THREE.Group>(null);
  const frameCounter = useRef(0);
  const [tick, setTick] = useState(0);
  const analyticsRef = useRef<Float32Array | null>(null);

  // ---- Early exit if feature is disabled ----
  // Respect both the visual clusterHulls.enabled toggle AND qualityGates.showClusters
  const clusterHullsEnabled = settings?.visualisation?.clusterHulls?.enabled;
  const showClusters = settings?.qualityGates?.showClusters;
  // If qualityGates.showClusters is explicitly false, disable hulls even if clusterHulls.enabled is true
  const enabled = showClusters === false ? false : clusterHullsEnabled;
  const opacity = settings?.visualisation?.clusterHulls?.opacity ?? 0.08;
  const padding = settings?.visualisation?.clusterHulls?.padding ?? 0.15;
  const maxHulls = settings?.visualisation?.clusterHulls?.maxHulls ?? DEFAULT_MAX_HULLS;
  // Z half-thickness of each hull slab. After the disc projection flattens z to
  // ~3%, a cluster's points are near-coplanar; a 3D ConvexGeometry of coplanar
  // points is degenerate. Extruding ±slabThickness gives the hull real volume.
  const slabThickness = settings?.visualisation?.clusterHulls?.slabThickness ?? 35;

  // ---- Increment tick every TICK_INTERVAL frames and refresh analytics ----
  // Pull the per-node analytics buffer (cluster_id at stride 0) from the
  // main-thread analytics store (ADR-03 D7 "Phase 5"). The store is fed from
  // the V3 binary protocol: POST /api/analytics/clustering/run populates
  // server-side node_analytics, whose cluster_id rides every position frame at
  // wire offset 36. With no clustering run the store returns null and
  // getClusterKey falls back to domain heuristics.
  useFrame(() => {
    frameCounter.current += 1;
    if (frameCounter.current >= TICK_INTERVAL) {
      frameCounter.current = 0;
      const buf = nodeAnalyticsStore.getIndexedBuffer(nodeIdToIndexMap);
      if (buf) analyticsRef.current = buf;
      setTick((t) => t + 1);
    }
  });

  // ---- Group nodes into clusters ----
  // ADR-031 D6: the hull layer renders ONLY server-provided clusters by default.
  //   1. Server cluster_id (Louvain) — drawn for nodes with a real (>0) cluster_id.
  //      Nodes the server left unclustered are NOT fabricated into label/spatial
  //      hulls — that masking is exactly what D6 removes.
  //   2. Spatial-grid fallback — opt-in only (clusterHulls.spatialFallback,
  //      default off). When the server sends no clusters and the fallback is off,
  //      the layer renders nothing (explicit empty state), never fabricated hulls.
  const clusterMap = useMemo(() => {
    const analytics = analyticsRef.current;
    const positions = nodePositionsRef.current;

    // Detect which server signals are present (stride ANALYTICS_STRIDE).
    // cluster_id (DBSCAN/k-means) is the preferred grouping; community_id
    // (Louvain) is the fallback when no DBSCAN run has populated cluster_id.
    let hasClusterId = false;
    let hasCommunityId = false;
    if (analytics) {
      for (const idx of nodeIdToIndexMap.values()) {
        const a = idx * ANALYTICS_STRIDE;
        if (analytics[a + ANALYTICS_CLUSTER_OFFSET] > 0) hasClusterId = true;
        if (analytics[a + ANALYTICS_COMMUNITY_OFFSET] > 0) hasCommunityId = true;
        if (hasClusterId) break; // cluster_id wins outright; stop early
      }
    }

    const map = new Map<string, string[]>();
    const colorByKey = new Map<string, string>();

    if (hasClusterId) {
      // Group ONLY nodes the server actually clustered (cluster_id > 0). The
      // domain/label heuristics in getClusterKey are not used to fabricate hulls
      // for the unclustered remainder.
      for (let ni = 0; ni < nodes.length; ni++) {
        const node = nodes[ni];
        const nodeIndex = nodeIdToIndexMap.get(node.id);
        if (nodeIndex === undefined) continue;
        const clusterId = analytics![nodeIndex * ANALYTICS_STRIDE + ANALYTICS_CLUSTER_OFFSET];
        if (!(clusterId > 0)) continue;
        const key = `cluster-${clusterId}`;
        let arr = map.get(key);
        if (!arr) { arr = []; map.set(key, arr); colorByKey.set(key, getDomainHullColor(key)); }
        arr.push(node.id);
      }
      return { map, colorByKey };
    }

    // ADR-031 D6: no DBSCAN cluster_id, but the live graph carries Louvain
    // community_id. Community_id is real server structure, but Louvain optimises
    // graph modularity, not spatial locality — a community's members scatter
    // across the disc, so its convex hull blankets the whole graph and the
    // hulls overlap into an uninspectable blob. So community hulls are an opt-in
    // tier (default off), same as the fabricated spatial fallback; the honest
    // default community signal is per-node colour (colorScheme: 'community').
    // Cluster hulls (DBSCAN, spatially compact) remain default-on above.
    const communityFallback = settings?.visualisation?.clusterHulls?.communityFallback === true;
    if (hasCommunityId && communityFallback) {
      for (let ni = 0; ni < nodes.length; ni++) {
        const node = nodes[ni];
        const nodeIndex = nodeIdToIndexMap.get(node.id);
        if (nodeIndex === undefined) continue;
        const communityId = analytics![nodeIndex * ANALYTICS_STRIDE + ANALYTICS_COMMUNITY_OFFSET];
        if (!(communityId > 0)) continue;
        const key = `community-${communityId}`;
        let arr = map.get(key);
        if (!arr) { arr = []; map.set(key, arr); colorByKey.set(key, getCommunityHullColor(communityId)); }
        arr.push(node.id);
      }
      return { map, colorByKey };
    }

    // No server clusters. The JS spatial-grid heuristic fabricates clusters and
    // masks missing server data, so it is opt-in (default off). When off, return
    // an empty map → the layer shows nothing.
    const spatialFallback = settings?.visualisation?.clusterHulls?.spatialFallback === true;
    if (!spatialFallback || !positions) return { map, colorByKey };

    const pts: ClusterPoint[] = [];
    for (let ni = 0; ni < nodes.length; ni++) {
      const node = nodes[ni];
      const idx = nodeIdToIndexMap.get(node.id);
      if (idx === undefined) continue;
      const base = idx * 3;
      if (base + 2 >= positions.length) continue;
      const x = positions[base], y = positions[base + 1], z = positions[base + 2];
      if (Number.isNaN(x) || Number.isNaN(y) || Number.isNaN(z)) continue;
      pts.push({ id: node.id, x, y, z });
    }
    // Spatial clusters carry no semantic key; hullEntries colours them by rank.
    return { map: buildSpatialClusters(pts, SPATIAL_TARGET_CELLS), colorByKey };
    // Deps intentionally narrowed; react-hooks/exhaustive-deps is not enforced in this config.
  }, [nodes, nodeIdToIndexMap, nodePositionsRef, tick, settings?.visualisation?.clusterHulls?.spatialFallback, settings?.visualisation?.clusterHulls?.communityFallback]);

  // ---- Build hull geometries from current positions ----
  const hullEntries = useMemo(() => {
    if (!enabled) return [];

    const positions = nodePositionsRef.current;
    if (!positions) return [];

    const { map, colorByKey } = clusterMap;
    const entries: Array<{
      key: string;
      color: string;
      geometry: ConvexGeometry;
    }> = [];

    // Rank clusters by node count and keep only the N largest, so the dense
    // tail of tiny communities doesn't bury the graph in overlapping hulls.
    const ranked = Array.from(map.entries())
      .filter(([, nodeIds]) => nodeIds.length >= MIN_CLUSTER_SIZE)
      .sort((a, b) => b[1].length - a[1].length)
      .slice(0, maxHulls);

    let rank = 0;
    for (const [key, nodeIds] of ranked) {
      const points: THREE.Vector3[] = [];

      for (const id of nodeIds) {
        const idx = nodeIdToIndexMap.get(id);
        if (idx === undefined) continue;

        const base = idx * 3;
        if (base + 2 >= positions.length) continue;

        const x = positions[base];
        const y = positions[base + 1];
        const z = positions[base + 2];

        // Skip zero/NaN positions
        if (Number.isNaN(x) || Number.isNaN(y) || Number.isNaN(z)) continue;

        points.push(new THREE.Vector3(x, y, z));
      }

      const geometry = buildHullGeometry(points, padding, slabThickness);
      if (geometry) {
        // Spatial clusters carry no semantic identity, so the dominant-domain
        // colour collapses to one hue when the population is single-domain —
        // 32 distinct regions then read as one mass. Colour spatial hulls by
        // rank from the palette so neighbouring regions stay visually separable.
        // Real cluster_id / domain hulls keep their semantic colour.
        const color = key.startsWith('sp-')
          ? GPU_CLUSTER_COLORS[rank % GPU_CLUSTER_COLORS.length]
          : (colorByKey.get(key) ?? DEFAULT_COLOR);
        entries.push({ key, color, geometry });
      }
      rank++;
    }

    return entries;
    // tick drives periodic recomputation
    // Deps intentionally narrowed; react-hooks/exhaustive-deps is not enforced in this config.
  }, [enabled, clusterMap, nodeIdToIndexMap, nodePositionsRef, padding, maxHulls, slabThickness, tick]);

  // ---- Cleanup previous geometries when entries change ----
  const prevGeometries = useRef<THREE.BufferGeometry[]>([]);
  useEffect(() => {
    prevGeometries.current = hullEntries.map((e) => e.geometry);
    return () => {
      for (const geo of prevGeometries.current) {
        geo.dispose();
      }
    };
  }, [hullEntries]);

  if (!enabled) return null;

  return (
    <group ref={groupRef} renderOrder={1}>
      {hullEntries.map(({ key, color, geometry }) => (
        <mesh
          key={`hull-${key}`}
          geometry={geometry}
          renderOrder={1}
        >
          <meshBasicMaterial
            color={color}
            transparent={true}
            opacity={opacity}
            side={THREE.DoubleSide}
            depthWrite={false}
          />
        </mesh>
      ))}
    </group>
  );
};
