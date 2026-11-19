import React, { useRef, useMemo, useEffect } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import type { GemNodesHandle } from './GemNodes';

interface KnowledgeRingsProps {
  nodes: Array<{ id: string; metadata?: any; position?: { x: number; y: number; z: number } }>;
  perNodeVisualModeMap: Map<string, string>;
  nodePositionsRef: React.RefObject<Float32Array | null>;
  nodeIdToIndexMap: Map<string, number>;
  connectionCountMap: Map<string, number>;
  hierarchyMap: Map<string, any>;
  settings: any;
  /**
   * Handle to the GemNodes instanced mesh whose per-instance colours mirror the
   * node hue (computeColor). The ring reads that already-computed colour buffer
   * and reuses it for its own instanceColor — no colour recomputation here.
   */
  nodeColorSourceRef?: React.RefObject<GemNodesHandle | null>;
}

// Fallback ring tint used only until the node colour buffer is populated.
const RING_COLOR = '#4FC3F7';
const RING_OPACITY = 0.6;
const BASE_SCALE = 1.5;
const ROTATION_SPEED = 0.5;

// Module-scope scratch colour — reused every frame, never allocated in useFrame.
const _ringColor = new THREE.Color();

export const KnowledgeRings: React.FC<KnowledgeRingsProps> = ({
  nodes,
  perNodeVisualModeMap,
  nodePositionsRef,
  nodeIdToIndexMap,
  settings,
  nodeColorSourceRef,
}) => {
  const meshRef = useRef<THREE.InstancedMesh>(null);

  // Pre-allocated objects to avoid GC pressure
  const tempMatrix = useRef(new THREE.Matrix4());
  const tempPosition = useRef(new THREE.Vector3());
  const tempQuaternion = useRef(new THREE.Quaternion());
  const tempScale = useRef(new THREE.Vector3());
  const tempEuler = useRef(new THREE.Euler());

  // One-shot latch: ring colours mirror the GemNodes colour buffer, which is a
  // SEMANTIC attribute (type/quality/sssp/depth) — it only changes when node
  // data / settings change, never frame-to-frame. We seed instanceColor once
  // the source buffer is available and skip rewriting it every frame. Reset to
  // false whenever the semantic inputs (or the ring set) change so colours
  // re-seed against the freshly recomputed source buffer.
  const colorsSeededRef = useRef(false);

  const knowledgeNodes = useMemo(() => {
    return nodes.filter((node) => {
      const mode = perNodeVisualModeMap.get(node.id);
      // Only show rings for nodes positively identified as knowledge_graph.
      // Nodes without an explicit visual mode tag should NOT get rings —
      // this prevents ontology/agent nodes from getting rings when
      // the global graphMode happens to be 'knowledge_graph'.
      return mode === 'knowledge_graph';
    });
  }, [nodes, perNodeVisualModeMap]);

  // GemNodes writes setColorAt(i, …) where i is the index into THIS same `nodes`
  // array (typeFilteredNodes). Precompute, per knowledge-ring instance, the
  // GemNodes instance index whose already-computed colour we mirror. Built once
  // per data change so the colour-seed loop is a flat array read (no per-ring
  // Map.get / String() churn) and never recomputes the hue.
  const ringColorSourceIndices = useMemo(() => {
    const idMap = new Map<string, number>();
    for (let i = 0; i < nodes.length; i++) idMap.set(String(nodes[i].id), i);
    const out = new Int32Array(knowledgeNodes.length);
    for (let i = 0; i < knowledgeNodes.length; i++) {
      const ci = idMap.get(String(knowledgeNodes[i].id));
      out[i] = ci === undefined ? -1 : ci;
    }
    return out;
  }, [nodes, knowledgeNodes]);

  // Semantic inputs changed → the GemNodes colour buffer will be recomputed, so
  // unlatch and let the seed loop re-mirror the fresh colours once.
  useEffect(() => {
    colorsSeededRef.current = false;
  }, [ringColorSourceIndices, settings]);

  const geometry = useMemo(() => {
    // tube radius is only 0.03 — 8×24 (~384 tris) reads as smooth as 32×64
    // (~4096 tris) at any on-screen size, for ~10× fewer triangles per ring.
    return new THREE.TorusGeometry(1.2, 0.03, 8, 24);
  }, []);

  const material = useMemo(() => {
    return new THREE.MeshBasicMaterial({
      // color stays white so per-instance instanceColor is shown unmodulated;
      // vertexColors:true is what makes InstancedMesh.instanceColor take effect
      // on a MeshBasicMaterial.
      color: '#FFFFFF',
      vertexColors: true,
      transparent: true,
      opacity: RING_OPACITY,
      depthWrite: false,
      toneMapped: false,
    });
  }, []);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      geometry.dispose();
      material.dispose();
    };
  }, [geometry, material]);

  // Register with bloom layers
  useEffect(() => {
    const mesh = meshRef.current;
    if (mesh) {
      if (!mesh.layers) {
        mesh.layers = new THREE.Layers();
      }
      mesh.layers.set(0);
      mesh.layers.enable(1);
    }
  }, [knowledgeNodes.length]);

  useFrame((state) => {
    const mesh = meshRef.current;
    const positions = nodePositionsRef.current;
    if (!mesh || !positions) return;

    const elapsed = state.clock.elapsedTime;
    const mat4 = tempMatrix.current;
    const pos = tempPosition.current;
    const quat = tempQuaternion.current;
    const scl = tempScale.current;
    const euler = tempEuler.current;

    // Colour is a static semantic attribute (mirrors the GemNodes hue buffer).
    // Seed it ONCE per data change — not every frame — the moment the source
    // buffer becomes available. Steady-state frames write zero colours.
    if (!colorsSeededRef.current) {
      const nodeColors = nodeColorSourceRef?.current?.getColorArray?.() ?? null;
      if (nodeColors) {
        let colorsDirty = false;
        for (let i = 0; i < knowledgeNodes.length; i++) {
          const ci = ringColorSourceIndices[i];
          if (ci < 0) continue;
          const c3 = ci * 3;
          if (c3 + 2 < nodeColors.length) {
            // GemNodes colours are stored linear; copy through THREE.Color so the
            // ring matches exactly under the same colour-management path.
            _ringColor.setRGB(nodeColors[c3], nodeColors[c3 + 1], nodeColors[c3 + 2], THREE.LinearSRGBColorSpace);
            mesh.setColorAt(i, _ringColor);
            colorsDirty = true;
          }
        }
        if (colorsDirty && mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
        colorsSeededRef.current = true;
      }
    }

    // Per-frame: matrix only. Ring rotation is genuinely animated
    // (elapsed * ROTATION_SPEED), so the matrix must stream every frame.
    for (let i = 0; i < knowledgeNodes.length; i++) {
      const node = knowledgeNodes[i];
      const idx = nodeIdToIndexMap.get(String(node.id));

      if (idx === undefined) continue;

      const offset = idx * 3;
      pos.set(positions[offset], positions[offset + 1], positions[offset + 2]);

      // Per-ring tilt using node index for variety
      const tiltOffset = (i * 0.4) % (Math.PI * 2);
      euler.set(
        Math.sin(tiltOffset) * 0.3,
        elapsed * ROTATION_SPEED + tiltOffset,
        Math.cos(tiltOffset) * 0.2,
      );
      quat.setFromEuler(euler);

      scl.set(BASE_SCALE, BASE_SCALE, BASE_SCALE);

      mat4.compose(pos, quat, scl);
      mesh.setMatrixAt(i, mat4);
    }

    mesh.instanceMatrix.needsUpdate = true;
    mesh.count = knowledgeNodes.length;
  });

  if (knowledgeNodes.length === 0) {
    return null;
  }

  return (
    <instancedMesh
      ref={meshRef}
      args={[geometry, material, knowledgeNodes.length]}
      frustumCulled={false}
      renderOrder={4}
    />
  );
};
