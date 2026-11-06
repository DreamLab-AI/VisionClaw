/**
 * Per-object render-fault hook (ADR-2181 D4 / ADR-2182: no silent skip).
 *
 * The renderObject guard (rendererFactory.ts) catches a draw that throws and
 * skips that object. An object that must not vanish silently sets
 * `object.userData[RENDER_FAULT_KEY] = (err) => …`; the guard calls it once,
 * on the first fault, alongside its console warning.
 */
import type * as THREE from 'three';

export const RENDER_FAULT_KEY = 'onRenderFault';

export type RenderFaultHandler = (err: unknown) => void;

export function onRenderFault(object: THREE.Object3D, handler: RenderFaultHandler | null): void {
  if (handler) object.userData[RENDER_FAULT_KEY] = handler;
  else delete object.userData[RENDER_FAULT_KEY];
}

/**
 * Throws on this many separate frames before a fault counts: the guard exists
 * to absorb transient init failures, so one throw must not disable a layer.
 */
export const FAULT_FRAMES = 3;
/** Throws closer together than this are the same frame. */
const SAME_FRAME_MS = 8;

/**
 * Wrap a fault handler so it fires only after throws on FAULT_FRAMES separate
 * frames. `now` is injectable for tests.
 */
export function debounceFault(handler: RenderFaultHandler, frames = FAULT_FRAMES, now: () => number = () => performance.now()): RenderFaultHandler {
  let count = 0;
  let last = -Infinity;
  let fired = false;
  return (err) => {
    if (fired) return;
    const t = now();
    if (t - last < SAME_FRAME_MS) return;
    last = t;
    if (++count >= frames) { fired = true; handler(err); }
  };
}

/** Called by the guard on every caught throw. Never throws. */
export function notifyRenderFault(object: THREE.Object3D | null | undefined, err: unknown): void {
  const h = object?.userData?.[RENDER_FAULT_KEY];
  if (typeof h !== 'function') return;
  try { (h as RenderFaultHandler)(err); } catch { /* a fault handler must not break the render loop */ }
}

// --- Pipeline and program failures --------------------------------------------
//
// A material whose shader fails to build never throws; three logs it and then
// draws nothing with it. Both backends of WebGPURenderer are routed to the same
// per-material handler (onPipelineError), so such a layer is not reported live
// while drawing nothing. A failure is permanent, so it fires at once (no
// debounce).
//
// WebGPU backend:  a WGSL compile or pipeline validation failure does not throw
// inside renderObject: three logs it through its console hook as
// "WebGPURenderer: … render pipeline creation failed (renderPipeline_<material
// name>_<material id>): …" and then silently skips every draw with that
// pipeline (WebGPUBackend.draw: pipelineData.error). installPipelineErrorWatch
// matches the material id in that label.
//
// WebGL2 backend: a GLSL link failure is reported through
// `renderer.debug.onShaderError(gl, program, vs, fs)` (WebGLBackend
// _logProgramError), with no material. installShaderErrorWatch records which
// material each program was created for (WebGLBackend.createRenderPipeline)
// and routes the failure to that material's handler.

const pipelineHandlers = new Map<number, RenderFaultHandler>();
const PIPELINE_LABEL = /renderPipeline_[^()\s]*?_(\d+)\)/;

/** Register (or with null, clear) a handler for pipeline failures of `material`. */
export function onPipelineError(material: THREE.Material, handler: RenderFaultHandler | null): void {
  if (handler) pipelineHandlers.set(material.id, handler);
  else pipelineHandlers.delete(material.id);
}

/** Route one three console message; returns true when it was a watched pipeline failure. */
export function routePipelineMessage(type: string, message: unknown): boolean {
  if (type !== 'error' || typeof message !== 'string') return false;
  const m = PIPELINE_LABEL.exec(message);
  if (!m) return false;
  const h = pipelineHandlers.get(Number(m[1]));
  if (!h) return false;
  try { h(new Error(message)); } catch { /* never break three's logging */ }
  return true;
}

type ConsoleFn = (type: string, message: string, ...params: unknown[]) => void;
let watchInstalled = false;

/**
 * Chain onto three's console hook (setConsoleFunction): watched pipeline
 * failures go to their handlers, and every message still reaches the previous
 * hook or the browser console. Idempotent.
 */
export function installPipelineErrorWatch(
  setConsoleFunction: (fn: ConsoleFn) => void,
  getConsoleFunction: () => ConsoleFn | null,
): void {
  if (watchInstalled) return;
  watchInstalled = true;
  const prev = getConsoleFunction();
  setConsoleFunction((type, message, ...params) => {
    routePipelineMessage(type, message);
    if (prev) { prev(type, message, ...params); return; }
    const c = console as unknown as Record<string, (...a: unknown[]) => void>;
    (c[type] ?? console.log)(message, ...params);
  });
}

interface ShaderErrorRenderer {
  debug?: { onShaderError?: ((gl: WebGL2RenderingContext, program: WebGLProgram, vs: WebGLShader, fs: WebGLShader) => void) | null };
  backend?: {
    isWebGLBackend?: boolean;
    get?: (key: object) => { programGPU?: WebGLProgram } | undefined;
    createRenderPipeline?: (renderObject: { material?: THREE.Material; pipeline?: object }, promises: unknown) => unknown;
  };
}

/** Route one WebGL2 program failure to the handler of the material it was built for. */
export function routeShaderFailure(materialId: number | undefined, message: string): boolean {
  if (materialId === undefined) return false;
  const h = pipelineHandlers.get(materialId);
  if (!h) return false;
  try { h(new Error(message)); } catch { /* never break three's compile path */ }
  return true;
}

function shaderLog(gl: WebGL2RenderingContext, shader: WebGLShader, kind: string): string {
  const log = (gl.getShaderInfoLog(shader) || '').trim();
  return log ? `${kind}: ${log}` : '';
}

/**
 * WebGL2 backend only: attribute GLSL link failures to their material and
 * route them like a WebGPU pipeline failure. Setting `debug.onShaderError`
 * replaces three's default report, so the failure is still logged here.
 * No-op on the WebGPU backend.
 */
export function installShaderErrorWatch(renderer: unknown): void {
  const r = renderer as ShaderErrorRenderer;
  const backend = r.backend;
  if (!backend?.isWebGLBackend || !r.debug || typeof backend.createRenderPipeline !== 'function') return;
  const programMaterial = new WeakMap<WebGLProgram, number>();
  let building: number | undefined;
  const create = backend.createRenderPipeline.bind(backend);
  backend.createRenderPipeline = (renderObject, promises) => {
    building = renderObject?.material?.id;
    try {
      return create(renderObject, promises);
    } finally {
      // With parallel compile (compileAsync) the link result arrives later:
      // keep the program → material record for that callback.
      const program = renderObject?.pipeline ? backend.get?.(renderObject.pipeline)?.programGPU : undefined;
      if (program && building !== undefined) programMaterial.set(program, building);
      building = undefined;
    }
  };
  r.debug.onShaderError = (gl, program, vs, fs) => {
    const detail = [(gl.getProgramInfoLog(program) || '').trim(), shaderLog(gl, vs, 'vertex'), shaderLog(gl, fs, 'fragment')]
      .filter(Boolean).join('\n');
    const message = `WebGLProgram: shader link failed: ${detail || 'no info log'}`;
    console.error(message);
    routeShaderFailure(building ?? programMaterial.get(program), message);
  };
}
