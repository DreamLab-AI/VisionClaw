---
title: VisionClaw REST API Integration Guide
description: Practical guide to integrating with VisionClaw's REST API — authentication, common workflows, error handling, pagination, and WebSocket combination patterns
category: how-to
tags: [api, rest, integration, authentication, nostr, how-to]
updated-date: 2026-04-09
---

# VisionClaw REST API Integration Guide

This guide shows how to accomplish real tasks with the VisionClaw REST API. For raw endpoint listings, see [REST API Reference](../reference/rest-api.md). For WebSocket binary frame format, see [WebSocket Binary Protocol](../reference/binary-protocol.md).

---

## 1. Prerequisites

- VisionClaw running locally or deployed. See [deployment.md](deployment.md).
- A Nostr keypair. Generate one with `nostr-tools` (see Section 2) or a browser extension such as Alby or nos2x.
- Any HTTP client: `curl`, `fetch`, `axios`, or similar.

**Base URLs**:

| Environment | REST API | WebSocket |
|-------------|----------|-----------|
| Development |  `http://localhost:4000` (direct) or `http://localhost:3001` (nginx) | `ws://localhost:4000/wss` |
| Production | `https://<your-host>` | `wss://<your-host>/wss` |

All REST paths are prefixed with `/api/`. The OpenAPI UI is available at `http://localhost:4000/swagger-ui/`.

```mermaid
graph TD
    API["VisionClaw API<br/>localhost:4000"] --> Graph["/api/graph/*<br/>Nodes · Edges · Data"]
    API --> Settings["/api/settings/*<br/>User Preferences"]
    API --> Ontology["/api/ontology/*<br/>OWL Query · Update"]
    API --> Admin["/api/admin/*<br/>Sync · Force-sync"]
    API --> Agents["/api/agents/*<br/>Status · Run Skills"]
    API --> Analytics["/api/analytics/*<br/>GPU Metrics"]
    API --> Solid["/solid/*<br/>Pod Resources"]
    API --> Health["/health<br/>Service Status"]
```

*Figure: VisionClaw API endpoint groups — all paths are served from port 4000 (or via nginx on :3001)*

---

## 2. Authentication — Step by Step

VisionClaw uses [NIP-98](https://github.com/nostr-protocol/nips/blob/master/98.md) HTTP authentication. Each mutating request requires a signed Nostr event embedded in the `Authorization` header. After the first authenticated request the server issues a session token, allowing subsequent requests to use the lighter `Bearer` scheme.

```mermaid
sequenceDiagram
    participant App as "Client App"
    participant Nostr as "nostr-tools"
    participant API as "VisionClaw API :4000"
    participant Store as "Oxigraph (embedded)"

    App->>Nostr: signEvent(kind 27235, url, method, payload_hash)
    Nostr-->>App: signed event (Schnorr sig)
    App->>App: base64url encode event
    App->>API: GET /api/graph/data with Authorization Nostr b64
    API->>API: Decode + verify Schnorr sig
    API->>API: Check timestamp ≤60s
    API->>API: Check payload SHA-256 tag
    API->>Store: Execute graph query (SPARQL)
    Store-->>API: Graph data
    API-->>App: 200 JSON response
```

*Figure: NIP-98 authentication sequence — every mutating request carries a freshly signed Nostr event*

### 2.1 Generate a keypair (one-time)

```typescript
import { generatePrivateKey, getPublicKey } from 'nostr-tools'

const privateKey = generatePrivateKey()   // 32-byte hex
const publicKey  = getPublicKey(privateKey)

// Store these securely. Never commit them to version control.
```

### 2.2 Build a NIP-98 auth event

```typescript
import { finishEvent } from 'nostr-tools'
import { createHash } from 'crypto'

function sha256Hex(body: string): string {
  return createHash('sha256').update(body).digest('hex')
}

function createAuthEvent(
  url: string,
  method: string,
  privateKey: string,
  body?: string
) {
  const tags: string[][] = [
    ['u', url],
    ['method', method.toUpperCase()],
  ]

  if (body) {
    tags.push(['payload', sha256Hex(body)])
  }

  return finishEvent(
    {
      kind: 27235,
      created_at: Math.floor(Date.now() / 1000),
      tags,
      content: '',
    },
    privateKey
  )
}
```

**Required constraints** (server enforces these):
- `created_at` must be within **60 seconds** of server time.
- Each event is single-use — replay protection is enforced server-side.
- POST/PUT requests must include a `payload` tag containing the SHA-256 hex hash of the request body.

### 2.3 Reusable authenticated fetch wrapper

```typescript
const BASE_URL = 'http://localhost:4000/api'

// After first NIP-98 auth the server returns a session token.
// Store it and reuse via Bearer to avoid signing every request.
let sessionToken: string | null =
  localStorage.getItem('nostr_session_token')

async function apiCall(
  endpoint: string,
  method = 'GET',
  body?: object
): Promise<unknown> {
  const url = `${BASE_URL}${endpoint}`
  const bodyStr = body ? JSON.stringify(body) : undefined

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    'X-Nostr-Pubkey': publicKey,
  }

  if (sessionToken) {
    headers['Authorization'] = `Bearer ${sessionToken}`
  } else {
    const authEvent = createAuthEvent(url, method, privateKey, bodyStr)
    headers['Authorization'] = `Nostr ${btoa(JSON.stringify(authEvent))}`
  }

  const response = await fetch(url, {
    method,
    headers,
    body: bodyStr,
  })

  // Capture session token from first successful auth
  const newToken = response.headers.get('X-Session-Token')
  if (newToken) {
    sessionToken = newToken
    localStorage.setItem('nostr_session_token', newToken)
  }

  if (!response.ok) {
    const err = await response.json().catch(() => ({ error: response.statusText }))
    const error = Object.assign(new Error(`API ${response.status}`), {
      status: response.status,
      body: err,
    })
    throw error
  }

  return response.json()
}
```

**Session token details**: Tokens are stored in `localStorage` as `nostr_session_token`. Expiry is controlled by the `AUTH_TOKEN_EXPIRY` environment variable (default: 3600 seconds). On expiry, re-authenticate with a fresh NIP-98 event.

**Dev bypass**: Set `SETTINGS_AUTH_BYPASS=true` on the server to treat all requests as `dev-user`. For local development only.

---

## 3. Common Workflows

### 3a. Fetch the knowledge graph

```typescript
const graph = await apiCall('/graph/data') as {
  nodes: { id: string; label: string; node_type: string; metadata: object }[]
  edges: { id: string; source: string; target: string; relationship: string; weight: number }[]
  node_count: number
  edge_count: number
}

// Filter to a single type
const knowledgeOnly = await apiCall('/graph/data?graph_type=knowledge')

// Lightweight stats check before committing to a full download
const stats = await apiCall('/graph/stats')
console.log(`Graph has ${stats.node_count} nodes and ${stats.edge_count} edges`)
```

Node IDs are sequential `u32` values starting at 1. Always use `String()` coercion when comparing IDs — never `===` on raw numbers.

### 3b. Get and update settings

```typescript
// Read all settings (works anonymously; returns user-specific settings when authenticated)
const settings = await apiCall('/settings')

// Update a single key
await apiCall('/settings/physics.damping', 'PUT', { value: 0.85 })

// Bulk update multiple keys in one round-trip
await apiCall('/settings/bulk', 'POST', {
  changes: [
    { key: 'physics.damping',         value: 0.85 },
    { key: 'physics.spring',          value: 0.3  },
    { key: 'rendering.maxNodes',      value: 500000 },
  ],
})

// User-specific filter settings
await apiCall('/settings/user/filter', 'PUT', {
  enabled:             true,
  quality_threshold:   0.8,
  authority_threshold: 0.6,
  filter_by_quality:   true,
  filter_by_authority: false,
  filter_mode:         'or',
  max_nodes:           5000,
})
```

### 3c. Query ontology

```typescript
// List all OWL classes
const { classes, total } = await apiCall('/ontology/classes') as {
  classes: { iri: string; label: string; subclassOf: string | null }[]
  total: number
}

// Retrieve the full hierarchy up to depth 3
const hierarchy = await apiCall('/ontology/hierarchy?max-depth=3')

// Run the Whelk EL++ reasoner to infer new axioms
const inferred = await apiCall('/ontology/classify', 'POST') as {
  'inferred-axioms': { axiomType: string; subjectIri: string; objectIri: string; confidence: number }[]
  'reasoning-time-ms': number
}
```

### 3d. Trigger a GitHub sync

```typescript
// Incremental sync (respects SHA1 cache — only re-processes changed files)
await apiCall('/admin/sync/trigger', 'POST')

// Force full re-sync regardless of SHA1 cache
await apiCall('/admin/sync/force', 'POST')

// Check sync status
const syncStatus = await apiCall('/admin/sync/status')
console.log(`Last sync: ${syncStatus.last_sync_at}, status: ${syncStatus.status}`)
```

Only files tagged `public:: true` become knowledge graph page nodes. Ontology blocks (`### OntologyBlock`) are extracted from all files.

### 3e. Work with bots (agents)

```typescript
// List registered bots
const { bots } = await apiCall('/bots') as {
  bots: { id: string; name: string; pubkey: string }[]
}

// Register a new bot
const bot = await apiCall('/bots/register', 'POST', {
  name:        'knowledge-curator',
  description: 'Automated knowledge graph curation agent',
  pubkey:      publicKey,
})

// Submit a brief and spawn role agents
const brief = await apiCall('/briefs', 'POST', {
  briefing: {
    content: 'Analyse the design patterns subgraph for clustering opportunities',
    roles:   ['analyst', 'curator', 'reviewer'],
  },
  user_context: { display_name: 'John', pubkey: publicKey },
}) as { brief_id: string; bead_id: string; role_tasks: object[] }

// Retrieve the consolidated debrief
await apiCall(`/briefs/${brief.brief_id}/debrief`, 'POST', {
  role_tasks:   brief.role_tasks,
  user_context: { display_name: 'John', pubkey: publicKey },
})
```

### 3f. GPU analytics

```typescript
// Shortest path between two nodes
const path = await apiCall('/analytics/pathfinding/42/99')
// { path: [42, 17, 99], distance: 2.0, computation_time_ms: 8 }

// Single-source shortest paths from node 0 (GPU-accelerated)
const sssp = await apiCall('/analytics/pathfinding/sssp', 'POST', {
  sourceIdx:   0,
  maxDistance: 5.0,   // omit for full-graph
})
// sssp.result.distances[i] = distance from node 0 to node i; Infinity means unreachable

// Connected components detection
const cc = await apiCall('/analytics/pathfinding/connected-components', 'POST', {
  maxIterations: 100,
})
console.log(`Graph has ${cc.result.numComponents} components`)

// Check whether GPU features are compiled in before issuing GPU calls
const flags = await apiCall('/analytics/feature-flags')
if (!flags.gpu_enabled) console.warn('GPU analytics unavailable — falling back')
```

### 3g. Explore a node's relations, then expand one predicate

These graph-navigation endpoints are **public reads** — no `Authorization`
header required — so plain `curl` works. They back both the desktop expansion
menu and the XR node menu. See
[REST API Reference §graph](../reference/rest-api.md#post-apigraphnodeidexpand)
for the full schema.

Step 1 — ask what a node is connected to, grouped by predicate and direction:

```bash
curl -s http://localhost:4000/api/graph/node/42/relations | jq .
# {
#   "outgoing": [ { "edgeType": "references", "label": "references", "count": 12 } ],
#   "incoming": [ { "edgeType": "authored",   "label": "authored by", "count": 3 } ]
# }
```

Step 2 — pull the neighbours along one predicate/direction (additive expansion):

```bash
curl -s -X POST http://localhost:4000/api/graph/node/42/expand \
  -H 'Content-Type: application/json' \
  -d '{ "edgeType": "references", "direction": "outgoing", "limit": 25 }' | jq .
# {
#   "nodes": [ { "id": 88, "metadataId": "…", "label": "…", "nodeType": "page" } ],
#   "edges": [ { "source": 42, "target": 88, "edgeType": "references", "weight": 1.0 } ]
# }
```

Merge `nodes`/`edges` into your current view; ids are `u32`, so `String()`-coerce
before comparing.

### 3h. Drive the semantic fold ladder

`GET /api/graph/fold` returns the collapse/expand plan for a fold level (0–3),
the same plan the XR **Fold +/-** buttons apply:

```bash
curl -s 'http://localhost:4000/api/graph/fold?level=2&graphType=knowledge' | jq .
# {
#   "level": 2, "graphType": "knowledge", "generation": 7,
#   "hidden": [ 101, 102, … ],
#   "groups": [ { "representativeId": 55, "memberIds": [56,57], "badge": "3", "kind": "subclass" } ],
#   "analyticsNodes": [ … ],
#   "hierarchyEdges": [ … ]
# }
```

Levels: `0` everything, `1` hide low-signal, `2` fold subclass chains, `3`
community fold. Pass `pinned` as a CSV of node ids to keep them visible through
the fold. `graphType` is `knowledge`, `ontology`, or `agent`.

### 3i. Run a triple-pattern query

Match a structural pattern and read the variable bindings back
(max 16 triples, 8 variables):

```bash
curl -s -X POST http://localhost:4000/api/graph/query/pattern \
  -H 'Content-Type: application/json' \
  -d '{
        "triples": [
          { "src": "?person", "edgeType": "authored",  "tgt": "?doc" },
          { "src": "?doc",    "edgeType": "references", "tgt": 42 }
        ],
        "limit": 24, "countOnly": false
      }' | jq .
# { "vars": ["?person","?doc"], "bindingCount": 3, "truncated": false,
#   "bindings": [ { "?person": 17, "?doc": 88 }, … ] }
```

Set `"countOnly": true` for just `bindingCount` — the cheap preview the visual
query builder issues on every pattern change. See
[Building Graph Queries](features/natural-language-queries.md).

### 3j. Switch the layout mode

`POST /api/layout/mode` selects the global layout. GPU modes
(`forceDirected`, `hierarchical`, `radial`, `clustered`) return an empty
`positions` array — positions stream over the WebSocket — while CPU modes
(`spectral`, `temporal`) return computed positions inline:

```bash
curl -s -X POST http://localhost:4000/api/layout/mode \
  -H 'Content-Type: application/json' \
  -d '{ "mode": "hierarchical", "transitionMs": 500 }' | jq .
# { "success": true, "mode": "hierarchical", "transitionMs": 500, "positions": [] }
```

### 3k. Configure the radial layout

`POST /api/layout/radial` tunes the radial arrangement — rank the DAG
(`dagRank`), tier by node type (`typeTier`), or centre on a focus node (`ego`):

```bash
curl -s -X POST http://localhost:4000/api/layout/radial \
  -H 'Content-Type: application/json' \
  -d '{ "mode": "ego", "focusNode": 42, "transitionMs": 500 }' | jq .
# { "success": true, "mode": "ego", "focusNode": 42, "transitionMs": 500 }
```

`focusNode` is required for `ego` and ignored for `dagRank`/`typeTier`. Full
schema: [REST API Reference §layout](../reference/rest-api.md#post-apilayoutmode).

---

## 4. Combining REST + WebSocket

The recommended pattern for real-time graph visualisation: load the static graph topology via REST, then stream live physics positions over the binary WebSocket.

```mermaid
graph LR
    subgraph "Initial Load"
        REST["REST<br/>GET /api/graph/data"] --> Render["Initial<br/>3D Render"]
    end
    subgraph "Live Updates"
        WS["WebSocket<br/>ws://host/wss"] --> Binary["Binary V3<br/>52 bytes/node"]
        Binary --> SAB["SharedArrayBuffer<br/>Position Updates"]
        SAB --> RAF["requestAnimationFrame<br/>Smooth Animation"]
    end
    Render --> RAF
```

*Figure: REST + WebSocket combination — REST delivers topology once, WebSocket streams position updates continuously*

```typescript
// Step 1 — load topology via REST
const { nodes, edges } = await apiCall('/graph/data') as {
  nodes: { id: string; label: string; node_type: string }[]
  edges: { source: string; target: string }[]
}
renderGraph(nodes, edges)

// Step 2 — stream live positions via WebSocket
const token = localStorage.getItem('nostr_session_token')
const ws = new WebSocket(
  `ws://localhost:4000/wss${token ? `?token=${token}` : ''}`
)

ws.onopen = () => {
  // Authenticate over the socket (required if not passing token in URL)
  ws.send(JSON.stringify({ type: 'authenticate', token, pubkey: publicKey }))

  // Nothing to subscribe to: the server streams positions to every registered session in
  // chunks. Acknowledge each chunk you apply with a 0x34 frame (see websocket-protocol.md,
  // "Position acknowledgement"), or the server resends it after its ack timeout.
}

ws.onmessage = (event: MessageEvent) => {
  if (typeof event.data === 'string') {
    // JSON control frame (state_sync, subscription_confirmed, heartbeat…)
    const msg = JSON.parse(event.data)
    if (msg.type === 'heartbeat') ws.send(JSON.stringify({ type: 'heartbeat' }))
    return
  }

  // Binary frame — single binary protocol: 1 preamble byte (0x42) + 8-byte LE
  // broadcast_sequence + 24 bytes/node. See docs/binary-protocol.md.
  const buffer = event.data as ArrayBuffer
  const view   = new DataView(buffer)
  const preamble = view.getUint8(0)

  if (preamble !== 0x42) {
    console.warn(`Unexpected WS binary preamble: 0x${preamble.toString(16)}`)
    return
  }

  const HEADER_SIZE = 9   // 1 preamble + 8 broadcast_sequence
  const NODE_SIZE   = 24  // u32 id + 6 × f32 (pos+vel)
  const nodeCount = (buffer.byteLength - HEADER_SIZE) / NODE_SIZE
  for (let i = 0; i < nodeCount; i++) {
    const offset = HEADER_SIZE + i * NODE_SIZE
    const nodeId = view.getUint32(offset,      true)
    const x      = view.getFloat32(offset + 4,  true)
    const y      = view.getFloat32(offset + 8,  true)
    const z      = view.getFloat32(offset + 12, true)
    updateNodePosition(String(nodeId), x, y, z)
  }
}

// Send a heartbeat every 25 seconds to keep the connection alive
setInterval(() => ws.send(JSON.stringify({ type: 'heartbeat' })), 25_000)
```

The server emits one binary protocol — there are no versions. The preamble byte (0x42) is a fixed sanity check, not a version dispatch. Sticky GPU outputs (`cluster_id`, `community_id`, `anomaly_score`, `sssp_distance`, `sssp_parent`) ride a separate `analytics_update` JSON message at recompute cadence. See [docs/binary-protocol.md](../reference/binary-protocol.md) and [ADR-061](../archive/adr/ADR-061-binary-protocol-unification.md).

---

## 5. Error Handling

VisionClaw returns structured error bodies for all 4xx/5xx responses.

```typescript
interface ApiError {
  error:    string          // human-readable message
  code?:    string          // VisionClaw error code, e.g. "AP-E-305"
  details?: Record<string, unknown>
}
```

Error code format: `[SYSTEM]-[SEVERITY]-[NUMBER]`. Key codes for integrators:

| Code | Meaning | Action |
|------|---------|--------|
| `AP-E-101` | Missing auth token | Add `Authorization` header |
| `AP-E-102` | Invalid or expired token | Re-authenticate with a new NIP-98 event |
| `AP-E-103` | Session token expired | Re-authenticate; issue new NIP-98 event |
| `AP-E-305` | Rate limit exceeded | Exponential back-off; check `Retry-After` header |
| `AP-E-307` | Operation timeout | Retry; consider reducing request scope |
| `AP-E-201` | Resource not found | Verify the ID exists via a GET first |

```typescript
async function apiCallWithRetry(
  endpoint: string,
  method = 'GET',
  body?: object,
  maxRetries = 3
): Promise<unknown> {
  for (let attempt = 0; attempt < maxRetries; attempt++) {
    try {
      return await apiCall(endpoint, method, body)
    } catch (err: unknown) {
      const e = err as { status?: number; body?: ApiError }

      if (e.status === 429) {
        // Rate limited — exponential back-off
        const delay = Math.pow(2, attempt) * 1000
        await new Promise(resolve => setTimeout(resolve, delay))
        continue
      }

      if (e.status === 401) {
        // Token expired — clear and force NIP-98 re-auth on next call
        sessionToken = null
        localStorage.removeItem('nostr_session_token')
        continue
      }

      throw err
    }
  }
  throw new Error(`Failed after ${maxRetries} attempts: ${endpoint}`)
}
```

---

## 6. Pagination

`/graph/data` returns the full graph in one payload. For large datasets use the node-level endpoint with cursor pagination:

```typescript
interface NodePage {
  nodes:       { id: string; label: string; node_type: string }[]
  next_cursor: string | null
}

async function fetchAllNodes(pageSize = 100): Promise<NodePage['nodes']> {
  const nodes: NodePage['nodes'] = []
  let cursor: string | null = null

  do {
    const params = new URLSearchParams({ limit: String(pageSize) })
    if (cursor) params.set('cursor', cursor)

    const page = await apiCall(`/graph/nodes?${params}`) as NodePage
    nodes.push(...page.nodes)
    cursor = page.next_cursor
  } while (cursor)

  return nodes
}
```

For ontology data, use `?max-depth=N` on `/api/ontology/hierarchy` to cap response size rather than paginating.

---

## 7. Client Libraries

There is no official SDK. The `apiCall` helper in Section 2 is sufficient for most integrations. For a more structured client, the minimal pattern below wraps all endpoint groups:

```typescript
class VisionClawClient {
  constructor(
    private base: string,
    private privateKey: string,
    private publicKey: string
  ) {}

  private async call(path: string, method = 'GET', body?: object) {
    // ... same logic as apiCall above, using this.base / this.privateKey
  }

  graph   = { data: (type?: string) => this.call(`/graph/data${type ? `?graph_type=${type}` : ''}`),
               stats: () => this.call('/graph/stats'),
               node:  (id: string) => this.call(`/graph/node/${id}`) }

  settings = { getAll:     ()                     => this.call('/settings'),
                update:    (key: string, val: unknown) => this.call(`/settings/${key}`, 'PUT', { value: val }),
                bulkUpdate: (changes: object[])   => this.call('/settings/bulk', 'POST', { changes }),
                userFilter: (f: object)           => this.call('/settings/user/filter', 'PUT', f) }

  ontology = { classes:   () => this.call('/ontology/classes'),
                hierarchy: (depth?: number) => this.call(`/ontology/hierarchy${depth ? `?max-depth=${depth}` : ''}`),
                classify:  () => this.call('/ontology/classify', 'POST') }

  sync     = { trigger:  () => this.call('/admin/sync/trigger', 'POST'),
                force:    () => this.call('/admin/sync/force',   'POST'),
                status:   () => this.call('/admin/sync/status') }

  bots     = { list:     () => this.call('/bots'),
                register: (b: object) => this.call('/bots/register', 'POST', b) }
}
```

---

## 8. Rate Limits Reference

| Endpoint group | Limit | Window | Error code |
|----------------|-------|--------|------------|
| All endpoints (default) | 100 req | 60 s | `AP-E-305` |
| POST `/ontology/classify` | 10 req | 60 s | `AP-E-305` |
| POST `/analytics/pathfinding/sssp` | 20 req | 60 s | `AP-E-305` |
| POST `/analytics/pathfinding/apsp` | 5 req | 60 s | `AP-E-305` |
| POST `/admin/sync/*` | 2 req | 300 s | `AP-E-305` |
| GET `/graph/data` | 60 req | 60 s | `AP-E-305` |

When rate-limited the server responds with HTTP 429 and a `Retry-After` header indicating seconds to wait. Use exponential back-off as shown in Section 5.

---

## 9. Testing API Calls with curl

For ad-hoc testing, generate a signed NIP-98 event from the command line using Node.js and `nostr-tools`:

```bash
# Install nostr-tools globally if not present
npm install -g nostr-tools

# Set your private key (hex)
SK="your_private_key_hex_here"
TARGET_URL="http://localhost:4000/api/graph/data"
METHOD="GET"

AUTH=$(node -e "
  const { finishEvent, getPublicKey } = require('nostr-tools')
  const sk = process.env.SK
  const pk = getPublicKey(sk)
  const event = finishEvent({
    kind:       27235,
    created_at: Math.floor(Date.now() / 1000),
    tags:       [['u', process.env.TARGET_URL], ['method', process.env.METHOD]],
    content:    '',
    pubkey:     pk,
  }, sk)
  console.log(Buffer.from(JSON.stringify(event)).toString('base64'))
" SK="$SK" TARGET_URL="$TARGET_URL" METHOD="$METHOD")

curl -s \
  -H "Authorization: Nostr $AUTH" \
  -H "X-Nostr-Pubkey: $(node -e "const {getPublicKey}=require('nostr-tools');console.log(getPublicKey('$SK'))")" \
  "$TARGET_URL" | jq .
```

For POST requests add a body hash to the `payload` tag and pass `-d` to curl:

```bash
BODY='{"changes":[{"key":"physics.damping","value":0.9}]}'
TARGET_URL="http://localhost:4000/api/settings/bulk"
METHOD="POST"

AUTH=$(node -e "
  const { finishEvent, getPublicKey } = require('nostr-tools')
  const { createHash } = require('crypto')
  const sk   = process.env.SK
  const pk   = getPublicKey(sk)
  const body = process.env.BODY
  const hash = createHash('sha256').update(body).digest('hex')
  const event = finishEvent({
    kind:       27235,
    created_at: Math.floor(Date.now() / 1000),
    tags:       [['u', process.env.TARGET_URL], ['method', 'POST'], ['payload', hash]],
    content:    '',
    pubkey:     pk,
  }, sk)
  console.log(Buffer.from(JSON.stringify(event)).toString('base64'))
" SK="$SK" TARGET_URL="$TARGET_URL" BODY="$BODY")

curl -s -X POST \
  -H "Authorization: Nostr $AUTH" \
  -H "Content-Type: application/json" \
  -d "$BODY" \
  "$TARGET_URL" | jq .
```

**Dev bypass** (local only): skip all auth by setting `SETTINGS_AUTH_BYPASS=true` on the server, then omit the `Authorization` header entirely.

---

## 10. Troubleshooting

**401 — Missing authorization token**
The `Authorization` header is absent or malformed. Ensure the header value is `Nostr <base64>` for NIP-98 or `Bearer <token>` for session auth.

**401 — Token is invalid or expired (AP-E-102/103)**
The NIP-98 event's `created_at` is outside the 60-second window, or the session token has expired (default 1 hour). Regenerate the event with the current timestamp. Re-authenticate to get a fresh session token.

**401 on a replay attempt**
VisionClaw enforces single-use events. Each request must use a freshly signed event with a new `created_at`.

**403 — Access forbidden**
The authenticated pubkey lacks the required permission. Power-user actions (sync, classify, bulk settings) require the pubkey to be listed in `POWER_USER_PUBKEYS` on the server.

**POST/PUT returns 400 with "missing payload tag"**
Mutation requests must include a `payload` tag in the NIP-98 event containing the SHA-256 hex hash of the request body. See Section 2.2.

**WebSocket disconnects immediately after upgrade**
Ensure the token is either passed as `?token=` in the WebSocket URL or sent as an `authenticate` JSON message within the first few seconds of connection. See Section 4.

**GPU analytics return `"GPU features not enabled"`**
The binary was compiled without the `gpu` feature flag, or no CUDA-capable GPU is present. Check `/api/analytics/feature-flags` before issuing GPU endpoint calls.

**Sync returns stale data**
Use `POST /api/admin/sync/force` to bypass the SHA1 incremental cache, or set `FORCE_FULL_SYNC=1` in the server environment and restart.

**Clock skew causes consistent 401s**
NIP-98 requires `created_at` within 60 seconds of server time. If the client clock is skewed, synchronise it via NTP or fetch server time from a `/api/health` endpoint before signing events.
