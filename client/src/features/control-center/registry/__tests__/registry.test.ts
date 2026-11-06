import { describe, it, expect } from 'vitest';
import { REGISTRY, ALL_FIELDS, ALL_PATHS, SOURCE_GROUPS, testIdFor } from '../settingsRegistry';
import { buildManifest, buildIa, serverBucketFor, scopeFor, scopeForField, PANELS } from '../manifest';
import { SECTION_DEFS, placeField } from '../placement';
import { ADR_2189_REMOVED_PATHS, DEAD_PATHS, REPLACED_PATHS, MERGED_PATHS, RENAMED_PATHS } from '../../../../store/settings/retiredPaths';
import { MACRO_PATHS, MACROS } from '../macros';
import manifestJson from '../settings-manifest.json';
// The SOURCE OF TRUTH for the frozen backend contract. Captured from the legacy
// ControlPanel/unifiedSettingsConfig.ts (now deleted — WP5 cutover) before deletion.
// Every registry path must equal one of these, and vice versa — zero drift.
import legacyFixture from './legacy-paths.fixture.json';
import semanticDoc from '../semanticFields.json';

// vite-node here cannot resolve static `fs`/`path` imports; take them from the runtime
// (same pattern as features/graph/__tests__/singleDrawnSource.test.ts).
const getBuiltin = (process as unknown as { getBuiltinModule(id: string): unknown }).getBuiltinModule;
const { readFileSync } = getBuiltin('fs') as typeof import('fs');
const { resolve } = getBuiltin('path') as typeof import('path');

/** Fields per SOURCE group (the files that declare them), after the ADR-2189 deletions. */
const EXPECTED_GROUP_COUNTS: Record<string, number> = {
  motion: 84,
  look: 23,
  labels: 7,
  quality: 28, // RuVector Integration removed (dead, settings audit 2026-11-06)
  atmosphere: 32,
  ai: 2,
  system: 17, // Auth Enabled / Auth Required removed (dead)
  agents: 26,
  provenance: 4,
  graphView: 21,
};

/** Fields per SECTION (ADR-2189): what the rail, the dock and hotkeys 1..7 open. */
const EXPECTED_SECTION_COUNTS: Record<string, number> = {
  layers: 14,
  explore: 28,
  encode: 19, // edge width, opacity and the three mode colours moved to Look › Edges
  layout: 80,
  look: 49,
  agents: 30,
  system: 24,
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

/**
 * Memory explorer paths (live RuVector memory cloud + query trajectory). They
 * post-date the WP5 baseline, extend the Atmosphere group's Embedding Cloud
 * settings, and route to the opaque `visual` bucket like the rest of
 * `visualisation.embeddingCloud.*`. Asserted independently in (c5).
 */
const MEMORY_EXPLORER_PATHS: string[] = [
  'visualisation.embeddingCloud.colorBy',
  'visualisation.embeddingCloud.trajectoryView',
  'visualisation.embeddingCloud.routeGlow',
  'visualisation.embeddingCloud.playbackSpeed',
  'visualisation.embeddingCloud.showRejected',
  'visualisation.embeddingCloud.dimOffRoute',
  'visualisation.embeddingCloud.learningEnabled',
  'visualisation.embeddingCloud.learningTargetRecall',
  'visualisation.embeddingCloud.learningRate',
  'visualisation.embeddingCloud.cinematic',
];

/**
 * Server network paths that post-date the WP5 baseline: the position-broadcast
 * stream settings (`streamChunkNodes` / `streamMoveThreshold` / `streamAckTimeoutMs`). They are PhysicsSettings fields, so they route to the
 * physics bucket, but it is presented under System > Network. Asserted in (c6).
 */
const NETWORK_PATHS: string[] = [
  'visualisation.graphs.knowledge.physics.streamChunkNodes',
  'visualisation.graphs.knowledge.physics.streamMoveThreshold',
  'visualisation.graphs.knowledge.physics.streamAckTimeoutMs',
];

/** Client tweening paths that post-date the baseline: the chase time replaced `lerpBase`. */
const TWEENING_PATHS: string[] = ['visualisation.graphs.knowledge.tweening.responseMs'];

/** The layout mode moved from `qualityGates.layoutMode` into the stored physics document
 *  (ADR-2140 item 1); the old path is retired below and this one post-dates the baseline. */
const LAYOUT_MODE_PATHS: string[] = ['visualisation.graphs.knowledge.physics.layoutMode'];

/**
 * Frozen legacy paths deliberately retired from the registry. Graph Separation
 * (`graphSeparationX`) went with ADR-2135's 2026-10-08 amendment: the three
 * bodies are always apart at a fixed separation, so it is no longer a setting.
 */
/** The semantic-force controls: the physics `semantic` group, defined once in Rust and emitted as
 *  semanticFields.json. They post-date the WP5 baseline, so (c) excludes them and (c6) asserts them. */
const SEMANTIC_PATHS: string[] = (semanticDoc as { fields: { path: string }[] }).fields.map(
  (f) => `visualisation.graphs.knowledge.physics.semantic.${f.path}`,
);

/**
 * The graph-view overview contract (ADR-2179..2186). Post-dates the WP5 baseline;
 * every path routes to the opaque `visual` bucket. Asserted in (c7).
 */
const GRAPH_VIEW_PATHS: string[] = [
  'fitTarget',
  'overviewEdges.enabled', 'overviewEdges.widthPx', 'overviewEdges.maxWidthPx', 'overviewEdges.fadeNearPx',
  'overviewEdges.fadeFarPx', 'overviewEdges.minOpacity', 'overviewEdges.glassAtCloseZoom',
  'nodes.spriteSizePx', 'nodes.gemLodDistance', 'nodes.bloomScope',
  'explore.colorBy', 'explore.sizeBy', 'explore.showLegend', 'explore.hideFiltered', 'explore.greyoutOpacity', 'explore.greyoutMix',
  'labels.budget', 'labels.dynamicBudget', 'labels.selectedBudget', 'labels.fontPx',
].map((p) => `visualisation.graphView.${p}`);

/**
 * Node-filter paths that post-date the baseline: the opt-in server-side removal
 * (ADR-2183 implementation notes, 2026-11-06). Routes to the nodeFilter bucket.
 * Asserted in (c8).
 */
const NODE_FILTER_PATHS: string[] = ['nodeFilter.serverRemove'];

const RETIRED_PATHS: string[] = [
  'visualisation.graphs.knowledge.physics.graphSeparationX',
  'visualisation.graphs.knowledge.tweening.lerpBase',   // replaced by tweening.responseMs (a time constant, lower = faster)
  // Moved into the physics `semantic` group (one stored definition, applied unscaled by the
  // GPU); the quality-gate copies and their REST propagation were removed (ADR-2140).
  'qualityGates.layoutMode',            // now physics.layoutMode (ADR-2140 item 1)
  'qualityGates.semanticForces',
  'qualityGates.dagLevelAttraction',
  'qualityGates.dagSiblingRepulsion',
  'qualityGates.typeClusterAttraction',
  'qualityGates.typeClusterRadius',
];

/** ADR-2189 deletions from the frozen legacy baseline (the Agents/Decisions deletions are post-baseline). */
const LEGACY_REMOVED = ADR_2189_REMOVED_PATHS.filter((p) =>
  (legacyFixture as { paths: string[] }).paths
    .map((q) => q.replace('visualisation.graphs.logseq.', 'visualisation.graphs.knowledge.'))
    .includes(p),
);

/**
 * Paths a merge added (ADR-2189 §6 "Merge"): the generic knowledge edge colour
 * became agent mode's own edge colour, so every graph mode has exactly one.
 */
const MERGE_ADDED_PATHS: string[] = ['visualisation.graphTypeVisuals.agent.edgeColor'];

const TOTAL_FIELDS =
  168 - RETIRED_PATHS.length - LEGACY_REMOVED.length + EXPECTED_GROUP_COUNTS.agents + EXPECTED_GROUP_COUNTS.provenance
  + WAVE1_LAYOUT_PATHS.length + MEMORY_EXPLORER_PATHS.length + NETWORK_PATHS.length + SEMANTIC_PATHS.length + TWEENING_PATHS.length
  + LAYOUT_MODE_PATHS.length + GRAPH_VIEW_PATHS.length + MERGE_ADDED_PATHS.length + NODE_FILTER_PATHS.length;

/**
 * Frozen client-only paths introduced by the W-G phase-1 Decisions/Provenance
 * groups (hotkeys 10/11). Like the Agents group they post-date the WP5 baseline,
 * so the zero-drift test (c) excludes them from the migrated comparison and (c3)
 * asserts them independently. All are clientOnly (serverBucketFor → null): they
 * never enter the settings PUT routing, so they add no backend-contract drift.
 */
const WG_GROUP_PATHS: string[] = [
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
  'visualisation.graphTypeVisuals.agent.showTrails',
  'visualisation.graphTypeVisuals.agent.trailLength',
  'visualisation.graphTypeVisuals.agent.bioluminescentIntensity',
  'visualisation.graphTypeVisuals.agent.nucleusGlowIntensity',
  'visualisation.graphTypeVisuals.agent.breathingSpeed',
  'visualisation.graphTypeVisuals.agent.breathingAmplitude',
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
  // ADR-2041 renamed the knowledge-graph settings key `logseq` → `knowledge`
  // (server alias retired by ADR-2115). The fixture stays byte-frozen as the
  // true WP5 historical record; the one sanctioned rename is applied here so
  // the zero-drift comparison still holds.
  return legacyFixture.paths.map((p) =>
    p.startsWith('visualisation.graphs.logseq.')
      ? p.replace('visualisation.graphs.logseq.', 'visualisation.graphs.knowledge.')
      : p,
  ).filter((p) => !RETIRED_PATHS.includes(p) && !ADR_2189_REMOVED_PATHS.includes(p));
}

/** Every field (path or localKey/action) declared in the legacy config. */
function legacyFieldCount(): number {
  return legacyFixture.fieldCount;
}

describe('control-center settings registry', () => {
  it('(a) enumerates exactly the legacy 168 fields (less retirements) plus the post-baseline groups', () => {
    expect(ALL_FIELDS.length).toBe(TOTAL_FIELDS);
    // the frozen legacy fixture it extends is still 168 (sanity on the baseline)
    expect(legacyFieldCount()).toBe(168);
  });

  it('(b) has the exact per-source-group and per-section field counts', () => {
    const bySource: Record<string, number> = {};
    for (const g of SOURCE_GROUPS) bySource[g.id] = g.fields.length;
    expect(bySource).toEqual(EXPECTED_GROUP_COUNTS);
    const bySection: Record<string, number> = {};
    for (const g of REGISTRY) bySection[g.id] = g.fields.length;
    expect(bySection).toEqual(EXPECTED_SECTION_COUNTS);
    // ADR-2189 §1: seven sections in this order, hotkeys 1..7 (every advertised key is typeable)
    expect(REGISTRY.map((g) => g.id)).toEqual(['layers', 'explore', 'encode', 'layout', 'look', 'agents', 'system']);
    expect(REGISTRY.map((g) => g.label)).toEqual([
      'Layers', 'Explore', 'Encode', 'Layout', 'Look', 'Agents & Governance', 'System',
    ]);
    expect(REGISTRY.map((g) => g.hotkey)).toEqual(['1', '2', '3', '4', '5', '6', '7']);
  });

  it('(b2) every field sits in exactly one declared section and subsection (two levels, ADR-2188 §6)', () => {
    const seen = new Map<string, string>();
    for (const sec of REGISTRY) {
      const def = SECTION_DEFS.find((d) => d.id === sec.id)!;
      for (const f of sec.fields) {
        const id = testIdFor(f, sec.id);
        expect(seen.has(id), `${id} placed twice`).toBe(false);
        seen.set(id, sec.id);
        expect(def.subsections, id).toContain(f.subgroup);
        expect(f.sourceGroup, id).toBeDefined();
        // the subsection label carries at most one '›' (section › subsection is the second level)
        expect((f.subgroup ?? '').split('›').length, id).toBeLessThanOrEqual(2);
      }
    }
    // placeField agrees with the assembled sections for every declared field
    for (const g of SOURCE_GROUPS) {
      for (const f of g.fields) {
        const { section } = placeField(g.id, f);
        expect(seen.get(testIdFor({ ...f, sourceGroup: g.id }, g.id)), `${g.id}.${f.key}`).toBe(section);
      }
    }
    expect(seen.size).toBe(TOTAL_FIELDS);
    // every bespoke panel has a launcher in a declared subsection
    for (const p of PANELS) expect(SECTION_DEFS.find((d) => d.id === p.section)!.subsections).toContain(p.subsection);
  });

  it('(b3) ADR-2189 deleted every dead path: none is still a control', () => {
    // 37 from ADR-2189 + 4 from the 2026-11-06 settings audit (node opacity, RuVector, Auth x2)
    expect(DEAD_PATHS).toHaveLength(41);
    // sizeScheme, edges.colorByType, edges.widthByWeight, and labels.desktopFontSize
    // (replaced by graphView.labels.fontPx when the render lane landed ADR-2186)
    // + the five legacy visualisation.bloom.* paths (replaced by visualisation.glow.*, settings audit 2026-11-06)
    expect(REPLACED_PATHS).toHaveLength(9);
    // generic knowledge edges.color (→ agent mode's edge colour), Show Clusters,
    // Show Communities (→ colour scheme = Cluster / Community)
    expect(MERGED_PATHS).toHaveLength(3);
    expect(new Set(ADR_2189_REMOVED_PATHS).size).toBe(53);
    for (const [from, to] of Object.entries(RENAMED_PATHS)) {
      expect(ADR_2189_REMOVED_PATHS).toContain(from);
      expect(ALL_PATHS, to).toContain(to);
    }
    for (const p of ADR_2189_REMOVED_PATHS) expect(ALL_PATHS, p).not.toContain(p);
    // the empty groups went with them
    expect(SOURCE_GROUPS.map((g) => g.id)).not.toContain('xr');
    expect(SOURCE_GROUPS.map((g) => g.id)).not.toContain('decisions');
  });

  it('(b4) every field has a storage scope, and only physics paths are shared', () => {
    for (const f of ALL_FIELDS) {
      const scope = scopeForField(f);
      if (f.path && scope === 'shared') expect(serverBucketFor(f.path)).toBe('physics');
      if (!f.path && f.scope === undefined) expect(scope).toBe('browser');
    }
    // the whole Layout section is the shared physics document, including Reset layout
    const layout = REGISTRY.find((g) => g.id === 'layout')!;
    expect(layout.fields.filter((f) => scopeForField(f) !== 'shared').map((f) => f.key)).toEqual([]);
  });

  it('(b4b) each dial reports the strongest scope of the paths it writes (dock and rows agree)', () => {
    const rank = { browser: 0, server: 1, shared: 2 } as const;
    for (const m of MACROS) {
      const scopes = [0, 0.5, 1].flatMap((t) => m.apply(t).map((w) => scopeFor(w.path)));
      const strongest = scopes.reduce((a, b) => (rank[b] > rank[a] ? b : a), 'browser' as keyof typeof rank);
      expect(m.scope, m.id).toBe(strongest);
    }
    // Spread and Motion move the shared layout; the view dials save server documents every viewer loads
    expect(MACROS.filter((m) => m.scope === 'shared').map((m) => m.id)).toEqual(['density', 'motion']);
    expect(MACROS.filter((m) => m.scope === 'server').map((m) => m.id)).toEqual(['luminosity', 'focus', 'atmosphere']);
  });

  it('(b5) ia.json: both committed copies equal a fresh buildIa() (desktop/XR parity, ADR-2189 §2)', () => {
    const fresh = JSON.parse(JSON.stringify(buildIa()));
    const copies = [
      resolve(__dirname, '../ia.json'),
      resolve(__dirname, '../../../../../../xr-client/data/ia.json'),
    ];
    for (const path of copies) {
      const committed = JSON.parse(readFileSync(path, 'utf8'));
      expect(committed, `${path} is stale: run npm run gen:manifest`).toEqual(fresh);
    }
    expect(fresh.sections.map((s: { id: string }) => s.id)).toEqual(REGISTRY.map((g) => g.id));
    const iaFieldCount = fresh.sections.reduce(
      (n: number, s: { subsections: { fields: unknown[] }[] }) => n + s.subsections.reduce((m, ss) => m + ss.fields.length, 0),
      0,
    );
    expect(iaFieldCount).toBe(TOTAL_FIELDS);
  });

  it('(c) has ZERO path drift vs legacy unifiedSettingsConfig for the migrated groups', () => {
    const legacySet = new Set(legacyPaths());
    const agentSet = new Set(AGENT_GROUP_PATHS);
    const wgSet = new Set(WG_GROUP_PATHS);
    const wave1Set = new Set(WAVE1_LAYOUT_PATHS);
    const memorySet = new Set(MEMORY_EXPLORER_PATHS);
    const networkSet = new Set(NETWORK_PATHS);
    const semanticSet = new Set(SEMANTIC_PATHS);
    const tweeningSet = new Set(TWEENING_PATHS);
    const layoutModeSet = new Set(LAYOUT_MODE_PATHS);
    const graphViewSet = new Set(GRAPH_VIEW_PATHS);
    // Compare only the pre-existing (migrated) groups against the frozen baseline;
    // the Agents group (c2), the W-G groups (c3) and the Wave-1 layout paths (c4)
    // post-date WP5 and are asserted independently.
    const migratedPaths = ALL_PATHS.filter(
      (p) =>
        !agentSet.has(p) && !wgSet.has(p) && !wave1Set.has(p) && !memorySet.has(p) && !networkSet.has(p) && !semanticSet.has(p) && !tweeningSet.has(p) && !layoutModeSet.has(p) && !graphViewSet.has(p) && !MERGE_ADDED_PATHS.includes(p) && !NODE_FILTER_PATHS.includes(p),
    );
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

  it('(c8) the server-removal opt-in is a nodeFilter path, disjoint from legacy', () => {
    const legacySet = new Set(legacyPaths());
    for (const p of NODE_FILTER_PATHS) {
      expect(ALL_PATHS).toContain(p);
      expect(legacySet.has(p)).toBe(false);
      expect(serverBucketFor(p)).toBe('nodeFilter');
    }
  });

  it('(c1) the layout mode is the stored physics field, not a quality gate (ADR-2140)', () => {
    expect(ALL_PATHS).toContain('visualisation.graphs.knowledge.physics.layoutMode');
    expect(ALL_PATHS).not.toContain('qualityGates.layoutMode');
    const field = REGISTRY.flatMap((g) => g.fields).find((f) => f.key === 'layoutMode')!;
    expect(field.options).toEqual([
      'forceDirected', 'hierarchical', 'radial', 'dagLeftRight', 'typeClustering', 'spectral', 'clustered',
    ]);
  });

  it('(c2) the Agents group exposes exactly its declared new paths, disjoint from legacy', () => {
    const legacySet = new Set(legacyPaths());
    const agentsGroup = SOURCE_GROUPS.find((g) => g.id === 'agents')!;
    const agentsPaths = agentsGroup.fields.map((f) => f.path).filter((p): p is string => Boolean(p));
    // the group's paths are exactly the declared set (no accidental additions/drops)
    expect(new Set(agentsPaths)).toEqual(new Set(AGENT_GROUP_PATHS));
    // the only path-less fields are the two per-viewer action buttons (demo toggle, agent
    // offset toggle); all other fields carry a path
    const actionButtons = agentsGroup.fields.filter(f => f.type === 'action-button');
    expect(actionButtons.map(f => f.action).sort()).toEqual(['toggle-agent-offsets', 'toggle-demo']);
    expect(agentsPaths.length).toBe(agentsGroup.fields.length - actionButtons.length);
    // and none of them collide with the frozen legacy baseline
    const collisions = agentsPaths.filter((p) => legacySet.has(p));
    expect(collisions).toEqual([]);
  });

  it('(c3) the W-G groups expose exactly their client-only paths, disjoint from legacy', () => {
    const legacySet = new Set(legacyPaths());
    const provenanceGroup = SOURCE_GROUPS.find((g) => g.id === 'provenance')!;
    const wgPaths = [...provenanceGroup.fields]
      .map((f) => f.path)
      .filter((p): p is string => Boolean(p));
    // exactly the declared set (no accidental additions/drops)
    expect(new Set(wgPaths)).toEqual(new Set(WG_GROUP_PATHS));
    // every field carries a frozen path (no transient/action fields here)
    expect(wgPaths.length).toBe(provenanceGroup.fields.length);
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

  it('(c6) the semantic-force controls are the shared JSON document, on the physics bucket', () => {
    const motion = SOURCE_GROUPS.find((g) => g.id === 'motion')!;
    const byPath = new Map(motion.fields.map((f) => [f.path, f]));
    const rows = (semanticDoc as { fields: { path: string; kind: string; label: string; min?: number; max?: number; step?: number }[] }).fields;
    expect(rows).toHaveLength(35);
    for (const row of rows) {
      const f = byPath.get(`visualisation.graphs.knowledge.physics.semantic.${row.path}`);
      expect(f, row.path).toBeDefined();
      expect(f!.type).toBe(row.kind);
      expect(f!.label).toBe(row.label);
      if (row.kind === 'slider') {
        expect([f!.min, f!.max, f!.step]).toEqual([row.min, row.max, row.step]);
      }
      expect(serverBucketFor(f!.path)).toBe('physics');
    }
    // eight forces, each with exactly one enable toggle
    expect(rows.filter((r) => r.kind === 'toggle')).toHaveLength(8);
    // and none of the retired quality-gate copies is still registered
    for (const old of ['semanticForces', 'dagLevelAttraction', 'dagSiblingRepulsion', 'typeClusterAttraction', 'typeClusterRadius']) {
      expect(ALL_PATHS).not.toContain(`qualityGates.${old}`);
    }
  });

  it('(c5) the memory explorer paths sit in Explore › Memory / Layers › Memory cloud, disjoint from legacy, on the visual bucket', () => {
    const legacySet = new Set(legacyPaths());
    const sections = REGISTRY.filter((g) => g.id === 'explore' || g.id === 'layers');
    const atmospherePaths = new Set(sections.flatMap((g) => g.fields.filter((f) => f.subgroup === 'Memory' || f.subgroup === 'Memory cloud')).map((f) => f.path));
    for (const p of MEMORY_EXPLORER_PATHS) expect(atmospherePaths.has(p)).toBe(true);
    expect(MEMORY_EXPLORER_PATHS.filter((p) => legacySet.has(p))).toEqual([]);
    for (const p of MEMORY_EXPLORER_PATHS) expect(serverBucketFor(p)).toBe('visual');
  });

  it('(c7) the graph-view paths are exactly the Graph View group, disjoint from legacy, on the visual bucket', async () => {
    const legacySet = new Set(legacyPaths());
    const group = SOURCE_GROUPS.find((g) => g.id === 'graphView')!;
    const paths = group.fields.map((f) => f.path).filter((p): p is string => Boolean(p));
    expect(new Set(paths)).toEqual(new Set(GRAPH_VIEW_PATHS));
    expect(paths.length).toBe(group.fields.length);
    expect(GRAPH_VIEW_PATHS.filter((p) => legacySet.has(p))).toEqual([]);
    for (const p of GRAPH_VIEW_PATHS) expect(serverBucketFor(p)).toBe('visual');
    // every slider default sits on its step grid, so first touch never snaps it
    const { DEFAULT_GRAPH_VIEW } = await import('../../../../api/settings/defaults');
    for (const f of group.fields) {
      const key = f.path!.replace('visualisation.graphView.', '');
      const value = key.split('.').reduce<unknown>((o, k) => (o as Record<string, unknown>)[k], DEFAULT_GRAPH_VIEW);
      expect(value, key).toBeDefined();
      if (f.type === 'slider') {
        const steps = ((value as number) - f.min!) / f.step!;
        expect(Math.abs(Math.round(steps) - steps), key).toBeLessThan(1e-6);
        expect(value as number).toBeGreaterThanOrEqual(f.min!);
        expect(value as number).toBeLessThanOrEqual(f.max!);
      }
      if (f.type === 'select') expect(f.options, key).toContain(value);
    }
  });

  it('(c6b) the stream settings sit in System › Connection on the physics bucket with the validated ranges', () => {
    const legacySet = new Set(legacyPaths());
    const systemGroup = REGISTRY.find((g) => g.id === 'system')!;
    const byPath = (p: string) => systemGroup.fields.find((f) => f.path === p)!;
    const [chunk, thr, timeout] = NETWORK_PATHS.map(byPath);
    for (const f of [chunk, thr, timeout]) {
      expect(f).toBeDefined();
      expect(f.subgroup).toBe('Connection');
      expect(f.type).toBe('slider');
    }
    // the same ranges the server validates (physics_bounds::STREAM_*)
    expect([chunk.min, chunk.max]).toEqual([100, 20000]);
    expect([thr.min, thr.max]).toEqual([0, 100]);
    expect([timeout.min, timeout.max]).toEqual([20, 10000]);
    expect(NETWORK_PATHS.filter((p) => legacySet.has(p))).toEqual([]);
    for (const p of NETWORK_PATHS) expect(serverBucketFor(p)).toBe('physics');
    // the group loads the physics subtree, so the sliders show the live values
    expect(systemGroup.loadPaths).toContain('visualisation.graphs.knowledge.physics');
    // the retired rate settings are gone
    expect(systemGroup.fields.some((f) => /broadcastFps|interactionBroadcastFps/.test(f.path))).toBe(false);
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
    'visualisation.graphs.knowledge.physics.maxForce',            // (2000-1)/5     = 399.8
    'visualisation.graphs.knowledge.physics.constraintMaxForcePerNode', // (2000-1)/5 = 399.8
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

  it('(i) the defect-3 sliders went with ADR-2189: nothing read them', () => {
    // maxNodeCount was logged by the server and never enforced; the label outline was never drawn.
    expect(ALL_PATHS).not.toContain('qualityGates.maxNodeCount');
    expect(ALL_PATHS).not.toContain('visualisation.graphs.knowledge.labels.textOutlineWidth');
  });

  it('(j) there is no separation control: the three bodies are always apart (ADR-2135, 2026-10-08)', () => {
    for (const p of RETIRED_PATHS) expect(ALL_PATHS).not.toContain(p);
    expect(ALL_FIELDS.some((f) => /graph separation|separate knowledge/i.test(f.label))).toBe(false);
    // the frozen baseline still records the retired path, so the retirement is deliberate
    expect(legacyFixture.paths).toContain('visualisation.graphs.logseq.physics.graphSeparationX');
    const dual = ALL_FIELDS.find((f) => f.path === 'visualisation.graphs.knowledge.physics.enableDualDiscLayout')!;
    expect(dual.description).toMatch(/centre/i);
    expect(dual.description).toMatch(/always/i);
  });
});

describe('settings audit 2026-11-06: opacity, misfiled controls and dead controls', () => {
  const ia = buildIa();
  const where = (path: string) => {
    for (const s of ia.sections) for (const sub of s.subsections) {
      const f = sub.fields.find((x) => x.path === path);
      if (f) return { section: s.id, subsection: sub.label, label: f.label, index: sub.fields.indexOf(f) };
    }
    return null;
  };
  const EDGE_OPACITY = 'visualisation.graphs.knowledge.edges.opacity';
  const LONG_EDGE = 'visualisation.graphView.overviewEdges.minOpacity';

  it('Edge Opacity sits next to the long-edge fade floor in Look › Edges', () => {
    const a = where(EDGE_OPACITY)!;
    const b = where(LONG_EDGE)!;
    expect([a.section, a.subsection]).toEqual(['look', 'Edges']);
    expect([b.section, b.subsection]).toEqual(['look', 'Edges']);
    expect(Math.abs(a.index - b.index)).toBe(1);
  });

  it('the two labels say which is which', () => {
    expect(where(EDGE_OPACITY)!.label).toMatch(/^Edge Opacity/);
    expect(where(LONG_EDGE)!.label).not.toMatch(/Opacity/);
    expect(where(LONG_EDGE)!.label).toMatch(/fade/i);
  });

  it('edge width and colour are Look concerns too (Encode keeps what edges mean)', () => {
    for (const p of [
      'visualisation.graphs.knowledge.edges.baseWidth',
      'visualisation.graphTypeVisuals.knowledgeGraph.edgeColor',
      'visualisation.graphTypeVisuals.ontology.edgeColor',
      'visualisation.graphTypeVisuals.agent.edgeColor',
    ]) expect(where(p)?.section, p).toBe('look');
  });

  it('dead controls are gone: RuVector Integration, Auth Enabled, Auth Required', () => {
    for (const p of ['qualityGates.ruvectorEnabled', 'auth.enabled', 'auth.required']) {
      expect(ALL_PATHS, p).not.toContain(p);
      expect(DEAD_PATHS, p).toContain(p);
    }
    expect(DEAD_PATHS).toContain('visualisation.graphs.knowledge.nodes.opacity');
  });
});
