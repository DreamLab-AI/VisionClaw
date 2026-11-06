/**
 * Source group — Agents (id `agents`). ADR-2189 places its fields under
 * Agents & Governance › Agent operations / Agent appearance / Agent colours.
 *
 * Agent node colour reads `visualisation.graphs.visionclaw.nodes.baseColor`
 * (BotsShared.getVisionClawColors). The per-type palette lives at
 * `visualisation.rendering.agentColors.*`, typed both sides (client
 * AgentColorsSettings ↔ server AgentColorsDTO). `graphTypeVisuals.agent.*` are
 * client-typed behaviour knobs consumed by AgentOverlay / TransientBeamsLayer.
 * ADR-2189 deleted the visionclaw node-material, edge and label fields, the
 * health-colour stops, swarm tint, membrane opacity and health bar: nothing
 * read them (docs/audits/menus-2026-11-06.md §4).
 *
 * These paths POST-DATE the frozen WP5 baseline, so registry.test.ts asserts them
 * separately from the legacy zero-drift fixture (see test (c)/(c2)).
 */
import type { GroupData, RegistryField } from '../types';

const N = 'visualisation.graphs.visionclaw.nodes.';
const A = 'visualisation.graphTypeVisuals.agent.';
// Knowledge-graph attention-heat knobs (client-typed graphTypeVisuals.knowledgeGraph.*).
// Grouped with the agent-action beam controls because they share the same 0x23
// source — agents touching KG nodes — just projected as node heat rather than a beam.
const K = 'visualisation.graphTypeVisuals.knowledgeGraph.';
// Per-agent-type palette — routes to the `rendering` server bucket and round-trips
// via AgentColorsDTO (types.rs) ← DevConfig.agent_colors; consumed by
// BotsShared.getVisionClawColors. Typed both sides (client AgentColorsSettings).
const C = 'visualisation.rendering.agentColors.';

const fields: RegistryField[] = [
  // Agent Nodes — visionclaw node material (Rust GraphSettings.nodes)
  { key: 'agentBaseColor', subgroup: 'Agent Nodes', label: 'Node Color', type: 'color', path: `${N}baseColor`, description: 'Base colour for agent capsule nodes' },
  // Agent Type Colours — per-type palette (rendering.agentColors, typed both sides via AgentColorsDTO)
  { key: 'agentColorCoordinator', subgroup: 'Agent Type Colours', label: 'Coordinator', type: 'color', path: `${C}coordinator`, description: 'Colour for coordinator agents' },
  { key: 'agentColorCoder', subgroup: 'Agent Type Colours', label: 'Coder', type: 'color', path: `${C}coder`, description: 'Colour for coder agents' },
  { key: 'agentColorArchitect', subgroup: 'Agent Type Colours', label: 'Architect', type: 'color', path: `${C}architect`, description: 'Colour for architect agents' },
  { key: 'agentColorAnalyst', subgroup: 'Agent Type Colours', label: 'Analyst', type: 'color', path: `${C}analyst`, description: 'Colour for analyst agents' },
  { key: 'agentColorTester', subgroup: 'Agent Type Colours', label: 'Tester', type: 'color', path: `${C}tester`, description: 'Colour for tester agents' },
  { key: 'agentColorResearcher', subgroup: 'Agent Type Colours', label: 'Researcher', type: 'color', path: `${C}researcher`, description: 'Colour for researcher agents' },
  { key: 'agentColorReviewer', subgroup: 'Agent Type Colours', label: 'Reviewer', type: 'color', path: `${C}reviewer`, description: 'Colour for reviewer agents' },
  { key: 'agentColorOptimizer', subgroup: 'Agent Type Colours', label: 'Optimizer', type: 'color', path: `${C}optimizer`, description: 'Colour for optimizer agents' },
  { key: 'agentColorDocumenter', subgroup: 'Agent Type Colours', label: 'Documenter', type: 'color', path: `${C}documenter`, description: 'Colour for documenter agents' },
  { key: 'agentColorQueen', subgroup: 'Agent Type Colours', label: 'Queen', type: 'color', path: `${C}queen`, description: 'Colour for the queen/orchestrator agent' },
  { key: 'agentColorDefault', subgroup: 'Agent Type Colours', label: 'Default / Other', type: 'color', path: `${C}default`, description: 'Fallback colour for agent types without a specific palette entry' },
  { key: 'agentShowTrails', subgroup: 'Behaviour', label: 'Motion Trails', type: 'toggle', path: `${A}showTrails`, description: 'Draw a fading motion-trail ribbon behind each moving agent so the swarm reads as in motion' },
  { key: 'agentTrailLength', subgroup: 'Behaviour', label: 'Trail Length', type: 'slider', min: 8, max: 48, step: 1, path: `${A}trailLength`, description: 'Number of sampled positions retained in each agent trail (longer = more history)' },
  { key: 'agentBioluminescence', subgroup: 'Behaviour', label: 'Bioluminescent Intensity', type: 'slider', min: 0, max: 3, step: 0.05, path: `${A}bioluminescentIntensity`, description: 'Membrane bioluminescence of agent capsules' },
  { key: 'agentNucleusGlow', subgroup: 'Behaviour', label: 'Nucleus Glow', type: 'slider', min: 0, max: 2, step: 0.05, path: `${A}nucleusGlowIntensity`, description: 'Base emissive of the agent capsule core' },
  { key: 'agentBreathingSpeed', subgroup: 'Behaviour', label: 'Breathing Speed', type: 'slider', min: 0, max: 3, step: 0.05, path: `${A}breathingSpeed`, description: 'Rate of the idle/active breathing pulse' },
  { key: 'agentBreathingAmplitude', subgroup: 'Behaviour', label: 'Breathing Amplitude', type: 'slider', min: 0, max: 1, step: 0.05, path: `${A}breathingAmplitude`, description: 'Depth of the breathing pulse' },
  { key: 'agentBeamRadius', subgroup: 'Behaviour', label: 'Action Beam Radius', type: 'slider', min: 0.05, max: 1.5, step: 0.05, path: `${A}beamRadius`, description: 'Cylinder radius of embodied agent-action beams (0x23)' },
  { key: 'agentBeamOpacity', subgroup: 'Behaviour', label: 'Action Beam Opacity', type: 'slider', min: 0, max: 1, step: 0.05, path: `${A}beamOpacity`, description: 'Peak opacity of agent-action beams during their hold phase' },
  // Nameplate LOD — declutter dense swarms (client-typed graphTypeVisuals.agent; consumed by AgentNameplate).
  { key: 'agentNameplateLod', subgroup: 'Behaviour', label: 'Nameplate LOD', type: 'toggle', path: `${A}nameplateLod`, description: 'Declutter dense swarms: show the full agent nameplate only when the camera is close (or the agent is hovered/selected/queen), collapse to a name-only line mid-range, and hide it far away' },
  { key: 'agentNameplateFullDistance', subgroup: 'Behaviour', label: 'Nameplate Full Distance', type: 'slider', min: 10, max: 120, step: 5, path: `${A}nameplateFullDistance`, description: 'Camera distance (world units) under which an agent shows its full 3-line nameplate; the name-only band extends to ~2.25× this before hiding' },
  // Attention heat — agent 0x23 actions heat knowledge/ontology nodes (client-typed
  // graphTypeVisuals.knowledgeGraph.*; consumed by GemNodes → metadata texture w).
  // Sibling to the beam controls: same agent-action source, projected onto the KG.
  { key: 'kgAttentionHeat', subgroup: 'Behaviour', label: 'File-Attention Heat', type: 'toggle', path: `${K}attentionHeatEnabled`, description: 'Knowledge/ontology nodes heat up (glow) as agents touch them via 0x23 actions, then cool as the heat decays' },
  { key: 'kgAttentionHeatHalfLife', subgroup: 'Behaviour', label: 'Attention Heat Half-Life', type: 'slider', min: 5, max: 60, step: 1, path: `${K}attentionHeatHalfLife`, description: 'Seconds for a node\'s attention heat to fade by half' },
  // Per-viewer visual layer; the XR HUD has the matching toggle (same default: on).
  { key: 'agentOffsetToggle', subgroup: 'Behaviour', label: 'Agent Offset Animation', type: 'action-button', action: 'toggle-agent-offsets', description: 'Agents move toward the node they work on and drift when idle. Off draws every agent exactly at its server position. This browser only; the headset has its own switch' },
  // Demo mode — synthetic agent injection for visual verification (no backend path).
  { key: 'agentDemoToggle', subgroup: 'Demo', label: 'Toggle Agent Demo', type: 'action-button', action: 'toggle-demo', description: 'Run six agents through a live work loop on the graph. One server-side demo: the headset shows the same agents working the same nodes, and either client can start or stop it' },
];

export const agents: GroupData = {
  id: 'agents',
  label: 'Agents',
  description: 'Agent/swarm node, edge, label appearance and per-type behaviour visuals.',
  loadPaths: [
    'visualisation.graphs.visionclaw.nodes',
    'visualisation.graphTypeVisuals',
    'visualisation.rendering',
  ],
  fields,
};
