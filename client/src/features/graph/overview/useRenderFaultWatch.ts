/**
 * Watch an overview mesh for a render fault (ADR-2181 D4: no silent skip).
 *
 * Two routes, both from rendering/renderFaults: the renderObject guard calls
 * the mesh's fault hook when its draw throws (debounced), and a material whose
 * shader fails to build (WebGPU pipeline or WebGL2 program) reports by
 * material id. Either way the layer is marked faulted (overviewLive), the mesh
 * hides, the gems / tubes take over, and OverviewEdgeNotice reports it.
 */
import { useEffect } from 'react';
import { useFrame } from '@react-three/fiber';
import type * as THREE from 'three';
import { debounceFault, onPipelineError, onRenderFault } from '../../../rendering/renderFaults';
import { isOverviewFaulted, markOverviewFault, type OverviewLayerName } from './overviewLive';

export function useRenderFaultWatch(mesh: THREE.Mesh | null, layer: OverviewLayerName): void {
  useEffect(() => {
    if (!mesh) return;
    const fault = (err: unknown) => {
      mesh.visible = false;
      markOverviewFault(layer, err instanceof Error ? err.message : String(err));
    };
    onRenderFault(mesh, debounceFault(fault));
    const material = mesh.material as THREE.Material;
    onPipelineError(material, fault);
    return () => { onRenderFault(mesh, null); onPipelineError(material, null); };
  }, [mesh, layer]);

  // A faulted layer stays hidden even if its owner sets visible again.
  useFrame(() => {
    if (mesh && isOverviewFaulted(layer)) mesh.visible = false;
  });
}
