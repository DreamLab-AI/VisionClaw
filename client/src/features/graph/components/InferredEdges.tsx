/**
 * InferredEdges — R3F overlay that renders edges from the inferred named graph
 * (`urn:ngm:graph:ontology:inferred`) in a visually-distinct style: dashed,
 * amber. Mirrors ontosphere's amber-dashed convention (PRD-018 WS-2, ADR-099 D4).
 *
 * Why a separate overlay (reuse-first, GPU-only-solving compliant):
 *  - It does NOT modify the optimised GlassEdges instanced pipeline or the
 *    GraphManager per-frame edge hot loop — zero risk to the asserted-edge path.
 *  - It does NO solving/layout. It reads node positions from the SAB-backed
 *    `nodePositionsRef` (the same buffer GlassEdges/labels read) and draws line
 *    segments between the inferred (source → target) node pairs each frame.
 *  - Gated by the `showInferred` toggle. Empty inferred set → renders nothing.
 *
 * Positions: `nodePositionsRef.current` is a Float32Array laid out [x,y,z] per
 * node, indexed by the node's render index via `nodeIdToIndexMap`.
 */

import React, { useMemo, useRef, useEffect } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import { useInferredEdgesStore } from '../../ontology/store/useInferredEdgesStore';

/** Default differentiated style — amber, dashed. Matches ADR-099 D4 convention. */
const INFERRED_COLOR = 0xffb000; // amber
const DASH_SIZE = 1.4;
const GAP_SIZE = 0.9;
const LINE_OPACITY = 0.85;

interface InferredEdgesProps {
  /** SAB-backed node positions, [x,y,z] per node, shared with GlassEdges. */
  nodePositionsRef: React.MutableRefObject<Float32Array | null>;
  /** Map from string node id to its index in the position buffer. */
  nodeIdToIndexMap: Map<string, number>;
}

export const InferredEdges: React.FC<InferredEdgesProps> = ({
  nodePositionsRef,
  nodeIdToIndexMap,
}) => {
  const showInferred = useInferredEdgesStore((s) => s.showInferred);
  const inferredEdges = useInferredEdgesStore((s) => s.inferredEdges);

  // Resolve each inferred edge to a (srcIndex, tgtIndex) pair once per data
  // change. Edges whose endpoints aren't in the current render set are dropped.
  const resolved = useMemo(() => {
    const pairs: Array<[number, number]> = [];
    for (const e of inferredEdges) {
      const si = nodeIdToIndexMap.get(String(e.sourceId));
      const ti = nodeIdToIndexMap.get(String(e.targetId));
      if (si !== undefined && ti !== undefined) pairs.push([si, ti]);
    }
    return pairs;
  }, [inferredEdges, nodeIdToIndexMap]);

  // Pre-allocated geometry sized to the resolved edge count (2 verts/edge).
  const geometry = useMemo(() => {
    const geo = new THREE.BufferGeometry();
    const positions = new Float32Array(Math.max(resolved.length, 1) * 2 * 3);
    geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    return geo;
  }, [resolved.length]);

  const material = useMemo(
    () =>
      new THREE.LineDashedMaterial({
        color: INFERRED_COLOR,
        dashSize: DASH_SIZE,
        gapSize: GAP_SIZE,
        transparent: true,
        opacity: LINE_OPACITY,
        depthWrite: false,
      }),
    [],
  );

  const linesRef = useRef<THREE.LineSegments | null>(null);

  // Dispose GPU resources on unmount / geometry swap.
  useEffect(() => {
    return () => {
      geometry.dispose();
    };
  }, [geometry]);
  useEffect(() => {
    return () => {
      material.dispose();
    };
  }, [material]);

  // Per-frame: pull current positions from the SAB and update the line buffer.
  useFrame(() => {
    if (!showInferred || resolved.length === 0) {
      if (linesRef.current) linesRef.current.visible = false;
      return;
    }
    const positions = nodePositionsRef.current;
    if (!positions) return;

    const attr = geometry.getAttribute('position') as THREE.BufferAttribute;
    const arr = attr.array as Float32Array;
    let w = 0;
    for (let i = 0; i < resolved.length; i++) {
      const [si, ti] = resolved[i];
      const so = si * 3;
      const to = ti * 3;
      arr[w++] = positions[so];
      arr[w++] = positions[so + 1];
      arr[w++] = positions[so + 2];
      arr[w++] = positions[to];
      arr[w++] = positions[to + 1];
      arr[w++] = positions[to + 2];
    }
    attr.needsUpdate = true;
    geometry.setDrawRange(0, resolved.length * 2);
    if (linesRef.current) {
      linesRef.current.visible = true;
      // Dashes require per-vertex line distances; recompute as positions move.
      linesRef.current.computeLineDistances();
    }
  });

  if (resolved.length === 0) return null;

  return (
    <lineSegments ref={linesRef} frustumCulled={false}>
      <primitive object={geometry} attach="geometry" />
      <primitive object={material} attach="material" />
    </lineSegments>
  );
};

export default InferredEdges;
