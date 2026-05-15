import React, { useEffect, useRef, useMemo } from 'react';
import { useThree, useFrame } from '@react-three/fiber';
import { useSettingsStore } from '../store/settingsStore';
import * as THREE from 'three';
import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('GemPostProcessing');

interface GemPostProcessingProps {
  enabled?: boolean;
}

/** Renderer extended with WebGPU detection flag set by rendererFactory. */
interface RendererWithWebGPUFlag {
  __isWebGPURenderer?: boolean;
  getDrawingBufferSize: (target: THREE.Vector2) => THREE.Vector2;
  render: (scene: THREE.Scene, camera: THREE.Camera) => void;
}

/** Bloom/glow settings shape with optional strength/intensity/radius/threshold. */
interface BloomLikeSettings {
  enabled?: boolean;
  strength?: number;
  intensity?: number;
  radius?: number;
  threshold?: number;
}

/** TSL texture node with optional toTexture method. */
interface TSLTextureNode {
  toTexture?: () => unknown;
  add: (node: unknown) => unknown;
}

/**
 * Unified post-processing component supporting both WebGPU (node-based) and WebGL (EffectComposer).
 *
 * Rendering ownership:
 *   R3F v9 skips its default gl.render() call whenever any useFrame subscriber
 *   has priority > 0 (see @react-three/fiber loop.ts: `if (!state.internal.priority ...)`).
 *   We register at priority 1 when post-processing is enabled, which means this
 *   component becomes the sole renderer -- no double-render occurs.
 *
 * WebGPU path uses:
 *   - RenderPipeline from three/webgpu (renamed from PostProcessing in r183)
 *   - pass() from three/tsl
 *   - bloom() from three/examples/jsm/tsl/display/BloomNode.js
 *
 * WebGL path uses:
 *   - EffectComposer + UnrealBloomPass from three/examples/jsm/postprocessing
 */
export const GemPostProcessing: React.FC<GemPostProcessingProps> = ({ enabled = true }) => {
  const { gl, scene, camera, size } = useThree();
  const settings = useSettingsStore(state => state.settings);
  const composerRef = useRef<{ render: () => void; dispose?: () => void; setSize: (w: number, h: number) => void } | null>(null);
  const rtRef = useRef<THREE.WebGLRenderTarget | null>(null);
  const postProcessingRef = useRef<{ render: () => void; dispose: () => void } | null>(null);
  const bloomNodeRef = useRef<{ strength?: { value: number }; radius?: { value: number }; threshold?: { value: number }; dispose?: () => void } | null>(null);
  const disposeRef = useRef<(() => void) | null>(null);
  const isWebGPU = (gl as unknown as RendererWithWebGPUFlag).__isWebGPURenderer === true;

  const glowSettings = settings?.visualisation?.glow;
  const bloomSettings = settings?.visualisation?.bloom;

  const effectEnabled = glowSettings?.enabled || bloomSettings?.enabled;
  const isEnabledWebGL = enabled && !isWebGPU && effectEnabled;
  const isEnabledWebGPU = enabled && isWebGPU && effectEnabled;

  // Extract primitive values to avoid stale closures and unnecessary effect deps.
  // Object references (glowSettings, bloomSettings) change on every settings update
  // even when the underlying values haven't changed -- primitives are stable.
  const activeSource: BloomLikeSettings | undefined = !bloomSettings?.enabled && glowSettings?.enabled ? glowSettings as BloomLikeSettings : bloomSettings as BloomLikeSettings;
  const bloomStrength = activeSource?.strength ?? activeSource?.intensity ?? 0.3;
  const bloomRadius = activeSource?.radius ?? 0.2;
  const bloomThreshold = activeSource?.threshold ?? 0.3;

  // Stable params object for WebGL EffectComposer (triggers rebuild on change — acceptable
  // because WebGL bloom is cheap to reconstruct).
  const effectParamsWebGL = useMemo(() => ({
    strength: bloomStrength,
    radius: bloomRadius,
    threshold: bloomThreshold,
  }), [bloomStrength, bloomRadius, bloomThreshold]);

  // Ref for WebGPU initial bloom values — avoids full RenderPipeline teardown on settings
  // change. The separate useEffect below (bloom uniform updater) handles live tweaks.
  const bloomParamsRef = useRef({ strength: bloomStrength, radius: bloomRadius, threshold: bloomThreshold });
  bloomParamsRef.current = { strength: bloomStrength, radius: bloomRadius, threshold: bloomThreshold };

  // WebGPU node-based post-processing path
  useEffect(() => {
    if (!isEnabledWebGPU) {
      // Clean up WebGPU resources if switching away
      if (disposeRef.current) {
        disposeRef.current();
        disposeRef.current = null;
      }
      postProcessingRef.current = null;
      bloomNodeRef.current = null;
      return;
    }

    let disposed = false;

    (async () => {
      try {
        const { RenderPipeline } = await import('three/webgpu');
        // eslint-disable-next-line @typescript-eslint/no-explicit-any -- Three.js TSL module exports are complex node builder types with no stable public API
        const tslMod = await import('three/tsl') as any;
        const pass = tslMod.pass as (scene: THREE.Scene, camera: THREE.Camera) => { getTextureNode: (name: string) => TSLTextureNode; dispose?: () => void };
        const { bloom } = await import('three/examples/jsm/tsl/display/BloomNode.js');

        if (disposed) return;

        const { strength, radius, threshold } = bloomParamsRef.current;

        // Build the node graph:
        //   scenePass -> intermediate texture copy -> bloom -> compose
        const scenePass = pass(scene, camera);
        const scenePassColor = scenePass.getTextureNode('output');

        // Break the WebGPU read/write synchronization scope: bloom must not
        // read the scene output texture while it's still a render attachment.
        // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- toTexture() always exists on r184+ TSL nodes
        const bloomInput = scenePassColor.toTexture!();

        const { Node: NodeClass } = await import('three/webgpu');
        const bloomPass = bloom(bloomInput as InstanceType<typeof NodeClass>, strength, radius, threshold);

        // Compose: scene + attenuated bloom. Raw `add` blows out bright nodes
        // because bloom(scene) at strength 0.8 pushes HDR values > 1.0 into
        // ACES compression territory, causing white-out and desaturation.
        // Scaling bloom by 0.5 preserves the glow effect while keeping values
        // in a range where ACES tone mapping retains color distinction.
        const outputNode = scenePassColor.add(bloomPass.mul(tslMod.float(0.5)));

        // eslint-disable-next-line @typescript-eslint/no-explicit-any -- RenderPipeline constructor types don't match R3F's renderer type
        const postProcessing = new RenderPipeline(gl as any, outputNode);

        postProcessingRef.current = postProcessing;
        bloomNodeRef.current = bloomPass;

        disposeRef.current = () => {
          postProcessing.dispose();
          if (bloomPass.dispose) bloomPass.dispose();
          if (scenePass.dispose) scenePass.dispose();
        };
      } catch (err) {
        logger.warn('[GemPostProcessing] Failed to init WebGPU bloom:', err);
      }
    })();

    return () => {
      disposed = true;
      if (disposeRef.current) {
        disposeRef.current();
        disposeRef.current = null;
      }
      postProcessingRef.current = null;
      bloomNodeRef.current = null;
    };
  // bloomParamsRef is intentionally NOT a dep — initial values are read from the ref,
  // and live updates are handled by the bloom uniform updater useEffect below.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isEnabledWebGPU, gl, scene, camera]);

  // Update WebGPU bloom uniforms when settings change without full rebuild
  useEffect(() => {
    if (!isEnabledWebGPU || !bloomNodeRef.current) return;
    const bloomNode = bloomNodeRef.current;
    if (bloomNode.strength) bloomNode.strength.value = bloomStrength;
    if (bloomNode.radius) bloomNode.radius.value = bloomRadius;
    if (bloomNode.threshold) bloomNode.threshold.value = bloomThreshold;
  }, [isEnabledWebGPU, bloomStrength, bloomRadius, bloomThreshold]);

  // WebGL EffectComposer path
  useEffect(() => {
    if (!isEnabledWebGL) {
      composerRef.current = null;
      return;
    }

    let disposed = false;
    (async () => {
      try {
        const { EffectComposer } = await import('three/examples/jsm/postprocessing/EffectComposer.js');
        const { RenderPass } = await import('three/examples/jsm/postprocessing/RenderPass.js');
        const { UnrealBloomPass } = await import('three/examples/jsm/postprocessing/UnrealBloomPass.js');
        const THREE = await import('three');

        if (disposed) return;

        const { strength, threshold, radius } = effectParamsWebGL;

        // Cap render target size to avoid GPU memory blowup on high-DPR displays.
        // EffectComposer inherits the renderer's drawing buffer size; clamp to 2048
        // on the long edge so bloom mip chain stays within GPU limits.
        const maxDim = 2048;
        const drawSize = new THREE.Vector2();
        (gl as unknown as RendererWithWebGPUFlag).getDrawingBufferSize(drawSize);
        const scale = Math.min(1, maxDim / Math.max(drawSize.x, drawSize.y));
        const rtWidth = Math.round(drawSize.x * scale);
        const rtHeight = Math.round(drawSize.y * scale);

        const rt = new THREE.WebGLRenderTarget(rtWidth, rtHeight, {
          type: THREE.HalfFloatType,
        });
        const composer = new EffectComposer(gl as unknown as THREE.WebGLRenderer, rt);
        composer.addPass(new RenderPass(scene, camera));

        const bloomPass = new UnrealBloomPass(
          new THREE.Vector2(rtWidth, rtHeight),
          strength,
          radius,
          threshold
        );
        composer.addPass(bloomPass);

        composerRef.current = composer;
        rtRef.current = rt;
      } catch (err) {
        logger.warn('[GemPostProcessing] Failed to init WebGL bloom:', err);
      }
    })();

    return () => {
      disposed = true;
      if (composerRef.current) {
        composerRef.current.dispose?.();
        composerRef.current = null;
      }
      // Dispose custom render target separately — EffectComposer.dispose()
      // only disposes its internal targets, not the one passed to constructor.
      if (rtRef.current) {
        rtRef.current.dispose();
        rtRef.current = null;
      }
    };
  }, [isEnabledWebGL, gl, scene, camera, effectParamsWebGL]);

  // Resize EffectComposer when window size changes (WebGL path).
  // Apply the same maxDim cap used during creation to avoid GPU memory blowup.
  useEffect(() => {
    if (composerRef.current && size.width > 0 && size.height > 0) {
      const maxDim = 2048;
      const dpr = Math.min(window.devicePixelRatio, 2);
      const w = Math.round(size.width * dpr);
      const h = Math.round(size.height * dpr);
      const scale = Math.min(1, maxDim / Math.max(w, h));
      composerRef.current.setSize(Math.round(w * scale), Math.round(h * scale));
    }
  }, [size.width, size.height]);

  // Render loop: delegate to whichever pipeline is active.
  //
  // Priority 1 tells R3F to skip its default gl.render() call (R3F v9 increments
  // internal.priority for any subscriber with priority > 0, and only calls
  // gl.render when internal.priority === 0). This prevents double-rendering.
  //
  // During async initialization, the post-processing refs are null while the
  // dynamic imports resolve. We fall back to gl.render() in that window to
  // avoid black frames.
  const isActive = isEnabledWebGPU || isEnabledWebGL;
  const ppErrorCountRef = useRef(0);
  useFrame(({ gl: renderer, scene: s, camera: cam }) => {
    if (postProcessingRef.current) {
      try {
        postProcessingRef.current.render();
      } catch (err: unknown) {
        // RenderPipeline can throw on WebGPU due to texture synchronization
        // constraints (read+write same texture in one pass). After 3 consecutive
        // failures, fall back to direct rendering for the rest of the session.
        ppErrorCountRef.current++;
        if (ppErrorCountRef.current <= 3) {
          logger.warn('[GemPostProcessing] PostProcessing.render() failed:', err instanceof Error ? err.message : err);
        }
        if (ppErrorCountRef.current >= 3) {
          logger.warn('[GemPostProcessing] Too many failures, disabling WebGPU bloom');
          postProcessingRef.current = null;
        }
        renderer.render(s, cam);
      }
    } else if (composerRef.current) {
      composerRef.current.render();
    } else if (isActive) {
      // Fallback: post-processing enabled but not yet initialized (async import in flight).
      // Render the scene directly so there are no black frames during init.
      renderer.render(s, cam);
    }
    // When !isActive, priority is undefined so R3F renders normally — no fallback needed.
  }, isActive ? 1 : undefined);

  return null;
};

export default GemPostProcessing;
