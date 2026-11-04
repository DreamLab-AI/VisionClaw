import { useEffect, useMemo, useRef } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import { sharedNodeIdToIndexMap, sharedNodePositions } from '../graph/contexts/NodePositionContext';
import { sceneFitBounds } from '../graph/utils/sceneFitBounds';
import { useReducedMotion } from '../visualisation/memoryCloud/useReducedMotion';
import { useSettingsStore } from '../../store/settingsStore';
import type { Sphere } from './fit';
import { registerSplatCommands } from './commands';
import { SplatController } from './SplatController';
import { useSplatStore } from './splatStore';

/** The graph's bounding sphere (both graphs; the memory cloud is not counted). */
function readGraphSphere(): Sphere | null {
  const n = sharedNodeIdToIndexMap.size;
  if (!sharedNodePositions || n === 0) return null;
  const b = sceneFitBounds(sharedNodePositions, n, null);
  return b ? { centre: b.centre, radius: b.radius } : null;
}

/**
 * Scene-level splat layer (ADR-2170 decisions 1, 2 and 9). Its group is a child
 * of the Canvas root, never of a graph node or the graph root. World-locked and
 * static; the backend is chosen at mount from the live renderer. Renders
 * nothing while no asset is open, and nothing at the low visual-quality setting.
 */
export default function SplatScene() {
  const gl = useThree((s) => s.gl);
  const groupRef = useRef<import('three').Group>(null);
  const reducedMotion = useReducedMotion();
  const reducedRef = useRef(reducedMotion);
  reducedRef.current = reducedMotion;
  const lowQuality = useSettingsStore((s) => s.settings?.visualisation?.graphs?.knowledge?.nodes?.quality === 'low');
  const assets = useSplatStore((s) => s.assets);
  const activeId = useSplatStore((s) => s.activeId);
  const mode = useSplatStore((s) => s.mode);
  const controllerRef = useRef<SplatController | null>(null);

  useEffect(() => {
    void useSplatStore.getState().loadListing();
    return registerSplatCommands();
  }, []);

  useEffect(() => {
    const group = groupRef.current;
    if (!group) return;
    const controller = new SplatController(gl, group, {
      graphSphere: readGraphSphere,
      reducedMotion: () => reducedRef.current,
      onStatus: (status, d) => useSplatStore.getState().setRenderStatus(status, d.variant, d.message),
    });
    controllerRef.current = controller;
    return () => {
      controller.dispose();
      controllerRef.current = null;
    };
  }, [gl]);

  const asset = useMemo(() => assets.find((a) => a.id === activeId) ?? null, [assets, activeId]);

  useEffect(() => {
    const controller = controllerRef.current;
    if (!controller) return;
    // low visual quality removes the cloud at once, with no transition
    if (asset && lowQuality) controller.withdraw();
    else controller.request(asset ? { asset, mode } : null);
  }, [asset, mode, lowQuality, gl]);

  useFrame((_, dt) => controllerRef.current?.update(dt));

  return <group ref={groupRef} name="splat-scene" />;
}
