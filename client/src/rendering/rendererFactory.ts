import * as THREE from 'three';
import type { RendererCapabilities } from '../features/settings/config/settings';
import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('GemRenderer');

/** Navigator with optional WebGPU API (not all browsers expose navigator.gpu). */
interface NavigatorWithGPU {
  gpu?: unknown;
}

/** WebGPU adapter info shape for capabilities reporting. */
interface GPUAdapterInfo {
  description?: string;
  device?: string;
}

/** WebGPU renderer shape for the dynamically imported three/webgpu module. */
interface WebGPURendererInstance extends THREE.WebGLRenderer {
  backend?: {
    constructor?: { name?: string };
    adapter?: { info?: GPUAdapterInfo } & GPUAdapterInfo;
  };
  init: () => Promise<void>;
  renderObject: (object: THREE.Object3D, ...rest: unknown[]) => void;
  __isWebGPURenderer?: boolean;
  __resizeObserver?: ResizeObserver;
}

/**
 * Whether the active renderer is a true WebGPU backend (not WebGPURenderer
 * falling back to its internal WebGL2 backend).
 *
 * Components check this to adjust material properties for WebGPU compatibility
 * (e.g. disabling transmission which crashes the transparent pass).
 *
 * On browsers without `navigator.gpu` (Firefox, older Safari), we skip
 * WebGPURenderer entirely and use WebGLRenderer — this avoids the hybrid
 * WebGPURenderer+WebGLBackend path that causes oversized render targets,
 * PMREM blowups, and incorrect material codepaths.
 */
export let isWebGPURenderer = false;

/**
 * Runtime renderer capabilities — populated after renderer init.
 * Read by the settings panel to display active rendering features.
 */
export let rendererCapabilities: RendererCapabilities = {
  backend: 'webgl',
  tslMaterialsActive: false,
  nodeBasedBloom: false,
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
 * Renderer factory for R3F <Canvas gl={rendererFactory}>.
 * R3F calls: await glConfig(defaultProps) where defaultProps = { canvas, antialias, ... }
 *
 * Strategy:
 *   1. Check `navigator.gpu` — if absent, go straight to WebGLRenderer.
 *   2. Create WebGPURenderer with forceWebGL: false.
 *   3. After init(), verify the backend is actually WebGPU (not internal WebGL2 fallback).
 *   4. If the backend fell back to WebGL2, discard and use clean WebGLRenderer instead.
 *   5. Timeout guard: if WebGPU init takes >5s, fall back to WebGL (Quest 3 sometimes hangs).
 */
/**
 * Runtime flag to force WebGL renderer even when WebGPU is available.
 * Toggled via the Effects tab "WebGPU Renderer" toggle. Persisted in localStorage.
 * Requires page reload to take effect (R3F Canvas can't swap renderers live).
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
 * production minification renames the class, so a name comparison never
 * matched and a failed adapter silently ran WebGPURenderer on its WebGL2
 * fallback backend (ML review, 2026-11-01).
 */
export function backendIsWebGPU(backend: unknown): boolean {
  const b = backend as { isWebGPUBackend?: boolean; isWebGLBackend?: boolean } | null | undefined;
  return b?.isWebGPUBackend === true && b?.isWebGLBackend !== true;
}

export async function createGemRenderer(defaultProps: Record<string, unknown>) {
  const canvas = defaultProps.canvas as HTMLCanvasElement;
  const maxDPR = getMaxPixelRatio();

  // Gate 0: user can force WebGL via Effects toggle
  if (forceWebGLOverride) {
    logger.info('[GemRenderer] WebGL forced by user preference (WebGPU disabled)');
  }

  // Gate 1: browser must expose the WebGPU API
  if (!forceWebGLOverride && typeof navigator !== 'undefined' && (navigator as NavigatorWithGPU).gpu) {
    try {
      const threeWebGPU = await import('three/webgpu');
      const WebGPURenderer = (threeWebGPU as Record<string, unknown>).WebGPURenderer as new (opts: Record<string, unknown>) => WebGPURendererInstance;

      if (typeof WebGPURenderer !== 'function') {
        throw new Error('WebGPURenderer export not found');
      }

      const renderer = new WebGPURenderer({
        canvas,
        antialias: true,
        alpha: true,
        powerPreference: 'high-performance',
        forceWebGL: false,
      });

      // Timeout guard: Quest 3's Oculus Browser can hang during WebGPU adapter
      // negotiation. Cap init to 5 seconds then fall back to WebGL.
      const initTimeout = new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error('WebGPU init timed out (5s)')), 5000)
      );
      await Promise.race([renderer.init(), initTimeout]);

      // Gate 2: verify the backend is actually WebGPU, not the internal WebGL2 fallback.
      // Three.js r182 and earlier silently fell back to WebGLBackend when the GPU
      // adapter request failed. r183+ still has this fallback path, so the check remains.
      if (!backendIsWebGPU(renderer.backend)) {
        logger.warn('[GemRenderer] WebGPURenderer fell back to WebGLBackend — using clean WebGLRenderer instead');
        renderer.dispose();
        throw new Error('WebGPU backend unavailable (got WebGLBackend)');
      }

      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      renderer.toneMappingExposure = 1.0;
      renderer.outputColorSpace = THREE.SRGBColorSpace;
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, maxDPR));

      // Force the WebGPU surface to match the canvas dimensions.
      // R3F measures the container before the async init() resolves, so the
      // WebGPU backend's internal surface may be 0×0 until a resize event.
      // Explicit setSize() here eliminates the "black until resize" bug —
      // but only when the canvas already has layout. If the canvas measures
      // 0×0 at init time, a ResizeObserver below re-applies setSize the
      // moment layout completes, otherwise the surface stays 0×0 forever.
      const rect = canvas.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        renderer.setSize(rect.width, rect.height, false);
      }

      // Robust fallback: re-apply setSize whenever the canvas dimensions
      // change. Catches the "rect was 0×0 at init" case and any subsequent
      // container resize that R3F's own resize logic may miss for WebGPU.
      if (typeof ResizeObserver !== 'undefined') {
        const ro = new ResizeObserver((entries) => {
          const entry = entries[0];
          if (!entry) return;
          const w = entry.contentRect.width;
          const h = entry.contentRect.height;
          if (w > 0 && h > 0) {
            renderer.setSize(w, h, false);
          }
        });
        ro.observe(canvas);
        // Stash on the renderer so dispose() can disconnect it.
        (renderer as WebGPURendererInstance).__resizeObserver = ro;
      }

      // Expose renderer type for components to check
      (renderer as WebGPURendererInstance).__isWebGPURenderer = true;
      isWebGPURenderer = true;

      // Guard against drawIndexed(Infinity) crashes in the WebGPU backend.
      // Some objects (e.g. InstancedMesh during async init, or three.js internal
      // passes) can transiently have invalid draw parameters. Rather than crashing
      // the entire render loop, we catch the TypeError and skip the object.
      // Precautionary for r183+: the root cause patches are in the materials, but
      // third-party geometry (troika Line2, etc.) can still trigger this edge case.
      const _origRenderObject = renderer.renderObject.bind(renderer);
      const _warnedObjects = new WeakSet<object>();
      renderer.renderObject = function (object: THREE.Object3D, ...rest: unknown[]) {
        try {
          return _origRenderObject(object, ...rest);
        } catch (err: unknown) {
          if (!_warnedObjects.has(object)) {
            _warnedObjects.add(object);
            console.warn(
              '[GemRenderer] renderObject skipped:',
              object?.name || object?.type || object?.uuid,
              err instanceof Error ? err.message : err,
            );
          }
        }
      };

      // Populate renderer capabilities for settings panel
      const adapterInfo = (renderer.backend?.adapter?.info ?? renderer.backend?.adapter ?? {}) as GPUAdapterInfo;
      rendererCapabilities = {
        backend: 'webgpu',
        tslMaterialsActive: true,  // TSL upgrade runs asynchronously per-material
        nodeBasedBloom: true,
        gpuAdapterName: adapterInfo.description
          || adapterInfo.device
          || 'WebGPU',
        maxTextureSize: 16384,  // WebGPU minimum guaranteed
        pixelRatio: Math.min(window.devicePixelRatio, maxDPR),
      };

      logger.info('[GemRenderer] WebGPU renderer initialized (backend: WebGPUBackend)');
      return renderer;
    } catch (err) {
      logger.warn('[GemRenderer] WebGPU unavailable, falling back to WebGL:', err);
    }
  } else {
    logger.info('[GemRenderer] navigator.gpu not available — using WebGL directly');
  }

  // WebGL fallback — clean path, no hybrid renderer quirks
  const renderer = new THREE.WebGLRenderer({
    ...defaultProps,
    antialias: true,
    alpha: true,
    powerPreference: 'high-performance',
  });

  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.0;
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, maxDPR));

  isWebGPURenderer = false;

  // Populate renderer capabilities for WebGL fallback
  const gl2 = renderer.getContext() as WebGLRenderingContext;
  const isXR = isXRHeadsetBrowser();
  rendererCapabilities = {
    backend: 'webgl',
    tslMaterialsActive: false,
    nodeBasedBloom: false,
    gpuAdapterName: (gl2.getParameter(gl2.RENDERER) as string) || (isXR ? 'WebGL (XR)' : 'WebGL'),
    maxTextureSize: (gl2.getParameter(gl2.MAX_TEXTURE_SIZE) as number) || 4096,
    pixelRatio: Math.min(window.devicePixelRatio, maxDPR),
  };

  if (isXR) {
    logger.info('[GemRenderer] XR headset detected — pixel ratio capped to', maxDPR);
  }

  logger.info('[GemRenderer] WebGL renderer initialized');
  return renderer;
}
