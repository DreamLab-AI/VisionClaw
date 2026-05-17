# Comprehensive Settings Inventory

> Branch: `feature/unified-control-surface`. Compiled from raw audit files in `docs/control-surface-audit/raw/01-06-*.md`.
> Captures every settings entity surfaced anywhere in the project (client UI, server schema, env, ADR, ancillary) regardless of connection validity.

## Legend

**Status:**
- **LIVE** — UI bound, API path bound, server consumes
- **CLIENT-ONLY** — UI/store path exists, no server endpoint (localStorage-only)
- **SERVER-ONLY** — server has it, no UI
- **STUB** — handler returns dummy data
- **ORPHAN** — defined but not consumed anywhere
- **DUPLICATE** — same effect via two different paths/controls
- **DEAD** — referenced in code/docs but no reads or writes
- **DEPRECATED** — replaced by newer mechanism, alias still active

**Surface (where exposed today):**
- CC = Control Center panel (`unifiedSettingsConfig.ts`)
- CC-AI = collapsed AI text-entry box
- AUTH = Login / Onboarding screen
- ENT = Enterprise Standalone (Broker / Workflows / KPI / Connectors / Policy)
- STD = Contributor Studio (4-pane shell)
- HEALTH = Monitoring / Health dashboard
- ENV = environment variable only
- TOML = `data/dev_config.toml` only
- AB = `agentbox.toml` only
- API = exposed via REST/WS path but no UI
- HARDCODED = compile-time constant

**Tier (today):** B = basic, A = advanced, PU = power-user-only, DEV = dev-only

---

## 1 Authentication & Identity

| # | Path / Key | Type | Source-of-truth | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 1.1 | `auth.enabled` | bool | localStorage | CC.System | A | CLIENT-ONLY | Toggles voice/editor gates client-side; no server schema |
| 1.2 | `auth.required` | bool | localStorage | CC.System | A,PU | CLIENT-ONLY | Cosmetic; never enforced |
| 1.3 | `auth.nostr.connected` | bool | nostrAuthService | CC.System | B | LIVE (local) | Read by `useNostrAuth` |
| 1.4 | `auth.nostr.publicKey` | hex | nostrAuthService + localStorage `nostr_user` | CC.System | B | LIVE (local) | Never sent to server |
| 1.5 | passkey register | flow | `/idp/passkey/register-new` | AUTH | B | LIVE | WebAuthn + PRF |
| 1.6 | passkey authenticate | flow | `/idp/passkey/authenticate` | AUTH | B | LIVE | |
| 1.7 | NIP-07 (PodKey/Alby) login | flow | window.nostr | AUTH | B | LIVE | extension-managed key |
| 1.8 | dev login | flow | `VITE_DEV_MODE_AUTH` + `VITE_DEV_POWER_USER_PUBKEY` | AUTH | DEV | LIVE | localhost-only auto-login |
| 1.9 | `nostr_user` | JSON | localStorage | (internal) | – | LIVE | session restore |
| 1.10 | `_localKeyHex` | hex | memory only | (internal) | – | LIVE | private key, never persisted |
| 1.11 | `nostr_passkey_pubkey` | hex | sessionStorage | (internal) | – | LIVE | re-auth check |
| 1.12 | `nostr_prf` | "0"/"1" | sessionStorage | (internal) | – | LIVE | PRF-derived flag |
| 1.13 | `nostr_privkey` | hex | sessionStorage | – | – | DEPRECATED | removed on init; legacy lingering |
| 1.14 | `apiKeys.perplexity` | string | server (Neo4j) | API only | PU | LIVE | POST `/api/auth/nostr/api-keys` |
| 1.15 | `apiKeys.openai` | string | server (Neo4j) | API only | PU | LIVE | same |
| 1.16 | `apiKeys.ragflow` | string | server (Neo4j) | API only | PU | LIVE | same |
| 1.17 | session token | string | (deprecated) | – | – | DEPRECATED | `getSessionToken()` returns null |
| 1.18 | `AUTH_TOKEN_EXPIRY` | seconds | env (default 3600) | ENV | DEV | LIVE | no refresh token |
| 1.19 | `VITE_VIRCADIA_AUTH_TOKEN` | string | .env | ENV | DEV | ORPHAN? | Vircadia adapter, possibly dead |
| 1.20 | `VITE_VIRCADIA_AUTH_PROVIDER` | string | .env | ENV | DEV | ORPHAN? | defaults "system" |
| 1.21 | OIDC config (`src/config/oidc.rs`) | struct | server | – | – | DEAD | client never calls OIDC routes |
| 1.22 | `APPROVED_PUBKEYS` | CSV pubkeys | env | ENV | – | LIVE | gates `register_new_user()` |
| 1.23 | `POWER_USER_PUBKEYS` | CSV pubkeys | env | ENV | – | LIVE | unlocks PU UI + settings sync |
| 1.24 | `SETTINGS_SYNC_ENABLED_PUBKEYS` | CSV pubkeys | env | ENV | – | LIVE | non-PU settings PUT allowlist |
| 1.25 | `PERPLEXITY_ENABLED_PUBKEYS` | CSV pubkeys | env | ENV | – | LIVE | gates Perplexity |
| 1.26 | `OPENAI_ENABLED_PUBKEYS` | CSV pubkeys | env | ENV | – | LIVE | gates OpenAI |
| 1.27 | `RAGFLOW_ENABLED_PUBKEYS` | CSV pubkeys | env | ENV | – | LIVE | gates RAG |
| 1.28 | `SETTINGS_AUTH_BYPASS` | bool | env | ENV | DEV | LIVE | rejected at startup if APP_ENV/RUST_ENV=production |
| 1.29 | `WS_AUTH_ENABLED` | bool | env | ENV | DEV | LIVE | NIP-98 for WS |
| 1.30 | `WS_AUTH_TOKEN` | string | env | ENV | DEV | DEPRECATED | superseded by NIP-98 |
| 1.31 | server-side feature_flags struct (9 toggles) | bool×9 | `app_settings.rs::FeatureFlags` | API only | – | SERVER-ONLY | gpu_clustering, ontology_validation, gpu_anomaly_detection, real_time_insights, advanced_visualizations, performance_monitoring, stress_majorization, semantic_constraints, sssp_integration |

## 2 Graph rendering — nodes, edges, labels

| # | Path | Type | Server schema | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 2.1 | `…nodes.baseColor` | color | `visualisation.nodes` | CC.Graph | B | LIVE | hex |
| 2.2 | `…nodes.nodeSize` | 0.2-2 | nodes.size | CC.Graph | B | LIVE | |
| 2.3 | `…nodes.opacity` | 0-1 | nodes.opacity | CC.Graph | B | LIVE | |
| 2.4 | `…nodes.enableInstancing` | bool | nodes | CC.Graph | B | LIVE | |
| 2.5 | `…nodes.metalness` | 0-1 | nodes | CC.Graph | A | LIVE | |
| 2.6 | `…nodes.roughness` | 0-1 | nodes | CC.Graph | A | LIVE | |
| 2.7 | `…nodes.enableMetadataShape` | bool | nodes | CC.Graph | A | LIVE | |
| 2.8 | `…nodes.enableMetadataVisualisation` | bool | nodes | CC.Graph | A | LIVE | |
| 2.9 | `…nodes.enableImportance` | bool | nodes | CC.Graph | A | LIVE | |
| 2.10 | `…nodeTypeVisibility.knowledge` | bool | nodes | CC.Graph + CC-AI | B | LIVE | |
| 2.11 | `…nodeTypeVisibility.ontology` | bool | nodes | CC.Graph + CC-AI | B | LIVE | |
| 2.12 | `…nodeTypeVisibility.agent` | bool | nodes | CC.Graph + CC-AI | B | LIVE | |
| 2.13 | `…edges.color` | color | edges | CC.Graph | B | LIVE | |
| 2.14 | `…edges.baseWidth` | 0.01-2 | edges | CC.Graph | B | LIVE | |
| 2.15 | `…edges.opacity` | 0-1 | edges | CC.Graph | B | LIVE | |
| 2.16 | `…edges.enableArrows` | bool | edges | CC.Graph | B | LIVE | |
| 2.17 | `…edges.arrowSize` | 0.01-0.5 | edges | CC.Graph | A | LIVE | |
| 2.18 | `…edges.useGradient` | bool | edges | CC.Graph | A | LIVE | |
| 2.19 | `…edges.distanceIntensity` | 0-10 | edges | CC.Graph | A | LIVE | |
| 2.20 | `graphTypeVisuals.knowledgeGraph.edgeColor` | color | visual blob | CC.Graph | B | LIVE | merge-PUT `/visual` |
| 2.21 | `graphTypeVisuals.ontology.edgeColor` | color | visual blob | CC.Graph | A | LIVE | |
| 2.22 | `…labels.enableLabels` | bool | labels | CC.Graph | B | LIVE | |
| 2.23 | `…labels.desktopFontSize` | 0.05-3.0 | labels | CC.Graph | B | LIVE | |
| 2.24 | `…labels.textColor` | color | labels | CC.Graph | B | LIVE | |
| 2.25 | `…labels.showMetadata` | bool | labels | CC.Graph | B | LIVE | |
| 2.26 | `…labels.textPadding` | -1.0-3.0 | labels | CC.Graph | B | LIVE | |
| 2.27 | `…labels.textOutlineColor` | color | labels | CC.Graph | A | LIVE | |
| 2.28 | `…labels.textOutlineWidth` | 0-0.01 | labels | CC.Graph | A | LIVE | |
| 2.29 | `…labels.labelDistanceThreshold` | 50-2000 | labels | CC.Graph | A | LIVE | |
| 2.30 | `…labels.maxLabelWidth` | 2-20 | labels | CC.Graph | A | LIVE | |
| 2.31 | `…labels.mobileFontSize` | – | labels | – | – | SERVER-ONLY | not in CC |

## 3 Lighting, post-processing, effects

| # | Path | Type | Server schema | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 3.1 | `visualisation.rendering.ambientLightIntensity` | 0-2 | rendering | CC.Graph | B | LIVE | broadcasts settingsUpdated |
| 3.2 | `visualisation.rendering.directionalLightIntensity` | 0-2 | rendering | CC.Graph | B | LIVE | |
| 3.3 | `visualisation.rendering.environmentIntensity` | 0+ | rendering | – | – | SERVER-ONLY | validated server-side, no UI |
| 3.4 | `visualisation.rendering.enableAntialiasing` | bool | rendering | CC.Graph | A | LIVE | |
| 3.5 | `visualisation.rendering.enableShadows` | bool | rendering | CC.Graph | A | LIVE | |
| 3.6 | `visualisation.rendering.enableAmbientOcclusion` | bool | rendering | CC.Graph | A | LIVE | |
| 3.7 | `visualisation.glow.enabled` | bool | visual blob | CC.Effects | B | LIVE | |
| 3.8 | `visualisation.glow.intensity` | 0-1.5 | visual | CC.Effects | B | LIVE | |
| 3.9 | `visualisation.glow.radius` | 0-1.0 | visual | CC.Effects | B | LIVE | |
| 3.10 | `visualisation.glow.threshold` | 0-1 | visual | CC.Effects | A | LIVE | |
| 3.11 | `visualisation.glow.color` | hex | visual + server | – | – | SERVER-ONLY | default #00ffff, no UI |
| 3.12 | `visualisation.glow.opacity` | 0.8 | visual + server | – | – | SERVER-ONLY | no UI |
| 3.13 | `visualisation.bloom.*` (intensity, radius, threshold, strength, knee) | nested | server | – | – | SERVER-ONLY | validated, no UI |
| 3.14 | `visualisation.hologram.ringCount` | 0-10 | visual | CC.Effects | B | LIVE | |
| 3.15 | `visualisation.hologram.ringColor` | color | visual | CC.Effects | B | LIVE | |
| 3.16 | `visualisation.hologram.ringOpacity` | 0-1 | visual | CC.Effects | B | LIVE | |
| 3.17 | `visualisation.hologram.ringRotationSpeed` | 0-5 | visual | CC.Effects | A | LIVE | |
| 3.18 | `visualisation.hologram.pulse/scanLine/dataStream` | nested | server | – | – | SERVER-ONLY | undocumented in UI |
| 3.19 | `visualisation.gemMaterial.ior` | 1.0-3.0 | visual | CC.Effects | A | LIVE | |
| 3.20 | `…gemMaterial.transmission` | 0-1 | visual | CC.Effects | A | LIVE | |
| 3.21 | `…gemMaterial.clearcoat` | 0-1 | visual | CC.Effects | A | LIVE | |
| 3.22 | `…gemMaterial.clearcoatRoughness` | 0-0.5 | visual | CC.Effects | A | LIVE | |
| 3.23 | `…gemMaterial.emissiveIntensity` | 0-2 | visual | CC.Effects | A | LIVE | |
| 3.24 | `…gemMaterial.iridescence` | 0-1 | visual | CC.Effects | A | LIVE | |
| 3.25 | `visualisation.sceneEffects.enabled` | bool | visual | CC.Effects | B | LIVE | WASM ambient |
| 3.26 | `…sceneEffects.particleCount` | 64-512 | visual | CC.Effects | B | LIVE | |
| 3.27 | `…sceneEffects.particleOpacity` | 0-1 | visual | CC.Effects | B | LIVE | |
| 3.28 | `…sceneEffects.particleDrift` | 0-2 | visual | CC.Effects | A | LIVE | |
| 3.29 | `…sceneEffects.wispsEnabled` | bool | visual | CC.Effects | B | LIVE | |
| 3.30 | `…sceneEffects.wispCount` | 8-128 | visual | CC.Effects | B | LIVE | |
| 3.31 | `…sceneEffects.wispOpacity` | 0-1 | visual | CC.Effects | B | LIVE | |
| 3.32 | `…sceneEffects.wispDriftSpeed` | 0-3 | visual | CC.Effects | A | LIVE | |
| 3.33 | `…sceneEffects.fogEnabled` | bool | visual | CC.Effects | B | LIVE | |
| 3.34 | `…sceneEffects.fogOpacity` | 0-0.15 | visual | CC.Effects | B | LIVE | |
| 3.35 | `…sceneEffects.atmosphereResolution` | 64-256 | visual | CC.Effects | A | LIVE | |
| 3.36 | `visualisation.embeddingCloud.enabled` | bool | visual | CC.Effects | B | LIVE | RuVector |
| 3.37 | `…embeddingCloud.cloudScale` | 0.5-20 | visual | CC.Effects | B | LIVE | |
| 3.38 | `…embeddingCloud.pointSize` | 0.5-25 | visual | CC.Effects | B | LIVE | |
| 3.39 | `…embeddingCloud.opacity` | 0-1 | visual | CC.Effects | B | LIVE | |
| 3.40 | `…embeddingCloud.rotationSpeed` | 0-0.005 | visual | CC.Effects | A | LIVE | |
| 3.41 | `…edges.enableFlowEffect` | bool | visual | CC.Effects | B | LIVE | |
| 3.42 | `…edges.flowSpeed` | 0.1-5 | visual | CC.Effects | A | LIVE | |
| 3.43 | `…edges.flowIntensity` | 0-10 | visual | CC.Effects | A | LIVE | |
| 3.44 | `…edges.glowStrength` | 0-5 | visual | CC.Effects | B | LIVE | |
| 3.45 | `visualisation.animations.enableNodeAnimations` | bool | visual | CC.Effects | B | LIVE | |
| 3.46 | `…animations.pulseEnabled` | bool | visual | CC.Effects | B | LIVE | |
| 3.47 | `…animations.pulseSpeed` | 0.1-2 | visual | CC.Effects | A | LIVE | |
| 3.48 | `…animations.pulseStrength` | 0.1-2 | visual | CC.Effects | A | LIVE | |
| 3.49 | `…animations.selectionWaveEnabled` | bool | visual | CC.Effects | A | LIVE | |
| 3.50 | `…animations.waveSpeed` | 0.1-2 | visual | CC.Effects | A | LIVE | |
| 3.51 | `visualisation.interaction.selectionHighlightColor` | color | visual | CC.Graph | B | LIVE | |
| 3.52 | `…interaction.selectionEdgeFlow` | bool | visual | CC.Graph | B | LIVE | |
| 3.53 | `…interaction.selectionEdgeFlowSpeed` | 0.5-5 | visual | CC.Graph | A | LIVE | |
| 3.54 | `…interaction.selectionEdgeWidth` | 0.1-2 | visual | CC.Graph | A | LIVE | |
| 3.55 | `…interaction.selectionEdgeOpacity` | 0.3-1 | visual | CC.Graph | A | LIVE | |
| 3.56 | `webgpuRenderer` action | toggle | local global `forceWebGLOverride` | CC.Effects | B | LIVE | reloads page |
| 3.57 | `rendererInfo` | readonly | runtime detection | CC.Effects | B | ORPHAN | no schema entry |

## 4 Physics & layout

| # | Path | Type | Server schema | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 4.1 | `…physics.enabled` | bool | physics (canonical) | CC.Physics | B | LIVE | cross-field: gravity≠0→must be true |
| 4.2 | `…physics.autoBalance` | bool | physics | CC.Physics | B | LIVE | |
| 4.3 | `…physics.autoBalanceIntervalMs` | u32 | physics | – | – | SERVER-ONLY | default 500 |
| 4.4 | `…physics.autoBalanceConfig.*` (40+ fields) | nested | physics | – | – | SERVER-ONLY | `stability_variance_threshold`, `clustering_distance_threshold`, … no UI |
| 4.5 | `…physics.autoPause.enabled` | bool | physics | – | – | SERVER-ONLY | |
| 4.6 | `…physics.autoPause.equilibriumVelocityThreshold` | 0-10 | physics | – | – | SERVER-ONLY | |
| 4.7 | `…physics.autoPause.equilibriumCheckFrames` | 1-300 | physics | – | – | SERVER-ONLY | |
| 4.8 | `…physics.autoPause.equilibriumEnergyThreshold` | 0-1 | physics | – | – | SERVER-ONLY | |
| 4.9 | `…physics.autoPause.pauseOnEquilibrium` | bool | physics | – | – | SERVER-ONLY | |
| 4.10 | `…physics.autoPause.resumeOnInteraction` | bool | physics | – | – | SERVER-ONLY | |
| 4.11 | `…physics.damping` | 0-1 | physics | CC.Physics + CC-AI | B | LIVE | |
| 4.12 | `…physics.springK` | 0.1-100 | physics | CC.Physics + CC-AI | B | LIVE | |
| 4.13 | `…physics.repelK` | 0-3000 | physics | CC.Physics + CC-AI | B | LIVE | |
| 4.14 | `…physics.attractionK` | 0-10 | physics | CC.Physics | B | LIVE | |
| 4.15 | `…physics.gravity` | -10..10 | physics | – | – | SERVER-ONLY | validated; no UI |
| 4.16 | `…physics.maxVelocity` | 0.1-500 | physics | CC.Physics | B | LIVE | CANONICAL_MAX_VELOCITY=200 |
| 4.17 | `…physics.maxForce` | 1-1000 | physics | CC.Physics | A | LIVE | CANONICAL_MAX_FORCE=50 |
| 4.18 | `…physics.dt` | 0.001-0.1 | physics | CC.Physics | A | LIVE | |
| 4.19 | `…physics.boundsSize` | 100-100000 | physics | CC.Physics | B | LIVE | |
| 4.20 | `…physics.enableBounds` | bool | physics | CC.Physics | B | LIVE | |
| 4.21 | `…physics.centerGravityK` | 0-10 | physics | CC.Physics | B | LIVE | |
| 4.22 | `…physics.restLength` | 1-200 | physics | CC.Physics | B | LIVE | |
| 4.23 | `…physics.iterations` | 1-5000 | physics | CC.Physics | A | LIVE | solver iters/frame |
| 4.24 | `…physics.warmupIterations` | 0-500 | physics | CC.Physics | A | LIVE | |
| 4.25 | `…physics.coolingRate` | 1e-5..0.01 | physics | CC.Physics | A | LIVE | |
| 4.26 | `…physics.minDistance` | 0.05-20 | physics | CC.Physics | A | LIVE | |
| 4.27 | `…physics.maxRepulsionDist` | 10-2000 | physics | CC.Physics | A | LIVE | |
| 4.28 | `…physics.repulsionCutoff` | 1-2000 | physics | CC.Physics | A | LIVE | DUPLICATE w/ dev_config.physics.repulsion_cutoff |
| 4.29 | `…physics.gridCellSize` | 1-2000 | physics | CC.Physics | A | LIVE | DUPLICATE w/ dev_config.physics.grid_cell_size |
| 4.30 | `…physics.repulsionSofteningEpsilon` | 1e-5..0.01 | physics | CC.Physics | A | LIVE | |
| 4.31 | `…physics.boundaryExtremeMultiplier` | 1-5 | physics | CC.Physics | A | LIVE | |
| 4.32 | `…physics.boundaryExtremeForceMultiplier` | 1-20 | physics | CC.Physics | A | LIVE | |
| 4.33 | `…physics.boundaryVelocityDamping` | 0-1 | physics | CC.Physics | A | LIVE | |
| 4.34 | `…physics.boundaryDamping` | 0-1 | physics | CC.Physics | A | LIVE | |
| 4.35 | `…physics.updateThreshold` | 0-0.5 | physics | CC.Physics | A | LIVE | |
| 4.36 | `…physics.temperature` | 1e-3..100 | physics | CC.Physics | A | LIVE | |
| 4.37 | `…physics.massScale` | 1e-3..100 | physics | CC.Physics | A | LIVE | |
| 4.38 | `…physics.separationRadius` | 0.01-200 | physics | CC.Physics | A | LIVE | |
| 4.39 | `…physics.graphSeparationX` | 0-500 | physics | CC.Physics | B | LIVE | dual-graph plane sep |
| 4.40 | `…physics.zDamping` | 0-0.1 | physics | CC.Physics | B | LIVE | 3D→2D flatten |
| 4.41 | `…physics.layoutAlgorithm` | enum | physics | CC.Physics | B | DUPLICATE | overlaps with `qualityGates.layoutMode` |
| 4.42 | `qualityGates.layoutMode` | enum | quality_gates | CC.Physics | B | DUPLICATE | calls `layoutApi.setMode()` |
| 4.43 | `…physics.clusteringAlgorithm` | string | physics | – | – | SERVER-ONLY | "modularity"/"louvain"/"greedy" |
| 4.44 | `…physics.clusteringResolution` | f32 | physics | – | – | SERVER-ONLY | |
| 4.45 | `qualityGates.ontologyPhysics` | bool | quality_gates | CC.Physics | B | LIVE | side-effect: `toggleOntologyPhysics()` |
| 4.46 | `qualityGates.ontologyStrength` | 0-1 | quality_gates | CC.Physics | A | LIVE | |
| 4.47 | `qualityGates.semanticForces` | bool | quality_gates | CC.Physics | B | LIVE | side-effect: `configureSemanticForces()` |
| 4.48 | `qualityGates.dagLevelAttraction` | 0-20 | quality_gates | CC.Physics | A | LIVE | |
| 4.49 | `qualityGates.dagSiblingRepulsion` | 0-20 | quality_gates | CC.Physics | A | LIVE | |
| 4.50 | `qualityGates.typeClusterAttraction` | 0-20 | quality_gates | CC.Physics | A | LIVE | |
| 4.51 | `qualityGates.typeClusterRadius` | 10-5000 | quality_gates | CC.Physics | A | LIVE | |
| 4.52 | `…tweening.enabled` | bool | localStorage | CC.Physics | B | CLIENT-ONLY | dispatched as custom event to worker |
| 4.53 | `…tweening.lerpBase` | 1e-4..0.15 | localStorage | CC.Physics | B | CLIENT-ONLY | |
| 4.54 | `…tweening.maxDivergence` | 1-100 | localStorage | CC.Physics | B | CLIENT-ONLY | |
| 4.55 | `…tweening.snapThreshold` | 0.01-1.0 | localStorage | CC.Physics | A | CLIENT-ONLY | |
| 4.56 | `physics` (root, deprecated) | struct | `app_settings.physics` | – | – | DEPRECATED | prefer nested under visualisation |
| 4.57 | `dev_config.physics.force_epsilon` | f32 | data/dev_config.toml | TOML | DEV | SERVER-ONLY | 1e-8 |
| 4.58 | `dev_config.physics.spring_length_multiplier` | f32 | TOML | TOML | DEV | SERVER-ONLY | 5.0 |
| 4.59 | `dev_config.physics.anomaly_detection_radius` | f32 | TOML | TOML | DEV | SERVER-ONLY | 150 |
| 4.60 | `dev_config.physics.cross_graph_repulsion_scale` | f32 | TOML | TOML | DEV | SERVER-ONLY | 0.3 |
| 4.61 | `dev_config.physics.stress_majorization_epsilon` | f32 | TOML | TOML | DEV | SERVER-ONLY | 0.001 |

## 5 Quality / filter / clustering / analytics

| # | Path | Type | Server schema | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 5.1 | `nodeFilter.enabled` | bool | node_filter | CC.Quality | B | LIVE | broadcast settingsUpdated |
| 5.2 | `nodeFilter.filterByQuality` | bool | node_filter | CC.Quality | B | LIVE | |
| 5.3 | `nodeFilter.qualityThreshold` | 0-1 | node_filter | CC.Quality | B | LIVE | |
| 5.4 | `nodeFilter.filterByAuthority` | bool | node_filter | CC.Quality | B | LIVE | |
| 5.5 | `nodeFilter.authorityThreshold` | 0-1 | node_filter | CC.Quality | B | LIVE | |
| 5.6 | `nodeFilter.filterMode` | "or"/"and" | node_filter | CC.Quality | A | LIVE? | client sets, server-side honour unverified |
| 5.7 | refreshGraph action | – | WS `forceRefreshFilter` | CC.Quality + CC-AI | B | LIVE | hardcoded action |
| 5.8 | `qualityGates.gpuAcceleration` | bool | quality_gates | CC.Quality | B | LIVE | maps to compute_mode |
| 5.9 | `qualityGates.autoAdjust` | bool | quality_gates | CC.Quality | B | LIVE | |
| 5.10 | `qualityGates.minFpsThreshold` | 15-60 | quality_gates | CC.Quality | B | LIVE | |
| 5.11 | `qualityGates.maxNodeCount` | 1k-500k | quality_gates | CC.Quality | B | LIVE | |
| 5.12 | `qualityGates.showClusters` | bool | quality_gates | CC.Quality | B | LIVE | |
| 5.13 | `qualityGates.showAnomalies` | bool | quality_gates | CC.Quality | B | LIVE | |
| 5.14 | `qualityGates.showCommunities` | bool | quality_gates | CC.Quality | A | LIVE | Louvain |
| 5.15 | `qualityGates.gnnPhysics` | bool | quality_gates | CC.Quality | A,PU | LIVE | |
| 5.16 | `qualityGates.ruvectorEnabled` | bool | quality_gates | CC.Quality | A,PU | LIVE | HNSW |
| 5.17 | `constraints.lodEnabled` | bool | constraints | CC.Quality | A | LIVE | validates near<medium<far |
| 5.18 | `constraints.farThreshold` | 100-2000 | constraints | CC.Quality | A | LIVE | |
| 5.19 | `constraints.mediumThreshold` | 50-500 | constraints | CC.Quality | A | LIVE | |
| 5.20 | `constraints.nearThreshold` | 5-100 | constraints | CC.Quality | A | LIVE | |
| 5.21 | `constraints.progressiveActivation` | bool | constraints | CC.Quality | A | LIVE | |
| 5.22 | `constraints.activationFrames` | 1-600 | constraints | CC.Quality | A | LIVE | |
| 5.23 | `analytics.enableMetrics` | bool | (visual blob?) | CC.Analytics | B | CLIENT-ONLY? | unclear server source |
| 5.24 | `analytics.updateInterval` | 1-60s | – | CC.Analytics | B | CLIENT-ONLY? | |
| 5.25 | `analytics.showDegreeDistribution` | bool | – | CC.Analytics | A | CLIENT-ONLY? | |
| 5.26 | `analytics.showClusteringCoefficient` | bool | – | CC.Analytics | A | CLIENT-ONLY? | |
| 5.27 | `analytics.showCentrality` | bool | – | CC.Analytics | A | CLIENT-ONLY? | |
| 5.28 | `analytics.clustering.algorithm` | enum | `/api/clustering/algorithm` | CC.Analytics | A | LIVE | none/kmeans/spectral/louvain/dbscan; mismatch w/ batch endpoint `/configure,/start` |
| 5.29 | `analytics.clustering.clusterCount` | 2-20 | clustering | CC.Analytics | A | LIVE | |
| 5.30 | `analytics.clustering.resolution` | 0.1-2 | clustering | CC.Analytics | A | LIVE | |
| 5.31 | `analytics.clustering.iterations` | 10-100 | clustering | CC.Analytics | A | LIVE | |
| 5.32 | `analytics.clustering.exportEnabled` | bool | clustering | CC.Analytics | A,PU | LIVE | |
| 5.33 | `analytics.clustering.importEnabled` | bool | clustering | CC.Analytics | A,PU | LIVE | |
| 5.34 | `visualisation.clusterHulls.enabled` | bool | visual | CC.Analytics + CC-AI | B | LIVE | |
| 5.35 | `visualisation.clusterHulls.opacity` | 0.01-0.3 | visual | CC.Analytics | B | LIVE | |
| 5.36 | `visualisation.clusterHulls.padding` | 0-0.5 | visual | CC.Analytics | A | LIVE | |
| 5.37 | per-user filter | NodeFilterSettings | `/api/user/filter` | API only | B | LIVE | pubkey-keyed; no UI |
| 5.38 | settings profiles | obj | `/api/settings/profiles*` | API only | – | STUB | always returns empty/dummy |

## 6 System / network / debug

| # | Path | Type | Server schema | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 6.1 | `system.persistSettings` | bool | localStorage | CC.System | B | CLIENT-ONLY | controls autoSaveManager.syncEnabled |
| 6.2 | `system.customBackendUrl` | string | localStorage | CC.System | A,PU | CLIENT-ONLY | also read by websocketStore |
| 6.3 | `system.debug.enabled` | bool | system.debug | CC.System + CC.Developer | A | DUPLICATE | same path two surfaces |
| 6.4 | `system.debug.enableDataDebug` | bool | system.debug | CC.Developer | A,PU | LIVE | |
| 6.5 | `system.debug.enableWebsocketDebug` | bool | system.debug | CC.Developer | A,PU | LIVE | |
| 6.6 | `system.debug.logBinaryHeaders` | bool | system.debug | CC.Developer | A,PU | LIVE | |
| 6.7 | `system.debug.logFullJson` | bool | system.debug | CC.Developer | A,PU | LIVE | |
| 6.8 | `system.debug.enablePhysicsDebug` | bool | system.debug | CC.Developer | A,PU | LIVE | |
| 6.9 | `system.debug.enableNodeDebug` | bool | system.debug | CC.Developer | A,PU | LIVE | |
| 6.10 | `system.debug.enableShaderDebug` | bool | system.debug | CC.Developer | A,PU | LIVE | |
| 6.11 | `system.debug.enableMatrixDebug` | bool | system.debug | CC.Developer | A,PU | LIVE | |
| 6.12 | `system.debug.enablePerformanceDebug` | bool | system.debug | CC.Developer | A,PU | LIVE | |
| 6.13 | `system.debug.enableVerboseLogging` | bool | system.debug | – | – | SERVER-ONLY | no UI binding |
| 6.14 | `developer.gpu.showForceVectors` | bool | developer_config | CC.Developer | A,PU | LIVE | |
| 6.15 | `developer.gpu.showConstraints` | bool | developer_config | CC.Developer | A,PU | LIVE | |
| 6.16 | `developer.gpu.showBoundaryForces` | bool | developer_config | CC.Developer | A,PU | LIVE | |
| 6.17 | `developer.gpu.showConvergenceGraph` | bool | developer_config | CC.Developer | A,PU | LIVE | |
| 6.18 | `system.network.bindAddress` | string | system.network | – | DEV | SERVER-ONLY | "0.0.0.0" |
| 6.19 | `system.network.port` | u16 | system.network | – | DEV | SERVER-ONLY | 8080 |
| 6.20 | `system.network.enableHttp2` | bool | system.network | – | DEV | SERVER-ONLY | |
| 6.21 | `system.network.enableRateLimiting` | bool | system.network | – | DEV | SERVER-ONLY | |
| 6.22 | `system.network.maxRequestSize` | usize | system.network | – | DEV | SERVER-ONLY | 10 MB |
| 6.23 | `system.network.apiClientTimeout` | u64 | system.network | – | DEV | SERVER-ONLY | 30 s |
| 6.24 | `system.network.maxConcurrentRequests` | u32 | system.network | – | DEV | SERVER-ONLY | 1000 |
| 6.25 | `system.websocket.maxConnections` | usize | system.websocket | – | DEV | SERVER-ONLY | 1000 |
| 6.26 | `system.websocket.maxMessageSize` | usize | system.websocket | – | DEV | SERVER-ONLY | 65536 |
| 6.27 | `system.websocket.heartbeatInterval` | u64 | system.websocket | – | DEV | SERVER-ONLY | 30 s |
| 6.28 | `system.websocket.compressionEnabled` | bool | system.websocket | – | DEV | SERVER-ONLY | |
| 6.29 | `system.security.enableTls` | bool | system.security | – | DEV | SERVER-ONLY | |
| 6.30 | `dev_config.network.ws_ping_interval_secs` | u64 | TOML | TOML | DEV | SERVER-ONLY | 30 — affects user-visible reconnect cadence |
| 6.31 | `dev_config.network.ws_pong_timeout_secs` | u64 | TOML | TOML | DEV | SERVER-ONLY | 10 |
| 6.32 | `dev_config.network.max_retry_attempts` | u32 | TOML | TOML | DEV | SERVER-ONLY | 3 |
| 6.33 | `dev_config.network.retry_base_delay_ms` | u64 | TOML | TOML | DEV | SERVER-ONLY | 100 |
| 6.34 | `dev_config.cuda.warmup_iterations_default` | u32 | TOML | TOML | DEV | SERVER-ONLY | 200 |
| 6.35 | `dev_config.cuda.max_kernel_time_ms` | u32 | TOML | TOML | DEV | SERVER-ONLY | 5000 |
| 6.36 | `dev_config.cuda.max_nodes` | u32 | TOML | TOML | DEV | SERVER-ONLY | 1M |
| 6.37 | `dev_config.cuda.max_edges` | u32 | TOML | TOML | DEV | SERVER-ONLY | 10M |
| 6.38 | `dev_config.rendering.agent_colors` | map<str,hex> | TOML | TOML | DEV | SERVER-ONLY | 10 agent types |
| 6.39 | `dev_config.rendering.agent_base_size` | f32 | TOML | TOML | DEV | SERVER-ONLY | |
| 6.40 | `dev_config.rendering.lod_distance_high` | f32 | TOML | TOML | DEV | SERVER-ONLY | |
| 6.41 | `dev_config.performance.batch_size_nodes` | usize | TOML | TOML | DEV | SERVER-ONLY | 1000 |
| 6.42 | `dev_config.performance.cache_ttl_secs` | u64 | TOML | TOML | DEV | SERVER-ONLY | 300 |
| 6.43 | `dev_config.performance.worker_threads` | usize | TOML | TOML | DEV | SERVER-ONLY | 4 |
| 6.44 | `dev_config.debug.enable_cuda_debug` | bool | TOML | TOML | DEV | SERVER-ONLY | |
| 6.45 | `dev_config.debug.enable_physics_debug` | bool | TOML | TOML | DEV | SERVER-ONLY | DUPLICATE w/ system.debug.enablePhysicsDebug |
| 6.46 | `dev_config.debug.enable_network_debug` | bool | TOML | TOML | DEV | SERVER-ONLY | |
| 6.47 | `dev_config.debug.log_slow_operations_ms` | u64 | TOML | TOML | DEV | SERVER-ONLY | 100 |

## 7 XR

| # | Path | Type | Server schema | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 7.1 | `xr.enabled` | bool | xr | CC.XR | A | CLIENT-ONLY | UI updates store; no server PUT exists for XR |
| 7.2 | `xr.quality` | Low/Med/High | xr | CC.XR | A | CLIENT-ONLY | |
| 7.3 | `xr.renderScale` | 0.5-2 | xr | CC.XR | A | CLIENT-ONLY | |
| 7.4 | `xr.enableHandTracking` | bool | xr.hand_tracking_enabled | CC.XR | A | CLIENT-ONLY | |
| 7.5 | `xr.enableHaptics` | bool | xr.haptic_enabled | CC.XR | A | CLIENT-ONLY | |
| 7.6 | `xr.gpu.enableOptimizedCompute` | bool | xr | CC.XR | A,PU | CLIENT-ONLY | |
| 7.7 | `xr.performance.preset` | enum | xr | CC.XR | A,PU | CLIENT-ONLY | Battery/Balanced/Performance |
| 7.8 | `xr.enableAdaptiveQuality` | bool | xr | CC.XR | A,PU | CLIENT-ONLY | |
| 7.9 | `xr.gestureSmoothing` | 0-1 | xr | – | – | SERVER-ONLY | default 0.7, no UI |
| 7.10 | `xr.teleportRayColor` | hex | xr | – | – | SERVER-ONLY | empty default, unvalidated |
| 7.11 | `xr.controllerRayColor` | hex | xr | – | – | SERVER-ONLY | |
| 7.12 | `xr.movementSpeed` | f32 | xr | – | – | SERVER-ONLY | |
| 7.13 | `xr.deadZone` | 0-1 | xr | – | – | SERVER-ONLY | |
| 7.14 | `xr.planeDetectionEnabled` | bool | xr | – | – | SERVER-ONLY | |
| 7.15 | `xr.passthroughEnabled` | bool | xr | – | – | SERVER-ONLY | |
| 7.16 | `xr.passthroughOpacity` | 0-1 | xr | – | – | SERVER-ONLY | |
| 7.17 | XR remaining fields (~30) | various | xr | – | – | SERVER-ONLY | unvalidated, no UI |

## 8 AI / Voice / Integrations

| # | Path | Type | Server schema | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 8.1 | `ragflow.apiBaseUrl` | url | ragflow | CC.AI | A,PU | LIVE | |
| 8.2 | `ragflow.agentId` | string | ragflow | CC.AI | A,PU | LIVE | |
| 8.3 | `ragflow.timeout` | 5000-120000 | ragflow | CC.AI | A,PU | LIVE | |
| 8.4 | `perplexity.model` | string | perplexity | CC.AI | A,PU | LIVE | |
| 8.5 | `perplexity.maxTokens` | 100-4096 | perplexity | CC.AI | A,PU | LIVE | |
| 8.6 | `perplexity.temperature` | 0-2 | perplexity | CC.AI | A,PU | LIVE | |
| 8.7 | `perplexity.frequencyPenalty` | f32 | env only (`PERPLEXITY_FREQUENCY_PENALTY`) | ENV | DEV | SERVER-ONLY | undocumented |
| 8.8 | `perplexity.presencePenalty` | f32 | env | ENV | DEV | SERVER-ONLY | |
| 8.9 | `perplexity.topP` | f32 | env | ENV | DEV | SERVER-ONLY | |
| 8.10 | `openai.baseUrl` | url | openai | CC.AI | A,PU | LIVE | |
| 8.11 | `openai.timeout` | 5000-120000 | openai | CC.AI | A,PU | LIVE | |
| 8.12 | `openai.rateLimit` | u32 | env | ENV | DEV | SERVER-ONLY | undocumented |
| 8.13 | `openai.orgId` | string | env | ENV | DEV | SERVER-ONLY | |
| 8.14 | `kokoro.apiUrl` | url | kokoro | CC.AI | A,PU | LIVE | TTS |
| 8.15 | `kokoro.defaultVoice` | string | kokoro | CC.AI | A,PU | LIVE | |
| 8.16 | `kokoro.defaultSpeed` | 0.5-2 | kokoro | CC.AI | A,PU | LIVE | |
| 8.17 | `whisper.apiUrl` | url | whisper | CC.AI | A,PU | LIVE | STT |
| 8.18 | `whisper.defaultModel` | string | whisper | CC.AI | A,PU | LIVE | |
| 8.19 | `whisper.defaultLanguage` | string | whisper | CC.AI | A,PU | LIVE | |
| 8.20 | `voice_routing.*` | nested | voice_routing | – | – | SERVER-ONLY | provider routing logic |
| 8.21 | `ontology_agent.*` | nested | ontology_agent | – | – | SERVER-ONLY | agent config |
| 8.22 | DEEPSEEK_API_KEY | string | env | ENV | DEV | LIVE | ADR-026 routing |
| 8.23 | DEEPSEEK_BASE_URL | url | env | ENV | DEV | LIVE | |
| 8.24 | OLLAMA_BASE_URL/_MODEL | – | agentbox compose | AB | DEV | UNDOCUMENTED in root .env.example |
| 8.25 | LOCAL_LLM_HOST/_PORT | – | agentbox | AB | DEV | UNDOCUMENTED |
| 8.26 | COMFYUI_API_ENDPOINT/_LOCAL_ENDPOINT | – | agentbox | AB | DEV | UNDOCUMENTED |
| 8.27 | NL query translate | flow | `/api/nl-query/translate` | API only | – | LIVE but UNUSED | UI does not call |
| 8.28 | NL query explain | flow | `/api/nl-query/explain` | API only | – | LIVE but UNUSED | |
| 8.29 | NL query validate | flow | `/api/nl-query/validate` | API only | – | LIVE but UNUSED | |
| 8.30 | NL query examples | flow | `/api/nl-query/examples` | API only | – | LIVE but UNUSED | |

## 9 GitHub / Knowledge-graph sync

| # | Path | Type | Source-of-truth | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 9.1 | GITHUB_TOKEN | string | env | ENV | – | LIVE | repo auth |
| 9.2 | GITHUB_OWNER | string | env | ENV | – | LIVE | |
| 9.3 | GITHUB_REPO | string | env | ENV | – | LIVE | |
| 9.4 | GITHUB_BRANCH | string | env | ENV | – | LIVE | |
| 9.5 | GITHUB_BASE_PATH | path | env | ENV | – | LIVE | "mainKnowledgeGraph/pages" |
| 9.6 | FORCE_FULL_SYNC | bool | env | ENV | DEV | LIVE | bypasses SHA1 incremental |
| 9.7 | sync trigger | action | `/api/admin/sync` (inferred) | – | – | SERVER-ONLY | not in CC |

## 10 Camera / SpacePilot / user_preferences

| # | Path | Type | Server schema | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 10.1 | `visualisation.camera.fov` | f32 | camera | – | – | SERVER-ONLY | no UI |
| 10.2 | `visualisation.camera.near` | f32 | camera | – | – | SERVER-ONLY | |
| 10.3 | `visualisation.camera.far` | f32 | camera | – | – | SERVER-ONLY | |
| 10.4 | `visualisation.camera.positionPresets` | array | camera | – | – | SERVER-ONLY | |
| 10.5 | `visualisation.spacepilot.sensitivity` | f32 | spacepilot | – | – | SERVER-ONLY | UI in SpacePilotStatus only shows status |
| 10.6 | `visualisation.spacepilot.invertX/Y/Z` | bool | spacepilot | – | – | SERVER-ONLY | |
| 10.7 | `visualisation.spacepilot.acceleration` | f32 | spacepilot | – | – | SERVER-ONLY | |
| 10.8 | `user_preferences.theme` | string | user_preferences | – | – | SERVER-ONLY | no UI |
| 10.9 | `user_preferences.language` | string | user_preferences | – | – | SERVER-ONLY | |
| 10.10 | `user_preferences.comfort.*` | nested | user_preferences | – | – | SERVER-ONLY | XR comfort settings |

## 11 Enterprise control surface

| # | Element | Type | Source-of-truth | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 11.1 | active panel selector | enum | local React state | ENT | B | LIVE | broker/workflows/kpi/connectors/policy |
| 11.2 | drawer open/closed | bool | localStorage `visionflow.enterprise.drawer` | ENT | B | LIVE | |
| 11.3 | drawer activeSection | string | same | ENT | B | LIVE | |
| 11.4 | broker case submit (title, description, priority) | form | BrokerActor -> `PublishActionRequest` (kind 31402) -> Nostr relay | ENT.Broker | B | WIRED (backend) | Server signs and broadcasts to relay; UI subscription pending |
| 11.5 | broker inbox load | Nostr subscription (kind 31402) + REST fallback | Relay + BrokerActor | ENT.Broker | B | WIRED (backend) | Relay stores NIP-33 events; REST polling fallback |
| 11.6 | broker panel definition | Nostr event (kind 31400) | BrokerActor -> `PublishGovernancePanel` -> Nostr relay | ENT.Broker | B | WIRED (backend) | Panel schema defines inbox layout, fields, actions |
| 11.7 | workflow proposals create | POST `/api/workflows/proposals` | – | ENT.Workflows | B | STUB | |
| 11.8 | workflow proposals promote | POST `/api/workflows/proposals/{id}/promote` | – | ENT.Workflows | B | STUB | |
| 11.9 | workflow proposals/patterns list | GET | – | ENT.Workflows | B | STUB | mock |
| 11.10 | KPI time window | enum | URL param to GET `/api/mesh-metrics?timeWindow=` | ENT.KPI | B | LIVE | 24h/7d/30d/90d |
| 11.11 | KPI sparklines | local generator | seeded by kpiKey | ENT.KPI | B | LIVE | not real data |
| 11.12 | connector setup org/repos/redaction | form | POST `/api/connectors` | ENT.Connectors | B | LIVE | |
| 11.13 | connector list | GET `/api/connectors` | – | ENT.Connectors | B | LIVE | |
| 11.14 | connector signals | GET `/api/connectors/{id}/signals` (implied) | – | ENT.Connectors | B | PARTIAL | mock |
| 11.15 | policy rule enable | toggle | local only (DEFAULT_RULES hardcoded) | ENT.Policy | B | CLIENT-ONLY | no backend |
| 11.16 | policy test action | enum | local | ENT.Policy | B | CLIENT-ONLY | |
| 11.17 | policy test confidence | 0-100 slider | local | ENT.Policy | B | CLIENT-ONLY | |
| 11.18 | policy run-test button | action | `evaluateLocally()` | ENT.Policy | B | CLIENT-ONLY | |
| 11.19 | drawer_fx WASM (flow-field) | WASM canvas | crate `drawer_fx` | ENT (drawer overlay) | – | LIVE | quality 0/1/2; reduced-motion fallback |

## 12 Contributor Studio (day-to-day user surface)

| # | Element | Type | Source-of-truth | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 12.1 | active workspaceId | string | studioWorkspaceStore | STD | B | LIVE | |
| 12.2 | workspace list | array | GET `/api/studio/workspaces` | STD | B | STUB | no-op fetch (agent C1) |
| 12.3 | workspace create wizard | form | POST `/api/studio/workspaces` | STD | B | STUB | |
| 12.4 | pane left/right widths | px | studioWorkspaceStore.layout | STD | B | CLIENT-ONLY | persists locally; backend stub |
| 12.5 | partner selection | string | studioWorkspaceStore | STD | B | STUB | POST `/api/studio/partner/select` |
| 12.6 | inbox unread count | int | studioInboxStore | STD | B | STUB | |
| 12.7 | nudges (terms/concepts/policies) | array | senseiStore | STD | B | STUB | |
| 12.8 | installed skills | array | studioWorkspaceStore | STD | B | STUB | |
| 12.9 | chat transcript | array | studioPartnerStore | STD | B | LIVE (WS) | |
| 12.10 | automation create wizard | form | POST `/api/studio/automations` | STD | B | STUB | agent C5 |
| 12.11 | command palette (15 commands) | registry | useStudioCommands | STD | B | LIVE | dispatches CustomEvents |
| 12.12 | onboarding overlay | wizard | local | (disabled) | – | DEAD | OnboardingProvider commented out |
| 12.13 | completed flows | array | localStorage `onboarding.completedFlows` | – | – | DEAD | no flows defined |

## 13 Health / Monitoring

| # | Element | Type | Source-of-truth | Surface | Tier | Status | Notes |
|---|---|---|---|---|---|---|---|
| 13.1 | overall health | poll 5 s | GET `/health` | HEALTH | B | LIVE | |
| 13.2 | component health grid | poll 5 s | GET `/health` components | HEALTH | B | LIVE | db/graph/physics/websocket |
| 13.3 | physics simulation health | poll 5 s | GET `/health/physics` | HEALTH | B | LIVE | |
| 13.4 | start MCP relay | action | POST `/health/mcp/start` | HEALTH | A | LIVE | |
| 13.5 | view MCP logs | action | GET `/health/mcp/logs` | HEALTH | A | LIVE | |
| 13.6 | refresh button | action | parallel re-fetch | HEALTH | B | LIVE | |

## 14 Server-only knobs that should consider exposure

| # | Path | Type | Source | Effect | Status |
|---|---|---|---|---|---|
| 14.1 | `feature_flags.gpu_clustering` | bool | server | gates GPU clustering algo | SERVER-ONLY |
| 14.2 | `feature_flags.ontology_validation` | bool | server | schema validation on mutations | SERVER-ONLY |
| 14.3 | `feature_flags.gpu_anomaly_detection` | bool | server | GPU outlier detection | SERVER-ONLY |
| 14.4 | `feature_flags.real_time_insights` | bool | server | real-time metric agg | SERVER-ONLY |
| 14.5 | `feature_flags.advanced_visualizations` | bool | server | high-fidelity rendering mode | SERVER-ONLY |
| 14.6 | `feature_flags.performance_monitoring` | bool | server | detailed perf profiling | SERVER-ONLY |
| 14.7 | `feature_flags.stress_majorization` | bool | server | stress-min layout | SERVER-ONLY |
| 14.8 | `feature_flags.semantic_constraints` | bool | server | semantic relationship constraints | SERVER-ONLY |
| 14.9 | `feature_flags.sssp_integration` | bool | server | SSSP distance metrics | SERVER-ONLY |
| 14.10 | `developer_config.debug_mode` | bool | server | DUPLICATE of system.debug.enabled | SERVER-ONLY |
| 14.11 | `developer_config.show_performance_stats` | bool | server | metrics in responses | SERVER-ONLY |
| 14.12 | `developer_config.enable_profiling` | bool | server | CPU/mem profiling | SERVER-ONLY |
| 14.13 | `developer_config.verbose_logging` | bool | server | DEBUG logs | SERVER-ONLY |
| 14.14 | `developer_config.dev_tools_enabled` | bool | server | dev API endpoints | SERVER-ONLY |
| 14.15 | CANONICAL_MAX_VELOCITY | 200 | `src/config/mod.rs` | physics responsiveness cap | HARDCODED |
| 14.16 | CANONICAL_MAX_FORCE | 50 | same | physics acceleration feel | HARDCODED |

## 15 Agentbox / federation knobs

| # | Path | Type | Source | Surface | Tier | Status |
|---|---|---|---|---|---|---|
| 15.1 | federation mode | "client"/"standalone" | agentbox.toml `[federation]` | AB | – | LIVE |
| 15.2 | adapter beads | "visionclaw"/"local-sqlite"/"off" | agentbox.toml | AB | – | LIVE |
| 15.3 | adapter pods | "visionclaw"/"local-jss"/"off" | agentbox.toml | AB | – | LIVE |
| 15.4 | adapter memory | "external-pg"/"embedded" | agentbox.toml | AB | – | LIVE |
| 15.5 | adapter events | "visionclaw" | agentbox.toml | AB | – | LIVE |
| 15.6 | adapter orchestrator | "stdio-bridge" | agentbox.toml | AB | – | LIVE |
| 15.7 | toolchains.cuda enabled | bool | agentbox.toml | AB | – | LIVE | +25 GB image |
| 15.8 | toolchains.code_server | bool | agentbox.toml | AB | – | OPTIONAL |
| 15.9 | toolchains.ctm | bool | agentbox.toml | AB | – | OPTIONAL |
| 15.10 | toolchains.blender | bool | agentbox.toml | AB | – | OPTIONAL |
| 15.11 | toolchains.tex | bool | agentbox.toml | AB | – | OPTIONAL |
| 15.12 | sovereign_mesh enabled | bool | agentbox.toml | AB | – | LIVE |
| 15.13 | telegram_mirror | bool | agentbox.toml | AB | – | OPTIONAL |
| 15.14 | publish_agent_events | bool | agentbox.toml | AB | – | OPTIONAL |
| 15.15 | RUVECTOR_PG_CONNINFO | conninfo | docker-compose.override.yml | ENV | DEV | LIVE; hardcoded |
| 15.16 | MANAGEMENT_API_AUTH_MODE | "hybrid"/"nip98"/"none" | agentbox compose | ENV | DEV | LIVE; values undocumented |
| 15.17 | MANAGEMENT_API_HOST | string | env | ENV | DEV | LIVE | 9090 (MAD) / 9190 (agentbox side-by-side) |
| 15.18 | MANAGEMENT_API_PORT | u16 | env | ENV | DEV | LIVE | |
| 15.19 | MCP_HOST/_TCP_PORT/_TRANSPORT | – | env | ENV | DEV | LIVE | |
| 15.20 | MCP_RECONNECT_ATTEMPTS/DELAY/CONNECTION_TIMEOUT | u32/ms | env | ENV | DEV | LIVE | |
| 15.21 | MCP_RELAY_FALLBACK_TO_MOCK | bool | env | ENV | DEV | LIVE | |
| 15.22 | ORCHESTRATOR_WS_URL | url | env | ENV | DEV | LIVE | |
| 15.23 | BOTS_ORCHESTRATOR_URL | url | env | ENV | DEV | LIVE | MAD-compat |

## 16 Database / persistence

| # | Path | Type | Source | Surface | Status |
|---|---|---|---|---|---|
| 16.1 | NEO4J_URI | url | env | ENV | LIVE |
| 16.2 | NEO4J_USER | string | env | ENV | LIVE |
| 16.3 | NEO4J_PASSWORD | string | env | ENV | LIVE; required, no default |
| 16.4 | NEO4J_DATABASE | string | env | ENV | LIVE |
| 16.5 | settings persistence (in-mem actor) | struct | OptimizedSettingsActor | – | LIVE |
| 16.6 | settings persistence (Neo4j) | struct | Neo4jSettingsRepository | – | LIVE |
| 16.7 | settings yaml | – | – | – | DEPRECATED (removed) |
| 16.8 | audit trail | – | – | – | MISSING (no persistent audit log) |

## 17 Build / runtime / observability

| # | Path | Type | Source | Surface | Status |
|---|---|---|---|---|---|
| 17.1 | CUDA_ARCH | string | env / build arg | ENV | LIVE |
| 17.2 | BUILD_TARGET | dev/prod | env / build arg | ENV | LIVE |
| 17.3 | NODEJS_VERSION | u8 | Dockerfile pinned | – | HARDCODED |
| 17.4 | RUST_LOG | string | env | ENV | LIVE |
| 17.5 | RUST_LOG_REDIRECT | bool | env | ENV | LIVE |
| 17.6 | DEBUG_ENABLED | bool | env | ENV | LIVE |
| 17.7 | VITE_DEV_SERVER_PORT | u16 | env | ENV | LIVE |
| 17.8 | VITE_API_PORT | u16 | env | ENV | LIVE |
| 17.9 | VITE_HMR_PORT | u16 | env | ENV | LIVE |
| 17.10 | SYSTEM_NETWORK_PORT | u16 | env | ENV | LIVE |
| 17.11 | TELEMETRY_ENABLED | bool | env | ENV | LIVE |
| 17.12 | TELEMETRY_METRICS_INTERVAL | ms | env | ENV | LIVE |
| 17.13 | SESSION_SECRET | string | env | ENV | LIVE; required to rotate in prod |
| 17.14 | SESSION_TIMEOUT | seconds | env | ENV | LIVE |
| 17.15 | SOLID_PROXY_SECRET_KEY | string | env | ENV | LIVE |
| 17.16 | VISIONFLOW_AGENT_KEY | string | env | ENV | DEPRECATED (use Nostr per ADR-040) |
| 17.17 | POD_NAME | string | computed | – | LIVE |
| 17.18 | CORS_ALLOWED_ORIGINS/_METHODS/_HEADERS | csv | env | ENV | LIVE |
| 17.19 | CLOUDFLARE_TUNNEL_TOKEN | string | env | ENV | LIVE; not in .env templates |
| 17.20 | VISIONCLAW_NOSTR_PRIVKEY | hex | env | ENV | LIVE |
| 17.21 | SERVER_NOSTR_PRIVKEY | hex | env | ENV | LIVE; falls back to VISIONCLAW |
| 17.22 | SERVER_NOSTR_AUTO_GENERATE | bool | env | ENV | LIVE; dev-only |
| 17.23 | FORUM_RELAY_URL | url | env | ENV | LIVE |
| 17.24 | NVIDIA_DRIVER_CAPABILITIES | string | compose | ENV | LIVE; multi-GPU strategy undocumented |
| 17.25 | NVIDIA_VISIBLE_DEVICES | string | compose | ENV | LIVE |

## 18 Cross-cutting disconnects (collected)

1. **CC-AI parser shallow** — keyword regex only; ignores live `/api/nl-query/*` Cypher handlers (8.27-8.30).
2. **Layout duplication** — `qualityGates.layoutMode` (4.42) vs `…physics.layoutAlgorithm` (4.41) overlap with different vocabularies.
3. **Debug duplication** — `system.debug.enabled` (6.3) appears in both System and Developer tabs; also duplicated server-side at `developer_config.debug_mode` (14.10).
4. **Tweening client-only** — 4.52-4.55 never reach the server, dispatched as custom events to the worker; if multi-user collaboration becomes a goal these must be syncable.
5. **Visual settings unvalidated** — server accepts any blob at `/api/settings/visual`; no schema check.
6. **Settings profiles all stub** — 5.38; routes return dummy.
7. **XR client-only** — 7.1-7.8 update store but never PUT; server-side `xr.*` exists but is unreachable.
8. **No WS settings push** — server broadcasts `settingsUpdated` only for rendering and nodeFilter; physics goes via Actix actor not WS; no other categories pushed.
9. **Power-user denial mid-session** — UI cache not cleared if server demotes user.
10. **Generated-types drift** — `client/src/types/generated/settings.ts` lacks many qualityGates / semantic-forces fields.
11. **Action-button registry hardcoded** — `refresh_graph`, `toggle-webgpu` not extensible.
12. **Route duplication** — `/api/settings` mounted twice (`webxr::settings::api` AND `settings_handler::routes`); confusion / shadowing.
13. **Auth fallthrough is 401** — no rate-limited anonymous read fallback.
14. **No audit trail** in Neo4j; only log lines.
15. **field_mappings gaps** — quality-gates and semantic-forces lack snake_case aliases.
16. **enterprise broker / workflows** — endpoints 11.4-11.9 are scaffolds.
17. **studio backend** — 12.2-12.10 mostly stubs.
18. **policy console local-only** — 11.15-11.18 hardcoded DEFAULT_RULES, never persisted.
19. **onboarding dead** — 12.12-12.13 commented out, no flows defined.
20. **OIDC dead** — 1.21 server config exists, client never calls.
21. **Vircadia auth** — 1.19-1.20 likely orphaned.
22. **legacy session keys** — 1.13 legacy sessionStorage `nostr_privkey` still removed on init defensively.
23. **MAD/agentbox env divergence** — 8.24-8.26 + 15.16 documented in agentbox compose only, missing from root `.env.example`.
24. **Secrets rotation missing** — 1.10, 17.13, 17.15, 17.20, 17.21 — no rotation/escrow policy.
25. **AutoBalance internals** — 4.4 has 40+ tunables exposed nowhere; black box.
26. **AutoPause internals** — 4.5-4.10 server-only; UX consequence (graph "freezes") not user-controllable.
27. **dev_config tunables** — 6.30-6.47 affect user-visible behavior (reconnect cadence, retry budget) but are toml-only.
28. **canonical caps** — 14.15-14.16 only changeable by recompile.
29. **camera/spacepilot** — 10.1-10.7 no UI for camera/spacepilot tuning despite hardware presence.
30. **user_preferences subtree** — 10.8-10.10 server schema with no UI binding.

## Counts

- Client-Center settings (§§ 2-8 plus a few in §1, §6): **~205** leaf controls across 10 tabs.
- Server `AppFullSettings` root fields: **14**, with **40+** nested structs.
- Server-only / unexposed tunables: **70+** (counting dev_config + auto-balance + auto-pause + xr + camera + spacepilot + bloom + hologram details + feature_flags + developer_config).
- Env vars: **120+**.
- Agentbox manifest knobs: **15**.
- Compose orchestration knobs: **15**.
- HTTP settings routes: **24** (10 GET + 10 PUT + 4 POST).
- WS broadcast topics: **2** (rendering, nodeFilter).
- Stub endpoints: **8** (broker.*, workflows.*, studio.workspaces, studio.automations, studio.partner, settings/profiles, etc.).
- Dead/orphan: **6+** (OIDC, Vircadia auth, legacy nostr_privkey, onboarding overlay, drawer flow-field demo route, hologram pulse/scanLine/dataStream).
- Duplicate effects: **5** (layoutMode/layoutAlgorithm, debug.enabled in two tabs + developer_config.debug_mode, repulsionCutoff in physics+dev_config, gridCellSize same, max_velocity/max_force in physics+dev_config+canonical const).
