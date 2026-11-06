/**
 * The seven-section information architecture (ADR-2189), shared by desktop and XR.
 *
 * Pure data: no React, no lucide-react, so the ts-node manifest emitter can import it.
 * The group files under ./groups stay the place a field is DECLARED; this module says
 * where each field is SHOWN. Every field lands in exactly one section and one
 * subsection, two levels deep, which is the ADR-2188 §6 limit for the headset.
 *
 * Placement is by `sourceGroup|subgroup` (SUBGROUP_PLACEMENT), with per-path
 * overrides (PATH_PLACEMENT) for fields whose old subgroup mixed concerns.
 * `placeField` throws on an unmapped field, so a new field cannot silently vanish
 * from the menus; the registry test enumerates every field through it.
 */
import type { RegistryField } from './types';

export type SectionId = 'layers' | 'explore' | 'encode' | 'layout' | 'look' | 'agents' | 'system';

export interface SectionDef {
  id: SectionId;
  label: string;
  /** the question the section answers (ADR-2189 §1) */
  description: string;
  hotkey: string;
  /** subsection display order; a field's subsection must be listed here */
  subsections: string[];
}

export const SECTION_DEFS: SectionDef[] = [
  {
    id: 'layers',
    label: 'Layers',
    description: 'What is in the scene: graph populations, the memory cloud and cluster hulls.',
    hotkey: '1',
    subsections: ['Populations', 'Memory cloud', 'Cluster hulls'],
  },
  {
    id: 'explore',
    label: 'Explore',
    description: 'Which part you are looking at: framing, filters, grouping, memory queries and the ontology browser.',
    hotkey: '2',
    subsections: ['Camera', 'Filters', 'Grouping', 'Memory', 'Ontology'],
  },
  {
    id: 'encode',
    label: 'Encode',
    description: 'What colour, size, edges and labels mean.',
    hotkey: '3',
    subsections: ['Colour & size', 'Edges', 'Labels', 'Emphasis', 'Analytic overlays'],
  },
  {
    id: 'layout',
    label: 'Layout',
    description: 'Where things sit. Layout settings are one shared document: changing them moves the graph for every viewer and the headset.',
    hotkey: '4',
    subsections: [
      'Mode',
      'Forces',
      'Forces › Advanced',
      'Forces › Communities & constraints',
      'Shape',
      'Semantic forces › DAG Layering',
      'Semantic forces › Type Clustering',
      'Semantic forces › Collision',
      'Semantic forces › Weighted Springs',
      'Semantic forces › Physicality Clustering',
      'Semantic forces › Role Clustering',
      'Semantic forces › Maturity Layering',
      'Semantic forces › Cross-Domain Links',
      'Simulation',
      'Simulation › Bounds',
    ],
  },
  {
    id: 'look',
    label: 'Look',
    description: 'How it renders: node and edge drawing, lighting, glow, ambience and animation.',
    hotkey: '5',
    subsections: [
      'Nodes',
      'Close-up nodes (gems)',
      'Edges',
      'Lighting',
      'Emphasis glow',
      'Ambience',
      'Animation',
      'Motion smoothing',
    ],
  },
  {
    id: 'agents',
    label: 'Agents & Governance',
    description: 'Who is acting and what was decided: agent operations and appearance, attribution and the provenance timeline.',
    hotkey: '6',
    subsections: ['Agent operations', 'Agent appearance', 'Agent colours', 'Governance', 'Governance › Timeline'],
  },
  {
    id: 'system',
    label: 'System',
    description: 'How the app is running: performance, connection, account and sync, voice, and developer logging.',
    hotkey: '7',
    subsections: ['Performance', 'Connection', 'Account & sync', 'Services', 'Developer'],
  },
];

export const SECTION_BY_ID: Record<SectionId, SectionDef> = Object.fromEntries(
  SECTION_DEFS.map((s) => [s.id, s]),
) as Record<SectionId, SectionDef>;

type Placement = [SectionId, string];

/** `sourceGroup|subgroup` → [section, subsection]. */
export const SUBGROUP_PLACEMENT: Record<string, Placement> = {
  'motion|Core Forces': ['layout', 'Forces'],
  'motion|Repulsion & Spacing': ['layout', 'Forces › Advanced'],
  'motion|Constraints': ['layout', 'Forces › Communities & constraints'],
  'motion|Simulation': ['layout', 'Simulation'],
  'motion|Bounds': ['layout', 'Simulation › Bounds'],
  'motion|Layout Forces': ['layout', 'Shape'],
  'motion|Layout': ['layout', 'Mode'],
  'motion|Smooth Movement': ['look', 'Motion smoothing'],

  'look|Nodes': ['encode', 'Colour & size'],
  'look|Edges': ['encode', 'Edges'],
  'look|Graph-Type Visuals': ['encode', 'Edges'],
  'look|Lighting & Rendering': ['look', 'Lighting'],
  'look|Selection': ['encode', 'Emphasis'],
  'look|Bloom / Glow': ['look', 'Emphasis glow'],
  'look|Gem Material': ['look', 'Close-up nodes (gems)'],

  'labels|Labels': ['encode', 'Labels'],

  'quality|Node Types': ['layers', 'Populations'],
  'quality|Node Filtering': ['explore', 'Filters'],
  'quality|Run Grouping': ['explore', 'Grouping'],
  'quality|GPU & Quality Gates': ['system', 'Performance'],
  'quality|Cluster Visualisation': ['encode', 'Analytic overlays'],
  'quality|Cluster Hulls': ['layers', 'Cluster hulls'],

  'atmosphere|Scene Particles': ['look', 'Ambience'],
  'atmosphere|Energy Wisps': ['look', 'Ambience'],
  'atmosphere|Atmosphere / Fog': ['look', 'Ambience'],
  'atmosphere|Animation': ['look', 'Animation'],
  'atmosphere|Embedding Cloud': ['layers', 'Memory cloud'],
  'atmosphere|Memory Explorer': ['explore', 'Memory'],

  'ai|PocketTts TTS': ['system', 'Services'],

  'system|Renderer': ['system', 'Performance'],
  'system|Authentication': ['system', 'Account & sync'],
  'system|Network': ['system', 'Connection'],
  'system|Debug Logging': ['system', 'Developer'],

  'agents|Demo': ['agents', 'Agent operations'],
  'agents|Agent Nodes': ['agents', 'Agent appearance'],
  'agents|Agent Edges': ['agents', 'Agent appearance'],
  'agents|Agent Labels': ['agents', 'Agent appearance'],
  'agents|Behaviour': ['agents', 'Agent appearance'],
  'agents|Agent Type Colours': ['agents', 'Agent colours'],
  'agents|Health': ['agents', 'Agent colours'],

  'provenance|Attribution': ['agents', 'Governance'],
  'provenance|Governance Gates': ['agents', 'Governance'],
  'provenance|Timeline': ['agents', 'Governance › Timeline'],

  'graphView|Camera': ['explore', 'Camera'],
  'graphView|Overview Edges': ['look', 'Edges'],
  'graphView|Overview Nodes': ['look', 'Nodes'],
  'graphView|Explore': ['encode', 'Colour & size'],
  'graphView|Labels': ['encode', 'Labels'],
};

const K = 'visualisation.graphs.knowledge.';

/** Per-path overrides where the old subgroup mixed concerns (ADR-2189 §7). */
export const PATH_PLACEMENT: Record<string, Placement> = {
  // Look › Nodes held both encoding and rendering: node size is a render gain.
  [`${K}nodes.nodeSize`]: ['look', 'Nodes'],
  // Lighting & Rendering held two performance budgets.
  'visualisation.rendering.maxEdgesCeiling': ['system', 'Performance'],
  'visualisation.rendering.softwareFallback': ['system', 'Performance'],
  // Label layout cadence is a performance budget (ADR-2186 §5).
  'visualisation.rendering.labelLayoutEvery': ['system', 'Performance'],
  // ADR-2182: the glow scope sits with the glow controls.
  'visualisation.graphView.nodes.bloomScope': ['look', 'Emphasis glow'],
  // ADR-2183: grey-out strength is emphasis, not encoding.
  'visualisation.graphView.explore.greyoutOpacity': ['encode', 'Emphasis'],
  'visualisation.graphView.explore.greyoutMix': ['encode', 'Emphasis'],
};

/** Bespoke panels (not registry fields) and the section/subsection that hosts their launcher. */
export const PANEL_PLACEMENT: Record<string, Placement> = {
  solid: ['system', 'Account & sync'],
  ontology: ['explore', 'Ontology'],
};

/** Where a field is shown. Throws for an unmapped field so nothing drops out of the menus. */
export function placeField(sourceGroup: string, field: RegistryField): { section: SectionId; subsection: string } {
  const byPath = field.path ? PATH_PLACEMENT[field.path] : undefined;
  let placement: Placement | undefined = byPath;
  if (!placement && sourceGroup === 'motion' && field.subgroup?.startsWith('Semantic: ')) {
    placement = ['layout', `Semantic forces › ${field.subgroup.slice('Semantic: '.length)}`];
  }
  placement ??= SUBGROUP_PLACEMENT[`${sourceGroup}|${field.subgroup ?? ''}`];
  if (!placement) {
    throw new Error(`placement: no section for ${sourceGroup}|${field.subgroup ?? ''} (${field.path ?? field.key})`);
  }
  const [section, subsection] = placement;
  if (!SECTION_BY_ID[section].subsections.includes(subsection)) {
    throw new Error(`placement: subsection "${subsection}" is not declared on section ${section}`);
  }
  return { section, subsection };
}
