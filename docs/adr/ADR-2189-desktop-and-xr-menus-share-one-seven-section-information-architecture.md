---
id: ADR-2189
title: Desktop and XR menus share one seven-section information architecture, generated from the settings registry
date: 2026-11-06
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: c360202dbeacdf1f5909ce85cb86b1018327b509
verified_paths: []
relates: [ADR-2179, ADR-2180, ADR-2181, ADR-2182, ADR-2183, ADR-2184, ADR-2185, ADR-2186, ADR-2187, ADR-2188]
owner: jjohare
review_trigger: "The render and explore lanes land the ADR-2180..2186 settings; a new registry group is proposed; a headset session finds a control more than two levels deep."
repo: visionclaw
domain: XR-client
evidence_class: source
acceptance_boundary: "Proposal written from a code audit of client/src and xr-client/scripts at c360202db and a live probe of the desktop dev app at 1280x900 and 1024x768 (docs/audits/menus-2026-11-06.md). XR was inventoried from code only, with no headset. The per-user scope of the /visual, /rendering, /qualityGates and /nodeFilter buckets was not verified; only the physics bucket was confirmed global."
---

# ADR-2189 — Desktop and XR menus share one seven-section information architecture

## Context
- The desktop has 11 registry groups (268 fields). On top of those sit a dock with 5 macro dials, 11 group icons, 4 chips, a command palette, a keyword CommandInput, and a Memory explorer panel. The audit (`docs/audits/menus-2026-11-06.md`) found four structural problems:
  - 40 setting paths have no reader.
  - Up to four writers exist per setting.
  - Hotkeys 10 and 11 cannot fire.
  - The only Fit button is occluded.
- The grouping follows the legacy code tabs, not user tasks. "Filtering & Quality" mixes scoping, highlight filters, GPU gates and analytics.
- The XR HUD has 10 tabs. None of their names matches a desktop group, and only 4 XR preferences persist (`xr_prefs.gd`).
- ADR-2180..2186 add controls that have no home yet: fit scope, edge fade, LOD, emphasis bloom, colour-by/size-by/legend, crossfilter widgets and label budget. ADR-2188 §6 requires XR to expose "the same information architecture … within two levels".
- `physics.*` is one global document. Each write is pushed to the GPU and to every session (`src/settings/api/settings_routes.rs:602-606`), so the Density and Motion dials change the layout for every viewer.
- There is already a shared-JSON precedent: `data/semantic_fields.json` drives both `groups/semanticForces.ts` and the XR Forces tab (`graph_parity.gd`).

## Decision

### 1. One vocabulary: seven sections, in this order, on both clients

| # | Section | Answers | Hotkey (desktop) |
|---|---|---|---|
| 1 | **Layers** | What is in the scene? | `1` |
| 2 | **Explore** | Which part am I looking at? | `2`; `/` focuses search |
| 3 | **Encode** | What do colour, size, edges and labels mean? | `3` |
| 4 | **Layout** | Where do things sit? | `4` |
| 5 | **Look** | How does it render? | `5` |
| 6 | **Agents & Governance** | Who is acting, and what was decided? | `6` |
| 7 | **System** | How is the app running? | `7` |

**1. Layers** (what is in the scene)
- Populations (Knowledge, Ontology and Agents scoping)
- Memory cloud
- Cluster hulls
- Splat scene (XR)
- Codescape link

**2. Explore** (which part I am looking at)
- Search
- ADR-2185 widgets (timeline, histograms, category bars)
- Filters
- Grouping runs
- Memory query
- Query builder and Pins (XR)
- Ontology browser

**3. Encode** (what colour, size, edges and labels mean)
- Colour by
- Size by
- Legend
- Nodes, Edges, Labels (with the label budget)
- Emphasis colour
- Analytic overlays

**4. Layout** (where things sit)
- Mode
- Forces
- Semantic forces
- Shape (hierarchy, planes, radial, dual disc)
- Simulation
- Reset layout

**5. Look** (how it renders)
- Nodes (overview sprite size, gem LOD threshold, gem material)
- Edges (overview line, tube LOD)
- Lighting
- Emphasis glow
- Ambience (particles, wisps, fog)
- Animation
- Motion smoothing

**6. Agents & Governance** (who is acting and what was decided)
- Agent operations
- Agent appearance and colours
- Governance (case queue, attribution, gate chips, provenance timeline)
- KPIs

**7. System** (how the app is running)
- Performance (quality gates, edge memory budget, label cadence, renderer fallback)
- Connection
- Account & sync (auth, Sync toggle, Solid pod)
- Services (TTS voice and speed)
- Headset
- Developer (debug logging, the ADR-2187 route flag)
- Help & keys

Rules for the seven sections:
- Every control lives in exactly one section and one subsection: two levels, matching ADR-2188 §6.
- XR's ten tabs collapse onto the same seven:

| XR tab today | Section |
|---|---|
| Graph | split by control: Spread to Layout, Colour to Encode, Layers to Layers |
| Layout | Layout |
| Shapes | Layout |
| Forces | Layout |
| Query | Explore |
| Pins | Explore |
| Key | Encode › Legend |
| Swarm | Agents & Governance |
| Session | System |
| Help | System |

### 2. Registry-driven mechanism
- `GroupData` gains `section` (one of the seven ids above). The rail and dock render sections, and the old group ids become subsections.
- `buildManifest` (run by `client/scripts/emit-settings-manifest.ts`) emits a new `ia.json` (section → subsection → field keys and labels). It writes two committed copies: one beside `settings-manifest.json`, and one at `xr-client/data/ia.json`. `hud.gd` builds its tab bar from the XR copy.
  - The precedent is `semantic_fields.json`. Its two copies (`client/src/features/control-center/registry/semanticFields.json` and `xr-client/data/semantic_fields.json`, which are byte-identical today) are pinned to their Rust source by a test at `crates/visionclaw-domain/src/types/semantic_config.rs:659-660`.
  - `ia.json`'s source is the TypeScript registry, so its pin is a Vitest test instead. The test regenerates the document and asserts that both committed copies equal it. That way the two clients cannot drift apart silently.
- Each field gets a derived **scope** from `serverBucketFor`: `shared` (physics), `server` (other buckets) or `browser` (null). `SettingRow` shows it as a badge.
  - `shared` controls carry the text "Changes the layout for everyone".
  - The dock's Layout dials carry the same marker.
  - The XR HUD shows the same badge.
- Hotkeys are generated from section order (1–7), so every advertised hotkey can be typed. `?` opens a generated shortcut sheet that lists every binding and wires the dead `controlcenter:help` and `show-keyboard-shortcuts` events.

### 3. Dock (desktop) and wand HUD header (XR)
- **Left: Layout dials.** These are the shared dials.
  - **Spread** replaces Density. The semantics stay the same, but the name now matches the direction.
  - **Motion** stays.
  - Both carry the shared marker.
- **Middle: view dials.** These are browser-scoped.
  - **Labels** replaces Focus. It is retargeted to the ADR-2186 overview budget, 0→120 labels.
  - **Glow** replaces Luminosity. It is retargeted to ambient light plus ADR-2182 emphasis-glow intensity and no longer touches global bloom.
  - **Atmosphere** stays. It is now the only control with that name; the fog toggle becomes "Fog".
- **Then:** the Physics switch, **Reset layout** (with confirmation, and a toast for the result), **Fit** (split button with ADR-2180 scope: Focus body / Both graphs / Whole scene), **Legend** (toggle), and **Explore** (opens the ADR-2185 panel).
- **Then:** 7 section icons, Ask, and an overflow menu holding Codescape.
- Dials show `aria-valuetext` naming what each one changes (for example "Spread 60% — repulsion 256").
- At widths below 1180 px the dock collapses the dials into a single "Dials" popover. It never positions off-screen.
- XR gets Fit, Legend and Explore as header buttons on the HUD, plus the same five dials as ± steppers on Layout and Look.

### 4. Chips and overlays
- All fixed surfaces use one z-scale and one corner layout:
  - Bottom-left: Agents, then Governance.
  - Bottom-right: Status, then KPI. Fit moves into the dock.
  - Bottom-centre: the dock only. The layout-mode label moves into the Layout dial tooltip and the Status flyout.
  - Top-right: Explore, Memory explorer, and NodeDetailPanel as tabs of one right-hand drawer.
- Warnings (voice and similar) go to a mounted toast region at the top-centre, never over controls.

### 5. Filters follow ADR-2183
- **Explore › Filters** (`nodeFilter.*`) and all ADR-2185 widgets write the highlight mask by default.
- A single "Hide filtered nodes" switch keeps today's server-side removal for users who want it.
- **Layers › Populations** (`nodeTypeVisibility.*`, XR `set_type_visible`) stays as scoping, which is the ADR-2179 P3 exception for population scoping.

### 6. Delete, merge and rename

Controls are deleted, merged or renamed as follows.

**Delete (37 paths, no reader)**
- `nodes.perNodeGlow`, `nodes.enableMetadataShape`, `ontology.ringTintByClass`
- `labels.textOutlineColor` and `textOutlineWidth`
- `qualityGates.maxNodeCount` and `gnnPhysics`
- `perplexity.*` (3) and `pocketTts.apiUrl`. The server reads its own config for these.
- The whole Immersion (XR) group, `xr.*` (5). Headset quality moves to System › Headset as read-only status from the XR client.
- `visualisation.graphs.visionclaw.{nodes.nodeSize,opacity,metalness,roughness; edges.*; labels.*}` (12)
- `agent.healthColors.*` (4), `agent.swarmTint`, `membraneOpacity`, `showHealthBar`
- The Decisions group (2)

**Replace (3)**
- `nodes.sizeScheme` is replaced by ADR-2184 Size by.
- `edges.widthByWeight` is replaced by ADR-2181 Edges › Width by.
- `edges.colorByType` becomes the "Relationship type" option of Edges › Colour by.

**Merge**
- The three edge colours (`edges.color`, `knowledgeGraph.edgeColor`, `ontology.edgeColor`) become one "Edge colour", stored per graph mode. Today `edges.color` is shadowed in both modes (`GraphManager.tsx:664-670`).
- `qualityGates.showClusters` and `showCommunities` become Colour by = cluster / community.
- The dock Physics switch and `physics.enabled` are one control in two places.
- The scrubber's Diff checkbox and `provenance.diffMode` are one control.
- The Memory explorer panel controls stay where they are but go through `useSettingField`. Their registry fields move to Explore › Memory, so the panel and the registry share one source.
- CommandInput keyword rules stop writing paths directly. Each rule becomes a `macro` or registry call, so magic numbers live in one place.

**Rename**

| Old name | New name |
|---|---|
| Density | Spread |
| Focus | Labels |
| Luminosity | Glow |
| Atmosphere toggle | Fog |
| Bloom Glow | Emphasis glow |
| Max Edges Ceiling | Edge memory budget |
| Node Color | Fallback node colour |

Enum options get display labels, for example "Graph distance (SSSP)" and "File size". Technical force names keep their label but gain a plain subtitle, for example "FA2 Scaling Ratio — how far clusters push apart".

### 7. Where each existing subgroup goes

This table is the subgroup-level map. The per-path table, all 268 rows, is Appendix A of the audit and is generated by the script named in Verification.

| Old group › subgroup | New home |
|---|---|
| Motion › Core Forces, Repulsion & Spacing, Constraints | Layout › Forces |
| Motion › Simulation, Bounds | Layout › Simulation |
| Motion › Layout Forces | Layout › Shape |
| Motion › Layout | Layout › Mode |
| Motion › Semantic: * (8 subgroups) | Layout › Semantic forces |
| Motion › Smooth Movement | Look › Motion smoothing |
| Look › Nodes | Encode › Nodes (colour-by, size-by) and Look › Nodes (size gain) |
| Look › Edges, Graph-Type Visuals | Encode › Edges |
| Look › Lighting & Rendering | Look › Lighting. `maxEdgesCeiling` and `softwareFallback` go to System › Performance. |
| Look › Selection | Encode › Emphasis |
| Look › Bloom / Glow | Look › Emphasis glow |
| Look › Gem Material | Look › Nodes (close-up gems) |
| Labels › Labels | Encode › Labels. `labelLayoutEvery` goes to System › Performance. |
| Filtering › Node Types | Layers › Populations |
| Filtering › Node Filtering | Explore › Filters |
| Filtering › Run Grouping | Explore › Grouping |
| Filtering › GPU & Quality Gates | System › Performance |
| Filtering › Cluster Visualisation | Encode › Colour by and Analytic overlays |
| Filtering › Cluster Hulls | Layers › Cluster hulls |
| Effects › Scene Particles, Energy Wisps, Atmosphere/Fog | Look › Ambience |
| Effects › Animation | Look › Animation |
| Effects › Embedding Cloud | Layers › Memory cloud |
| Effects › Memory Explorer | Explore › Memory |
| Immersion (XR) | deleted; System › Headset (status) |
| Intelligence (AI) | System › Services (voice and speed only) |
| System › Renderer | System › Performance |
| System › Authentication | System › Account & sync |
| System › Network | System › Connection |
| System › Debug Logging | System › Developer |
| Agents › * | Agents & Governance › Agent appearance, Agent colours, Agent operations |
| Decisions | deleted |
| Provenance › * | Agents & Governance › Governance |
| Panels Solid Pod / Ontology | System › Account & sync / Explore › Ontology |

### 8. Where the ADR-2179..2188 controls go

| Control | Home |
|---|---|
| Fit scope (2180) | Dock Fit split button; persisted per client under System › Account & sync |
| Edge fade px thresholds, minimum alpha, width by (2181) | Encode › Edges |
| Dropped-edge notice (2181) | In-canvas notice. Budget under System › Performance. |
| Overview node pixel size min/max, gem LOD threshold, gem budget (2182) | Look › Nodes |
| Bloom scope (2182) | Look › Emphasis glow |
| Grey-out strength, highlight colours (2183) | Encode › Emphasis |
| Colour by, Size by, palette, legend (2184) | Encode, plus the Legend dock toggle |
| Timeline, histograms, category bars, search (2185) | Explore |
| Label budget tiers 40/40/100, priority column (2186) | Encode › Labels, plus the Labels dial for the overview tier |
| cosmos.gl `/overview` flag (2187) | System › Developer |
| XR overview size, edge count, label budget (2188) | The same sections on the HUD |

## Consequences
- **Discoverability.** A user can say where any control lives from its section name. XR and desktop share one map, so documentation and help are written once.
- **Registry migration.** The dead groups and fields go away (37 deletions, 8 merges). The registry tests that pin group count, hotkeys and `fieldCount` (`registry/__tests__`) and the e2e test ids built from group ids must be updated in the same change. Test ids derived from paths do not change.
- **Persisted state.** Deleted paths stay in users' persisted `graph-viz-settings-v2` and in server stores. Add a one-way migration in the settings-store `merge` to drop them, and remove them from `defaults.ts`, `settings.ts` and the Rust config structs in a follow-on change. Removing the Rust fields affects the wire, so it is recorded in `docs/PROTOCOL-registry.md`.
- **Rebuilt surfaces.**
  - CommandInput must be rewritten as a thin layer over macros and the registry.
  - The dock needs a responsive layout.
  - The chips need one shared corner and z-index scheme.
- **XR work.** `hud.gd` must read `ia.json` and persist its view preferences (colour mode, layout mode, Reduce motion, Low-cost visuals, HUD position) to `xr_prefs.cfg`.
- **ADR-2181 premise.** ADR-2181 §2 says edge colour "keeps today's `colorByType`". Today that toggle does nothing: type colouring and weight-based width are both applied unconditionally (`useEdgeBufferComputation.ts:130-135`). The "Colour by" and "Width by" selects defined here make that behaviour visible and adjustable.
- **Governing doc.** No governing document is amended by this proposal. On acceptance, the IA and the scope badges are added to `docs/XR-client.md` and to the desktop governing document in the same change, as AGENTS.md requires.
- **Retargeted dials.** Labels and Glow change meaning. Users with saved dial positions will see a different result after upgrading. This is accepted: the old targets are superseded by ADR-2182 and ADR-2186.

## Verification
- Not implemented. This record's evidence is the audit at `c360202db`.
  - Dead-control scan: `node dead.js <worktree> readers.json`, with the script in Appendix B of the audit and per-path greps listed in §4.
  - Occlusion: `elementsFromPoint` probes in the live app.
  - Global physics scope: `settings_routes.rs:602-606`.
- Mapping table: generated from `settings-manifest.json` by a subgroup → section map with per-path overrides. It has 268 rows and 0 unmapped (K 213, D 37, M 8, R 6, W 4).
- **Acceptance checks for the implementation:**
  - Every manifest field has a `section`.
  - A Vitest parity test shows both committed `ia.json` copies equal the regenerated document. A GUT test shows the HUD tab list equals `ia.json`'s sections.
  - Each hotkey in an aria-label fires its handler (unit test).
  - At 1024x768 and 1280x900, `elementsFromPoint` at the centre of every dock, chip and Fit control returns that control.
  - The dead-scan script reports 0 paths with no reader.
