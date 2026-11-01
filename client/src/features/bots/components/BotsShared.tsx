/**
 * BotsShared.tsx
 * Colour helpers for the agent overlay and markers: generateAgentTypeColor, getVisionClawColors, swarm
 * tint. No positions: where an agent is drawn is decided only by the marker pass (ADR-2142 / ADR-2135).
 */
import * as THREE from 'three';

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
