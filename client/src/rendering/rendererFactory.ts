import * as THREE from 'three';
import { installPipelineErrorWatch, installShaderErrorWatch, notifyRenderFault } from './renderFaults';
import type { RendererCapabilities } from '../features/settings/config/settings';
import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('GemRenderer');

/** WebGPU adapter info shape for capabilities reporting. */
interface GPUAdapterInfo {
  description?: string;
  device?: string;
}

/** The subset of three's WebGPURenderer (three/webgpu) this factory touches. */
interface WebGPURendererInstance extends THREE.WebGLRenderer {
  backend?: {
    isWebGPUBackend?: boolean;
    isWebGLBackend?: boolean;
    gl?: WebGL2RenderingContext;
    adapter?: { info?: GPUAdapterInfo } & GPUAdapterInfo;
  };
  init: () => Promise<void>;
  renderObject: (object: THREE.Object3D, ...rest: unknown[]) => void;
  __resizeObserver?: ResizeObserver;
}

/**
 * Runtime renderer capabilities — populated after renderer init.
 * Read by the settings panel to display active rendering features.
 */
export let rendererCapabilities: RendererCapabilities = {
  backend: 'webgl',
  tslMaterialsActive: true,
  nodeBasedBloom: true,
  gpuAdapterName: 'unknown',
  maxTextureSize: 0,
  pixelRatio: 1,
};

/**
 * Detect XR headset user agents (Quest 3, Oculus Browser, etc.)
 * for pixel ratio capping and WebGPU init timeout.
 */
function isXRHeadsetBrowser(): boolean {
  if (typeof navigator === 'undefined') return false;
  const ua = navigator.userAgent || '';
  return /Quest|OculusBrowser|Pico|VR/i.test(ua);
}

/**
 * Resolve a max pixel ratio appropriate for the device.
 * XR headsets get capped to 1.0 to avoid GPU memory blowup on
 * the stereoscopic render targets (each eye = full resolution).
 */
function getMaxPixelRatio(): number {
  return isXRHeadsetBrowser() ? 1.0 : 2.0;
}

/**
 * Force three's WebGL2 backend even where WebGPU is available. Toggled from
 * the Effects tab and persisted in localStorage (`visionclaw-force-webgl`);
 * maps to WebGPURenderer's `forceWebGL: true`. Needs a reload: R3F cannot swap
 * the renderer of a live Canvas.
 */
export let forceWebGLOverride = typeof localStorage !== 'undefined'
  && localStorage.getItem('visionclaw-force-webgl') === 'true';

export function setForceWebGLOverride(force: boolean) {
  forceWebGLOverride = force;
  if (typeof localStorage !== 'undefined') {
    if (force) {
      localStorage.setItem('visionclaw-force-webgl', 'true');
    } else {
      localStorage.removeItem('visionclaw-force-webgl');
    }
  }
}

/**
 * True only for three's real WebGPU backend. Reads the backend's own
 * `isWebGPUBackend` / `isWebGLBackend` flags, never `constructor.name`:
 * production minification renames the class (ML review, 2026-11-01).
 */
export function backendIsWebGPU(backend: unknown): boolean {
  const b = backend as { isWebGPUBackend?: boolean; isWebGLBackend?: boolean } | null | undefined;
  return b?.isWebGPUBackend === true && b?.isWebGLBackend !== true;
}

/** Whether the live renderer runs on the WebGPU backend (false on the WebGL2 fallback). */
export function activeBackendIsWebGPU(): boolean {
  return rendererCapabilities.backend === 'webgpu';
}

/** Quest 3's Oculus Browser can hang negotiating a WebGPU adapter. */
const INIT_TIMEOUT_MS = 5000;

/**
 * Renderer factory for R3F <Canvas gl={createGemRenderer}>.
 *
 * The one desktop renderer is three's WebGPURenderer. Where WebGPU is missing
 * it runs on its own WebGL2 backend (WebGPURenderer.getFallback →
 * WebGLBackend); every material is TSL, so both backends draw the same
 * scene. `forceWebGLOverride` asks for the WebGL2 backend outright, and a
 * WebGPU init that hangs past INIT_TIMEOUT_MS is retried on it.
 */
export async function createGemRenderer(defaultProps: Record<string, unknown>) {
  const canvas = defaultProps.canvas as HTMLCanvasElement;
  const maxDPR = getMaxPixelRatio();
  const { WebGPURenderer } = await import('three/webgpu') as unknown as {
    WebGPURenderer: new (opts: Record<string, unknown>) => WebGPURendererInstance;
  };
  const make = (forceWebGL: boolean) => new WebGPURenderer({
    canvas,
    antialias: true,
    alpha: true,
    powerPreference: 'high-performance',
    forceWebGL,
  });

  if (forceWebGLOverride) logger.info('[GemRenderer] WebGL2 backend forced by user preference');
  let renderer = make(forceWebGLOverride);
  try {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error(`WebGPU init timed out (${INIT_TIMEOUT_MS / 1000}s)`)), INIT_TIMEOUT_MS);
    });
    try { await Promise.race([renderer.init(), timeout]); } finally { clearTimeout(timer); }
  } catch (err) {
    if (forceWebGLOverride) throw err;
    logger.warn('[GemRenderer] WebGPU init failed; retrying on the WebGL2 backend:', err);
    renderer.dispose();
    renderer = make(true);
    await renderer.init();
  }

  const webgpu = backendIsWebGPU(renderer.backend);
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.0;
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, maxDPR));

  // R3F measures the container before the async init() resolves, so the
  // backend surface may be 0×0 until a resize. Apply the size now when the
  // canvas has layout, and re-apply it from a ResizeObserver otherwise.
  const rect = canvas.getBoundingClientRect();
  if (rect.width > 0 && rect.height > 0) {
    renderer.setSize(rect.width, rect.height, false);
  }
  if (typeof ResizeObserver !== 'undefined') {
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const w = entry.contentRect.width;
      const h = entry.contentRect.height;
      if (w > 0 && h > 0) renderer.setSize(w, h, false);
    });
    ro.observe(canvas);
    renderer.__resizeObserver = ro;
  }

  // ADR-2181 D4, no silent skip. Pipeline (WebGPU) and program (WebGL2)
  // failures never throw; route them to the per-material fault handlers.
  installPipelineErrorWatch(
    THREE.setConsoleFunction as unknown as Parameters<typeof installPipelineErrorWatch>[0],
    THREE.getConsoleFunction as unknown as Parameters<typeof installPipelineErrorWatch>[1],
  );
  installShaderErrorWatch(renderer);

  // A draw that throws (transient invalid draw parameters during async init,
  // third-party geometry) skips that object instead of killing the frame.
  // Objects that must not vanish silently count every throw; their handlers
  // fall back after repeated throws (renderFaults.debounceFault).
  const origRenderObject = renderer.renderObject.bind(renderer);
  const warnedObjects = new WeakSet<object>();
  renderer.renderObject = function (object: THREE.Object3D, ...rest: unknown[]) {
    try {
      return origRenderObject(object, ...rest);
    } catch (err: unknown) {
      if (!warnedObjects.has(object)) {
        warnedObjects.add(object);
        console.warn(
          '[GemRenderer] renderObject skipped:',
          object?.name || object?.type || object?.uuid,
          err instanceof Error ? err.message : err,
        );
      }
      notifyRenderFault(object, err);
    }
  };

  const pixelRatio = Math.min(window.devicePixelRatio, maxDPR);
  if (webgpu) {
    const adapterInfo = (renderer.backend?.adapter?.info ?? renderer.backend?.adapter ?? {}) as GPUAdapterInfo;
    rendererCapabilities = {
      backend: 'webgpu',
      tslMaterialsActive: true,
      nodeBasedBloom: true,
      gpuAdapterName: adapterInfo.description || adapterInfo.device || 'WebGPU',
      maxTextureSize: 16384, // WebGPU minimum guaranteed
      pixelRatio,
    };
  } else {
    const gl2 = renderer.backend?.gl;
    rendererCapabilities = {
      backend: 'webgl',
      tslMaterialsActive: true,
      nodeBasedBloom: true,
      gpuAdapterName: (gl2?.getParameter(gl2.RENDERER) as string) || 'WebGL2',
      maxTextureSize: (gl2?.getParameter(gl2.MAX_TEXTURE_SIZE) as number) || 4096,
      pixelRatio,
    };
  }

  if (isXRHeadsetBrowser()) logger.info('[GemRenderer] XR headset detected — pixel ratio capped to', maxDPR);
  logger.info(`[GemRenderer] WebGPURenderer initialised on the ${webgpu ? 'WebGPU' : 'WebGL2'} backend`);
  return renderer;
}
