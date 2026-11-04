import { backendIsWebGPU } from '../../rendering/rendererFactory';
import type { SplatBackendKind, SplatLayer } from './SplatLayer';
import type { RenderPath } from './variants';

interface RendererFlags {
  __isWebGPURenderer?: boolean;
  backend?: unknown;
}

/**
 * Which backend suits the live renderer. Read from the renderer instance at
 * mount, never from `isWebGPURenderer`, which is false until the renderer has
 * initialised. Any WebGPURenderer gets the core addon (Spark cannot run under
 * it, even on its WebGL2 backend); a plain WebGLRenderer gets Spark.
 */
export function pickBackendKind(gl: unknown): SplatBackendKind {
  const r = (gl ?? {}) as RendererFlags;
  return r.__isWebGPURenderer === true || backendIsWebGPU(r.backend) ? 'core-addon' : 'spark';
}

/** The variant-preference path that goes with a backend. */
export function renderPathFor(kind: SplatBackendKind): RenderPath {
  return kind === 'core-addon' ? 'webgpu' : 'webgl';
}

/**
 * Build a layer for the live renderer. Each backend module is imported only
 * when chosen, so neither library is evaluated under the wrong renderer.
 */
export async function createSplatLayer(gl: unknown, kind: SplatBackendKind = pickBackendKind(gl)): Promise<SplatLayer> {
  if (kind === 'core-addon') {
    const { CoreAddonSplatLayer } = await import('./coreAddonBackend');
    return new CoreAddonSplatLayer();
  }
  const { SparkSplatLayer } = await import('./sparkBackend');
  return new SparkSplatLayer(gl as import('three').WebGLRenderer);
}
