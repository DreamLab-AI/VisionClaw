import React, { useRef, useState, useEffect, useCallback, lazy, Suspense } from 'react';
import { Canvas, useThree } from '@react-three/fiber';
import { OrbitControls, Stats, Environment, Lightformer } from '@react-three/drei';
import * as THREE from 'three';
import { createGemRenderer } from '../../../rendering/rendererFactory';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('GraphCanvas');

// GraphManager for rendering the actual graph
import GraphManager from './GraphManager';
// Render-loop diagnostics — DEV-only, lazily loaded so the module (and its
// __perf window side effects) is tree-shaken out of production builds.
// Arm via __perf.on() / ?perf=1 on the Vite dev server.
const PerfProbe = import.meta.env.DEV ? lazy(() => import('./PerfProbe')) : null;
// Post-processing effects - unified gem post-processing (WebGPU + WebGL bloom)
import { GemPostProcessing } from '../../../rendering/GemPostProcessing';
// Bots visualization for agent graph
import { BotsVisualization } from '../../bots/components';
// Agent action connections visualization
// SpacePilot Integration - using simpler version that works with useFrame
import SpacePilotSimpleIntegration from '../../visualisation/components/SpacePilotSimpleIntegration';
// Head Tracking for Parallax
import { HeadTrackedParallaxController } from '../../visualisation/components/HeadTrackedParallaxController';
// XR Support - causes graph to disappear
// import XRController from '../../xr/components/XRController';
// import XRVisualisationConnector from '../../xr/components/XRVisualisationConnector';

// Scene ambient effects (particles, fog, glow ring)
import WasmSceneEffects from '../../visualisation/components/WasmSceneEffects';
// Embedding cloud layer (live RuVector memory cloud + query trajectory)
import EmbeddingCloudLayer from '../../visualisation/components/EmbeddingCloudLayer';
// Memory explorer — HTML overlay for querying the cloud, mounted with it
import MemoryExplorerPanel from '../../visualisation/memoryCloud/MemoryExplorerPanel';
// Echo Pulse — commit-time expanding ring wash (control-center signature flourish)
import EchoPulseLayer from '../../control-center/echo/EchoPulseLayer';

// Bi-temporal provenance timeline scrubber (ADR-049) — bottom-docked HTML
// overlay, gated on the client-only provenance.enableTimeline flag (default off).
import TimelineScrubber from './TimelineScrubber';

// Store and utils
import { useSettingsStore } from '../../../store/settingsStore';
import { graphDataManager, type GraphData } from '../managers/graphDataManager';
import { CAMERA_FIT_EVENT } from '../hooks/useCameraAutoFit';

// ============================================================================
// Fit-view control — frames the whole graph bbox on demand
// ============================================================================
//
// Dispatches CAMERA_FIT_EVENT, which useCameraAutoFit (inside the Canvas) picks
// up and uses to reframe the camera around the node bounding box. HTML overlay
// so it lives outside the R3F tree, matching the LayoutModeIndicator pattern.
const FitViewButton: React.FC = () => {
  const handleFit = () => window.dispatchEvent(new CustomEvent(CAMERA_FIT_EVENT));
  return (
    <button
      onClick={handleFit}
      aria-label="Fit graph to view"
      title="Fit view (frame the whole graph)"
      style={{
        position: 'absolute',
        bottom: '14px',
        right: '14px',
        zIndex: 100,
        backgroundColor: 'rgba(10, 10, 30, 0.7)',
        color: 'rgba(255,255,255,0.85)',
        padding: '6px 12px',
        borderRadius: '6px',
        fontSize: '12px',
        fontFamily: '"Inter", "Segoe UI", sans-serif',
        border: '1px solid rgba(255,255,255,0.15)',
        cursor: 'pointer',
        backdropFilter: 'blur(8px)',
        pointerEvents: 'auto',
        userSelect: 'none',
      }}
    >
      ⤢ Fit view
    </button>
  );
};

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
// Camera aspect sync — keeps the perspective projection square to the canvas
// ============================================================================
//
// On the WebGPU renderer path R3F does not propagate the canvas aspect to the
// camera: its `setSize` leaves `camera.aspect` untouched, and the initial
// measure can be 0×0 because the async `createGemRenderer` init resolves after
// R3F's first layout pass. A stale `aspect` of 0 makes `updateProjectionMatrix`
// set the X scale (`projectionMatrix.elements[0]`) to Infinity, so every
// screen-space X projection becomes NaN — HTML-overlay labels collapse to the
// left edge and the view frustum degenerates (frustum culling lets thousands of
// off-screen labels through). This effect re-derives the aspect whenever the
// measured size changes, which is the WebGL path's behaviour too — harmless
// there since R3F already keeps it in sync.
const CameraAspectSync: React.FC = () => {
  const camera = useThree(s => s.camera);
  const width = useThree(s => s.size.width);
  const height = useThree(s => s.size.height);
  const invalidate = useThree(s => s.invalidate);
  const scene = useThree(s => s.scene);
  const gl = useThree(s => s.gl);
  const controls = useThree(s => s.controls);

  useEffect(() => {
    const w = window as any;
    w.__camera = camera;
    w.__scene = scene;
    w.__gl = gl;
    w.__controls = controls;
    w.__invalidate = invalidate;
  }, [camera, scene, gl, controls, invalidate]);

  useEffect(() => {
    // R3F tags cameras it manages with a `manual` flag that THREE's type omits.
    const cam = camera as THREE.PerspectiveCamera & { manual?: boolean };
    if (!cam.isPerspectiveCamera || cam.manual || width <= 0 || height <= 0) return;
    const aspect = width / height;
    if (Math.abs(cam.aspect - aspect) > 1e-6) {
      cam.aspect = aspect;
      cam.updateProjectionMatrix();
      invalidate();
    }
  }, [camera, width, height, invalidate]);

  return null;
};

// ============================================================================
// Event-system connection guard
// ============================================================================
//
// With an async `gl` factory (createGemRenderer resolves the WebGPU renderer
// after R3F's first commit) plus StrictMode's mount→unmount→remount, R3F can
// finish mounting with its pointer-event manager never attached to the canvas
// (`events.connected === false`). Every node interaction — click, drag,
// double-click — then dies silently: DOM events reach the canvas but R3F never
// raycasts them. Re-connect explicitly whenever we detect the disconnected
// state. `connect` tears down any previous target first, so this is idempotent.
const EnsureEventsConnected: React.FC = () => {
  const events = useThree(s => s.events);
  const gl = useThree(s => s.gl);
  useEffect(() => {
    const target = gl?.domElement;
    // `connected` can read false while a previous listener set is STILL
    // attached (R3F re-configures events with fresh handler identities and
    // resets the flag). Connecting again in that state stacks a second
    // listener set — every pointer event then delivers twice (double
    // page-opens on double-click). disconnect() first is not enough: it
    // consults the same stale flag and skips removal. So: only connect when
    // the event system has never attached anything (no prior connect call),
    // tracked via a marker on the canvas element itself.
    const el = target as (Element & { __r3fEventsAttached?: boolean }) | undefined;
    if (el && events.connect && !events.connected && !el.__r3fEventsAttached) {
      el.__r3fEventsAttached = true;
      events.connect(el);
      logger.info('[GraphCanvas] R3F event system was disconnected — connected to canvas');
    } else if (el && events.connected) {
      // R3F connected on its own — record it so we never double-attach.
      el.__r3fEventsAttached = true;
    }
  }, [events, gl]);
  return null;
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
    const ambientLightIntensity = useSettingsStore(s => s.settings?.visualisation?.rendering?.ambientLightIntensity ?? 0.25);
    const directionalLightIntensity = useSettingsStore(s => s.settings?.visualisation?.rendering?.directionalLightIntensity ?? 0.3);
    const sceneEffects = useSettingsStore(s => s.settings?.visualisation?.sceneEffects);
    const embeddingCloudEnabled = useSettingsStore(s => s.settings?.visualisation?.embeddingCloud?.enabled ?? false);
    const enableTimeline = useSettingsStore(s => s.settings?.provenance?.enableTimeline ?? false);
    
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
                    invalidate();
                }}
            >
                {/* Keep the camera projection square to the canvas — critical on
                    the WebGPU path where R3F leaves camera.aspect at 0. */}
                <CameraAspectSync />

                {/* Re-attach R3F pointer events if the async-renderer +
                    StrictMode mount left them disconnected (see component). */}
                <EnsureEventsConnected />

                {/* DEV-only render-loop diagnostics (no-op until __perf.on()).
                    Tree-shaken to zero in production builds. */}
                {import.meta.env.DEV && PerfProbe && (
                    <Suspense fallback={null}>
                        <PerfProbe />
                    </Suspense>
                )}

                {/* Lighting tuned for gem refraction -- driven by settings */}
                <ambientLight intensity={ambientLightIntensity} />
                <directionalLight position={[10, 10, 10]} intensity={directionalLightIntensity} />
                <directionalLight position={[-5, -5, -10]} intensity={0.2} />

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
                    {/* A flat near-black env (#111) gave the near-mirror glass
                        gems (roughness 0.08) nothing to reflect, so nodes read
                        as black. A brighter base plus a few Lightformer panels
                        supply reflected light and specular highlights so the
                        glass is visibly lit — the scene background stays dark
                        because background={false}. */}
                    <color attach="background" args={['#181b22']} />
                    <Lightformer intensity={1.4} position={[0, 6, -9]} scale={[12, 12, 1]} color="#cfe0ff" />
                    <Lightformer intensity={1.0} position={[-7, 1, -2]} scale={[12, 3, 1]} color="#a8c4ff" />
                    <Lightformer intensity={1.0} position={[7, -1, -2]} scale={[12, 3, 1]} color="#ffd9b0" />
                    <Lightformer intensity={0.8} position={[0, -6, 4]} scale={[12, 12, 1]} color="#6b7a9c" />
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

                {/* Echo Pulse — expanding ring wash on setting commit; self-gates on the
                    control-center feature flag + prefers-reduced-motion, zero cost when idle */}
                <EchoPulseLayer />

                {/* Embedding cloud — live memory sample behind graph nodes; draws query routes */}
                <EmbeddingCloudLayer enabled={embeddingCloudEnabled} />

                {/* onDragStateChange restores the node-drag ↔ camera arbitration
                    lost when GraphViewport was retired: while a node is being
                    dragged, OrbitControls must be disabled or every drag also
                    orbits the camera and the node never tracks the pointer.
                    Imperative ref toggle — no Canvas re-render per drag. */}
                {canvasReady && nodeCount > 0 && (
                    <GraphManager onDragStateChange={(isDragging) => {
                        if (orbitControlsRef.current) orbitControlsRef.current.enabled = !isDragging;
                    }} />
                )}
                
                {}
                
                {}
                <BotsVisualization />

                {/* Agent action beams are rendered by TransientBeamsLayer (mounted
                    inside GraphManager) — the sole action renderer since 2026-07-15. */}
                {}
                <OrbitControls
                    ref={(c: any) => { orbitControlsRef.current = c; (window as any).__orbit = c; }}
                    makeDefault
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

            {/* Fit-view control — reframes the camera on the whole graph bbox */}
            {canvasReady && nodeCount > 0 && <FitViewButton />}

            {/* Bi-temporal provenance timeline (ADR-049) — bottom-docked overlay,
                mounted only when the client-only provenance.enableTimeline flag is on. */}
            {enableTimeline && <TimelineScrubber />}

            {/* Memory explorer — query the live memory cloud and replay the
                route; mounted only while the embedding cloud is switched on. */}
            {embeddingCloudEnabled && <MemoryExplorerPanel />}
        </div>
    );
};

export default GraphCanvas;