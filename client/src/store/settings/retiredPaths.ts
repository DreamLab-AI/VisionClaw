/**
 * Setting paths deleted by ADR-2189 because nothing reads them
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
 * The 3 dead paths a newer control replaces (ADR-2189 §6 "Replace"):
 * size-by is `visualisation.graphView.explore.sizeBy` (ADR-2184); edge colour
 * by type and width by weight are always on today (useEdgeBufferComputation.ts)
 * and become options of ADR-2181's edge encodings.
 */
export const REPLACED_PATHS: readonly string[] = [
  `${K}nodes.sizeScheme`,
  `${K}edges.colorByType`,
  `${K}edges.widthByWeight`,
];

/** Every path ADR-2189 removed from the menus and from persisted settings. */
export const ADR_2189_REMOVED_PATHS: readonly string[] = [...DEAD_PATHS, ...REPLACED_PATHS];

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
