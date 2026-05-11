# Panel 2 — "THE ENGINE" (Portrait, High Resolution)

## Image Generation Prompt

Create a high-resolution portrait-orientation (2:3 aspect ratio, minimum 2400×3600px) technical infographic panel with the following precise visual specifications and content layout. This is panel 2 of a triptych — it covers the core technology, data pipelines, and runtime architecture. No content from panels 1 or 3 appears here.

---

### GLOBAL VISUAL STYLE

**Background:** Deep navy-black (#0a0e1a) with subtle star field particles (tiny white/blue dots at 5-8% opacity), faint teal-cyan nebula wisps in upper-left and lower-right corners, barely-visible orthographic grid lines (#1a2040 at 3% opacity) creating a technical blueprint feel. The overall impression is deep space meets engineering schematic.

**Color Palette (strict — identical to panels 1 and 3):**
- Primary: Cyan/teal (#00e5ff, #00bcd4) — borders, primary text headers, infrastructure elements
- Accent 1: Magenta/hot pink (#ff4081, #e040fb) — security/identity concepts
- Accent 2: Lime/green (#76ff03, #69f0ae) — data flow arrows, active states, success indicators
- Accent 3: Amber/gold (#ffd740, #ffab40) — metrics, numbers, warning states
- Accent 4: Deep purple (#b388ff, #7c4dff) — agent/AI elements, secondary containers
- Text: White (#ffffff) at 90% opacity for body, 60% opacity for secondary labels
- Borders: 1.5px strokes with outer glow (4px blur, 30% opacity of stroke color)

**Typography:** (identical to panel 1)
- Title: Bold condensed sans-serif, uppercase, subtle outer glow
- Section headers: ALL CAPS, 18-22pt, thin colored underline at 60% width
- Body: Clean technical sans-serif, 9-11pt, white at 85%
- Metrics: Extra-bold, 48-72pt, amber/gold with glow
- Labels: Monospace, 7-8pt, white at 55%

**Containers, Connectors, Icons:** Same specs as panel 1 — rounded rectangles (8px radius), 1.5px neon borders, orthogonal dashed connectors, outlined geometric icons 24-32px.

---

### LAYOUT (top to bottom, full width)

---

#### ZONE A — TITLE BANNER (top 6% of canvas)

**Left-aligned:**
- "THE ENGINE" in cyan bold condensed, 56pt, with teal outer glow
- Below: "Runtime Architecture, Data Pipelines & Actor Mesh" in white at 60% opacity, 13pt

**Right side:** Five pill badges:
- "19 BOUNDED CONTEXTS" (cyan border, hexagon icon)
- "52 CUDA KERNELS" (green border, GPU chip icon)
- "16 MCP TOOLS" (purple border, wrench icon)
- "40 AGENT SKILLS" (amber border, lightning icon)
- "HTTP 402 PAYMENTS" (gold border, coin icon)

**Thin horizontal cyan line** with diamond ornament center.

---

#### ZONE B — ACTOR MESH ARCHITECTURE (next 22% of canvas)

**Section header:** "ACTIX ACTOR MESH — 35 SUPERVISED ACTORS" in cyan, ALL CAPS, thin cyan underline.

**Central visualization:** A hierarchical tree/mesh diagram showing the actor supervision structure.

**Top level (single node, double cyan border, largest):**
- "GRAPH SERVICE SUPERVISOR" with crown/star icon
- Label: "Root supervisor. Heartbeat monitoring. Backoff restart."
- Small badge: "GraphSupervisionStrategy"

**Second level (5 nodes in a row, connected by downward lines from supervisor):**

Node 1 (cyan border):
- "GraphStateActor"
- "Node/edge state. SAB positions."
- Icon: graph/network

Node 2 (green border):
- "PhysicsOrchestratorActor"
- "GPU dispatch. Broadcast positions."
- Icon: atom/physics

Node 3 (magenta border):
- "BrokerActor"
- "Judgment workbench. Precedent registry."
- Icon: gavel/scales

Node 4 (purple border):
- "ClientCoordinatorActor"
- "WebSocket fan-out. Binary frames."
- Icon: broadcast/antenna

Node 5 (amber border):
- "ServerNostrActor"
- "7 Nostr event types. NIP-17 DMs."
- Icon: relay/signal

**Third level (5 more nodes, smaller, connected to relevant parents):**

- "OntologyActor" (under GraphState) — "OWL class parsing, validation jobs"
- "SemanticProcessorActor" (under GraphState) — "AI feature extraction, MiniLM-L6"
- "ForceComputeActor" (under Physics) — "CUDA kernel dispatch, delta compression"
- "ShareOrchestratorActor" (under Broker) — "Share state ladder, contributor promotion"
- "PaymentGateActor" (under ClientCoordinator) — "HTTP 402 debit-or-reject. Per-endpoint cost table. Agent job estimation + settlement."

**Between nodes:** Small label annotations on the connector lines showing message types:
- Supervisor→GraphState: "GetGraphData, UpdateNodes"
- Supervisor→Physics: "BroadcastPositions, SetParams"
- Supervisor→Broker: "SubmitCase, DecideCase"
- Supervisor→Client: "BroadcastBinary, SendFiltered"
- Supervisor→Nostr: "SignAuditRecord, SignSealedDM"

**Right margin annotation (vertical text, small):** "Actix 0.13 + Tokio runtime — message-passing, no Arc<RwLock<T>>"

---

#### ZONE C — GPU PHYSICS PIPELINE (next 18% of canvas)

**Section header:** "GPU-ACCELERATED FORCE-DIRECTED LAYOUT" in green, ALL CAPS, thin green underline.

**Horizontal pipeline diagram — left to right flow with five stages:**

**Stage 1 (leftmost, cyan rounded box):**
- "SimParams" header
- Content: "35 physics parameters. Repulsion, attraction, gravity, damping, spring constants."
- Small list: "centerGravityK, repulsionStrength, attractionStrength, dampingFactor"
- Icon: sliders/controls

**Arrow →** (green, labeled "CUDA dispatch")

**Stage 2 (green rounded box):**
- "CUDA KERNELS" header
- Content: "52 kernels. Barnes-Hut tree. N-body force computation."
- Small visual: simplified tree structure showing octree subdivision
- Stats: "7K lines CUDA C++" | "PTX ISA 9.0"
- Icon: GPU chip

**Arrow →** (green, labeled "24-byte binary frames")

**Stage 3 (amber rounded box):**
- "BROADCAST OPTIMIZER" header
- Content: "Delta compression. Only changed positions cross the wire."
- Stats: "Periodic full broadcast every 300 iterations"
- Detail: "Warmup window: 600 frames (~10s)"
- Icon: filter/funnel

**Arrow →** (green, labeled "SharedArrayBuffer")

**Stage 4 (purple rounded box):**
- "CLIENT COORDINATOR" header
- Content: "Per-client WebSocket. Binary position stream. Filter sync."
- Detail: "Flag bits encode node type: agent (0x80), knowledge (0x40), ontology (0x1C)"
- Icon: broadcast tower

**Arrow →** (green, labeled "60fps render")

**Stage 5 (rightmost, cyan rounded box):**
- "THREE.JS RENDERER" header
- Content: "InstancedMesh. Zero-alloc label layout. Frustum culling."
- Detail: "GlassEdges: cylinder instances. SAB direct read."
- Icon: eye/display

**Below pipeline:** A thin container spanning full width with the binary protocol spec:
- Header: "BINARY PROTOCOL — docs/binary-protocol.md (ADR-061)"
- Content in monospace: "24 bytes/node: [nodeId:u32][x:f32][y:f32][z:f32][vx:f32][vy:f32][vz:f32] — position + velocity, no versions"

---

#### ZONE D — KNOWLEDGE GRAPH PIPELINE (next 20% of canvas)

**Section header:** "KNOWLEDGE GRAPH — INGEST, ENRICH, FEDERATE" in amber, ALL CAPS, thin amber underline.

**Left column (45% width) — Ingest Pipeline:**

**Vertical flow diagram, four stages connected by downward green arrows:**

**Box 1 (top, cyan border):**
- "GIT INGEST SURFACE (G1)" header
- "git-over-HTTP clone into tmp worktree. DID-gated remote registry (G2). PAT or Nostr-signed auth."
- Icon: git-branch

**Box 2 (green border):**
- "PROVENANCE ENCODER (G3)" header
- "Every enrichment signed with agent DID. Commit metadata carries provenance chain. Nostr kind 30300."
- Icon: stamp/seal

**Box 3 (amber border):**
- "NEO4J GRAPH STORE" header
- "Nodes: page, linked_page, owl_*, agent/bot. Edges: LINKS_TO, SUBCLASS_OF, transient."
- Stats in monospace: "~199 public KG pages | 623 OWL subclass rels | 998 source files"
- Icon: database/cylinder

**Box 4 (bottom, magenta border):**
- "WRITE-BACK SAGA (G4)" header
- "5-phase async saga. Per-remote mutex (DashMap + tokio::Mutex). Conflict classification (R4)."
- Detail: "non-fast-forward → Conflict variant → broker re-review notification"
- Icon: upload/push

**Right column (45% width) — Broker Workbench:**

**Central rounded container with double magenta border:**

**Header:** "JUDGMENT BROKER WORKBENCH" with gavel icon

**Inside — vertical stack of three sub-boxes:**

**Sub-box 1 (top):**
- "CASE SUBMISSION" header
- "6 categories: ContributorMeshShare, KnowledgeEnrichment, WorkflowReview, PolicyException, TrustAlert, ManualSubmission"
- "Priority: u8 → Low/Medium/High/Critical projection"

**Sub-box 2 (middle):**
- "DECISION ORCHESTRATION" header
- "6 outcomes: Approve, Reject, Amend, Delegate, Promote, Precedent"
- Small flow: "Submit → Claim → Review → Decide → Persist → Broadcast"
- "Auto-approve: PrecedentRegistry checks scope after N=3 approvals"

**Sub-box 3 (bottom):**
- "PERSISTENCE & BROADCAST" header
- "Domain→Legacy projection adapter (broker_case_projection.rs)"
- "Neo4j persistence via BrokerRepository port"
- "Real-time push via ClientCoordinatorActor (R1)"
- "NIP-17 sealed DMs for broker↔agent dialogue (R5)"

**Between left and right columns:** A bidirectional arrow labeled "enrichment proposals flow left→right, approved write-backs flow right→left"

---

#### ZONE E — AGENTBOX & SOLID POD RUNTIME (next 18% of canvas)

**Section header:** "AGENT CONTAINER & DATA SOVEREIGNTY RUNTIME" in purple, ALL CAPS, thin purple underline.

**Two equal columns:**

**Left column — "AGENTBOX" (purple border, large container):**

**Header:** "NIX-BASED SOVEREIGN CONTAINER" with container icon

**Internal layout — four small boxes stacked:**

Box 1: "MANAGEMENT API" — "Express.js on :9190. Git bridge routes. Broker bridge SSE. Agent event stream :9700."

Box 2: "NOSTR-RS-RELAY" — "Embedded relay. NIP-42 AUTH. NIP-17 sealed DMs. Event persistence."

Box 3: "SOLID POD" — "CSS (Community Solid Server) on :8484. LDP containers. WAC access control. WebID-TLS."

Box 4: "SKILL PROVIDER" — "Pluggable adapter architecture (ADR-005). Skill packages. Evaluation FSM. Compatibility scanner."

**Port mapping table at bottom:**
```
Mgmt API  :9190    Code Server :8180
VNC       :5902    SSH         :2223
Solid Pod :8484    Events      :9700
Metrics   :9191
```

**Right column — "SOLID-POD-RS" (magenta border, large container):**

**Header:** "FOUNDATION LIBRARY — DATA SOVEREIGNTY + PAYMENT PRIMITIVES" with shield icon

**Internal layout — six small boxes stacked:**

Box 1: "LDP (Linked Data Platform)" — "Container/resource CRUD. RDF turtle serialisation. Hierarchical pod storage."

Box 2: "WAC (Web Access Control)" — "ACL resources. Agent/group/public modes. Read/write/append/control."

Box 3: "WebID / DID" — "did:nostr canonical format. SchnorrSecp256k1VerificationKey2019. z-form base58btc multibase. NIP-98 HTTP auth."

Box 4: "HTTP 402 WEB LEDGERS" — "PayConfig, WebLedger, PaymentStore trait. Multi-chain TXO deposit verification. balance_response(), payment_required_body(). Agents and humans share the same DID-keyed ledger."

Box 5: "MRC20 TOKEN ENGINE" — "RFC 8785 JCS canonicalisation. SHA-256 hash chain state verification. BIP-341 taproot key chaining (feature-gated). Token buy/withdraw at operator-defined rate."

Box 6: "TIER-3 KEY CUSTODY" — "HSM-backed private keys. WAC-gated key material. BIP-341 per-user deposit address derivation. Rotation protocol (ADR-081)."

**Connecting annotation between columns:** "solid-pod-rs is consumed by agentbox's CSS instance, VisionClaw's pay_handler.rs, and nostr-rust-forum's pod-worker — single source of truth for all Solid + payment operations"

---

#### ZONE F — FOOTER (bottom 6% of canvas)

**Left:** "PANEL 2 OF 3 — THE ENGINE" in white at 40% opacity
**Center:** DreamLab AI logo mark (outlined, cyan)
**Right:** "dreamlab.ai" in white at 40% opacity

**Bottom edge:** Thin gradient line full width, cyan → magenta → green → amber → purple.

---

### RENDERING NOTES

- The actor mesh diagram (Zone B) is the visual centrepiece — give it generous space and make the hierarchy clear
- GPU pipeline (Zone C) should read strictly left-to-right like a factory production line
- All monospace code snippets should be in a slightly darker sub-container (#0d1220) with 1px border
- Maintain consistent 12-16px padding, 8-12px gutters throughout
- Every text element must be crisp and legible at print resolution
- No photographs, no 3D renders — flat neon on dark, technical poster aesthetic
- Portrait orientation: 2400×3600px (2:3 ratio)
