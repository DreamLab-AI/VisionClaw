import { backendIsWebGPU } from '../../rendering/rendererFactory';
import type { SplatLayer } from './SplatLayer';
import type { RenderPath } from './variants';

/**
 * The variant path for the live renderer, read from the renderer instance at
 * mount. One splat backend, three's GaussianSplat addon, draws under
 * WebGPURenderer on either backend (GaussianSplat.js documents the forceWebGL
 * fallback as supported). Its GPU sort and SH compute run only on the WebGPU
 * backend, so the WebGL2 backend gets the lighter `desktop` tier (ADR-2170
 * decision 5, amended 2026-11-06).
 */
export function renderPathFor(gl: unknown): RenderPath {
  return backendIsWebGPU((gl as { backend?: unknown } | null | undefined)?.backend) ? 'webgpu' : 'webgl';
}

/** Build a layer for the live renderer. The addon module is imported on first use. */
export async function createSplatLayer(gl: unknown): Promise<SplatLayer> {
  const { CoreAddonSplatLayer } = await import('./coreAddonBackend');
  return new CoreAddonSplatLayer(gl);
}
