/**
 * Setting paths ADR-2189 deleted: dead (nothing reads them), replaced by a newer
 * control, or merged into another control
 * (docs/audits/menus-2026-11-06.md §4, Appendix B: each one has no reader
 * outside the registry, the defaults and the type declarations).
 *
 * Pure data, imported by four places so they cannot disagree:
 *  - the registry test, which proves none of them is still a control;
 *  - the settings-store rehydrate `merge`, which drops them from a persisted
 *    `graph-viz-settings-v2` blob written by an older client;
 *  - the server-load merge in the settings store, which drops them from a
 *    `/visual` (or other bucket) response that still carries them;
 *  - the old-blob migration test.
 */

const K = 'visualisation.graphs.knowledge.';
const V = 'visualisation.graphs.visionclaw.';
const A = 'visualisation.graphTypeVisuals.agent.';

/** The 37 dead paths with no replacement. */
export const DEAD_PATHS: readonly string[] = [
  `${K}nodes.perNodeGlow`,
  `${K}nodes.enableMetadataShape`,
  'visualisation.graphTypeVisuals.ontology.ringTintByClass',
  `${K}labels.textOutlineColor`,
  `${K}labels.textOutlineWidth`,
  'qualityGates.maxNodeCount',
  'qualityGates.gnnPhysics',
  'perplexity.model',
  'perplexity.maxTokens',
  'perplexity.temperature',
  'pocketTts.apiUrl',
  'xr.enabled',
  'xr.quality',
  'xr.renderScale',
  'xr.enableHandTracking',
  'xr.enableHaptics',
  `${V}nodes.nodeSize`,
  `${V}nodes.opacity`,
  `${V}nodes.metalness`,
  `${V}nodes.roughness`,
  `${V}edges.color`,
  `${V}edges.opacity`,
  `${V}edges.baseWidth`,
  `${V}edges.enableArrows`,
  `${V}edges.colorByType`,
  `${V}labels.enableLabels`,
  `${V}labels.desktopFontSize`,
  `${V}labels.textColor`,
  `${A}healthColors.excellent`,
  `${A}healthColors.good`,
  `${A}healthColors.warning`,
  `${A}healthColors.critical`,
  `${A}swarmTint`,
  `${A}membraneOpacity`,
  `${A}showHealthBar`,
  'decisions.showDecisionChains',
  'decisions.highlightPrecedents',
];

/**
 * The dead paths a newer control replaces (ADR-2189 §6 "Replace"):
 * size-by is `visualisation.graphView.explore.sizeBy` (ADR-2184); edge colour
 * by type and width by weight are always on today (useEdgeBufferComputation.ts)
 * and become options of ADR-2181's edge encodings.
 */
export const REPLACED_PATHS: readonly string[] = [
  `${K}nodes.sizeScheme`,
  // replaced by visualisation.graphView.labels.fontPx when the render lane moved
  // labels to the ADR-2186 budget (InstancedLabels sizes in CSS px)
  `${K}labels.desktopFontSize`,
  `${K}edges.colorByType`,
  `${K}edges.widthByWeight`,
];

/**
 * Paths merged into another control (ADR-2189 §6 "Merge", closed 2026-11-06):
 *  - the generic knowledge edge colour was only visible outside knowledge-graph
 *    and ontology modes, which each have their own colour; it becomes the agent
 *    mode's colour, so every graph mode has exactly one (RENAMED_PATHS moves a
 *    stored value across);
 *  - Show Clusters / Show Communities coloured nothing (GemNodes colours by
 *    cluster or community only through the colour scheme); colour scheme =
 *    Cluster / Community is the one control.
 */
export const MERGED_PATHS: readonly string[] = [
  `${K}edges.color`,
  'qualityGates.showClusters',
  'qualityGates.showCommunities',
];

/**
 * Old path → new path. stripRemovedPaths copies a stored value to the new path
 * (when the new path has no value yet) before deleting the old one, so a
 * customised colour survives the merge.
 */
export const RENAMED_PATHS: Readonly<Record<string, string>> = {
  [`${K}edges.color`]: `${A}edgeColor`,
};

/** Every path ADR-2189 removed from the menus and from persisted settings. */
export const ADR_2189_REMOVED_PATHS: readonly string[] = [...DEAD_PATHS, ...REPLACED_PATHS, ...MERGED_PATHS];

const isRecord = (v: unknown): v is Record<string, unknown> =>
  v !== null && typeof v === 'object' && !Array.isArray(v);

/**
 * Return a copy of a (partial) settings object with every removed path deleted.
 * Tolerant by design: missing branches, non-object values and unknown keys are
 * left alone, so any blob an older client wrote still loads. Branches emptied by
 * the removal (e.g. `xr`, `decisions`, `perplexity`) are dropped too.
 */
export function stripRemovedPaths<T>(settings: T, paths: readonly string[] = ADR_2189_REMOVED_PATHS): T {
  if (!isRecord(settings)) return settings;
  const root = structuredCloneSafe(settings);
  // Renames first: carry a stored value to its new home.
  for (const path of paths) {
    const to = RENAMED_PATHS[path];
    if (!to) continue;
    const value = readPath(root, path);
    if (value === undefined || readPath(root, to) !== undefined) continue;
    writePath(root, to, value);
  }
  for (const path of paths) {
    const keys = path.split('.');
    const trail: Array<[Record<string, unknown>, string]> = [];
    let cur: unknown = root;
    for (const key of keys.slice(0, -1)) {
      if (!isRecord(cur)) break;
      trail.push([cur, key]);
      cur = cur[key];
    }
    if (trail.length !== keys.length - 1 || !isRecord(cur)) continue;
    const leaf = keys[keys.length - 1];
    if (!(leaf in cur)) continue;
    delete cur[leaf];
    // prune parents left empty by the delete, innermost first
    for (let i = trail.length - 1; i >= 0; i--) {
      const [parent, key] = trail[i];
      const child = parent[key];
      if (isRecord(child) && Object.keys(child).length === 0) delete parent[key];
      else break;
    }
  }
  return root as T;
}

function readPath(root: Record<string, unknown>, path: string): unknown {
  let cur: unknown = root;
  for (const key of path.split('.')) {
    if (!isRecord(cur)) return undefined;
    cur = cur[key];
  }
  return cur;
}

function writePath(root: Record<string, unknown>, path: string, value: unknown): void {
  const keys = path.split('.');
  let cur: Record<string, unknown> = root;
  for (const key of keys.slice(0, -1)) {
    const next = cur[key];
    if (!isRecord(next)) {
      if (next !== undefined) return; // a non-object is in the way: leave it alone
      cur[key] = {};
    }
    cur = cur[key] as Record<string, unknown>;
  }
  cur[keys[keys.length - 1]] = value;
}

function structuredCloneSafe<T>(v: T): T {
  if (typeof structuredClone === 'function') {
    try {
      return structuredClone(v);
    } catch {
      /* non-cloneable leaf (a function): fall through to JSON */
    }
  }
  return JSON.parse(JSON.stringify(v)) as T;
}
