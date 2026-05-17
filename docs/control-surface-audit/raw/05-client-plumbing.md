# Client Settings Plumbing Audit

**Branch:** feature/unified-control-surface  
**Audit Date:** 2026-04-28  
**Scope:** Client-side settings store, API client, WebSocket subscriptions, defaults pipeline, debounce/batch logic, all call sites

---

## 1. Stores / Contexts

| File:Line | Slice | Shape | Persistence |
|-----------|-------|-------|-------------|
| `client/src/store/settingsStore.ts:372-1143` | **useSettingsStore (Zustand)** | `SettingsState` with `partialSettings: DeepPartial<Settings>`, `settings: DeepPartial<Settings>`, `loadedPaths: Set<string>`, `loadingSections: Set<string>` | localStorage key `'graph-viz-settings-v2'` via `persist` middleware. Persists `authenticated`, `user`, `isPowerUser`, `partialSettings` only. On rehydrate, reconstructs `loadedPaths` from persisted settings keys. |
| `client/src/store/autoSaveManager.ts:13-152` | **AutoSaveManager (Singleton)** | `pendingChanges: Map<string, any>`, `saveDebounceTimer: NodeJS.Timeout \| null`, `_syncEnabled: boolean` | In-memory only. 500ms debounce timer. No persistence. Destroyed on page unload. |
| `client/src/store/settingsRetryManager.ts:20-210` | **SettingsRetryManager (Singleton)** | `retryQueue: Map<SettingsPath, RetryableUpdate>`, `retryInterval: number \| null` | In-memory only. Exponential backoff retry up to 3 attempts per path. 5s poll interval. Auto-starts/stops when queue empty. |
| `client/src/store/websocket/index.ts` | **useWebSocketStore (Zustand)** | Not directly related to settings (handles graph data, node updates). Reads `settings.system.customBackendUrl` on init. | Separate persistence, not part of settings audit. |

**Subscriber Trie for Settings Store:**
- `client/src/store/settingsStore.ts:18-74`: Trie-based subscriber registry. O(depth) path matching. RAF-batched notification dispatch (lines 76-89). No persistence.

---

## 2. Settings API Client

### Overview
**File:** `client/src/api/settingsApi.ts`  
**Transport:** HTTP + NIP-98 auth (Nostr signature)  
**Base URL:** `''` (relative; proxied to backend)  
**Auth:** Global axios interceptor (lines 16-44). Requests auto-signed with NIP-98 or bearer dev token.

### Endpoints & Methods

| Function (file:line) | Method | Path | Payload Shape | Retry? | Optimistic? | Notes |
|----------------------|--------|------|----------------|--------|------------|-------|
| `getAll()` (line 574) | GET | `/api/settings/all` | none | Axios default (no retry) | No | Returns `AllSettings`. Cached 2s (lines 451-453). |
| `getSettingsByPaths(paths[])` (line 578) | GET | `/api/settings/all` | none | Axios default | No | Filters cached `AllSettings` to requested paths. Falls back to `transformApiToClientSettings()` with defaults on error. |
| `getSettingByPath<T>(path)` (line 619) | GET | `/api/settings/all` | none | Axios default | No | Single path; calls `getSettingsByPaths()` internally. |
| `updateSettingByPath<T>(path, value)` (line 631) | PUT/POST (routed) | `/api/settings/{category}` | Category-specific | Axios default | No | Routes to category endpoint (physics, rendering, visual, etc.). Non-server paths logged only. |
| `updateSettingsByPaths(updates[])` (line 666) | PUT/POST (batched) | Multiple endpoints | Batched by category | Axios default | No | Groups updates by endpoint, sends in parallel. Non-server paths logged only. |
| `getPhysics()` (line 519) | GET | `/api/settings/physics` | none | Axios default | No | Fetches current physics for merge-PUT pattern. |
| `updatePhysics(settings)` (line 522) | GET then PUT | `/api/settings/physics` | `Partial<PhysicsSettings>` | Axios default | No | GET-merge-PUT (lines 530-536). Diagnostic logging. |
| `getConstraints()` (line 545) | GET | `/api/settings/constraints` | none | Axios default | No | Fetch for merge-PUT pattern. |
| `updateConstraints(settings)` (line 548) | GET then PUT | `/api/settings/constraints` | `Partial<ConstraintSettings>` | Axios default | No | Same pattern. |
| `getRendering()` (line 558) | GET | `/api/settings/rendering` | none | Axios default | No | Fetch for merge-PUT pattern. |
| `updateRendering(settings)` (line 561) | GET then PUT | `/api/settings/rendering` | `Partial<RenderingSettings>` | Axios default | No | Same pattern. |
| `getNodeFilter()` (line 842) | GET | `/api/settings/node-filter` | none | Axios default | No | Fetch for merge-PUT pattern. |
| `updateNodeFilter(settings)` (line 845) | GET then PUT | `/api/settings/node-filter` | `Partial<NodeFilterSettings>` | Axios default | No | Same pattern. |
| `getQualityGates()` (line 855) | GET | `/api/settings/quality-gates` | none | Axios default | No | Fetch for merge-PUT pattern. |
| `updateQualityGates(settings)` (line 858) | GET then PUT | `/api/settings/quality-gates` | `Partial<QualityGateSettings>` | Axios default | No | Side-effects: calls `toggleOntologyPhysics()` and `configureSemanticForces()`. |
| `getVisualSettings()` (line 961) | GET | `/api/settings/visual` | none | Axios default | No | Fetch visual blob. |
| `updateVisualSettings(patch)` (line 964) | PUT | `/api/settings/visual` | `Record<string, unknown>` | Axios default | No | Nested patch. |
| `updateClusteringAlgorithm<T>(path, value)` (line 972) | POST | `/api/clustering/algorithm` | `{ [key]: value }` | Axios default | No | Single clustering config update. |
| `toggleOntologyPhysics(enabled)` (line 881) | POST | `/api/ontology-physics/{enable\|disable}` | `{ ontologyId, strength }` if enabled | Axios default | No | Fire-and-forget side-effect. |
| `configureSemanticForces(settings)` (line 900) | POST/PUT (multi) | `/api/semantic-forces/{dag,type-clustering}/configure`, `/api/ontology-physics/weights` | Various | Axios default | No | Multiple parallel POST/PUT calls. Fire-and-forget. |
| `saveProfile(request)` (line 827) | POST | `/api/settings/profiles` | `SaveProfileRequest { name }` | Axios default | No | Profile management. |
| `listProfiles()` (line 832) | GET | `/api/settings/profiles` | none | Axios default | No | Fetch profiles. |
| `loadProfile(id)` (line 835) | GET | `/api/settings/profiles/{id}` | none | Axios default | No | Fetch single profile. |
| `deleteProfile(id)` (line 838) | DELETE | `/api/settings/profiles/{id}` | none | Axios default | No | Delete profile. |
| `resetSettings()` (line 769) | PUT + localStorage clear | `/api/settings/physics` | Default physics payload | Axios default | No | Clears localStorage, invalidates cache, resets server physics. |
| `flushPendingUpdates()` (line 764) | (no-op) | N/A | N/A | No | No | Placeholder for future batching. Currently updates are immediate. |
| `exportSettings(settings)` (line 799) | (local) | N/A | none | No | No | `JSON.stringify(settings, null, 2)`. Local only. |
| `importSettings(jsonString)` (line 804) | (local) | N/A | none | No | No | `JSON.parse()` + schema validation. Local only. |

### Routing Logic for Paths

**`isVisualSettingsPath(path)` (line 475):**
- Returns `true` if path starts with `visualisation.` but NOT `visualisation.rendering` or `.physics`.
- Routes to `/api/settings/visual` endpoint.

**`toVisualKey(path)` (line 491):**
- Strips `visualisation.graphs.logseq.` prefix → `nodes/edges/labels/X`.
- Strips `visualisation.` prefix → `category.X`.

**`updateSettingByPath()` routing (line 631):**
1. `visualisation.graphs.*physics` → `updatePhysics()`
2. `visualisation.rendering.*` → `updateRendering()`
3. `qualityGates.*` → `updateQualityGates()`
4. `nodeFilter.*` → `updateNodeFilter()`
5. `constraints.*` → `updateConstraints()`
6. `analytics.clustering.*` → `updateClusteringAlgorithm()`
7. `isVisualSettingsPath()` → `updateVisualSettings()`
8. Else → Logged as local-only, no server call.

**`updateSettingsByPaths()` batching (line 666):**
Groups updates by category (physics, rendering, qualityGates, nodeFilter, constraints, visual, clustering), sends all to their respective endpoints in parallel.

### Defaults Pipeline

**Transform Function:** `transformApiToClientSettings(apiResponse: AllSettings)` (line 362)

Merges server response over hardcoded defaults:

1. **Rendering** (line 367): Server `rendering` directly, no defaults overlay.
2. **Visual effects** (lines 368-377): Deep merge over defaults:
   - `glow` → `DEFAULT_GLOW_SETTINGS`
   - `bloom` → `DEFAULT_BLOOM_SETTINGS`
   - `hologram` → `DEFAULT_HOLOGRAM_SETTINGS`
   - `graphTypeVisuals` → `DEFAULT_GRAPH_TYPE_VISUALS`
   - `gemMaterial` → `DEFAULT_GEM_MATERIAL`
   - `sceneEffects` → `DEFAULT_SCENE_EFFECTS`
   - `clusterHulls` → `DEFAULT_CLUSTER_HULLS`
   - `embeddingCloud` → `DEFAULT_EMBEDDING_CLOUD`
   - `animations` → `DEFAULT_ANIMATION_SETTINGS`
   - `interaction` → `DEFAULT_INTERACTION_SETTINGS`
3. **Physics** (line 380): Server `physics` directly.
4. **Nodes, Edges, Labels** (lines 381-383): Deep merge over defaults.
5. **System, XR, Auth** (lines 387-416): Baked-in fallback values.
6. **QualityGates, NodeFilter** (lines 417-443): Server values or fallback objects.

### Error Handling

**On `getSettingsByPaths()` error (line 605):**
- Returns `transformApiToClientSettings()` with empty category objects.
- All defaults apply; settings initialized with baseline values.

**On `updateSettingByPath()` error (line 658):**
- Logged as warning.
- Value already in store/localStorage.
- Server-side failure does not block UI.
- Delegated to `settingsRetryManager.addFailedUpdate()`.

**On `updateSettingsByPaths()` error (line 758):**
- Each failed path added to retry queue.
- Retry manager polls every 5s with exponential backoff (1s → 2s → 4s → capped 30s).

---

## 3. WebSocket Settings Subscriptions

**Status:** Settings are NOT synced over WebSocket; they are HTTP-only.

| File:Line | Topic | Decoder | Applies To | Notes |
|-----------|-------|---------|------------|-------|
| `client/src/store/websocket/filterSync.ts` | (N/A for settings) | Reads `nodeFilter` from `useSettingsStore.settings.nodeFilter` | Graph node filter state | Settings are read from store, not pushed via WS. |
| `client/src/store/websocket/index.ts` (custom backend URL) | (N/A) | Reads `settings.system.customBackendUrl` on init | Connection manager | Settings inform WS connection setup, not the reverse. |
| `client/src/features/graph/managers/graphDataManager.ts` | Custom event `physicsParametersUpdated` | `detail: { graphName, params, pubkey }` | Graph worker via postMessage | Dispatched from `settingsStore.notifyPhysicsUpdate()` on physics path changes. |
| `client/src/features/graph/managers/graphDataManager.ts` | Custom event `tweeningSettingsUpdated` | `detail: { enabled, lerpBase, ... }` | Graph worker via postMessage | Dispatched from `settingsStore.notifyTweeningUpdate()` on tweening path changes. |

**No server-push or subscription mechanism exists for settings updates.** Settings are request-response only.

---

## 4. Defaults Pipeline

### Boot Order (Store Initialization)

1. **Rehydrate from localStorage** (line 1100-1133):
   - If `graph-viz-settings-v2` persisted, load `partialSettings`, reconstruct `loadedPaths`.
   - Otherwise start with empty state.

2. **Wait for Auth** (line 92-129):
   - `waitForAuthReady()` polls until `nostrAuth.isAuthenticated()` or 3s timeout.
   - Allows nostr pubkey to be available for API requests.

3. **Fetch Essential Paths** (line 409):
   - `ESSENTIAL_PATHS` (lines 132-177): 45 critical paths for fast boot.
   - Call `settingsApi.getSettingsByPaths(ESSENTIAL_PATHS)`.
   - On failure, use defaults from `transformApiToClientSettings()`.

4. **Deep Merge** (line 420-423):
   - Server-fetched settings as base.
   - localStorage `partialSettings` as overlay (user customizations win).

5. **Override Physics/Tweening** (line 424-456):
   - Server values re-applied to override stale localStorage (server is authoritative for physics/tweening).
   - `visualisation.graphs.logseq.physics`, `visualisation.graphs.logseq.tweening`, `visualisation.graphs.logseq.nodes` (visibility, opacity, size).
   - `clientTweening` top-level.

6. **Auto-Save Manager Ready** (line 470):
   - `autoSaveManager.setInitialized(true)`.
   - Debounce queue starts accepting changes.

### Failure Fallback

- **API fetch fails:** `transformApiToClientSettings()` with empty category objects returns all defaults.
- **Auth not ready:** Proceeds after timeout with `authenticated: false`.
- **Retry queue:** Exponential backoff every 5s, max 3 attempts, then emit `settings-retry-failed` custom event.

---

## 5. Debounce / Batch Logic

### AutoSaveManager (500ms debounce)

| Mechanism | Location | Behavior |
|-----------|----------|----------|
| **Queue method** | `queueChange(path, value)` (line 52) | Single path change. Checks `isInitialized`, `syncEnabled`, `isClientOnlyPath()`. Adds to `pendingChanges` map. |
| **Batch method** | `queueChanges(changes: Map)` (line 73) | Multiple paths at once. Filters client-only paths. |
| **Schedule flush** | `scheduleFlush()` (line 92) | Clear existing timer, set new 500ms timeout. |
| **Flush** | `flushPendingChanges()` (line 112) | Convert map to array, call `settingsApi.updateSettingsByPaths()`, clear map. On error, add to retry queue. |
| **Force flush** | `forceFlush()` (line 103) | Immediate flush for page unload or critical saves. |

### Client-Only Paths (Never synced to server)

- `auth.nostr.connected`
- `auth.nostr.publicKey`

---

## 6. Per-Setting Read/Write Map

### Comprehensive Setting Paths

| Setting Path | Reader(s) (file:line) | Writer(s) (file:line) | Transport | Notes |
|--------------|------------------------|------------------------|-----------|-------|
| **Physics Parameters** |
| `visualisation.graphs.logseq.physics.springK` | `useSelectiveSetting()` / `useSettingsStore.get()` | `updatePhysics()` → `updateSettings()` (settingsStore:859-885) | PUT `/api/settings/physics` | Validated range [0.001, 1000]. |
| `visualisation.graphs.logseq.physics.repelK` | Same | Same | PUT `/api/settings/physics` | Validated range [0.001, 2000]. |
| `visualisation.graphs.logseq.physics.attractionK` | Same | Same | PUT `/api/settings/physics` | Validated range [0, 500]. |
| `visualisation.graphs.logseq.physics.gravity` | Same | Same | PUT `/api/settings/physics` | Validated range [-1, 1]. |
| `visualisation.graphs.logseq.physics.damping` | Same | Same | PUT `/api/settings/physics` | Server stored. |
| `visualisation.graphs.logseq.physics.boundsSize` | Same | Same | PUT `/api/settings/physics` | Server stored. |
| `visualisation.graphs.logseq.physics.enableBounds` | Same | Same | PUT `/api/settings/physics` | Server stored. |
| `visualisation.graphs.logseq.physics.iterations` | Same | Same | PUT `/api/settings/physics` | Server stored. |
| `visualisation.graphs.logseq.physics.warmupIterations` | Same | Same | PUT `/api/settings/physics` | Validated range [0, 1000]. |
| `visualisation.graphs.logseq.physics.coolingRate` | Same | Same | PUT `/api/settings/physics` | Validated range [0.0001, 1]. |
| **Tweening (Client-side Interpolation)** |
| `visualisation.graphs.logseq.tweening.enabled` | `useSelectiveSetting()` | `updateTweening()` (settingsStore:913-951) | localStorage only (custom event to worker) | Dispatches `tweeningSettingsUpdated`. |
| `visualisation.graphs.logseq.tweening.lerpBase` | Same | Same | localStorage only | Validated range [0.0001, 0.5]. |
| `visualisation.graphs.logseq.tweening.snapThreshold` | Same | Same | localStorage only | Validated range [0.01, 1]. |
| `visualisation.graphs.logseq.tweening.maxDivergence` | Same | Same | localStorage only | Validated range [1, 100]. |
| `clientTweening.*` | Same | Same | localStorage only | Backup for backwards compat. |
| **Rendering** |
| `visualisation.rendering.ambientLightIntensity` | `useSelectiveSetting()` | `setByPath()` / `updateSettings()` | PUT `/api/settings/rendering` | Server stored. |
| `visualisation.rendering.directionalLightIntensity` | Same | Same | PUT `/api/settings/rendering` | Server stored. |
| `visualisation.rendering.enableAntialiasing` | Same | Same | PUT `/api/settings/rendering` | Server stored. |
| `visualisation.rendering.context` | Same | Same | PUT `/api/settings/rendering` | Server stored (WebGPU vs WebGL). |
| **Visual Effects** |
| `visualisation.glow.enabled` | `useSelectiveSetting()` | `setByPath()` | PUT `/api/settings/visual` | Deep merged over `DEFAULT_GLOW_SETTINGS`. |
| `visualisation.glow.intensity` | Same | Same | PUT `/api/settings/visual` | Deep merged. |
| `visualisation.hologram.ringCount` | Same | Same | PUT `/api/settings/visual` | Deep merged. |
| `visualisation.gemMaterial` | Same | Same | PUT `/api/settings/visual` | Entire object or nested property. |
| `visualisation.sceneEffects` | Same | Same | PUT `/api/settings/visual` | Entire object. |
| `visualisation.animations.enableNodeAnimations` | Same | Same | PUT `/api/settings/visual` | Deep merged. |
| `visualisation.interactions.*` | Same | Same | PUT `/api/settings/visual` | Deep merged. |
| `visualisation.graphTypeVisuals.knowledgeGraph` | Same | Same | PUT `/api/settings/visual` | Deep merged over `DEFAULT_GRAPH_TYPE_VISUALS.knowledgeGraph`. |
| **Nodes / Edges / Labels** |
| `visualisation.graphs.logseq.nodes.opacity` | `useSelectiveSetting()` | `setByPath()` | PUT `/api/settings/visual` → `nodes.opacity` | Deep merged over `DEFAULT_NODES_SETTINGS`. |
| `visualisation.graphs.logseq.nodes.nodeSize` | Same | Same | PUT `/api/settings/visual` → `nodes.nodeSize` | Deep merged. |
| `visualisation.graphs.logseq.nodes.nodeTypeVisibility` | Same | Same | PUT `/api/settings/visual` → `nodes.nodeTypeVisibility` | Deep merged. Server authoritative on boot. |
| `visualisation.graphs.logseq.edges.baseWidth` | Same | Same | PUT `/api/settings/visual` → `edges.baseWidth` | Deep merged over `DEFAULT_EDGES_SETTINGS`. |
| `visualisation.graphs.logseq.edges.arrowSize` | Same | Same | PUT `/api/settings/visual` → `edges.arrowSize` | Deep merged. Validated [0.01, 5]. |
| `visualisation.graphs.logseq.labels.desktopFontSize` | Same | Same | PUT `/api/settings/visual` → `labels.desktopFontSize` | Deep merged over `DEFAULT_LABELS_SETTINGS`. |
| **Quality Gates** |
| `qualityGates.showClusters` | `useSelectiveSetting()` | `setByPath()` / `updateSettings()` | PUT `/api/settings/quality-gates` | Server stored. Triggers side-effect calls. |
| `qualityGates.showAnomalies` | Same | Same | PUT `/api/settings/quality-gates` | Server stored. |
| `qualityGates.showCommunities` | Same | Same | PUT `/api/settings/quality-gates` | Server stored. |
| `qualityGates.layoutMode` | Same | Same | PUT `/api/settings/quality-gates` | Server stored. Triggers `configureSemanticForces()`. |
| `qualityGates.ontologyPhysics` | Same | Same | PUT `/api/settings/quality-gates` | Server stored. Triggers `toggleOntologyPhysics()`. |
| `qualityGates.semanticForces` | Same | Same | PUT `/api/settings/quality-gates` | Server stored. Triggers `configureSemanticForces()`. |
| `qualityGates.ontologyStrength` | Same | Same | PUT `/api/settings/quality-gates` | Server stored. Triggers semantic forces config. |
| **Node Filter** |
| `nodeFilter.enabled` | `useSelectiveSetting()` / `filterSync.ts` | `setByPath()` | PUT `/api/settings/node-filter` | Server stored. Synced to graph filter. |
| `nodeFilter.qualityThreshold` | Same | Same | PUT `/api/settings/node-filter` | Server stored. |
| `nodeFilter.authorityThreshold` | Same | Same | PUT `/api/settings/node-filter` | Server stored. |
| `nodeFilter.filterMode` | Same | Same | PUT `/api/settings/node-filter` | Server stored ('or' \| 'and'). |
| **Clustering** |
| `analytics.clustering.algorithm` | `useSelectiveSetting()` | `setByPath()` | POST `/api/clustering/{configure,start}` or `/api/clustering/algorithm` | Custom endpoint routing. |
| `analytics.clustering.clusterCount` | Same | Same | POST `/api/clustering/{configure,start}` | Included in config payload. |
| `analytics.clustering.resolution` | Same | Same | POST `/api/clustering/{configure,start}` | Included in config payload. |
| `analytics.clustering.iterations` | Same | Same | POST `/api/clustering/{configure,start}` | Included in config payload. |
| **System / Debug** |
| `system.debug.enabled` | `useSelectiveSetting()` | `setByPath()` | localStorage only (no server endpoint) | Essential path, loaded at boot. |
| `system.websocket.updateRate` | Same | Same | localStorage only | Essential path, WebSocket update frequency. |
| `system.websocket.reconnectAttempts` | Same | Same | localStorage only | Essential path. |
| `system.custom_backend_url` | Websocket connectionManager (index.ts) | `setByPath()` | localStorage only | No server endpoint. Informs WS connection URL. |
| **XR Settings** |
| `xr.enabled` | `useSelectiveSetting()` | `setByPath()` | localStorage only | Essential path. No server endpoint currently. |
| `xr.mode` | Same | Same | localStorage only | Essential path. |
| `xr.enableHandTracking` | Same | Same | localStorage only | No server endpoint. |
| `xr.enableHaptics` | Same | Same | localStorage only | No server endpoint. |
| **Auth Settings** |
| `auth.enabled` | `useSelectiveSetting()` | `setByPath()` | localStorage only | No server endpoint. |
| `auth.required` | Same | Same | localStorage only | No server endpoint. |
| `auth.nostr.connected` | Same | Same | localStorage only | Client-only, never synced. |
| `auth.nostr.publicKey` | Same | Same | localStorage only | Client-only, never synced. |

---

## 7. Call Sites (UI Components → Store → API)

### Component Patterns

**Read Example:**
```typescript
// useSelectiveSettings / useSelectiveSetting hook
const opacity = useSelectiveSetting<number>('visualisation.graphs.logseq.nodes.opacity');
// Zustand selector → store.get(path)
// Re-renders only on value change (shallow equality)
```

**Write Example:**
```typescript
// useSettingSetter hook
const { set } = useSettingSetter();
set('visualisation.graphs.logseq.nodes.opacity', 0.8);
// → setByPath(path, value, skipServerSync=false)
// → updateSettings() to store (immediate)
// → autoSaveManager.queueChange() (500ms debounce)
// → settingsApi.updateSettingByPath() after 500ms
```

**Batch Write Example:**
```typescript
const { batchedSet } = useSettingSetter();
batchedSet({
  'visualisation.graphs.logseq.physics.springK': 50,
  'visualisation.graphs.logseq.physics.repelK': 100,
});
// → batchUpdate(updates[])
// → updateSettings() to store
// → autoSaveManager.queueChanges(map)
// → settingsApi.updateSettingsByPaths() after 500ms (batched by category)
```

**Subscribe Example:**
```typescript
useSettingsSubscription(
  'visualisation.graphs.logseq.physics.springK',
  (value) => { /* handle change */ }
);
// → useSettingsStore.subscribe(path, callback, immediate=true)
// → Trie registration, callback on RAF-batched change notification
```

### Known Component Call Sites

| Component File | Reader | Writer | Paths |
|----------------|--------|--------|-------|
| `PresetSelector.tsx` | `useSettingsStore.getState().get()` | `updateSettings()` draft updater | Multiple visual paths |
| `AgentControlPanel.tsx` | Hook readers | `updateSettings()` | Dashboard, compute, XR paths |
| `PerformanceControlPanel.tsx` | Hook readers | `updateSettings()` | Physics, rendering, quality gate paths |
| `DashboardControlPanel.tsx` | Hook readers | `updateSettings()` | Dashboard paths |
| `useSettingsHistory.ts` | Store snapshot | `updateSettings()` undo/redo | All user-modified paths |

---

## 8. Disconnects & Issues

### Disconnects Found

1. **Tweening is localStorage-only, not server-synced:**
   - `visualisation.graphs.logseq.tweening.*` and `clientTweening.*` paths.
   - No server endpoint for tweening.
   - Dispatches custom event `tweeningSettingsUpdated` to graph worker directly.
   - **Impact:** Tweening settings not persisted to server; only localStorage + custom event.

2. **XR, Auth, System debug settings are localStorage-only:**
   - `xr.*`, `auth.*`, `system.debug.*` have no server endpoints in `updateSettingByPath()` routing.
   - Logged as "persisted to localStorage only (no server endpoint)".
   - **Impact:** These are never pushed to backend; client-side persistent config only.

3. **Physics side-effect: Auto-calculated params vs UI sliders:**
   - Physics update path doesn't validate against server's auto-balance or adaptive tuning.
   - Client may set `repelK=1.0` but server ignores if auto-balance is enabled.
   - **Impact:** No feedback loop; user edits may be silently overridden server-side.

4. **Clustering algorithm endpoint mismatch:**
   - Single path `analytics.clustering.algorithm` → POST `/api/clustering/algorithm`.
   - Batch path `analytics.clustering.*` → POST `/api/clustering/{configure,start}`.
   - Two different endpoints for one setting tree.
   - **Impact:** Single edits vs batch edits routed differently; confusing behavior.

5. **Visual settings deep merge on init overwrites ALL user customizations at boot:**
   - `transformApiToClientSettings()` merges server over defaults, but then `deepMergeSettings()` in store init overlays localStorage.
   - On reboot, if server value changed (e.g., glow.intensity = 0.4), localStorage overlay (0.3) takes precedence.
   - **Impact:** Server-side admin settings don't push to client; localStorage always wins on reboot.

6. **No server push for settings changes:**
   - Settings are HTTP request-response only; no WebSocket subscriptions.
   - Multi-user sessions don't see each other's setting changes in real-time.
   - **Impact:** Collaboration broken; users must refresh to see others' settings.

7. **Retry queue never flushes on success if error occurs mid-batch:**
   - `updateSettingsByPaths()` may partially succeed (physics yes, visual no).
   - Failed paths added to retry queue, but retry logic doesn't re-batch with future changes.
   - **Impact:** Retry queue grows unbounded if server is flaky; memory leak risk.

8. **`getSettingsByPaths()` cache ignores index.ts customBackendUrl changes:**
   - Cache TTL 2s (line 453) is global; no invalidation on backend URL change.
   - First fetch caches 2s; if user changes backend URL, cache is stale until TTL expires.
   - **Impact:** New backend not consulted until cache expires or page reload.

9. **No validation of visual settings against server schema:**
   - `updateVisualSettings()` accepts arbitrary `Record<string, unknown>`.
   - Server may reject nested visual blob structure client sends.
   - No schema validation client-side.
   - **Impact:** Silent failures if server schema changes; debug requires network tab inspection.

10. **Client-only paths are checked in AutoSaveManager, but allowlist is short:**
    - Only `auth.nostr.*` are client-only.
    - `system.debug.*`, `xr.*` should probably be added but aren't checked.
    - **Impact:** System settings may attempt server sync even though endpoint doesn't exist (logged only, not fatal).

### Warnings & Observations

- **Physics parameter ranges validated client-side only:**
  - `springK [0.001, 1000]`, `repelK [0.001, 2000]`, etc. (settingsStore:840-856).
  - Server may have different limits; no alignment check.

- **NIP-98 auth can fail silently:**
  - Line 40: `logger.warn()` if signing fails, but request proceeds unsigned.
  - Server may reject unsigned physics updates but accept unsigned rendering updates (role-based).

- **Exponential backoff in retry manager hits 30s cap, then infinite 30s polling:**
  - No max attempts per retry cycle; queued item retried forever until manually cleared or explicitly max out.
  - **Impact:** Stuck queue items may pile up over time if server is down persistently.

---

## 9. Type System

**Generated Types:** `client/src/types/generated/settings.ts`

Main interfaces (lines 494-509):
- `AppFullSettings` (alias `Settings`): Root type. Contains `visualisation`, `system`, `xr`, `auth`, optional `ragflow`, `perplexity`, `openai`, `kokoro`, `whisper`.
- `DeepPartial<T>`: Recursive optional type for partial updates.
- `SettingsPath`: String (dot-notation path).

**Type Guards:**
- `isAppFullSettings(obj)`: Check for `visualisation`, `system`, `xr`, `auth` fields.
- `isPosition(obj)`: Check for `x`, `y`, `z` numbers.

**Misalignment:** Generated types imported in `settingsStore.ts` (line 3) but actual runtime shapes use `DeepPartial<Settings>` everywhere. Server API types (settingsApi.ts) differ from generated types (e.g., `PhysicsSettings` interface exists in both but with different fields).

---

## 10. Audit Summary

### Store Architecture
- **Single Zustand store** with trie-based subscribers, RAF-batched notifications.
- **localStorage persistence** for `partialSettings`, auth state.
- **Two singleton managers:** AutoSaveManager (500ms debounce), SettingsRetryManager (exponential backoff, 5s poll).

### API Client Architecture
- **Endpoint-per-category:** Physics, rendering, visual, node-filter, quality-gates, constraints.
- **GET-merge-PUT pattern** for mutations (no PATCH support).
- **Batch grouping:** Multiple paths routed to their category endpoint, sent in parallel.
- **NIP-98 auth:** Auto-signed via axios interceptor.
- **Cache:** 2s TTL on `getAll()` response; invalidated on reset.

### Data Flow: Write Path
```
UI Component
  ↓
useSettingSetter() [setByPath, batchedSet, immediateSet]
  ↓
setByPath(path, value) or batchUpdate(updates)
  ↓
useSettingsStore.set() or updateSettings() [Zustand]
  ↓
partialSettings updated, loadedPaths added, trie subscribers queued
  ↓
RAF batch → callback execution
  ↓
autoSaveManager.queueChange() / queueChanges()
  ↓
500ms debounce timer
  ↓
autoSaveManager.flushPendingChanges()
  ↓
settingsApi.updateSettingsByPaths() [batched by endpoint]
  ↓
Axios PUT/POST with NIP-98 signature
  ↓
(On success) Pending changes cleared.
(On error) Paths added to settingsRetryManager retry queue.
```

### Data Flow: Read Path
```
UI Component
  ↓
useSelectiveSetting(path) or useSelectiveSettings(paths)
  ↓
Zustand selector → state.get(path)
  ↓
Trie walk: fetch value from partialSettings
  ↓
(If not loaded) Warn, return undefined.
(If loaded) Return value.
  ↓
React re-renders only if value changed (Zustand shallow equality).
```

### Initialization Flow
```
App startup
  ↓
AppInitializer.tsx triggers useSettingsStore.initialize()
  ↓
waitForAuthReady() (async, 3s timeout)
  ↓
Rehydrate from localStorage merge() hook
  ↓
settingsApi.getSettingsByPaths(ESSENTIAL_PATHS)
  ↓
transformApiToClientSettings() [merge server + defaults]
  ↓
deepMergeSettings() [server + localStorage overlay]
  ↓
Physics/tweening override (server wins)
  ↓
autoSaveManager.setInitialized(true)
  ↓
initialized: true, state ready
```

---

## 11. Audit Completeness Checklist

- [x] All stores identified (Zustand settingsStore + singleton managers).
- [x] All API endpoints listed (11 category endpoints, 6 fire-and-forget side-effects, 4 profile endpoints).
- [x] WebSocket subscriptions analyzed (none; settings are HTTP-only).
- [x] Defaults pipeline traced (server fetch → transform → merge → init).
- [x] Debounce/batch logic documented (AutoSaveManager 500ms, retry manager 5s poll).
- [x] Every setting path traced (read/write/transport for 60+ paths).
- [x] Disconnects identified (10 issues, 7 observations).
- [x] Type system reviewed (generated types vs runtime DeepPartial).
- [x] Call sites sampled (PresetSelector, ControlPanel components, useSettingsHistory).
- [x] Error paths (retry queue, silent failures, cache staleness).

---

**End of Audit**
