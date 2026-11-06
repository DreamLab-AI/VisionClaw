/**
 * Which colour space the scene's alpha blending happens in (ADR-2183 D3).
 *
 * Measured in the browser sidecar (0.8 grey at α 0.1 over #101018):
 *   - WebGPURenderer: linear (82/255);
 *   - WebGLRenderer through GemPostProcessing's composer (bloomScope 'scene' or
 *     'emphasis', post-processing on): linear, into a HalfFloat target (82/255);
 *   - WebGLRenderer drawing straight to the canvas (post-processing off): sRGB
 *     (37/255).
 * Overview opacities mean their linear-blend look; only the sRGB path needs a
 * correction (highlightVisual.srgbBlendAlpha). GemPostProcessing publishes
 * whether its WebGL composer owns the frame.
 */
import { useSyncExternalStore } from 'react';
import { isWebGPURenderer } from './rendererFactory';

let composited = false;
const listeners = new Set<() => void>();

/** GemPostProcessing: true while its WebGL composer renders the scene. */
export function setSceneComposited(on: boolean): void {
  if (composited === on) return;
  composited = on;
  for (const l of listeners) l();
}

export function sceneBlendsLinear(): boolean {
  return isWebGPURenderer || composited;
}

export function useSceneBlendsLinear(): boolean {
  return useSyncExternalStore(
    (l) => { listeners.add(l); return () => { listeners.delete(l); }; },
    sceneBlendsLinear,
    sceneBlendsLinear,
  );
}
