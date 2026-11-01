/**
 * BotsShared.tsx
 * Shared primitives for the Bots visualization:
 *   - CSS keyframe injection (runs once at module load)
 *   - Pure helpers: generateAgentTypeColor, getVisionClawColors
 *   - Pre-allocated module-scope Three.js objects (zero-alloc pattern)
 */
import * as THREE from 'three';

// ---------------------------------------------------------------------------
// Pre-allocated Three.js objects — never re-create in render/frame loops
// ---------------------------------------------------------------------------
export const _tempVec3A = new THREE.Vector3();
export const _tempVec3B = new THREE.Vector3();
export const _tempVec3Mid = new THREE.Vector3();
export const _tempVec3Perp = new THREE.Vector3();
export const QUEEN_GOLD = new THREE.Color('#FFD700');
export const ADDITIVE_BLENDING = THREE.AdditiveBlending;
export const BACK_SIDE = THREE.BackSide;

// ---------------------------------------------------------------------------
// CSS injection (runs once at module import time)
// ---------------------------------------------------------------------------
const pulseKeyframes = `
  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.7; transform: scale(0.95); }
  }
  @keyframes fadeIn {
    from { opacity: 0; transform: translateY(-2px); }
    to { opacity: 1; transform: translateY(0); }
  }
  @keyframes sparkle {
    0%, 100% { opacity: 1; text-shadow: 0 0 4px rgba(243, 156, 18, 0.8); }
    50% { opacity: 0.8; text-shadow: 0 0 8px rgba(243, 156, 18, 1); }
  }
`;

if (typeof document !== 'undefined' && !document.querySelector('#bots-visualization-styles')) {
  const style = document.createElement('style');
  style.id = 'bots-visualization-styles';
  style.textContent = pulseKeyframes;
  document.head.appendChild(style);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Pre-allocated scratch for swarm-tint HSL maths (module scope — no per-call alloc).
const _tintColor = new THREE.Color();
const _tintHsl = { h: 0, s: 0, l: 0 };

/**
 * Stable hue offset (in 0-1 hue units) derived from a swarmId. Deterministic hash
 * → ±0.05 rotation (~±18°) so distinct swarms read as related-but-separate families
 * without drifting far from the base type colour. Empty/undefined swarmId → 0.
 */
export const swarmHueOffset = (swarmId: string | undefined): number => {
  if (!swarmId) return 0;
  let hash = 0;
  for (let i = 0; i < swarmId.length; i++) {
    hash = ((hash << 5) - hash + swarmId.charCodeAt(i)) | 0;
  }
  return (((hash >>> 0) % 101) - 50) / 1000; // -0.05 .. +0.05
};

/** Hue-rotate a base colour by the swarm's stable offset. */
export const applySwarmTint = (baseColor: string, swarmId: string | undefined): string => {
  const offset = swarmHueOffset(swarmId);
  if (offset === 0) return baseColor;
  _tintColor.set(baseColor).getHSL(_tintHsl);
  _tintColor.setHSL((_tintHsl.h + offset + 1) % 1, _tintHsl.s, _tintHsl.l);
  return `#${_tintColor.getHexString()}`;
};

/** Hash-based colour from agent type string. */
export const generateAgentTypeColor = (agentType: string): string => {
  let hash = 0;
  for (let i = 0; i < agentType.length; i++) {
    const char = agentType.charCodeAt(i);
    hash = ((hash << 5) - hash) + char;
    hash = hash & hash;
  }

  const coordColors  = ['#F1C40F', '#F39C12', '#E67E22', '#D68910', '#B7950B'];
  const devColors    = ['#2ECC71', '#27AE60', '#1ABC9C', '#16A085', '#229954'];
  const specialColors = ['#9B59B6', '#8E44AD', '#E74C3C', '#C0392B', '#3498DB'];

  const coordTypes = ['queen', 'coordinator', 'architect', 'monitor', 'manager'];
  const devTypes   = ['coder', 'tester', 'reviewer', 'documenter', 'developer'];

  let colorPalette = specialColors;
  if (coordTypes.some(type => agentType.toLowerCase().includes(type))) {
    colorPalette = coordColors;
  } else if (devTypes.some(type => agentType.toLowerCase().includes(type))) {
    colorPalette = devColors;
  }

  const colorIndex = Math.abs(hash) % colorPalette.length;
  return colorPalette[colorIndex];
};

/** Resolve per-agent colour configuration from settings. */
export const getVisionClawColors = (settings: Record<string, unknown> | undefined) => {
  const vis              = settings?.visualisation as Record<string, unknown> | undefined;
  const graphs           = vis?.graphs as Record<string, unknown> | undefined;
  const visionclawSettings = graphs?.visionclaw as Record<string, unknown> | undefined;
  const vfNodes          = visionclawSettings?.nodes as Record<string, unknown> | undefined;
  const baseColor        = (vfNodes?.baseColor as string | undefined) || '#F1C40F';

  const rendering   = vis?.rendering as Record<string, unknown> | undefined;
  const agentColors = rendering?.agentColors as Record<string, string> | undefined;

  const fallback = {
    getAgentColor: (agentType: string) => generateAgentTypeColor(agentType),
    coder: generateAgentTypeColor('coder'),
    tester: generateAgentTypeColor('tester'),
    researcher: generateAgentTypeColor('researcher'),
    reviewer: generateAgentTypeColor('reviewer'),
    documenter: generateAgentTypeColor('documenter'),
    specialist: generateAgentTypeColor('specialist'),
    queen: generateAgentTypeColor('queen'),
    coordinator: baseColor,
    architect: generateAgentTypeColor('architect'),
    monitor: generateAgentTypeColor('monitor'),
    analyst: generateAgentTypeColor('analyst'),
    optimizer: generateAgentTypeColor('optimizer'),
    edge: '#3498DB',
    activeEdge: '#2980B9',
    active: '#2ECC71',
    busy: '#F39C12',
    idle: '#95A5A6',
    error: '#E74C3C',
  };

  if (!agentColors || Object.keys(agentColors).length === 0) return fallback;

  return {
    getAgentColor: (agentType: string) => agentColors[agentType] || generateAgentTypeColor(agentType),
    coder: agentColors.coder || generateAgentTypeColor('coder'),
    tester: agentColors.tester || generateAgentTypeColor('tester'),
    researcher: agentColors.researcher || generateAgentTypeColor('researcher'),
    reviewer: agentColors.reviewer || generateAgentTypeColor('reviewer'),
    documenter: agentColors.documenter || generateAgentTypeColor('documenter'),
    specialist: agentColors.default || generateAgentTypeColor('specialist'),
    queen: agentColors.queen || generateAgentTypeColor('queen'),
    coordinator: agentColors.coordinator || baseColor,
    architect: agentColors.architect || generateAgentTypeColor('architect'),
    monitor: agentColors.default || generateAgentTypeColor('monitor'),
    analyst: agentColors.analyst || generateAgentTypeColor('analyst'),
    optimizer: agentColors.optimizer || generateAgentTypeColor('optimizer'),
    edge: '#3498DB',
    activeEdge: '#2980B9',
    active: '#2ECC71',
    busy: '#F39C12',
    idle: '#95A5A6',
    error: '#E74C3C',
  };
};

/** Format processing logs for display (passthrough, no mock generation). */
export const formatProcessingLogs = (logs: string[] | undefined): string[] => logs || [];
