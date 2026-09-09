/**
 * Group 9 — Agents (id `agents`, hotkey 9, 41 fields).
 *
 * Genuinely settings-manageable look-and-feel for the agent/swarm population.
 * The server resolves "visionclaw"|"agent"|"bots" → graphs.visionclaw
 * (app_settings.rs), so agent nodes/edges/labels read from
 * `visualisation.graphs.visionclaw.{nodes,edges,labels}.*` — real fields on the
 * Rust GraphSettings AND the client typed mirror. The per-type palette lives at
 * `visualisation.rendering.agentColors.*` — typed both sides (client
 * AgentColorsSettings ↔ server AgentColorsDTO) and consumed by
 * BotsShared.getVisionClawColors. `graphTypeVisuals.agent.*` are client-typed
 * behaviour knobs (consumed by GemNodes / BotsNode), mirroring the existing
 * `look` group's graphTypeVisuals.{knowledgeGraph,ontology} precedent; `swarmTint`
 * is a client-only toggle read by BotsVisualization → BotsNode.
 *
 * These paths POST-DATE the frozen WP5 baseline, so registry.test.ts asserts them
 * separately from the legacy zero-drift fixture (see test (c)/(c2)).
 */
import type { GroupData, RegistryField } from '../types';

const N = 'visualisation.graphs.visionclaw.nodes.';
const E = 'visualisation.graphs.visionclaw.edges.';
const L = 'visualisation.graphs.visionclaw.labels.';
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
  { key: 'agentNodeSize', subgroup: 'Agent Nodes', label: 'Node Size', type: 'slider', min: 0.1, max: 1, step: 0.05, path: `${N}nodeSize`, description: 'Global size gain for agent nodes (per-node magnitude also scales with workload)' },
  { key: 'agentOpacity', subgroup: 'Agent Nodes', label: 'Node Opacity', type: 'slider', min: 0, max: 1, step: 0.05, path: `${N}opacity`, description: 'Agent node material opacity' },
  { key: 'agentMetalness', subgroup: 'Agent Nodes', label: 'Metalness', type: 'slider', min: 0, max: 1, step: 0.05, path: `${N}metalness`, description: 'PBR metalness of agent node material' },
  { key: 'agentRoughness', subgroup: 'Agent Nodes', label: 'Roughness', type: 'slider', min: 0, max: 1, step: 0.05, path: `${N}roughness`, description: 'PBR roughness of agent node material' },
  // Agent Edges — visionclaw edge appearance (Rust GraphSettings.edges + client-only colorByType)
  { key: 'agentEdgeColor', subgroup: 'Agent Edges', label: 'Edge Color', type: 'color', path: `${E}color`, description: 'Base colour for agent connection lines' },
  { key: 'agentEdgeOpacity', subgroup: 'Agent Edges', label: 'Edge Opacity', type: 'slider', min: 0, max: 1, step: 0.05, path: `${E}opacity`, description: 'Agent connection line opacity' },
  { key: 'agentEdgeWidth', subgroup: 'Agent Edges', label: 'Edge Thickness', type: 'slider', min: 0.02, max: 0.5, step: 0.01, path: `${E}baseWidth`, description: 'Base width of agent connection lines' },
  { key: 'agentEdgeArrows', subgroup: 'Agent Edges', label: 'Show Arrows', type: 'toggle', path: `${E}enableArrows`, description: 'Draw directional arrowheads on agent edges' },
  { key: 'agentEdgeColorByType', subgroup: 'Agent Edges', label: 'Colour edges by type', type: 'toggle', path: `${E}colorByType`, description: 'Colour each agent edge by its relationship type instead of the single base colour' },
  // Agent Labels — visionclaw label text (Rust GraphSettings.labels)
  { key: 'agentEnableLabels', subgroup: 'Agent Labels', label: 'Show Labels', type: 'toggle', path: `${L}enableLabels`, description: 'Display agent type/status labels' },
  { key: 'agentLabelSize', subgroup: 'Agent Labels', label: 'Label Size', type: 'slider', min: 0.05, max: 3.0, step: 0.05, path: `${L}desktopFontSize`, description: 'Font size for agent labels' },
  { key: 'agentLabelColor', subgroup: 'Agent Labels', label: 'Label Color', type: 'color', path: `${L}textColor`, description: 'Colour of agent label text' },
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
  // Health — four configurable health→glow stops (client-typed graphTypeVisuals.agent.healthColors;
  // consumed by agentVisualConstants.healthGlowColor). Defaults preserve the historical six-tier ramp.
  { key: 'agentHealthExcellent', subgroup: 'Health', label: 'Excellent (≥95%)', type: 'color', path: `${A}healthColors.excellent`, description: 'Glow colour for agents at ≥95% health' },
  { key: 'agentHealthGood', subgroup: 'Health', label: 'Good (≥80%)', type: 'color', path: `${A}healthColors.good`, description: 'Glow colour for agents at ≥80% health' },
  { key: 'agentHealthWarning', subgroup: 'Health', label: 'Warning (≥50%)', type: 'color', path: `${A}healthColors.warning`, description: 'Glow colour for agents at ≥50% health' },
  { key: 'agentHealthCritical', subgroup: 'Health', label: 'Critical (<25%)', type: 'color', path: `${A}healthColors.critical`, description: 'Glow colour for agents below 25% health' },
  // Behaviour — per-type agent visuals (client-typed graphTypeVisuals.agent + swarmTint)
  { key: 'agentSwarmTint', subgroup: 'Behaviour', label: 'Swarm hue tint', type: 'toggle', path: `${A}swarmTint`, description: 'Hue-rotate each agent by a stable per-swarm offset so swarms read as related-but-distinct families' },
  { key: 'agentShowTrails', subgroup: 'Behaviour', label: 'Motion Trails', type: 'toggle', path: `${A}showTrails`, description: 'Draw a fading motion-trail ribbon behind each moving agent so the swarm reads as in motion' },
  { key: 'agentTrailLength', subgroup: 'Behaviour', label: 'Trail Length', type: 'slider', min: 8, max: 48, step: 1, path: `${A}trailLength`, description: 'Number of sampled positions retained in each agent trail (longer = more history)' },
  { key: 'agentBioluminescence', subgroup: 'Behaviour', label: 'Bioluminescent Intensity', type: 'slider', min: 0, max: 3, step: 0.05, path: `${A}bioluminescentIntensity`, description: 'Membrane bioluminescence of agent capsules' },
  { key: 'agentNucleusGlow', subgroup: 'Behaviour', label: 'Nucleus Glow', type: 'slider', min: 0, max: 2, step: 0.05, path: `${A}nucleusGlowIntensity`, description: 'Base emissive of the agent capsule core' },
  { key: 'agentBreathingSpeed', subgroup: 'Behaviour', label: 'Breathing Speed', type: 'slider', min: 0, max: 3, step: 0.05, path: `${A}breathingSpeed`, description: 'Rate of the idle/active breathing pulse' },
  { key: 'agentBreathingAmplitude', subgroup: 'Behaviour', label: 'Breathing Amplitude', type: 'slider', min: 0, max: 1, step: 0.05, path: `${A}breathingAmplitude`, description: 'Depth of the breathing pulse' },
  { key: 'agentMembraneOpacity', subgroup: 'Behaviour', label: 'Membrane Opacity', type: 'slider', min: 0, max: 1, step: 0.05, path: `${A}membraneOpacity`, description: 'Opacity of the outer bioluminescent membrane' },
  { key: 'agentShowHealthBar', subgroup: 'Behaviour', label: 'Show Health Bar', type: 'toggle', path: `${A}showHealthBar`, description: 'Draw the per-agent health bar beneath each node' },
  { key: 'agentBeamRadius', subgroup: 'Behaviour', label: 'Action Beam Radius', type: 'slider', min: 0.05, max: 1.5, step: 0.05, path: `${A}beamRadius`, description: 'Cylinder radius of embodied agent-action beams (0x23)' },
  { key: 'agentBeamOpacity', subgroup: 'Behaviour', label: 'Action Beam Opacity', type: 'slider', min: 0, max: 1, step: 0.05, path: `${A}beamOpacity`, description: 'Peak opacity of agent-action beams during their hold phase' },
  // Nameplate LOD — declutter dense swarms (client-typed graphTypeVisuals.agent; consumed by BotsNode).
  { key: 'agentNameplateLod', subgroup: 'Behaviour', label: 'Nameplate LOD', type: 'toggle', path: `${A}nameplateLod`, description: 'Declutter dense swarms: show the full agent nameplate only when the camera is close (or the agent is hovered/selected/queen), collapse to a name-only line mid-range, and hide it far away' },
  { key: 'agentNameplateFullDistance', subgroup: 'Behaviour', label: 'Nameplate Full Distance', type: 'slider', min: 10, max: 120, step: 5, path: `${A}nameplateFullDistance`, description: 'Camera distance (world units) under which an agent shows its full 3-line nameplate; the name-only band extends to ~2.25× this before hiding' },
  // Attention heat — agent 0x23 actions heat knowledge/ontology nodes (client-typed
  // graphTypeVisuals.knowledgeGraph.*; consumed by GemNodes → metadata texture w).
  // Sibling to the beam controls: same agent-action source, projected onto the KG.
  { key: 'kgAttentionHeat', subgroup: 'Behaviour', label: 'File-Attention Heat', type: 'toggle', path: `${K}attentionHeatEnabled`, description: 'Knowledge/ontology nodes heat up (glow) as agents touch them via 0x23 actions, then cool as the heat decays' },
  { key: 'kgAttentionHeatHalfLife', subgroup: 'Behaviour', label: 'Attention Heat Half-Life', type: 'slider', min: 5, max: 60, step: 1, path: `${K}attentionHeatHalfLife`, description: 'Seconds for a node\'s attention heat to fade by half' },
  // Demo mode — synthetic agent injection for visual verification (no backend path).
  { key: 'agentDemoToggle', subgroup: 'Demo', label: 'Toggle Agent Demo', type: 'action-button', action: 'toggle-demo', description: 'Inject/remove 6 synthetic agents that cycle through work→idle to exercise sprite, nudge and fade features' },
];

export const agents: GroupData = {
  id: 'agents',
  label: 'Agents',
  description: 'Agent/swarm node, edge, label appearance and per-type behaviour visuals.',
  hotkey: '9',
  loadPaths: [
    'visualisation.graphs.visionclaw.nodes',
    'visualisation.graphs.visionclaw.edges',
    'visualisation.graphs.visionclaw.labels',
    'visualisation.graphTypeVisuals',
    'visualisation.rendering',
  ],
  fields,
};
