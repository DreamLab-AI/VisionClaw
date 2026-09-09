import { describe, it, expect } from 'vitest';
import { REGISTRY, ALL_FIELDS, ALL_PATHS, testIdFor } from '../settingsRegistry';
import { buildManifest, serverBucketFor } from '../manifest';
import { MACRO_PATHS } from '../macros';
import manifestJson from '../settings-manifest.json';
// The SOURCE OF TRUTH for the frozen backend contract. Captured from the legacy
// ControlPanel/unifiedSettingsConfig.ts (now deleted — WP5 cutover) before deletion.
// Every registry path must equal one of these, and vice versa — zero drift.
import legacyFixture from './legacy-paths.fixture.json';

const EXPECTED_GROUP_COUNTS: Record<string, number> = {
  motion: 55,
  look: 29,
  labels: 10,
  quality: 32,
  atmosphere: 22,
  xr: 5,
  ai: 6,
  system: 16,
  agents: 44,
  decisions: 2,
  provenance: 4,
};

/**
 * Total registry field count: the 168 frozen legacy fields + the Agents group +
 * the two W-G phase-1 client-only groups (decisions, provenance).
 */
/**
 * Backend physics paths introduced by the Wave-1 immersive-layout additions
 * (continuous Z compression already existed in the WP5 baseline; these three are
 * new): dual-disc layout, DAG radial rank-bias force strength and level spacing.
 * They post-date the frozen WP5 baseline, so the zero-drift test (c) excludes them
 * from the migrated comparison and (c4) asserts them independently. All route to
 * the physics bucket (serverBucketFor → 'physics').
 */
const WAVE1_LAYOUT_PATHS: string[] = [
  'visualisation.graphs.knowledge.physics.enableDualDiscLayout',
  'visualisation.graphs.knowledge.physics.dagBiasK',
  'visualisation.graphs.knowledge.physics.dagLevelDistance',
  // ADR-141 layout programme (P2/P4): stratified planes + Sugiyama layers.
  'visualisation.graphs.knowledge.physics.planeBiasK',
  'visualisation.graphs.knowledge.physics.planeSpacing',
  'visualisation.graphs.knowledge.physics.layerBiasK',
  'visualisation.graphs.knowledge.physics.layerSpacing',
];

const TOTAL_FIELDS =
  168 + EXPECTED_GROUP_COUNTS.agents + EXPECTED_GROUP_COUNTS.decisions + EXPECTED_GROUP_COUNTS.provenance
  + WAVE1_LAYOUT_PATHS.length;

/**
 * Frozen client-only paths introduced by the W-G phase-1 Decisions/Provenance
 * groups (hotkeys 10/11). Like the Agents group they post-date the WP5 baseline,
 * so the zero-drift test (c) excludes them from the migrated comparison and (c3)
 * asserts them independently. All are clientOnly (serverBucketFor → null): they
 * never enter the settings PUT routing, so they add no backend-contract drift.
 */
const WG_GROUP_PATHS: string[] = [
  'decisions.showDecisionChains',
  'decisions.highlightPrecedents',
  'provenance.showAttribution',
  'provenance.showGateChips',
  'provenance.enableTimeline',
  'provenance.diffMode',
];

/**
 * Frozen backend paths introduced by the Agents group (hotkey 9). They post-date
 * the WP5 legacy baseline, so the zero-drift test (c) compares only the migrated
 * groups against the fixture and asserts these separately in (c2). Every path
 * exists in the client typed settings tree; visionclaw.* also exist on Rust
 * GraphSettings and rendering.agentColors.* on the server AgentColorsDTO, while
 * graphTypeVisuals.agent.* are client-typed (localStorage).
 */
const AGENT_GROUP_PATHS: string[] = [
  'visualisation.graphs.visionclaw.nodes.baseColor',
  'visualisation.graphs.visionclaw.nodes.nodeSize',
  'visualisation.graphs.visionclaw.nodes.opacity',
  'visualisation.graphs.visionclaw.nodes.metalness',
  'visualisation.graphs.visionclaw.nodes.roughness',
  'visualisation.graphs.visionclaw.edges.color',
  'visualisation.graphs.visionclaw.edges.opacity',
  'visualisation.graphs.visionclaw.edges.baseWidth',
  'visualisation.graphs.visionclaw.edges.enableArrows',
  'visualisation.graphs.visionclaw.edges.colorByType',
  'visualisation.graphs.visionclaw.labels.enableLabels',
  'visualisation.graphs.visionclaw.labels.desktopFontSize',
  'visualisation.graphs.visionclaw.labels.textColor',
  'visualisation.rendering.agentColors.coordinator',
  'visualisation.rendering.agentColors.coder',
  'visualisation.rendering.agentColors.architect',
  'visualisation.rendering.agentColors.analyst',
  'visualisation.rendering.agentColors.tester',
  'visualisation.rendering.agentColors.researcher',
  'visualisation.rendering.agentColors.reviewer',
  'visualisation.rendering.agentColors.optimizer',
  'visualisation.rendering.agentColors.documenter',
  'visualisation.rendering.agentColors.queen',
  'visualisation.rendering.agentColors.default',
  'visualisation.graphTypeVisuals.agent.swarmTint',
  'visualisation.graphTypeVisuals.agent.showTrails',
  'visualisation.graphTypeVisuals.agent.trailLength',
  'visualisation.graphTypeVisuals.agent.bioluminescentIntensity',
  'visualisation.graphTypeVisuals.agent.nucleusGlowIntensity',
  'visualisation.graphTypeVisuals.agent.breathingSpeed',
  'visualisation.graphTypeVisuals.agent.breathingAmplitude',
  'visualisation.graphTypeVisuals.agent.membraneOpacity',
  'visualisation.graphTypeVisuals.agent.showHealthBar',
  'visualisation.graphTypeVisuals.agent.healthColors.excellent',
  'visualisation.graphTypeVisuals.agent.healthColors.good',
  'visualisation.graphTypeVisuals.agent.healthColors.warning',
  'visualisation.graphTypeVisuals.agent.healthColors.critical',
  'visualisation.graphTypeVisuals.agent.beamRadius',
  'visualisation.graphTypeVisuals.agent.beamOpacity',
  'visualisation.graphTypeVisuals.agent.nameplateLod',
  'visualisation.graphTypeVisuals.agent.nameplateFullDistance',
  // Attention-heat knobs (task V1): grouped under Agents > Behaviour with the
  // beam controls; they target graphTypeVisuals.knowledgeGraph.* and, like the
  // rest of this group, post-date the frozen WP5 baseline.
  'visualisation.graphTypeVisuals.knowledgeGraph.attentionHeatEnabled',
  'visualisation.graphTypeVisuals.knowledgeGraph.attentionHeatHalfLife',
];

/** The frozen `path` strings captured from the legacy unifiedSettingsConfig. */
function legacyPaths(): string[] {
  // ADR-2041 renamed the knowledge-graph settings key `logseq` → `knowledge`.
  // The fixture stays byte-frozen as the true WP5 historical record; the one
  // sanctioned rename is applied here so the zero-drift comparison still holds.
  // Remove this normalisation with the alias (ADR-2041 review_trigger).
  return legacyFixture.paths.map((p) =>
    p.startsWith('visualisation.graphs.logseq.')
      ? p.replace('visualisation.graphs.logseq.', 'visualisation.graphs.knowledge.')
      : p,
  );
}

/** Every field (path or localKey/action) declared in the legacy config. */
function legacyFieldCount(): number {
  return legacyFixture.fieldCount;
}

describe('control-center settings registry', () => {
  it('(a) enumerates exactly the legacy 168 fields plus the Agents group', () => {
    expect(ALL_FIELDS.length).toBe(TOTAL_FIELDS);
    // the frozen legacy fixture it extends is still 168 (sanity on the baseline)
    expect(legacyFieldCount()).toBe(168);
  });

  it('(b) has the exact per-group field counts', () => {
    const actual: Record<string, number> = {};
    for (const g of REGISTRY) actual[g.id] = g.fields.length;
    expect(actual).toEqual(EXPECTED_GROUP_COUNTS);
    // group order realises hotkeys 1..11 (decisions/provenance are the new W-G groups)
    expect(REGISTRY.map((g) => g.id)).toEqual([
      'motion', 'look', 'labels', 'quality', 'atmosphere', 'xr', 'ai', 'system', 'agents', 'decisions', 'provenance',
    ]);
    expect(REGISTRY.map((g) => g.hotkey)).toEqual(['1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11']);
  });

  it('(c) has ZERO path drift vs legacy unifiedSettingsConfig for the migrated groups', () => {
    const legacySet = new Set(legacyPaths());
    const agentSet = new Set(AGENT_GROUP_PATHS);
    const wgSet = new Set(WG_GROUP_PATHS);
    const wave1Set = new Set(WAVE1_LAYOUT_PATHS);
    // Compare only the pre-existing (migrated) groups against the frozen baseline;
    // the Agents group (c2), the W-G groups (c3) and the Wave-1 layout paths (c4)
    // post-date WP5 and are asserted independently.
    const migratedPaths = ALL_PATHS.filter((p) => !agentSet.has(p) && !wgSet.has(p) && !wave1Set.has(p));
    const migratedSet = new Set(migratedPaths);

    // No migrated path exists that is absent from the frozen legacy set.
    const addedByRegistry = [...migratedSet].filter((p) => !legacySet.has(p));
    // No frozen legacy path was dropped by the registry.
    const droppedFromLegacy = [...legacySet].filter((p) => !migratedSet.has(p));

    expect(addedByRegistry).toEqual([]);
    expect(droppedFromLegacy).toEqual([]);
    // identical size ⇒ identical sets given the two empty diffs above
    expect(migratedSet.size).toBe(legacySet.size);
    // no duplicate paths within the whole registry
    expect(ALL_PATHS.length).toBe(new Set(ALL_PATHS).size);
  });

  it('(c2) the Agents group exposes exactly its declared new paths, disjoint from legacy', () => {
    const legacySet = new Set(legacyPaths());
    const agentsGroup = REGISTRY.find((g) => g.id === 'agents')!;
    const agentsPaths = agentsGroup.fields.map((f) => f.path).filter((p): p is string => Boolean(p));
    // the group's paths are exactly the declared set (no accidental additions/drops)
    expect(new Set(agentsPaths)).toEqual(new Set(AGENT_GROUP_PATHS));
    // the demo toggle is the only action-button (no path); all other fields carry one
    const actionButtons = agentsGroup.fields.filter(f => f.type === 'action-button');
    expect(actionButtons.length).toBe(1);
    expect(agentsPaths.length).toBe(agentsGroup.fields.length - actionButtons.length);
    // and none of them collide with the frozen legacy baseline
    const collisions = agentsPaths.filter((p) => legacySet.has(p));
    expect(collisions).toEqual([]);
  });

  it('(c3) the W-G groups expose exactly their client-only paths, disjoint from legacy', () => {
    const legacySet = new Set(legacyPaths());
    const decisionsGroup = REGISTRY.find((g) => g.id === 'decisions')!;
    const provenanceGroup = REGISTRY.find((g) => g.id === 'provenance')!;
    const wgPaths = [...decisionsGroup.fields, ...provenanceGroup.fields]
      .map((f) => f.path)
      .filter((p): p is string => Boolean(p));
    // exactly the declared set (no accidental additions/drops)
    expect(new Set(wgPaths)).toEqual(new Set(WG_GROUP_PATHS));
    // every field carries a frozen path (no transient/action fields here)
    expect(wgPaths.length).toBe(decisionsGroup.fields.length + provenanceGroup.fields.length);
    // none collide with the frozen legacy baseline
    expect(wgPaths.filter((p) => legacySet.has(p))).toEqual([]);
    // and they are all clientOnly (serverBucketFor → null): no backend routing
    for (const p of wgPaths) expect(serverBucketFor(p)).toBeNull();
  });

  it('(c4) the Wave-1 layout paths are registered on physics, disjoint from legacy', () => {
    const legacySet = new Set(legacyPaths());
    const registrySet = new Set(ALL_PATHS);
    // every declared Wave-1 path is present exactly once in the registry
    for (const p of WAVE1_LAYOUT_PATHS) expect(registrySet.has(p)).toBe(true);
    // none collide with the frozen legacy baseline
    expect(WAVE1_LAYOUT_PATHS.filter((p) => legacySet.has(p))).toEqual([]);
    // and they all route to the physics bucket (not clientOnly)
    for (const p of WAVE1_LAYOUT_PATHS) expect(serverBucketFor(p)).toBe('physics');
  });

  it('(d) every testid is unique', () => {
    const ids = REGISTRY.flatMap((g) => g.fields.map((f) => testIdFor(f, g.id)));
    expect(ids.length).toBe(TOTAL_FIELDS);
    expect(new Set(ids).size).toBe(TOTAL_FIELDS);
  });

  it('(e) manifest count matches the registry count', () => {
    const fresh = buildManifest();
    expect(fresh.count).toBe(ALL_FIELDS.length);
    expect(fresh.settings.length).toBe(TOTAL_FIELDS);
    // the committed JSON is in sync with the live registry (CI freshness)
    expect(manifestJson.count).toBe(fresh.count);
    expect(manifestJson.settings.length).toBe(fresh.settings.length);
    expect(manifestJson.groups.map((g) => `${g.id}:${g.fieldCount}`))
      .toEqual(fresh.groups.map((g) => `${g.id}:${g.fieldCount}`));
  });

  it('(f) every macro writes only to registered frozen paths (no macro drift)', () => {
    const paths = new Set(ALL_PATHS);
    const offRegistry = MACRO_PATHS.filter((p) => !paths.has(p));
    expect(offRegistry).toEqual([]);
  });

  it('(g) transient fields carry a localKey or action, never a path', () => {
    for (const f of ALL_FIELDS) {
      const isTransient = f.type === 'action-button' || f.localKey !== undefined;
      if (f.localKey !== undefined) expect(f.path).toBeUndefined();
      if (f.type === 'action-button') expect(f.path).toBeUndefined();
      // every field is addressable: it has a path, a localKey, or is an action
      expect(Boolean(f.path) || Boolean(f.localKey) || isTransient).toBe(true);
    }
  });

  // Every slider's range must be an exact whole number of steps: (max - min) / step
  // must be integral, otherwise the max (and any default sitting on it) is off the
  // step grid and silently snaps on first touch/restore — the defect-3 class of bug.
  // Known pre-existing off-grid sliders are whitelisted by path (out of defect-3
  // scope; documented here so future drift on the FIXED fields is still caught).
  const OFF_GRID_WHITELIST = new Set<string>([
    'visualisation.graphs.knowledge.tweening.lerpBase',            // (0.15-0.0001)/0.001 = 149.9
    'visualisation.graphs.knowledge.physics.maxForce',            // (2000-1)/5     = 399.8
    'visualisation.graphs.knowledge.physics.constraintMaxForcePerNode', // (2000-1)/5 = 399.8
    'perplexity.maxTokens',                                    // (4096-100)/100 = 39.96
  ]);

  it('(h) every slider range is a whole number of steps (no step-grid drift)', () => {
    const offenders: string[] = [];
    for (const f of ALL_FIELDS) {
      if (f.type !== 'slider') continue;
      const id = f.path ?? f.localKey ?? f.key;
      expect(typeof f.min, `slider ${id} missing min`).toBe('number');
      expect(typeof f.max, `slider ${id} missing max`).toBe('number');
      expect(typeof f.step, `slider ${id} missing step`).toBe('number');
      if (OFF_GRID_WHITELIST.has(f.path ?? '')) continue;
      const steps = ((f.max as number) - (f.min as number)) / (f.step as number);
      if (Math.abs(Math.round(steps) - steps) > 1e-6) {
        offenders.push(`${id} → (max-min)/step = ${steps}`);
      }
    }
    expect(offenders).toEqual([]);
  });

  it('(i) the defect-3 sliders now land exactly on their step grid', () => {
    const maxNodeCount = ALL_FIELDS.find((f) => f.path === 'qualityGates.maxNodeCount')!;
    expect(((maxNodeCount.max! - maxNodeCount.min!) / maxNodeCount.step!)).toBe(100);

    const outline = ALL_FIELDS.find((f) => f.path === 'visualisation.graphs.knowledge.labels.textOutlineWidth')!;
    expect(outline.step).toBe(0.0001);
    // the 0.0074725277 stored default now snaps to 0.0075 (loss ~2.7e-5) not 0.007 (loss ~4.7e-4)
    const snapped = Math.round(0.0074725277 / outline.step!) * outline.step!;
    expect(Math.abs(snapped - 0.0074725277)).toBeLessThan(0.0001);
  });
});
