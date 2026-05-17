# Aspirational UI Element Inventory

> Companion to `comprehensive-settings-table.md`. Same entities, rationalised.
> **Scope:** entity-level decisions only. No grouping, layout, visual-design, or
> interaction-pattern choices — those happen later once the language is set.

## Decision codes

- **KEEP** — entity stays as exposed today (may receive a tier change).
- **PROMOTE** — surface to lower friction (advanced/PU → standard, or back-end → user-facing).
- **DEMOTE** — push to a deeper tier (basic → advanced, or expose-via-config-only).
- **EXPOSE** — currently server-only or env-only; surface in the unified UI.
- **HIDE** — keep server-side, remove from UI.
- **CUT** — remove from both UI and server (no value, no consumer).
- **MERGE → X** — fold into entity X.
- **RENAME → X** — same effect, clearer key.
- **WIRE** — UI exists but transport broken; complete the wiring.
- **VALIDATE** — keep, add validation.
- **AUTH-GATE** — keep, add server enforcement of access tier.
- **TBD** — keep until product intent is clear; flag for review.

**Target tier (aspiration):**
- 1 = visible by default
- 2 = revealed by an "advanced" disclosure
- 3 = power-user (server-enforced)
- 4 = operator/admin (deploy-time only — not in user UI)

## Guiding principles applied

1. **One source of truth per concept.** If the same effect is reachable via two paths, keep one.
2. **No client-only fork of a server-truthful concept.** If multi-user collaboration is in scope, things like tweening, XR toggles, "show metadata" must round-trip.
3. **Tier reflects competence required to use it safely**, not how interesting the developer found it.
4. **Don't expose internal numerical constants** (epsilons, cell sizes, batch sizes) unless the user can interpret them. Promote *outcomes* (FPS targets, "tighter clusters") instead.
5. **Operator/admin knobs are not user UI.** They live in env or `agentbox.toml`. Document them, don't surface them.
6. **Cut anything dead.** OIDC, Vircadia, legacy session keys, hardcoded policy rules, hologram subfields, drawer demo routes.

---

## 1 Authentication & Identity

| # | Path / Key | Decision | Tier | Justification |
|---|---|---|---|---|
| 1.1 | `auth.enabled` | **CUT** | – | Cosmetic flag; gates don't trip without an auth state anyway. Replace with reading `useNostrAuth().authenticated` directly. |
| 1.2 | `auth.required` | **CUT** | – | Never enforced; misleading switch. |
| 1.3 | `auth.nostr.connected` | **KEEP** | 1 | Read-only indicator; needs to render in chrome. |
| 1.4 | `auth.nostr.publicKey` | **KEEP** | 1 | User identity badge. |
| 1.5–1.7 | passkey / NIP-07 / NIP-98 flows | **KEEP** | 1 | Sole entry to authenticated state. |
| 1.8 | dev login | **KEEP** | 4 | Localhost only; required for dev. Do not expose in prod build. |
| 1.9–1.12 | session storage keys | **KEEP** | – | Internal; not UI. |
| 1.13 | `nostr_privkey` (legacy) | **CUT** | – | Defensive removal-on-init no longer needed once a release ships without it. |
| 1.14–1.16 | `apiKeys.{perplexity,openai,ragflow}` | **KEEP** | 3 | Stored server-side already; needs a "manage API keys" surface tied to PU. |
| 1.17 | session token (deprecated `getSessionToken`) | **CUT** | – | Returns null; remove. |
| 1.18 | `AUTH_TOKEN_EXPIRY` | **KEEP** | 4 | Operator knob; stays env. |
| 1.19–1.20 | Vircadia auth env vars | **CUT** | – | No live consumer found. Audit-confirm before deletion. |
| 1.21 | OIDC config (`src/config/oidc.rs`) | **CUT** | – | Server config with no client/UI; remove or move behind a feature flag. |
| 1.22 | `APPROVED_PUBKEYS` | **KEEP** | 4 | Operator allowlist. |
| 1.23 | `POWER_USER_PUBKEYS` | **KEEP** | 4 | Operator. |
| 1.24 | `SETTINGS_SYNC_ENABLED_PUBKEYS` | **MERGE → 1.23** | 4 | A second allowlist achieving "power-user-lite" muddles the model. Either you can write settings or you can't; collapse into PU. |
| 1.25–1.27 | `{PERPLEXITY,OPENAI,RAGFLOW}_ENABLED_PUBKEYS` | **KEEP** | 4 | Operator gate per integration. Aspirationally these become a per-user feature-grant table in PU UI (8.0). |
| 1.28 | `SETTINGS_AUTH_BYPASS` | **KEEP** | 4 | Already production-fail-closed. Document loudly. |
| 1.29 | `WS_AUTH_ENABLED` | **KEEP** | 4 | Operator. |
| 1.30 | `WS_AUTH_TOKEN` (legacy) | **CUT** | – | Superseded by NIP-98. |
| 1.31a | `feature_flags.gpu_clustering` | **EXPOSE** | 3 | User-visible cost/quality trade-off. |
| 1.31b | `feature_flags.ontology_validation` | **EXPOSE** | 3 | User-visible behaviour change. |
| 1.31c | `feature_flags.gpu_anomaly_detection` | **EXPOSE** | 3 | Same as above. |
| 1.31d | `feature_flags.real_time_insights` | **EXPOSE** | 3 | Affects update cadence. |
| 1.31e | `feature_flags.advanced_visualizations` | **MERGE → 14.5** | – | Folds into the rendering quality tier control (14.5 below). |
| 1.31f | `feature_flags.performance_monitoring` | **EXPOSE** | 3 | Toggles diagnostic overlay. |
| 1.31g | `feature_flags.stress_majorization` | **MERGE → layoutAlgorithm** | – | Becomes one option in the unified layout-algorithm dropdown (4.41+4.42 merged). |
| 1.31h | `feature_flags.semantic_constraints` | **MERGE → 4.45** | – | Same effect as `qualityGates.ontologyPhysics`; unify. |
| 1.31i | `feature_flags.sssp_integration` | **EXPOSE** | 3 | Distance-metric toggle, observable. |

**New entities introduced:** `apiKeys.manage` PU page (entity 1.A); per-user feature-grant table (entity 1.B) replacing the three pubkey allowlists' aspiration of "give-this-user-X".

## 2 Graph rendering — nodes, edges, labels

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 2.1–2.4 | nodes.baseColor / size / opacity / enableInstancing | **KEEP** | 1 | Bread-and-butter visuals. |
| 2.5–2.6 | nodes.metalness / roughness | **KEEP** | 2 | Material tuning. |
| 2.7–2.9 | nodes.enableMetadataShape / Visualisation / enableImportance | **MERGE → "metadata visualisation: off/shape/full"** | 2 | Three booleans expressing one tri-state. |
| 2.10–2.12 | nodeTypeVisibility.{knowledge,ontology,agent} | **KEEP** | 1 | First-class user verb ("show me only knowledge nodes"). |
| 2.13–2.16 | edges.color / baseWidth / opacity / enableArrows | **KEEP** | 1 | |
| 2.17 | edges.arrowSize | **PROMOTE** | 1 | Arrows-on without size control surprises. |
| 2.18–2.19 | edges.useGradient / distanceIntensity | **KEEP** | 2 | |
| 2.20 | knowledgeGraph.edgeColor | **KEEP** | 1 | Directly user-visible. |
| 2.21 | ontology.edgeColor | **PROMOTE** | 1 | Same shape as 2.20; tier mismatch is arbitrary. |
| 2.22–2.26 | labels.enableLabels / fontSize / textColor / showMetadata / textPadding | **KEEP** | 1 | |
| 2.27–2.30 | labels.outlineColor / outlineWidth / labelDistanceThreshold / maxLabelWidth | **KEEP** | 2 | |
| 2.31 | labels.mobileFontSize | **EXPOSE** | 2 | Server-only today; mobile users do exist. Aspirationally expose alongside desktop. |
| 3.1–3.2 | rendering.{ambient,directional}LightIntensity | **KEEP** | 1 | |
| 3.3 | rendering.environmentIntensity | **EXPOSE** | 2 | Server validates it; UI just hasn't surfaced it. |
| 3.4–3.6 | rendering.enable{Antialiasing,Shadows,AmbientOcclusion} | **MERGE → "Render quality preset (Off/Lite/Standard/High)"** | 1 | Three independent perf toggles confuse non-experts. Aspirationally one preset + an "advanced" disclosure exposes the originals. |
| 3.7–3.10 | glow.* (4 fields exposed today) | **KEEP** | 2 | |
| 3.11–3.12 | glow.color / glow.opacity | **EXPOSE** | 2 | Server has them, UI doesn't; complete the set. |
| 3.13 | bloom.* | **EXPOSE** | 2 | Same family as glow; expose intensity/radius/threshold. Validate cross-field consistency already in server. |
| 3.14–3.17 | hologram.ring* | **KEEP** | 2 | |
| 3.18 | hologram.pulse / scanLine / dataStream | **CUT** | – | Per ADR-058-era cleanup (scene-effects pruning) these are the same family of dead hologram subfeatures already removed from settings types/defaults. |
| 3.19–3.24 | gemMaterial.* | **KEEP** | 2 | |
| 3.25–3.35 | sceneEffects.* | **KEEP** | mixed | Already split B/A reasonably. |
| 3.36–3.40 | embeddingCloud.* | **KEEP** | mixed | |
| 3.41–3.44 | edges.flow* / glowStrength | **KEEP** | mixed | |
| 3.45–3.50 | animations.* | **KEEP** | mixed | |
| 3.51–3.55 | interaction.selection* | **KEEP** | mixed | |
| 3.56 | webgpuRenderer toggle | **KEEP** | 2 | Page-reload action; flag clearly. |
| 3.57 | rendererInfo readonly | **KEEP** | 2 | Diagnostic; needs to surface "WebGPU vs WebGL", GPU adapter name. Add to schema so UI binding stops being magic. |

## 4 Physics & layout

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 4.1 | physics.enabled | **KEEP** | 1 | Master switch. |
| 4.2 | physics.autoBalance | **KEEP** | 2 | Affects user-perceived stability. |
| 4.3 | autoBalanceIntervalMs | **HIDE** | 4 | Internal heuristic. |
| 4.4 | autoBalanceConfig.* (40 fields) | **HIDE** | 4 | Black-box tuning; expose at most a single "stability sensitivity" slider as a future entity. |
| 4.5–4.10 | autoPause.* | **EXPOSE** | 2 | Users notice when the graph "freezes"; surface a single "auto-pause when settled" toggle. The internal thresholds stay hidden. |
| 4.11–4.14 | damping / springK / repelK / attractionK | **KEEP** | 1 | Core layout dials; users do touch these. |
| 4.15 | gravity | **CUT** | – | Default 0; no user benefit; delete from schema. |
| 4.16–4.17 | maxVelocity / maxForce | **KEEP** | 2 | Stability caps. Single source-of-truth: pick one of physics/dev_config/canonical-const and unify (see 14.15-14.16). |
| 4.18 | dt | **DEMOTE** | 3 | Almost never user-tuned; safety risk if wrong. |
| 4.19–4.20 | boundsSize / enableBounds | **KEEP** | 1 | |
| 4.21 | centerGravityK | **KEEP** | 1 | "Cluster tightness" labelling is good. |
| 4.22 | restLength | **KEEP** | 1 | "Node spacing". |
| 4.23–4.27 | iterations / warmupIterations / coolingRate / minDistance / maxRepulsionDist | **KEEP** | 2 | Solver tuning. |
| 4.28 | repulsionCutoff | **MERGE → 14.B** (with dev_config.physics.repulsion_cutoff) | 2 | Two paths, same effect; pick `…physics.repulsionCutoff`, delete dev_config copy. |
| 4.29 | gridCellSize | **MERGE → 14.B** (with dev_config.physics.grid_cell_size) | 2 | Same. |
| 4.30 | repulsionSofteningEpsilon | **HIDE** | 4 | Numerical stability constant. |
| 4.31–4.34 | boundary{Extreme,ExtremeForce,Velocity,}Damping/Multiplier | **MERGE → "Boundary behaviour preset (Soft/Standard/Hard)"** | 2 | Four interrelated knobs; aspirationally one preset + advanced disclosure. |
| 4.35 | updateThreshold | **HIDE** | 4 | Solver internal. |
| 4.36 | temperature | **HIDE** | 4 | Solver internal; affects "stress" indirectly; cover via "stability sensitivity" macro. |
| 4.37 | massScale | **HIDE** | 4 | Same. |
| 4.38 | separationRadius | **KEEP** | 2 | |
| 4.39 | graphSeparationX | **KEEP** | 1 | Dual-graph plane separation has clear visual meaning. |
| 4.40 | zDamping | **RENAME → "Flatten to plane"** | 1 | Already a good intent. |
| 4.41 | physics.layoutAlgorithm | **MERGE → 4.42** | 1 | Pick `qualityGates.layoutMode` as the one. Server-side: align option set. |
| 4.42 | qualityGates.layoutMode | **KEEP (canonical)** | 1 | Calls layoutApi.setMode with smooth transition; right transport. |
| 4.43 | physics.clusteringAlgorithm | **EXPOSE** | 3 | Server has it; analytics tab references it via `analytics.clustering.algorithm` (5.28); merge those two. |
| 4.44 | physics.clusteringResolution | **MERGE → 5.30** | 3 | Same effect, one canonical path. |
| 4.45 | qualityGates.ontologyPhysics | **KEEP** | 1 | OWL constraint forces are user-visible. |
| 4.46 | qualityGates.ontologyStrength | **PROMOTE** | 1 | If you turn it on, you want to dial it. |
| 4.47–4.51 | semanticForces / dag* / typeCluster* | **KEEP** | 2 | |
| 4.52–4.55 | tweening.* (client-only) | **WIRE → server** | 1 | Either commit to local-only (and document) or actually persist. Multi-user collaboration arguments for persistence; current divergence is a bug. |
| 4.56 | physics root struct (deprecated) | **CUT** | – | Use only nested `visualisation.graphs.*.physics`. ADR-039 already canonicalised. |
| 4.57–4.61 | dev_config.physics.* | **HIDE** | 4 | Toml-only; numerical constants. |

## 5 Quality / filter / clustering / analytics

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 5.1–5.5 | nodeFilter.{enabled,filterByQuality,qualityThreshold,filterByAuthority,authorityThreshold} | **KEEP** | 1 | First-class verb. |
| 5.6 | nodeFilter.filterMode | **VALIDATE + KEEP** | 2 | Verify server honours `or`/`and`. If it doesn't, either fix or cut. |
| 5.7 | refreshGraph action | **KEEP** | 1 | Move to the unified action registry (entity 18.A) so it isn't hardcoded. |
| 5.8–5.13 | qualityGates.{gpuAcceleration,autoAdjust,minFpsThreshold,maxNodeCount,showClusters,showAnomalies} | **KEEP** | 1 | |
| 5.14 | qualityGates.showCommunities | **PROMOTE** | 1 | Same family as showClusters/showAnomalies; tier mismatch is arbitrary. |
| 5.15 | qualityGates.gnnPhysics | **KEEP** | 3 | Real cost; PU appropriate. |
| 5.16 | qualityGates.ruvectorEnabled | **KEEP** | 3 | Same. |
| 5.17–5.22 | constraints.lod* / progressive* | **MERGE → "Detail policy: off / auto / aggressive"** | 2 | Six interrelated knobs. Aspirationally one preset; advanced disclosure shows the originals. |
| 5.23–5.27 | analytics.* (5 fields) | **WIRE** | 1-2 | Currently CLIENT-ONLY; need a real server source. Either implement server endpoint or honestly mark as client display preferences. |
| 5.28–5.31 | analytics.clustering.{algorithm,clusterCount,resolution,iterations} | **KEEP** | 2 | Reconcile transport: single endpoint vs batch (5.28 disconnect). |
| 5.32–5.33 | clustering.{exportEnabled,importEnabled} | **KEEP** | 3 | Cost/data-egress; PU appropriate. |
| 5.34–5.36 | clusterHulls.* | **KEEP** | mixed | |
| 5.37 | per-user filter | **KEEP** | 2 | Per-pubkey; needs a UI surface (entity 5.A "my filter"). Today only API. |
| 5.38 | settings profiles | **WIRE** or **CUT** | 2 | Stub; either implement (save/load named presets) or remove the routes. |

## 6 System / network / debug

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 6.1 | system.persistSettings | **KEEP** | 2 | Useful kill-switch when debugging. |
| 6.2 | system.customBackendUrl | **KEEP** | 3 | Power user / dev escape hatch. |
| 6.3 | system.debug.enabled | **MERGE → 6.A "Diagnostics: off / errors / verbose"** | 3 | Replace the 10 individual debug toggles plus the duplicate `developer_config.debug_mode` with a single tri-state. |
| 6.4–6.12 | system.debug.* | **MERGE → 6.A** | 3 | Same. Power-users will rarely want to flip a single one; "verbose" expands the set. |
| 6.13 | system.debug.enableVerboseLogging | **MERGE → 6.A** | 3 | |
| 6.14–6.17 | developer.gpu.show* | **KEEP** | 3 | Visual overlays are a different concern from logging; keep distinct entity 6.B "GPU diagnostics overlay" with sub-toggles. |
| 6.18–6.29 | system.network.* / system.websocket.* / system.security.* | **HIDE** | 4 | Operator deploy-time knobs; do not surface. Document in operator guide. |
| 6.30 | dev_config.network.ws_ping_interval_secs | **EXPOSE** | 3 | User-visible reconnect cadence; promote into `system.websocket.heartbeat_interval` (6.27) and surface as PU. |
| 6.31 | ws_pong_timeout_secs | **MERGE → 6.30** | 3 | Same family. |
| 6.32–6.33 | network.max_retry_attempts / retry_base_delay_ms | **HIDE** | 4 | Internal; user has the front-end retry indicator (`SettingsRetryStatus`). |
| 6.34–6.37 | dev_config.cuda.* | **HIDE** | 4 | Hardware caps; deploy-time. |
| 6.38 | dev_config.rendering.agent_colors | **EXPOSE** | 2 | User-visible palette; surface as a "Agent colours" group. |
| 6.39 | dev_config.rendering.agent_base_size | **EXPOSE** | 2 | Symmetric to nodeSize for agents. |
| 6.40 | dev_config.rendering.lod_distance_high | **MERGE → constraints.farThreshold** | 2 | Same domain. |
| 6.41–6.43 | dev_config.performance.* | **HIDE** | 4 | Internal. |
| 6.44–6.47 | dev_config.debug.* | **MERGE → 6.A** | 3 | Same family as 6.3-6.12. |

## 7 XR

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 7.1 | xr.enabled | **WIRE** | 1 | UI toggles, server has the schema, no transport between them. Either complete the round-trip or drop XR from the UI tab. |
| 7.2 | xr.quality | **WIRE** | 2 | Same. |
| 7.3 | xr.renderScale | **WIRE** | 2 | Same. |
| 7.4 | xr.enableHandTracking | **WIRE** | 2 | Same. |
| 7.5 | xr.enableHaptics | **WIRE** | 2 | Same. |
| 7.6 | xr.gpu.enableOptimizedCompute | **WIRE** | 3 | Same. |
| 7.7 | xr.performance.preset | **WIRE** | 2 | Same. |
| 7.8 | xr.enableAdaptiveQuality | **WIRE** | 2 | Same. |
| 7.9 | xr.gestureSmoothing | **EXPOSE** | 2 | User-tunable feel. |
| 7.10–7.11 | xr.{teleport,controller}RayColor | **EXPOSE + VALIDATE** | 2 | Server stores empty defaults, no hex validation. Surface and validate. |
| 7.12 | xr.movementSpeed | **EXPOSE** | 2 | User comfort. |
| 7.13 | xr.deadZone | **EXPOSE** | 2 | User comfort. |
| 7.14–7.16 | xr.plane / passthrough fields | **EXPOSE** | 2 | First-class XR features. |
| 7.17 | XR remaining ~30 fields | **TBD** | 2-4 | Audit each; fold internal tunables to tier 4, surface user-comfort fields to tier 2. |

## 8 AI / Voice / Integrations

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 8.1–8.3 | ragflow.* | **KEEP** | 3 | |
| 8.4–8.6 | perplexity.{model,maxTokens,temperature} | **KEEP** | 3 | |
| 8.7–8.9 | perplexity.{frequency,presence,topP} | **EXPOSE** | 3 | Already env, lifecycle-relevant for tuning agents; promote to PU UI. |
| 8.10–8.11 | openai.{baseUrl,timeout} | **KEEP** | 3 | |
| 8.12 | openai.rateLimit | **EXPOSE** | 3 | User notices throttling; expose. |
| 8.13 | openai.orgId | **EXPOSE** | 3 | Routing-relevant. |
| 8.14–8.16 | kokoro.* | **KEEP** | 3 | |
| 8.17–8.19 | whisper.* | **KEEP** | 3 | |
| 8.20 | voice_routing.* | **EXPOSE** | 3 | "Which provider for which task" is a user concern when multiple are wired. |
| 8.21 | ontology_agent.* | **EXPOSE** | 3 | If agent runs on user request, user must configure it. |
| 8.22–8.23 | DEEPSEEK_API_KEY / BASE_URL | **MERGE → 1.14** | 3 | Should join the per-user API-keys table, not be env-only. |
| 8.24–8.26 | OLLAMA / LOCAL_LLM / COMFYUI envs | **EXPOSE + DOCUMENT** | 3 | Currently in agentbox compose only; surface in a unified "AI providers" table and add to root `.env.example`. |
| 8.27–8.30 | NL query translate / explain / validate / examples | **WIRE → CC-AI box** | 1 | The collapsed AI text-entry parser is shallow keyword regex; the live `/api/nl-query/*` endpoints exist and are unused. Connect them. |

## 9 GitHub / Knowledge-graph sync

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 9.1–9.5 | GITHUB_* env vars | **EXPOSE** | 3 | Currently env-only; ADR-058-era cleanup left GITHUB_KG_PATH dead. Aspirationally a "Source repo" config card in PU. |
| 9.6 | FORCE_FULL_SYNC | **EXPOSE** | 3 | Operator action; user-triggerable button. |
| 9.7 | sync trigger | **EXPOSE** | 3 | "Re-sync from GitHub" action button + last-sync timestamp display. |

## 10 Camera / SpacePilot / user_preferences

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 10.1–10.4 | camera.{fov,near,far,positionPresets} | **EXPOSE** | 2 | FOV in particular has direct user effect. Position presets — a "view" save/load surface (entity 10.A). |
| 10.5–10.7 | spacepilot.* | **EXPOSE** | 2 | Hardware exists in chrome (`SpacePilotStatus`); user with the device must be able to tune. |
| 10.8 | user_preferences.theme | **EXPOSE** | 1 | Theme is a basic preference. |
| 10.9 | user_preferences.language | **EXPOSE** | 1 | i18n primer. |
| 10.10 | user_preferences.comfort.* | **EXPOSE** | 2 | XR comfort: vignette, snap-turn, etc. |

## 11 Enterprise control surface

| # | Element | Decision | Tier | Justification |
|---|---|---|---|---|
| 11.1 | active panel selector | **KEEP** | 1 | |
| 11.2–11.3 | drawer state | **KEEP** | 1 | |
| 11.4–11.6 | broker case submit / inbox / timeline | **WIRED** | 1 | BrokerActor now publishes cases via `PublishActionRequest` (kind 31402) to Nostr relay; panel definitions via `PublishGovernancePanel` (kind 31400). Wire from Nostr subscription to UI panel rendering. |
| 11.7–11.9 | workflow proposals / patterns | **WIRE** | 1 | Same. |
| 11.10 | KPI time window | **KEEP** | 1 | |
| 11.11 | KPI sparklines | **WIRE → real metrics** | 1 | Currently locally seeded; should pull real history. |
| 11.12–11.14 | connector setup / list / signals | **KEEP + WIRE signals** | 1 | Signals partial; complete. |
| 11.15–11.18 | policy console (rule toggle, test action, confidence, run-test) | **WIRE** | 2 | Today fully local with hardcoded DEFAULT_RULES. Decide: local-eval-only is a legitimate sandbox feature, *or* persist to backend. Pick one and label honestly. |
| 11.19 | drawer_fx WASM | **KEEP** | 1 | Quality tier (0/1/2) should join the unified rendering quality preset (3.4-3.6 merge). |

## 12 Contributor Studio

| # | Element | Decision | Tier | Justification |
|---|---|---|---|---|
| 12.1 | active workspaceId | **KEEP** | 1 | |
| 12.2–12.10 | workspace / partner / inbox / nudges / skills / chat / automation | **WIRE** | 1 | Stub-backed; depends on agents C1/C5/X1. Out of scope for this audit but tracked. |
| 12.11 | command palette | **KEEP** | 1 | Should be the single command-execution surface; adopt across all surfaces. |
| 12.12–12.13 | onboarding overlay & flows | **CUT** | – | Disabled, no flows defined, no consumer. Remove or commit to building. |

## 13 Health / Monitoring

| # | Element | Decision | Tier | Justification |
|---|---|---|---|---|
| 13.1–13.3 | health polls | **KEEP** | 2 | Diagnostic surface. |
| 13.4–13.5 | MCP relay start / logs | **KEEP** | 3 | Operator action exposed in PU. |
| 13.6 | refresh button | **KEEP** | 2 | |
| **NEW 13.A** | "audit trail" feed | **ADD** | 3 | No persistent audit log today (server disconnect §18.14). Aspirationally a paged feed of who-changed-what-when. |

## 14 Hidden server-side knobs to surface

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 14.1–14.9 | feature_flags.* | (handled in §1.31) | 3 | |
| 14.10 | developer_config.debug_mode | **MERGE → 6.A** | – | Duplicate. |
| 14.11 | developer_config.show_performance_stats | **EXPOSE** | 3 | "Show stats" toggle in HEALTH or chrome. |
| 14.12 | developer_config.enable_profiling | **EXPOSE** | 3 | PU diagnostic. |
| 14.13 | developer_config.verbose_logging | **MERGE → 6.A** | – | Duplicate. |
| 14.14 | developer_config.dev_tools_enabled | **KEEP** | 3 | Gates dev-only API. |
| 14.15 | CANONICAL_MAX_VELOCITY (200) | **PROMOTE → 4.16** | 2 | Become *the* source-of-truth; remove dev_config + physics duplicates. |
| 14.16 | CANONICAL_MAX_FORCE (50) | **PROMOTE → 4.17** | 2 | Same. |
| **NEW 14.B** | physics-tuning canonical struct | **CONSOLIDATE** | 2 | Single source for max_velocity, max_force, repulsion_cutoff, grid_cell_size, warmup_iterations. Today they live in physics + dev_config + canonical const concurrently. |

## 15 Agentbox / federation (operator)

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 15.1–15.14 | federation / adapter / toolchain / sovereign_mesh knobs | **KEEP** | 4 | Operator deploy-time; document, don't surface. Aspirationally a read-only "this deployment config" panel in HEALTH at PU tier. |
| 15.15 | RUVECTOR_PG_CONNINFO | **KEEP + DOCUMENT** | 4 | Hardcoded today; should derive from secret-manager. |
| 15.16 | MANAGEMENT_API_AUTH_MODE | **DOCUMENT VALUES** | 4 | Currently undocumented enum. |
| 15.17–15.23 | MANAGEMENT_API_* / MCP_* / ORCHESTRATOR_WS_URL / BOTS_ORCHESTRATOR_URL | **KEEP** | 4 | Operator. Aspirational read-only display in HEALTH. |

## 16 Database / persistence

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 16.1–16.4 | NEO4J_* | **KEEP** | 4 | Operator. |
| 16.5–16.6 | settings persistence layers | **KEEP** | – | Internal. |
| 16.7 | settings yaml | **CUT** | – | Already disabled; remove residual code. |
| 16.8 | audit trail | **ADD** | – | New entity 13.A; persist as Neo4j table. |

## 17 Build / runtime / observability

| # | Path | Decision | Tier | Justification |
|---|---|---|---|---|
| 17.1–17.5 | CUDA_ARCH / BUILD_TARGET / NODEJS_VERSION / RUST_LOG / RUST_LOG_REDIRECT | **KEEP** | 4 | Build-time. |
| 17.6 | DEBUG_ENABLED | **KEEP** | 4 | Build flag. |
| 17.7–17.10 | VITE_* / SYSTEM_NETWORK_PORT | **KEEP** | 4 | |
| 17.11 | TELEMETRY_ENABLED | **EXPOSE** | 3 | Privacy-sensitive; user must be able to opt out. |
| 17.12 | TELEMETRY_METRICS_INTERVAL | **HIDE** | 4 | Internal. |
| 17.13 | SESSION_SECRET | **KEEP** | 4 | Add rotation policy doc. |
| 17.14 | SESSION_TIMEOUT | **EXPOSE** | 3 | User-relevant. |
| 17.15 | SOLID_PROXY_SECRET_KEY | **KEEP** | 4 | Same. Add rotation policy. |
| 17.16 | VISIONFLOW_AGENT_KEY | **CUT** | – | Deprecated by ADR-040 Nostr identity. |
| 17.17 | POD_NAME | **KEEP** | 4 | Computed. |
| 17.18 | CORS_ALLOWED_* | **KEEP** | 4 | Operator. |
| 17.19 | CLOUDFLARE_TUNNEL_TOKEN | **DOCUMENT** | 4 | Add to .env templates. |
| 17.20–17.23 | NOSTR identity / FORUM_RELAY_URL / SERVER_NOSTR_AUTO_GENERATE | **KEEP** | 4 | Operator. Add rotation policy. |
| 17.24–17.25 | NVIDIA_DRIVER_CAPABILITIES / VISIBLE_DEVICES | **KEEP + DOCUMENT** | 4 | Multi-GPU strategy guidance missing. |

## 18 New entities introduced by this rationalisation

| # | Entity | Why | Tier |
|---|---|---|---|
| 18.A | **Action registry** | Replace hardcoded `refresh_graph` / `toggle-webgpu` action-buttons with a typed registry so new actions (e.g. "re-sync GitHub" 9.7) compose in. | 1 |
| 18.B | **AI providers table** | Single PU page enumerating ragflow / perplexity / openai / kokoro / whisper / deepseek / ollama / comfyui / local-LLM, each with: enabled?, base URL, model, key (linked to apiKeys table 1.A), per-user grant. | 3 |
| 18.C | **Per-user feature grants** | Replace 4 separate pubkey allowlists (1.24-1.27) with a single "grant feature X to user Y" matrix. | 3 |
| 18.D | **Operator deployment view** | Read-only display of effective values for §15-§17 entities. Surfaces what's actually running without exposing secrets. | 3 |
| 18.E | **My filter** UI | Expose the per-user `/api/user/filter` endpoint (5.37) which has no UI today. | 2 |
| 18.F | **Saved views** | Replace stub settings profiles (5.38) with a "save view" / "load view" surface, OR remove the routes if not implementing. | 2 |
| 18.G | **NL command surface** | Promote the collapsed AI text-entry box (CC-AI) into a real NL command surface backed by `/api/nl-query/*` (8.27-8.30); not just a keyword regex. | 1 |
| 18.H | **Provider grant matrix** | Cross of "user × provider" replacing PERPLEXITY_ENABLED_PUBKEYS et al. | 3 |

## 19 Counts after rationalisation

- **CUT outright:** 12 entities (auth.enabled, auth.required, gravity, deprecated physics root, OIDC, Vircadia auth, legacy nostr_privkey, settings yaml, hologram pulse/scan/dataStream, deprecated session token, WS_AUTH_TOKEN, VISIONFLOW_AGENT_KEY).
- **MERGE / consolidate:** 11 mergers (3 metadata flags → 1 tri-state; 3 render-quality toggles → 1 preset; 6 LOD constraints → 1 detail policy; 4 boundary knobs → 1 preset; 10 debug toggles + 2 server dups → 1 diagnostics tri-state; layoutMode/layoutAlgorithm; clusteringAlgorithm/Resolution dup; lod_distance_high → farThreshold; perf-tuning canonical struct; SETTINGS_SYNC_ENABLED_PUBKEYS → POWER_USER_PUBKEYS; deepseek key → apiKeys table).
- **EXPOSE (server-only → user UI):** ~35 entities (feature_flags*, developer_config diagnostic toggles, glow.color/opacity, bloom.*, autoPause toggle, env vars for LLM/voice routing/ontology agent, agent_colors, agent_base_size, ws_ping, GitHub envs, camera, spacepilot, user_preferences, XR fields).
- **WIRE (UI exists, transport broken/stub):** ~25 entities (XR set 7.1-7.8, broker, workflows, studio, policy console, settings profiles, NL query, tweening).
- **HIDE (keep server, drop from UI):** ~30 entities (network/websocket/security operator knobs, dev_config tunables, autoBalanceConfig 40+ fields, internal numerical constants).
- **KEEP unchanged:** the bulk of CC.Graph, CC.Effects, CC.Physics core dials, CC.Quality basics — settings users actually touch.
- **NEW entities introduced:** 8 (action registry, AI providers table, per-user grants, operator view, my filter, saved views, NL command surface, provider grant matrix).

**Net effect on user-facing surface:** ≈205 leaf controls today → roughly **140-150** entities visible at tier 1+2, with a clearer tier-3 PU subsurface for ~25 power-user concepts and tier-4 operator-only documentation for the rest. A substantial portion of "advanced toggles that nobody needs" collapses into preset macros, while a comparable number of currently-hidden settings (LLM provider config, GitHub sync, audit trail, per-user features) becomes visible where they actually belong.

## 20 What this document deliberately does NOT decide

- Grouping into tabs, panels, sections, drawers.
- Visual hierarchy, typography, iconography, colour.
- Disclosure mechanism (modal vs inline expand vs side panel).
- Adaptive surfacing (recommendation, ML-driven defaults).
- Any naming above the entity-key level (label copy, tooltip text).
- Per-user personalisation strategy.

These are the next conversation, downstream of an entity-level agreement.
