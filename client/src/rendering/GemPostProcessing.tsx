import React, { useEffect, useRef, useMemo, useState } from 'react';
import { useThree, useFrame } from '@react-three/fiber';
import { useSettingsStore } from '../store/settingsStore';
import * as THREE from 'three';
import { createLogger } from '../utils/loggerConfig';
import { setSceneComposited } from './blendSpace';
import type { Node as TslNode } from 'three/webgpu';
import { BLOOM_LAYER } from './bloomLayer';
import { glowEffectParams } from './glowEffect';
import {
  initRenderGuard,
  recordRenderFailure,
  recordRenderSuccess,
} from './postProcessingGuard';

const logger = createLogger('GemPostProcessing');

/**
 * EffectComposer.dispose() frees only its own two render targets and copy pass,
 * not the passes added to it, so each rebuild leaked the UnrealBloomPass mip
 * chain and any ShaderPass material. Dispose every pass first.
 */
function disposeComposer(composer: { passes?: Array<{ dispose?: () => void }>; dispose?: () => void }): void {
  for (const pass of composer.passes ?? []) pass.dispose?.();
  composer.dispose?.();
}

/**
 * Consecutive failed render frames tolerated before post-processing is
 * permanently disabled for the session and rendering is handed back to the
 * default R3F renderer. Small enough that a genuinely broken backend gives up
 * within a few frames; large enough that a transient GPU context-loss/restore
 * blip recovers without disabling the effect.
 */
const MAX_CONSECUTIVE_RENDER_FAILURES = 8;

interface GemPostProcessingProps {
  enabled?: boolean;
}

/** Renderer extended with WebGPU detection flag set by rendererFactory. */
interface RendererWithWebGPUFlag {
  __isWebGPURenderer?: boolean;
  getDrawingBufferSize: (target: THREE.Vector2) => THREE.Vector2;
  render: (scene: THREE.Scene, camera: THREE.Camera) => void;
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

  // Terminal state: once a backend proves it cannot render the post-processing
  // pipeline (repeated throws), we disable the subsystem for the session and
  // let R3F's default renderer own every frame — no further retries.
  const [ppDisabled, setPpDisabled] = useState(false);
  // True only while an actual pipeline (WebGPU RenderPipeline or WebGL
  // EffectComposer) is constructed and owns the frame. Gates the render
  // priority so that during async init — or if init never completes — R3F
  // renders directly instead of us calling a throwing fallback.
  const [pipelineReady, setPipelineReady] = useState(false);
  // ADR-2183 D3: the WebGL composer blends the scene in linear space; tell the
  // overview layers which blend space their opacities land in.
  useEffect(() => {
    setSceneComposited(pipelineReady);
    return () => setSceneComposited(false);
  }, [pipelineReady]);
  const guardRef = useRef(initRenderGuard());

  const glow = glowEffectParams(settings?.visualisation);

  // ADR-2182 D4: what blooms. 'emphasis' (default) blooms only the bloom layer
  // (emphasis halos + the selection's tubes) from a camera that sees nothing
  // else; 'scene' is the global-threshold bloom over the whole scene; 'off'
  // drops the post-processing pipeline altogether.
  const bloomScope = (settings?.visualisation?.graphView?.nodes?.bloomScope ?? 'emphasis') as 'emphasis' | 'scene' | 'off';
  const selective = bloomScope === 'emphasis';
  const effectEnabled = glow.enabled && bloomScope !== 'off';
  // The camera the selective bloom pass renders from: a copy of the scene
  // camera, re-synced every frame, that sees only BLOOM_LAYER.
  const bloomCamera = useMemo(() => {
    const c = new THREE.PerspectiveCamera();
    c.layers.set(BLOOM_LAYER);
    return c;
  }, []);
  const ppAvailable = !ppDisabled;
  const isEnabledWebGL = enabled && !isWebGPU && effectEnabled && ppAvailable;
  const isEnabledWebGPU = enabled && isWebGPU && effectEnabled && ppAvailable;

  // Primitives (stable across settings updates whose values did not change).
  const bloomStrength = glow.strength;
  const bloomRadius = glow.radius;
  const bloomThreshold = glow.threshold;

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
      setPipelineReady(false);
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
        // r183+: toTexture() is the standard API. rtt() (r170-r182) kept as fallback.
        let bloomInput: unknown = scenePassColor;
        if (typeof scenePassColor.toTexture === 'function') {
          bloomInput = scenePassColor.toTexture();
        } else if (typeof tslMod.rtt === 'function') {
          bloomInput = tslMod.rtt(scenePassColor);
        }

        type EmphasisPass = { getTextureNode: (name: string) => TSLTextureNode; dispose?: () => void; updateBefore?: (frame: unknown) => unknown };
        let emphasisPass: EmphasisPass | null = null;
        if (selective) {
          // ADR-2182 D4: bloom only what the bloom-layer camera sees. The
          // pass renders without the scene background, so only emphasis
          // geometry (on a clear target) reaches the bloom.
          const ep = pass(scene, bloomCamera as unknown as THREE.Camera) as unknown as EmphasisPass;
          const orig = ep.updateBefore?.bind(ep);
          if (orig) {
            ep.updateBefore = (frame: unknown) => {
              const bg = scene.background;
              scene.background = null;
              try { return orig(frame); } finally { scene.background = bg; }
            };
          }
          const em = ep.getTextureNode('output');
          bloomInput = typeof em.toTexture === 'function' ? em.toTexture() : em;
          emphasisPass = ep;
        }
        const emphasisPassRef: EmphasisPass | null = emphasisPass;

        const bloomPass = bloom(bloomInput as TslNode<'vec4'>, strength, radius, selective ? 0 : threshold);

        const outputNode = scenePassColor.add(bloomPass);

        // eslint-disable-next-line @typescript-eslint/no-explicit-any -- RenderPipeline constructor types don't match R3F's renderer type
        const postProcessing = new RenderPipeline(gl as any, outputNode as TslNode);

        postProcessingRef.current = postProcessing;
        bloomNodeRef.current = bloomPass;
        // Fresh pipeline — reset the failure streak and take render ownership.
        guardRef.current = initRenderGuard();
        setPipelineReady(true);

        disposeRef.current = () => {
          postProcessing.dispose();
          if (bloomPass.dispose) bloomPass.dispose();
          if (scenePass.dispose) scenePass.dispose();
          emphasisPassRef?.dispose?.();
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
      setPipelineReady(false);
    };
  // bloomParamsRef is intentionally NOT a dep — initial values are read from the ref,
  // and live updates are handled by the bloom uniform updater useEffect below.
  // Deps intentionally narrowed; react-hooks/exhaustive-deps is not enforced in this config.
  }, [isEnabledWebGPU, gl, scene, camera, selective, bloomCamera]);

  // Update WebGPU bloom uniforms when settings change without full rebuild
  useEffect(() => {
    if (!isEnabledWebGPU || !bloomNodeRef.current) return;
    const bloomNode = bloomNodeRef.current;
    if (bloomNode.strength) bloomNode.strength.value = bloomStrength;
    if (bloomNode.radius) bloomNode.radius.value = bloomRadius;
    if (bloomNode.threshold) bloomNode.threshold.value = selective ? 0 : bloomThreshold;
  }, [isEnabledWebGPU, bloomStrength, bloomRadius, bloomThreshold, selective]);

  // WebGL EffectComposer path
  useEffect(() => {
    if (!isEnabledWebGL) {
      composerRef.current = null;
      setPipelineReady(false);
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

        if (!selective) {
          const bloomPass = new UnrealBloomPass(
            new THREE.Vector2(rtWidth, rtHeight),
            strength,
            radius,
            threshold
          );
          composer.addPass(bloomPass);
          composerRef.current = composer;
        } else {
          // ADR-2182 D4 selective bloom: a bloom-only composer renders the
          // bloom layer (bloomCamera) without the background into its own
          // target; the final composer adds that bloom over the full scene.
          const { ShaderPass } = await import('three/examples/jsm/postprocessing/ShaderPass.js');
          const bloomRt = new THREE.WebGLRenderTarget(rtWidth, rtHeight, { type: THREE.HalfFloatType });
          const bloomComposer = new EffectComposer(gl as unknown as THREE.WebGLRenderer, bloomRt);
          bloomComposer.renderToScreen = false;
          bloomComposer.addPass(new RenderPass(scene, bloomCamera));
          bloomComposer.addPass(new UnrealBloomPass(new THREE.Vector2(rtWidth, rtHeight), strength, radius, 0));
          const mix = new ShaderPass(new THREE.ShaderMaterial({
            uniforms: { baseTexture: { value: null }, bloomTexture: { value: bloomComposer.renderTarget2.texture } },
            vertexShader: 'varying vec2 vUv; void main() { vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }',
            // Last pass to the canvas: encode linear → sRGB like UnrealBloomPass's output
            // copy does (without it the whole frame came out too dark).
            fragmentShader: 'uniform sampler2D baseTexture; uniform sampler2D bloomTexture; varying vec2 vUv; void main() { gl_FragColor = texture2D(baseTexture, vUv) + vec4(texture2D(bloomTexture, vUv).rgb, 0.0);\n#include <colorspace_fragment>\n}',
          }), 'baseTexture');
          mix.needsSwap = true;
          composer.addPass(mix);
          composerRef.current = {
            render: () => {
              const bg = scene.background;
              scene.background = null;
              try { bloomComposer.render(); } finally { scene.background = bg; }
              composer.render();
            },
            setSize: (w: number, h: number) => { bloomComposer.setSize(w, h); composer.setSize(w, h); },
            dispose: () => { disposeComposer(bloomComposer); bloomRt.dispose(); disposeComposer(composer); },
          };
        }
        rtRef.current = rt;
        // Fresh pipeline — reset the failure streak and take render ownership.
        guardRef.current = initRenderGuard();
        setPipelineReady(true);
      } catch (err) {
        logger.warn('[GemPostProcessing] Failed to init WebGL bloom:', err);
      }
    })();

    return () => {
      disposed = true;
      if (composerRef.current) {
        disposeComposer(composerRef.current as Parameters<typeof disposeComposer>[0]);
        composerRef.current = null;
      }
      // Dispose custom render target separately — EffectComposer.dispose()
      // only disposes its internal targets, not the one passed to constructor.
      if (rtRef.current) {
        rtRef.current.dispose();
        rtRef.current = null;
      }
      setPipelineReady(false);
    };
  }, [isEnabledWebGL, gl, scene, camera, effectParamsWebGL, selective, bloomCamera]);

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

  // Render loop: only runs while an actual pipeline owns the frame (priority 1).
  //
  // Priority 1 tells R3F to skip its default gl.render() call (R3F v9 increments
  // internal.priority for any subscriber with priority > 0, and only calls
  // gl.render when internal.priority === 0). This prevents double-rendering.
  //
  // We take priority ONLY once a pipeline ref is actually built (pipelineReady).
  // While the dynamic imports resolve, if init fails, or once terminally
  // disabled, priority is undefined and R3F renders the scene directly via its
  // own working WebGPU/WebGL path. We therefore never issue a bare
  // renderer.render() fallback from here: on the WebGPU backend that throws
  // "Cannot read properties of null (reading 'configure')" during GPU
  // context loss/restore windows, and with priority 1 (R3F suppressed) it spins
  // into an unbounded stream of identical uncaught errors.
  const renderActive = pipelineReady && !ppDisabled;
  useFrame(() => {
    if (!renderActive) return;
    if (selective) {
      // keep the bloom-layer camera on the scene camera (copy() also copies
      // layers, so restore the bloom-only mask)
      bloomCamera.copy(camera as THREE.PerspectiveCamera, false);
      bloomCamera.layers.set(BLOOM_LAYER);
    }

    try {
      if (postProcessingRef.current) {
        postProcessingRef.current.render();
      } else if (composerRef.current) {
        composerRef.current.render();
      } else {
        // pipelineReady is true but the ref was cleared mid-frame (teardown
        // race). Nothing to draw; R3F resumes next frame once priority drops.
        return;
      }
      recordRenderSuccess(guardRef.current);
    } catch (err: unknown) {
      const { logFirstFailure, disableNow } = recordRenderFailure(guardRef.current, {
        maxConsecutiveFailures: MAX_CONSECUTIVE_RENDER_FAILURES,
      });
      if (logFirstFailure) {
        // One structured line per failure streak. Transient context-loss blips
        // recover on the next good frame (recordRenderSuccess resets the streak).
        logger.warn(
          '[GemPostProcessing] render failed (backing off):',
          err instanceof Error ? err.message : err,
        );
      }
      if (disableNow) {
        logger.warn(
          `[GemPostProcessing] disabling post-processing after ${MAX_CONSECUTIVE_RENDER_FAILURES} consecutive render failures — default renderer resumes`,
        );
        // Terminal state: tear down the pipeline and relinquish the frame to
        // R3F. Dropping pipelineReady/ppAvailable also stops the init effects
        // from rebuilding, so there is no retry loop.
        if (disposeRef.current) {
          try { disposeRef.current(); } catch { /* already torn down */ }
          disposeRef.current = null;
        }
        postProcessingRef.current = null;
        composerRef.current = null;
        bloomNodeRef.current = null;
        setPipelineReady(false);
        setPpDisabled(true);
      }
    }
  }, renderActive ? 1 : undefined);

  return null;
};

export default GemPostProcessing;
