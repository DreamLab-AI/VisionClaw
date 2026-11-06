/**
 * Watch an overview mesh for a render fault (ADR-2181 D4: no silent skip).
 *
 * WebGPU: the renderObject guard calls the mesh's fault hook when its draw
 * throws. WebGL: once the mesh has drawn, the program's link diagnostics are
 * read once. Either way the layer is marked faulted (overviewLive), the mesh
 * hides, the gems / tubes take over, and OverviewEdgeNotice reports it.
 * Returns whether the layer is faulted, for the caller's frame loop.
 */
import { useEffect, useRef } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import type * as THREE from 'three';
import { debounceFault, onPipelineError, onRenderFault, webglProgramFailure } from '../../../rendering/renderFaults';
import { isOverviewFaulted, markOverviewFault, type OverviewLayerName } from './overviewLive';

/** Frames to wait for a WebGL program before giving up the check. */
const PROGRAM_CHECK_FRAMES = 120;

export function useRenderFaultWatch(mesh: THREE.Mesh | null, layer: OverviewLayerName): void {
  const gl = useThree((s) => s.gl);
  const check = useRef({ mesh: null as THREE.Mesh | null, frames: 0, done: false });

  useEffect(() => {
    if (!mesh) return;
    const fault = (err: unknown) => {
      mesh.visible = false;
      markOverviewFault(layer, err instanceof Error ? err.message : String(err));
    };
    onRenderFault(mesh, debounceFault(fault));
    // WebGPU pipeline failures do not throw; they arrive by material id.
    const material = mesh.material as THREE.Material;
    onPipelineError(material, fault);
    return () => { onRenderFault(mesh, null); onPipelineError(material, null); };
  }, [mesh, layer]);

  useFrame(() => {
    if (!mesh) return;
    if (isOverviewFaulted(layer)) { mesh.visible = false; return; }
    const c = check.current;
    if (c.mesh !== mesh) { c.mesh = mesh; c.frames = 0; c.done = false; }
    if (c.done || !mesh.visible) return;
    if (++c.frames > PROGRAM_CHECK_FRAMES) { c.done = true; return; }
    const failure = webglProgramFailure(gl, mesh.material as THREE.Material);
    if (failure) {
      c.done = true;
      mesh.visible = false;
      markOverviewFault(layer, failure);
      return;
    }
    // A built program that links ends the check.
    const props = (gl as unknown as { properties?: { get(m: unknown): { currentProgram?: unknown } } }).properties;
    if (props?.get(mesh.material)?.currentProgram) c.done = true;
  });
}
