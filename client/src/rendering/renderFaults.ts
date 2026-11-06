/**
 * Per-object render-fault hook (ADR-2181 D4 / ADR-2182: no silent skip).
 *
 * The WebGPU renderObject guard (rendererFactory.ts) catches a draw that
 * throws and skips that object. An object that must not vanish silently sets
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

/**
 * WebGL has no equivalent guard: a shader that fails to compile or link is
 * logged by three and the object then draws nothing. three records the
 * outcome on the program (`diagnostics.runnable`, with checkShaderErrors on,
 * the default), so a caller can ask once after the first render.
 * Returns the failure text, or null when the program is fine or not built yet.
 */
export function webglProgramFailure(gl: unknown, material: THREE.Material): string | null {
  const props = (gl as { properties?: { get(m: unknown): { currentProgram?: { diagnostics?: { runnable: boolean; programLog?: string; fragmentShader?: { log?: string }; vertexShader?: { log?: string } } } } } }).properties;
  if (!props || typeof props.get !== 'function') return null;
  const d = props.get(material)?.currentProgram?.diagnostics;
  if (!d || d.runnable) return null;
  return (d.programLog || d.vertexShader?.log || d.fragmentShader?.log || 'shader failed to compile').trim().slice(0, 200);
}

// --- WebGPU pipeline failures ------------------------------------------------
//
// On real WebGPU a WGSL compile or pipeline validation failure does not throw
// inside renderObject: three logs it through its console hook as
// "WebGPURenderer: … render pipeline creation failed (renderPipeline_<material
// name>_<material id>): …" and then silently skips every draw with that
// pipeline (WebGPUBackend.draw: pipelineData.error). The watch below routes
// those messages to a per-material handler so such a layer is not reported
// live while drawing nothing. A pipeline failure is permanent, so it fires at
// once (no debounce).

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
