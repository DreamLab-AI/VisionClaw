import React, { useRef, useState, useEffect, useCallback } from 'react';
import { Canvas, useThree } from '@react-three/fiber';
import { OrbitControls, Stats, Environment } from '@react-three/drei';
import * as THREE from 'three';
import { createGemRenderer } from '../../../rendering/rendererFactory';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('GraphCanvas');

// GraphManager for rendering the actual graph
import GraphManager from './GraphManager';
// Post-processing effects - unified gem post-processing (WebGPU + WebGL bloom)
import { GemPostProcessing } from '../../../rendering/GemPostProcessing';
// Bots visualization for agent graph
import { BotsVisualization } from '../../bots/components';
// Agent action connections visualization
import { AgentActionVisualization } from '../../visualisation/components/AgentActionVisualization';
// SpacePilot Integration - using simpler version that works with useFrame
import SpacePilotSimpleIntegration from '../../visualisation/components/SpacePilotSimpleIntegration';
// Head Tracking for Parallax
import { HeadTrackedParallaxController } from '../../visualisation/components/HeadTrackedParallaxController';
// XR Support - causes graph to disappear
// import XRController from '../../xr/components/XRController';
// import XRVisualisationConnector from '../../xr/components/XRVisualisationConnector';

// Scene ambient effects (particles, fog, glow ring)
import WasmSceneEffects from '../../visualisation/components/WasmSceneEffects';
// Embedding cloud layer (PCA-projected RuVector vector embeddings)
import EmbeddingCloudLayer from '../../visualisation/components/EmbeddingCloudLayer';

// Store and utils
import { useSettingsStore } from '../../../store/settingsStore';
import { graphDataManager, type GraphData } from '../managers/graphDataManager';

// ============================================================================
// Layout Mode Indicator — displays current layout mode above the canvas
// ============================================================================

const LAYOUT_MODE_LABELS: Record<string, string> = {
  'force-directed':  'Force Directed',
  'dag-topdown':     'DAG Top-Down',
  'dag-radial':      'DAG Radial',
  'dag-leftright':   'DAG Left-Right',
  'type-clustering': 'Type Clustering',
  'forceDirected':   'Force Directed',
  'hierarchical':    'Hierarchical',
  'radial':          'Radial',
  'spectral':        'Spectral',
  'temporal':        'Temporal',
  'clustered':       'Clustered',
};

const LayoutModeIndicator: React.FC = () => {
  const layoutMode = useSettingsStore(s =>
    (s.settings as unknown as Record<string, Record<string, unknown>>)?.qualityGates?.layoutMode as string | undefined
  );
  const [transitioning, setTransitioning] = useState(false);
  const transitionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!layoutMode) return;
    setTransitioning(true);
    if (transitionTimerRef.current) clearTimeout(transitionTimerRef.current);
    transitionTimerRef.current = setTimeout(() => setTransitioning(false), 1200);
    return () => {
      if (transitionTimerRef.current) clearTimeout(transitionTimerRef.current);
    };
  }, [layoutMode]);

  if (!layoutMode) return null;

  const label = LAYOUT_MODE_LABELS[layoutMode] || layoutMode;

  return (
    <div style={{
      position: 'absolute',
      bottom: '14px',
      left: '50%',
      transform: 'translateX(-50%)',
      zIndex: 100,
      backgroundColor: 'rgba(0, 0, 0, 0.55)',
      color: transitioning ? '#34d399' : 'rgba(255,255,255,0.65)',
      padding: '3px 10px',
      borderRadius: '4px',
      fontSize: '10px',
      letterSpacing: '0.05em',
      fontFamily: 'monospace',
      border: `1px solid ${transitioning ? 'rgba(52,211,153,0.5)' : 'rgba(255,255,255,0.1)'}`,
      transition: 'color 0.3s, border-color 0.3s',
      pointerEvents: 'none',
      userSelect: 'none',
    }}>
      {transitioning ? `Transitioning to ${label}...` : `Layout: ${label}`}
    </div>
  );
};

// ============================================================================
// Phase 6 (ADR-04 D5 / T3) — Software-WebGL detection + Environment fallback
// ============================================================================
//
// `<Environment resolution={256}>` triggers drei's PMREM generator which
// renders 6 cube faces through the WebGL pipeline. On hardware GL that's
// GPU-side and fast. On software GL (SwiftShader, llvmpipe, etc.) each draw
// is CPU-rasterised, holding the JS thread for 2–4s on mount and producing
// the headless-mode freeze.
//
// Detection uses WEBGL_debug_renderer_info / UNMASKED_RENDERER_WEBGL. If the
// extension is absent (some browsers / privacy modes block it), we default
// to the hardware path — failing graceful.
//
// Policy (settings.rendering.softwareFallback):
//   - 'auto'       (default) — detect renderer; use fallback only on software
//   - 'force-on'   — always use fallback (no <Environment>)
//   - 'force-off'  — always render <Environment>, ignore detection
// ============================================================================

const SOFTWARE_RENDERER_PATTERNS = [
  'swiftshader',
  'llvmpipe',
  'software',
  'microsoft basic render driver',
];

function detectSoftwareRenderer(gl: THREE.WebGLRenderer): { isSoftware: boolean; rendererString: string | null } {
  try {
    const ctx = gl.getContext();
    const ext = ctx.getExtension('WEBGL_debug_renderer_info');
    if (!ext) {
      // Extension blocked / unavailable — default to hardware
      return { isSoftware: false, rendererString: null };
    }
    const renderer = ctx.getParameter(ext.UNMASKED_RENDERER_WEBGL) as string;
    if (typeof renderer !== 'string') return { isSoftware: false, rendererString: null };
    const lower = renderer.toLowerCase();
    const isSoftware = SOFTWARE_RENDERER_PATTERNS.some(p => lower.includes(p));
    return { isSoftware, rendererString: renderer };
  } catch {
    return { isSoftware: false, rendererString: null };
  }
}

// Main GraphCanvas component
const GraphCanvas: React.FC = () => {

    const containerRef = useRef<HTMLDivElement>(null);
    const orbitControlsRef = useRef<any>(null);
    // Phase 6 (ADR-04 D5): software-renderer detection result.
    // null = not yet detected; true = software; false = hardware/unknown.
    const [isSoftwareRenderer, setIsSoftwareRenderer] = useState<boolean | null>(null);
    const softwareDetectedLoggedRef = useRef<boolean>(false);
    const softwareFallbackPolicy = useSettingsStore(
      s => s.settings?.visualisation?.rendering?.softwareFallback as ('auto' | 'force-on' | 'force-off' | undefined)
    ) ?? 'auto';
    // Narrow selectors — prevent full Canvas tree re-render on unrelated settings changes.
    // Each selector returns a primitive or stable nested ref so Zustand's Object.is comparison
    // only triggers re-renders when that specific value actually changes.
    const showStats = useSettingsStore(s => s.settings?.system?.debug?.enablePerformanceDebug ?? false);
    const enableGlow = useSettingsStore(s => s.settings?.visualisation?.glow?.enabled !== false);
    const ambientLightIntensity = useSettingsStore(s => s.settings?.visualisation?.rendering?.ambientLightIntensity ?? 0.5);
    const directionalLightIntensity = useSettingsStore(s => s.settings?.visualisation?.rendering?.directionalLightIntensity ?? 0.4);
    const sceneEffects = useSettingsStore(s => s.settings?.visualisation?.sceneEffects);
    const embeddingCloudEnabled = useSettingsStore(s => s.settings?.visualisation?.embeddingCloud?.enabled ?? false);
    
    // Lightweight subscription: only track counts to avoid storing full graph data in two places
    const [nodeCount, setNodeCount] = useState(0);
    const [edgeCount, setEdgeCount] = useState(0);
    const [canvasReady, setCanvasReady] = useState(false);

    useEffect(() => {
        let mounted = true;

        const handleGraphData = (data: GraphData) => {
            if (mounted) {
                setNodeCount(data.nodes.length);
                setEdgeCount(data.edges.length);
            }
        };

        const unsubscribe = graphDataManager.onGraphDataChange(handleGraphData);

        graphDataManager.getGraphData().then((data) => {
            if (mounted) {
                setNodeCount(data.nodes.length);
                setEdgeCount(data.edges.length);
            }
        }).catch((error) => {
            logger.error('Failed to load initial graph data:', error);
        });

        return () => {
            mounted = false;
            unsubscribe();
        };
    }, []);

    return (
        <div
            ref={containerRef}
            role="img"
            aria-label="Interactive 3D graph visualization"
            style={{
                position: 'fixed',
                top: 0,
                left: 0,
                width: '100vw',
                height: '100vh',
                backgroundColor: '#000033',
                zIndex: 0
            }}
        >
            <span className="sr-only">3D graph with {nodeCount} nodes and {edgeCount} edges</span>
            {}
            {showStats && (
                <div style={{
                    position: 'absolute',
                    top: '10px',
                    left: '10px',
                    color: 'white',
                    backgroundColor: 'rgba(0, 0, 0, 0.6)',
                    padding: '5px 10px',
                    zIndex: 1000,
                    fontSize: '12px'
                }}>
                    Nodes: {nodeCount} | Edges: {edgeCount} | Ready: {canvasReady ? 'Yes' : 'No'}
                </div>
            )}

            <Canvas
                gl={createGemRenderer}
                dpr={[1, 2]}
                camera={{
                    fov: 75,
                    near: 0.1,
                    far: 5000,
                    position: [80, 60, 80]
                }}
                onCreated={({ gl, camera, scene, invalidate }) => {
                    gl.setClearColor(0x000033, 1);

                    // Phase 6 (ADR-04 D5): one-shot software-renderer detection.
                    // Only run if gl is a WebGLRenderer (skip WebGPU paths).
                    if (gl instanceof THREE.WebGLRenderer) {
                        const { isSoftware, rendererString } = detectSoftwareRenderer(gl);
                        setIsSoftwareRenderer(isSoftware);
                        if (!softwareDetectedLoggedRef.current) {
                            softwareDetectedLoggedRef.current = true;
                            const fallbackDecision = softwareFallbackPolicy === 'force-off'
                                ? 'forced-environment'
                                : (softwareFallbackPolicy === 'force-on' || isSoftware)
                                    ? 'software-fallback'
                                    : 'environment';
                            logger.info(
                                `[GraphCanvas] WebGL renderer: ${rendererString === null ? 'unknown (extension blocked)' : `"${rendererString}"`} ` +
                                `→ software detected: ${isSoftware}, policy: ${softwareFallbackPolicy}, decision: ${fallbackDecision}`
                            );
                        }
                    }

                    setCanvasReady(true);
                    // Force initial render — Edge/WebGPU doesn't paint until
                    // a resize event occurs (e.g. opening DevTools). Scheduling
                    // invalidation + a synthetic resize ensures first frame draws.
                    invalidate();
                    setTimeout(() => {
                        invalidate();
                        window.dispatchEvent(new Event('resize'));
                    }, 100);
                    setTimeout(() => {
                        invalidate();
                        window.dispatchEvent(new Event('resize'));
                    }, 500);
                }}
            >
                {/* Lighting tuned for gem refraction -- driven by settings */}
                <ambientLight intensity={ambientLightIntensity} />
                <directionalLight position={[10, 10, 10]} intensity={directionalLightIntensity} />
                <directionalLight position={[-5, -5, -10]} intensity={0.3} />

                {/* Environment map for PBR glass material reflections.
                    Uses a generated environment instead of CDN-hosted HDR to avoid
                    network failures in Docker/LAN/offline environments.

                    Phase 6 (ADR-04 D5): on software-rendered WebGL contexts the
                    PMREM generation freezes the JS thread for 2-4s. Detection
                    happens in `onCreated`; below we skip <Environment> on
                    software, or honour the explicit policy override. The result
                    is graceful: hardware retains the full PBR look; software
                    gets a flat-but-fast scene. */}
                {(softwareFallbackPolicy === 'force-off' ||
                  (softwareFallbackPolicy === 'auto' && isSoftwareRenderer !== true)) && (
                  <Environment background={false} resolution={256}>
                    <color attach="background" args={['#111']} />
                  </Environment>
                )}

                {/* Scene ambient effects (WASM particles, wisps, atmosphere) */}
                <WasmSceneEffects
                    enabled={sceneEffects?.enabled !== false}
                    particleCount={sceneEffects?.particleCount ?? 256}
                    intensity={sceneEffects?.particleOpacity ?? 0.6}
                    particleDrift={sceneEffects?.particleDrift ?? 0.5}
                    particleColor={sceneEffects?.particleColor}
                    wispsEnabled={sceneEffects?.wispsEnabled !== false}
                    wispCount={sceneEffects?.wispCount ?? 48}
                    wispDriftSpeed={sceneEffects?.wispDriftSpeed ?? 1.0}
                    wispColor={sceneEffects?.wispColor}
                    atmosphereEnabled={sceneEffects?.fogEnabled !== false}
                    atmosphereResolution={sceneEffects?.atmosphereResolution ?? 128}
                />

                {/* Embedding cloud — background layer behind graph nodes */}
                <EmbeddingCloudLayer enabled={embeddingCloudEnabled} />

                {}
                {canvasReady && nodeCount > 0 && (
                    <GraphManager />
                )}
                
                {}
                
                {}
                <BotsVisualization />

                {/* Agent Action Connections - ephemeral animated connections */}
                <AgentActionVisualization showStats={showStats} />

                {}
                <OrbitControls
                    ref={orbitControlsRef}
                    enablePan={true}
                    enableZoom={true}
                    enableRotate={true}
                    zoomSpeed={0.8}
                    panSpeed={0.8}
                    rotateSpeed={0.8}
                />
                {}
                <SpacePilotSimpleIntegration orbitControlsRef={orbitControlsRef} />

                {}
                <HeadTrackedParallaxController />

                {}
                {}
                {}
                
                {}
                <GemPostProcessing enabled={enableGlow} />
                
                {}
                {showStats && <Stats />}
            </Canvas>

            {/* Layout mode indicator — rendered in HTML overlay above the canvas */}
            <LayoutModeIndicator />
        </div>
    );
};

export default GraphCanvas;