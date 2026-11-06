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
