# PRD-013: Solid Pod Git Ingest Surface — Agent-Mediated Knowledge Federation

**Status:** G1-G7 Implemented; Phase 7 (GitHub REST deprecation) Planned
**Priority:** P1
**Author:** Architecture Agent / Dr John O'Hare
**Date:** 2026-05-08
**Supersedes:** GitHub REST API ingest (src/services/github/)
**Depends on:** solid-pod-rs 0.4.0-alpha.5+, ADR-041, ADR-049, ADR-051, ADR-074, PRD-010
**ADR (companion):** ADR-086 (to be written on acceptance)
**DDD Context:** BC2 (Graph Data), BC11 (Judgment Broker), BC13 (Discovery), BC20 (Agentbox Integration)

---

## Problem Statement

VisionClaw's knowledge graph ingest is locked to a single source type: a GitHub repository accessed via the GitHub REST API (`api.github.com`, PAT-authenticated). This creates three structural limitations:

1. **Vendor lock-in.** Every knowledge base must be hosted on GitHub. Logseq graphs stored on GitLab, Codeberg, a self-hosted Gitea, or a Solid pod are unreachable.

2. **No write-back path.** The ingest pipeline is strictly pull-only. Enriched data (embeddings from PRD-009, ontology promotions from ADR-049, agent-proposed edges) stays trapped in Neo4j with no mechanism to commit it back to the source of truth. The knowledge base author never sees the system's work.

3. **No identity-mediated access.** GitHub PATs are bearer tokens scoped to a single GitHub account. There is no way for a third party to grant VisionClaw read access to their knowledge base using a decentralised identity. Access control is GitHub's RBAC, not the pod owner's WAC policy.

Meanwhile, solid-pod-rs 0.4.0-alpha.5 now ships three composable primitives that dissolve all three limitations:

- **`solid-pod-rs-git`** — Git smart HTTP backend (clone/push/pull against pod storage, NIP-98 auth on push)
- **`solid-pod-rs-nostr`** — `did:nostr` ↔ WebID bidirectional resolver + embedded NIP-01/11/16 relay
- **`mashlib`** module — SolidOS data-browser rendering of RDF resources from any static host

The architectural insight: **any Solid pod with git-http-backend enabled is simultaneously a Git repo, an LDP server, and a mashlib-renderable website.** GitHub becomes just another git remote — not a special-cased API. A knowledge base hosted on a Solid pod can be cloned, ingested, enriched, and pushed back using standard git semantics, with access control mediated by `did:nostr` pubkeys and NIP-98 signatures rather than platform-specific tokens.

This unification also unlocks bidirectional sync for agentbox: AI agents can clone a pod's knowledge base, reason over it, commit enrichments with full URN/URI/IRI/pubkey/DID provenance in each commit, and push those enrichments back — all gated through the Judgment Broker (ADR-041) to prevent unreviewed mutations.

---

## Solution Overview

Replace the GitHub REST API ingest with a **git-over-HTTP ingest surface** that treats every knowledge source — GitHub, GitLab, Solid pod, any git remote — identically. Layer identity-mediated access via `did:nostr` + NIP-98. Gate write-back through the existing Judgment Broker (BC11) so enrichments flow from Neo4j back to source pods as broker-approved commits.

### Seven components

| # | Component | New code location | Depends on |
|---|-----------|-------------------|------------|
| G1 | **Git Ingest Adapter** | `src/services/git_ingest/` | `solid-pod-rs-git` (GitHttpService), `git2` |
| G2 | **DID-Gated Remote Registry** | `src/services/git_ingest/remote_registry.rs` | `solid-pod-rs-nostr` (NostrWebIdResolver) |
| G3 | **Provenance Commit Encoder** | `src/services/git_ingest/provenance.rs` | `urn:visionclaw:*` minting (src/uri/) |
| G4 | **Write-Back Saga** | `src/services/git_ingest/writeback_saga.rs` | BC11 (DecisionOrchestrator), IngestSaga (ADR-051) |
| G5 | **Agentbox Pod Bridge** | `agentbox/` adapter surface | BC20 anti-corruption layer, nostr-rs-relay |
| G6 | **Broker Review Surface** | agentbox panes + VisionClaw WebSocket | BC11 Decision Canvas, linked objects viewer |
| G7 | **Nostr Control Plane** | agentbox relay + VisionClaw ServerNostrActor | IS-Envelope (ADR-075), nostr-rs-relay, nostr-rust-forum |

---

## User Stories

### US-1: Ingest from any git remote

> As a knowledge engineer, I want to point VisionClaw at any git repository URL — GitHub, GitLab, a Solid pod, a bare repo on a VPS — and have it ingest my markdown/RDF knowledge base identically.

**Acceptance criteria:**
- `POST /api/ingest/remotes` accepts `{ "url": "https://pod.example.com/alice/kg/git/", "auth": "did:nostr" }` and `{ "url": "https://github.com/user/repo", "auth": "pat" }`
- Both produce identical `SyncStatistics` output
- SHA1 incremental filtering works across all remote types
- GitHub REST API path (`EnhancedContentAPI`) is deprecated with a migration shim

### US-2: DID-gated pod access

> As a pod owner, I want to grant VisionClaw read access to my knowledge base by adding its `did:nostr` pubkey to my WAC ACL, rather than creating a platform-specific API token.

**Acceptance criteria:**
- VisionClaw presents a stable `did:nostr:<hex>` identity derived from `SERVER_NOSTR_PRIVKEY`
- `git clone` against a WAC-protected pod path uses NIP-98 auth headers
- Access denied returns a clear error referencing the DID the pod owner needs to authorise
- No GitHub PAT required for pod-hosted knowledge bases

### US-3: Write enrichments back to source pod

> As a knowledge engineer, I want VisionClaw's enrichments (embeddings, discovered gaps, ontology promotions) to appear as commits in my knowledge base's git history, so I can review them, revert them, and share them.

**Acceptance criteria:**
- Write-back is gated: only `DecisionOutcome::Approve` or `DecisionOutcome::Promote` from the Judgment Broker triggers a push
- Each write-back commit message includes: `urn:visionclaw:` URI of the enriched node, `did:nostr:<hex>` of the approving broker, timestamp, and reasoning hash
- `git log` on the source pod shows VisionClaw's enrichments interleaved with human commits
- Write-back is disabled by default (`WRITEBACK_ENABLED=false`); opt-in per remote

### US-4: Agent-driven ingest and enrichment

> As an agentbox AI agent, I want to clone a pod's knowledge base into my workspace, run reasoning/embedding pipelines, and commit results back — all using my own `did:nostr` keypair for provenance.

**Acceptance criteria:**
- Agentbox agents can register git remotes via the BC20 adapter
- Agent commits include `Signed-off-by: did:nostr:<agent-hex>` and `Approved-by: did:nostr:<broker-hex>` trailers
- Agent write-backs require Broker approval (no autonomous push without human-in-the-loop)
- The nostr-rs-relay in agentbox can relay write-back approval events between agents and brokers

### US-5: Provenance-traced commits

> As an auditor, I want every machine-generated commit to carry cryptographic provenance — which node was enriched, which agent proposed it, which broker approved it, and which DID identity signed the push.

**Acceptance criteria:**
- Commit message format: structured trailers parseable by `git log --format='%(trailers)'`
- Trailers: `Urn: urn:visionclaw:concept:<pubkey>:<slug>`, `Proposed-by: did:nostr:<agent>`, `Approved-by: did:nostr:<broker>`, `Reasoning-hash: sha256:<hex>`
- NIP-98 signature on the `git push` HTTP request binds the commit to the pushing identity
- `git verify-commit` path available when NIP-98 Schnorr signatures are embedded (stretch goal)

### US-6: Visual signoff via broker review surface

> As a knowledge base owner, I want to review agent-proposed enrichments as visual diffs — current state vs proposed state — and approve or reject them with a single action, without ever editing raw data myself.

**Acceptance criteria:**
- Agentbox's linked objects viewer renders a `KnowledgeEnrichment` broker case as a two-pane diff: source content (left) vs proposed enrichment (right)
- The diff is human-readable: markdown rendering for `.md` changes, syntax-highlighted Turtle for `.ttl` OWL fragments, tabular display for `.embeddings.json` vectors
- Approve/reject/delegate actions are available inline — no navigation to a separate system
- The provenance trailer block (G3) is shown below the diff so the reviewer sees who proposed it, which agent reasoned over it, and the commit that will land
- WebSocket push from VisionClaw's BrokerActor (`broker:new_case`) delivers new cases to the reviewer in real time

### US-7: Agent-mediated data conformance

> As a system operator, I want all knowledge base mutations to pass through AI agents that understand the ontology, the URN scheme, and the RDF shape constraints — so that human users never need to hand-edit triples or worry about data conformance.

**Acceptance criteria:**
- Users interact via voice (agentbox TTS/STT surface) or text (chat, nostr-rust-forum) — never via raw data editing interfaces
- Agent produces a conformant mutation (well-formed Turtle, valid URN, correct `owl:Class` hierarchy) and submits it as a `BrokerCase`
- If the agent's output fails schema validation, the case is auto-rejected before it reaches the broker — the human never sees malformed data
- The mutation trail is fully traceable: user request → agent reasoning → broker approval → git commit → pod state change

### US-8: Nostr-mediated coordination

> As an agent operator, I want write-back approval events, agent status updates, and human feedback to flow through Nostr relays — so that the coordination plane works across system boundaries using the same identity and messaging infrastructure as the data plane.

**Acceptance criteria:**
- Broker approval emits a Nostr kind-30300 event (existing `AuditEvent` scaffold) to the agentbox relay
- Agents subscribe to approval events filtered by their own `did:nostr` pubkey
- Human users can send feedback on enrichments via the nostr-rust-forum relay — the agent receives it as a Nostr DM (NIP-17) or sealed event (NIP-59)
- All Nostr events carry `did:nostr` identity; no anonymous events in the control plane
- The messaging surface is optional: the system works without it (polling fallback), but Nostr enables real-time push across trust boundaries

---

## Interaction Model: Agent-Mediated, Human-Reviewed

A foundational design principle: **users never touch data directly**. The AI agent layer in agentbox is the sole data manipulation surface. Agents understand ontology constraints, URN minting rules, RDF shape validation, and commit provenance encoding. Humans provide intent (voice, text, chat) and signoff (visual review, approve/reject).

### The flow

```
Human intent (voice / text / chat / nostr message)
    │
    ▼
Agentbox AI agent (reasoning, schema validation, conformance check)
    │
    ▼
Agent produces structured mutation + provenance metadata
    │
    ▼
BrokerCase submitted (CaseCategory::KnowledgeEnrichment)
    │
    ▼
Broker Review Surface (G6) renders visual diff
    │
    ▼
Human reviewer: Approve │ Reject │ Amend │ Delegate │ Promote │ Precedent
    │
    ▼ (on Approve/Promote)
Write-Back Saga (G4) commits to source pod with full provenance
    │
    ▼
Nostr event (G7) notifies agent + optional human subscribers
```

### Why not direct data access?

1. **Conformance.** RDF, OWL, and the VisionClaw URN scheme have structural constraints that are easy to violate in a free-text editor. Agents enforce these at mutation time.

2. **Provenance.** Every change needs a full provenance chain (who requested, who reasoned, who approved, which DID signed). Direct editing breaks this chain — there is no agent to attribute the reasoning to, no broker case to reference.

3. **Auditability.** The Judgment Broker's append-only `DecisionHistory` provides a complete audit trail. Direct edits bypass this entirely.

4. **Progressive trust.** The `DecisionOutcome::Precedent` path (ADR-041) allows the system to learn which enrichment types are safe for auto-approval. Over time, routine enrichments (embedding updates, well-understood ontology promotions) can be auto-approved without human review. This is only possible if all mutations flow through the broker — direct edits cannot be auto-approved because they have no agent proposal to pattern-match against.

### Web surface priority

Given the agent-mediated model, the web surfaces serve distinct roles:

| Priority | Surface | Role | Location |
|----------|---------|------|----------|
| **P0** | Broker Review Pane | Visual diff + approve/reject for enrichment cases | Agentbox `/lo/` pane (new: `enrichment-review-pane.js`) |
| **P1** | Agent Event Stream | Watch agents work in real time | Agentbox `/v1/agent-events` (existing) |
| **P1** | Status Dashboard | System health, sync status, remote registry | Agentbox `/v1/status` (existing, extended) |
| **P2** | Linked Objects Browser | Inspect pod data, debug agent output | Agentbox `/lo/` (existing upstream panes) |
| **P2** | Mashlib Data Browser | SolidOS-compatible pod browsing (operator/dev inspection) | solid-pod-rs-server `Accept: text/html` (config-enabled) |
| **P3** | Nostr Chat Surface | Human ↔ agent text coordination | nostr-rust-forum relay (existing) |
| **P3** | 3D Graph Visualisation | Spatial exploration of enrichment impact | VisionClaw client (existing) |

The mashlib and linked objects browser remain valuable for operator inspection and debugging, but the **primary user surface is the Broker Review Pane** — a single-purpose diff viewer that renders agent proposals and collects human decisions.

---

## Architecture

### G1: Git Ingest Adapter

Replaces `GitHubClient` + `EnhancedContentAPI` with a local-clone-based pipeline:

```
Remote Registry (G2)
    │
    ▼
git clone / git fetch (libgit2 via git2 crate, or shelling to git CLI)
    │
    ▼
Local worktree on disk (/app/data/git-ingest/<remote-id>/)
    │
    ▼
Existing parser pipeline (KnowledgeGraphParser, OntologyParser, block_level_parser)
    │
    ▼
IngestSaga (ADR-051) → Pod-first, Neo4j-second
```

Key design decisions:

- **`git2` (libgit2) for clone/fetch.** The `solid-pod-rs-git` crate's `GitHttpService` is a *server-side* service (it handles incoming git requests). For the *client-side* clone/fetch, we use `git2` which handles git smart HTTP protocol natively, including custom auth headers.

- **NIP-98/did:nostr auth via HTTP Basic credentials.** For `did:nostr`-authenticated remotes, `SERVER_NOSTR_PRIVKEY` is used as HTTP Basic credentials to the git transport. `git2::RemoteCallbacks::credentials()` injects these on each HTTP request.

- **Local clone, not streaming.** The existing parser pipeline reads files from disk. Rather than rewriting it to stream from HTTP, we clone to a local directory under `/app/data/git-ingest/` and point the parsers at it. Incremental: `git fetch` + diff only changed files.

- **GitHub backward compat.** GitHub repos are just `https://github.com/owner/repo.git` remotes with PAT auth. The adapter handles them natively. The `EnhancedContentAPI` tree endpoint becomes an optimisation path (single API call vs full clone) that can be deprecated once operators migrate to git-clone.

### G2: DID-Gated Remote Registry

A persistent registry of configured knowledge sources:

```rust
pub struct GitRemote {
    pub id: String,                          // uuid
    pub url: String,                         // git remote URL
    pub auth: RemoteAuth,                    // PAT | DidNostr | None
    pub owner_did: Option<String>,           // did:nostr:<hex> of pod owner
    pub base_paths: Vec<String>,             // subdirs to ingest (like GITHUB_BASE_PATHS)
    pub branch: String,                      // default: "main"
    pub sync_interval_secs: u64,             // 0 = manual only
    pub writeback_enabled: bool,             // default: false
    pub last_sync: Option<DateTime<Utc>>,
    pub last_commit_sha: Option<String>,     // for incremental fetch
}

pub enum RemoteAuth {
    /// No auth (public repos, public pod paths)
    None,
    /// GitHub/GitLab personal access token (legacy compat)
    Pat { token_env_var: String },
    /// did:nostr NIP-98 auth against a Solid pod
    DidNostr {
        /// VisionClaw's server keypair signs the NIP-98 events.
        /// The pod owner must have granted this DID read (and optionally write) access.
        server_identity: bool,
        /// Optional: override with a per-remote keypair
        keypair_env_var: Option<String>,
    },
}
```

REST API (implemented):
- `GET /api/ingest/remotes` — list configured remotes
- `POST /api/ingest/remotes` — register a new remote
- `DELETE /api/ingest/remotes/{id}` — remove
- `POST /api/ingest/sync` — trigger sync (supports `sync_by_id()` and `sync_all()`)
- `POST /api/ingest/writeback` — trigger write-back saga for approved enrichments

Env migration: existing `GITHUB_TOKEN`, `GITHUB_OWNER`, `GITHUB_REPO`, `GITHUB_BASE_PATH` are read at startup and auto-registered as a PAT-authenticated remote with `id = "legacy-github"`. New deployments use the REST API or a `GIT_REMOTES` JSON env var.

### G3: Provenance Commit Encoder

Every machine-generated commit carries structured provenance:

```
feat(ontology): promote vc:bc/smart-contract to OWL class

Enrichment applied by VisionClaw ingest pipeline.

Urn: urn:visionclaw:concept:1a2b3c...:smart-contract
Proposed-by: did:nostr:4d5e6f...
Approved-by: did:nostr:7a8b9c...
Broker-case: case-2026-05-08-001
Decision: approve
Reasoning-hash: sha256:abc123...
Timestamp: 2026-05-08T14:32:00Z
Signed-off-by: did:nostr:1a2b3c...
```

The encoder:
- Mints `urn:visionclaw:` URIs using the existing `src/uri/mint.rs` infrastructure
- Includes the `did:nostr` of the proposing agent (if agentbox-originated) or the system identity
- Includes the `did:nostr` of the approving broker from the `DecisionHistoryEntry`
- Hashes the broker's reasoning text (SHA-256) for tamper-evidence without leaking full text
- Authenticates the `git push` HTTP request via `SERVER_NOSTR_PRIVKEY` as HTTP Basic credentials, binding the transport to the pushing identity

### G4: Write-Back Saga

Fully implemented. Extends the existing IngestSaga (ADR-051) with a reverse flow:

```
Discovery Engine (PRD-009) / Ontology Pipeline / Agent proposal
    │
    ▼
BrokerCase created (CaseCategory::KnowledgeEnrichment — new variant)
    │
    ▼
Broker reviews in Decision Canvas
    │
    ▼
DecisionOutcome::Approve or ::Promote
    │
    ▼
WriteBackSaga::execute(remote_id, enrichment_payload, decision_report)
    ├─ Phase 1: git fetch latest from remote (ensure no conflicts)
    ├─ Phase 2: Apply enrichment to local worktree
    │   ├─ Ontology promotion: write OWL fragment as .ttl alongside .md
    │   ├─ Embedding update: write vector to .embeddings.json sidecar
    │   ├─ Gap detection: write proposed-edge as .proposals.md
    │   └─ Agent reasoning: write structured annotation
    ├─ Phase 3: Commit with provenance trailers (G3)
    ├─ Phase 4: git push to remote (auth via SERVER_NOSTR_PRIVKEY)
    └─ Phase 5: Record push result in Neo4j (audit trail)
```

New `CaseCategory` variant for the Judgment Broker:

```rust
pub enum CaseCategory {
    ContributorMeshShare,
    WorkflowReview,
    PolicyException,
    TrustAlert,
    ManualSubmission,
    KnowledgeEnrichment,  // PRD-013 write-back gating
}
```

The `KnowledgeEnrichment` category carries a `SubjectRef` pointing at the enriched `KGNode` or `OntologyClass`, with `from_state` and `to_state` representing the enrichment type (e.g., `None → Embedding`, `KGNode → OntologyClass`, `None → ProposedEdge`).

### G5: Agentbox Pod Bridge

Agentbox agents operate in isolated Nix containers with their own `did:nostr` keypairs. The BC20 anti-corruption layer (existing, per DDD context map) mediates between agentbox's `urn:agentbox:*` namespace and VisionClaw's `urn:visionclaw:*` namespace.

For git ingest, the bridge:

1. **Exposes a git clone endpoint** to agents via the management API (port 9190). Agents request a clone of a registered remote; the bridge performs the clone using VisionClaw's credentials and mounts the worktree into the agent's sandbox.

2. **Collects agent commits** after reasoning completes. The agent commits to a local branch; the bridge reads the commits, validates provenance trailers, and submits a `BrokerCase` for human review.

3. **Relays approval events** via the embedded nostr-rs-relay. When the broker approves, a Nostr event (kind 30300, per existing `AuditEvent` scaffold in IngestSaga) notifies the agent. The bridge then pushes the approved commits to the source remote.

4. **Nostr messaging surface** (optional, deferred). Human ↔ agent ↔ human messaging around write-back decisions can flow through the nostr-rs-relay. The nostr-rust-forum's relay is already connected. This surface is underdeveloped and not required for MVP, but the event plumbing is in place for it.

### G6: Broker Review Surface

The primary human interface for this PRD. A new agentbox viewer pane (`enrichment-review-pane.js`) that renders `KnowledgeEnrichment` broker cases as visual diffs with inline approval actions.

**Data flow:**

```
VisionClaw BrokerActor
    │ WebSocket: broker:new_case / broker:case_decided
    ▼
Agentbox management API (proxied or direct WS subscription)
    │
    ▼
enrichment-review-pane.js (linked objects viewer at /lo/)
    │ Renders: two-pane diff (source ← → enrichment)
    │ Shows: provenance trailers, agent identity, reasoning summary
    │ Actions: Approve / Reject / Amend / Delegate / Promote / Precedent
    ▼
POST /api/broker/cases/:id/decide (VisionClaw REST)
    │
    ▼
DecisionOrchestrator → WriteBackSaga (G4)
```

**Pane contract** (per agentbox's existing pane system):

```js
export default {
  id: 'enrichment-review',
  label: 'Enrichment Review',
  icon: '⚖',
  surface: 'S12',
  matches: [{ '@type': 'agbx:KnowledgeEnrichmentCase' }],
  canHandle(subject, store) { /* check for broker case type */ },
  render(subject, store, container, rawData) {
    // Two-pane diff: rawData.source_content vs rawData.proposed_enrichment
    // Provenance trailer block below
    // Action buttons wired to VisionClaw broker REST API
  },
};
```

The pane consumes the same JSON-LD representation that the S05 provenance surface (existing) and S01 pods surface (existing) already produce. No new linked-data surface needed — the pane composes existing surfaces into a review workflow.

For VisionClaw's existing React client, the Broker Inbox (ADR-041 implementation) already has REST routes (`GET /api/broker/inbox`, `POST /api/broker/cases/:id/decide`) and WebSocket events (`broker:new_case`, `broker:case_decided`, `broker:case_claimed`). The agentbox pane calls these directly. No new backend routes required.

### G7: Nostr Control Plane

The Nostr messaging layer connects the system's coordination events across trust boundaries. Three event kinds serve the control plane:

| Kind | Purpose | Producer | Consumer |
|------|---------|----------|----------|
| 30300 | Audit event (broker decision recorded) | VisionClaw ServerNostrActor | Agentbox agents, external subscribers |
| 30301 | Enrichment proposal (agent submits for review) | Agentbox agent | VisionClaw BrokerActor |
| 4 (NIP-17) | Human ↔ agent text coordination | Any Nostr client | Agentbox agent, nostr-rust-forum |

**Relay topology:**

```
Agentbox embedded relay (nostr-rs-relay)
    ↕ NIP-42 AUTH gate (did:nostr pubkey allowlist)
VisionClaw ServerNostrActor
    ↕ Subscription filter: kinds [30300, 30301, 4]
nostr-rust-forum relay (external, optional)
    ↕ Public-facing; human ↔ human messaging
External Nostr relays (optional)
    ↕ For cross-system federation per PRD-010
```

**IS-Envelope mapping (ADR-075):**

Enrichment proposals and broker decisions map to IS-Envelope v1 kinds:

| IS-Envelope kind | Nostr kind | Use case |
|------------------|------------|----------|
| `tool_invoke` | 30301 | Agent submits enrichment for review |
| `tool_result` | 30300 | Broker decision result |
| `chat` | 4 | Human ↔ agent coordination |
| `knowledge_link` | 30078 | Cross-pod knowledge graph link announcement |

All events are NIP-59 gift-wrapped on the wire when crossing relay boundaries. Within the agentbox ↔ VisionClaw boundary (same network), plain signed events suffice.

**Gating:** The Nostr control plane is optional. The system works without it — the broker REST API + WebSocket is the primary path. Nostr adds:
- Push notifications across trust boundaries (external brokers, federated agents)
- Human feedback loop via any Nostr client (not just the agentbox UI)
- Agent-to-agent coordination for multi-agent enrichment workflows
- Audit event durability (events persisted in relay, not just VisionClaw's Neo4j)

---

## Dependency Version Alignment

### solid-pod-rs bump

| Consumer | Current | Target | Features to add |
|----------|---------|--------|-----------------|
| **VisionClaw** (Cargo.toml) | `0.4.0-alpha.1` | `0.4.0-alpha.5` | `did-nostr`, `config-loader`, `acl-origin`, `webhook-signing` |
| **Agentbox** (lib/solid-pod-rs.nix) | `0.4.0-alpha.1+sprint-9` (rev `7f8bc89`) | `0.4.0-alpha.5` (rev `298818e`) | `mashlib` module available; Nix hash refresh required |
| **nostr-bbs-pod-worker** | `workspace = true` (already at alpha.5 in kit) | No change | Already aligned |

### New dependencies

| Crate | Purpose | Consumer |
|-------|---------|----------|
| `git2` | libgit2 bindings for clone/fetch/push | VisionClaw |
| `solid-pod-rs-git` | Git auth types (`BasicNostrExtractor`, `GitAuth` trait) — used only for type compat, not the server service | VisionClaw |
| `solid-pod-rs-nostr` | `NostrWebIdResolver` for DID ↔ WebID resolution on remote registration | VisionClaw |

### Env var changes

| Variable | Status | Purpose |
|----------|--------|---------|
| `GITHUB_TOKEN` | Deprecated (still read for legacy shim) | GitHub PAT |
| `GITHUB_OWNER` | Deprecated | GitHub repo owner |
| `GITHUB_REPO` | Deprecated | GitHub repo name |
| `GITHUB_BASE_PATH` | Deprecated | Subdirectory filter |
| `GIT_REMOTES` | **New** (optional) | JSON array of `GitRemote` configs for bootstrapping |
| `GIT_INGEST_ROOT` | **New** | Local clone storage path (default: `/app/data/git-ingest/`) |
| `WRITEBACK_ENABLED` | **New** | Global kill-switch for write-back (default: `false`) |
| `SERVER_NOSTR_PRIVKEY` | Existing | Used for NIP-98 signing on git operations |
| `POD_BASE_URL` | Existing | Used for local pod write-back (IngestSaga) |

---

## Migration Path

### Phase 1: Git Ingest Adapter (G1 + G2) — COMPLETE

- `GitIngestService` implemented with `git2` clone/fetch in `src/services/git_ingest/`
- Remote registry with Neo4j persistence (`remote_registry.rs`)
- Legacy GitHub shim: auto-registers `GITHUB_*` env vars as a PAT remote on startup
- Parser pipeline reads from local worktree (no changes to parsers)
- `GitIngestService` exposes `sync_by_id()` and `sync_all()` methods
- REST routes wired: `GET /api/ingest/remotes`, `POST /api/ingest/remotes`, `DELETE /api/ingest/remotes/{id}`, `POST /api/ingest/sync`

### Phase 2: DID-Gated Auth + Solid Pod Support (G2 extension) — COMPLETE

- NIP-98/did:nostr auth implemented via `SERVER_NOSTR_PRIVKEY` as HTTP Basic credentials to the git transport
- REST API for remote management fully wired
- Git ingest is the default path

### Phase 3: Write-Back Saga (G3 + G4) — COMPLETE

- `KnowledgeEnrichment` case category in Judgment Broker
- Provenance commit encoder (`provenance.rs`)
- Write-back saga fully implemented with 5 phases:
  1. Fetch latest from remote (ensure no conflicts)
  2. Apply enrichment to local worktree
  3. Commit with provenance trailers (G3)
  4. Push with NIP-98/did:nostr auth
  5. Record audit trail in Neo4j
- REST route wired: `POST /api/ingest/writeback`
- Feature flag: `WRITEBACK_ENABLED=true` opt-in per remote

### Phase 4: Agentbox Bridge (G5) — Implemented

- BC20 adapter: `agentbox/management-api/routes/git-bridge.js`
- Agent clone, commit collection, and enrichment submission via `/api/enrichment-proposals`
- Agent DID identity binding (impersonation guard)
- Webhook HMAC-SHA256 verification, filtered git subprocess env
- Path traversal and argument injection defences
- Agent provenance trailers (`Proposed-by: did:nostr:<agent>`)

### Phase 5: Broker Review Surface (G6) — Implemented

- `enrichment-review-pane.js` in agentbox viewer panes
- `agentbox/management-api/routes/broker-bridge.js`: SSE bridge from VisionClaw BrokerActor
- Decide endpoint calls `POST /api/enrichment-proposals/{id}/decide`
- SSE connection cap (MAX_SSE_CONNECTIONS), event type sanitisation
- .git/ path blocking, 5xx error scrubbing, CORS origin allowlist
- Real-time case delivery via `broker:new_case` WebSocket event

### Phase 6: Nostr Control Plane (G7) — Implemented

- Kind 30300 event emission from VisionClaw `ServerNostrActor` (`SignBrokerDecision`)
- BrokerActor emits Nostr events for all KnowledgeEnrichment decisions
- Agent subscription to approval events via agentbox embedded relay
- NIP-42 AUTH gate on agentbox relay (did:nostr pubkey allowlist)
- IS-Envelope v1 mapping for cross-relay event federation
- HTTPS enforcement for DidNostr auth transport

### Phase 7: Deprecate GitHub REST API + Convergence — Planned

- Remove `EnhancedContentAPI`, `GitHubClient`
- Remove `GITHUB_*` env vars from `.env.example`
- Migration guide for operators
- Auto-approval via `DecisionOutcome::Precedent` for routine enrichment types

---

## Non-Functional Requirements

| ID | Requirement | Target |
|----|-------------|--------|
| NFR-1 | Incremental sync latency (fetch + parse changed files) | < 30s for 1000-file repo with 10 changed files |
| NFR-2 | Full clone + ingest of 2000-file knowledge base | < 5 min |
| NFR-3 | Write-back commit round-trip (broker approve → push lands) | < 10s |
| NFR-4 | Concurrent remote syncs | Up to 10 remotes syncing in parallel |
| NFR-5 | Storage overhead per remote | < 2x raw repo size (local clone + index) |
| NFR-6 | Auth credential isolation | Server keypair in memory only; no PATs written to disk |

---

## Security Considerations

1. **NIP-98 replay protection.** Each git HTTP request carries a fresh NIP-98 event with a bounded `created_at` window. The pod's `NIP98_REPLAY` KV store (or solid-pod-rs's built-in jti cache) prevents replay.

2. **WAC enforcement on push.** Write-back requires `acl:Write` or `acl:Append` on the target container. The pod enforces this server-side; VisionClaw does not bypass WAC.

3. **Broker-gated mutation.** No write-back occurs without a `DecisionOutcome::Approve` or `::Promote` from a human broker. The `self-review` invariant (ADR-041) prevents the proposing agent from approving its own enrichment.

4. **Provenance non-repudiation.** Commit trailers are append-only in git history. The `Reasoning-hash` allows verification without exposing the full reasoning text. The NIP-98 transport signature binds the push to a specific `did:nostr` identity.

5. **Path traversal.** `solid-pod-rs-git`'s `guard::path_safe` rejects traversal attempts. On the client side, `git2` handles path safety internally. Local clone paths are sandboxed under `GIT_INGEST_ROOT`.

---

## Relationship to Existing Architecture

| Component | Relationship |
|-----------|-------------|
| **IngestSaga (ADR-051)** | G4 extends the saga with a reverse (write-back) flow. Phase 1 (pod write) becomes Phase 4 (git push). Same two-phase-commit semantics. |
| **Judgment Broker (ADR-041)** | G4 adds `CaseCategory::KnowledgeEnrichment`. The six decision outcomes all apply: Approve triggers push, Reject blocks it, Amend modifies the enrichment, Delegate routes to a domain expert, Promote elevates and pushes, Precedent flags for future auto-approval. |
| **Insight Migration Loop (ADR-049)** | The promotion tutorial's "opens a GitHub pull request" step becomes "commits to the source pod via write-back saga". No PR needed when the source is a pod — the commit *is* the mutation. For GitHub-hosted sources, the PR path remains available as an alternative. |
| **Discovery Engine (PRD-009)** | Embeddings, gap detection, and related-node proposals become write-back candidates. Each discovery output can generate a `BrokerCase` for review. |
| **URN namespace (src/uri/)** | G3 uses `urn:visionclaw:concept:<pubkey>:<slug>` for commit provenance. The mint/parse infrastructure is reused as-is. |
| **Feature Pipeline (PRD-009)** | N-hop materialised edges and KGE vectors are write-back candidates (`.embeddings.json` sidecar format). |
| **Nostr-rust-forum relay** | Optional messaging surface for write-back notifications. Not required for MVP. The embedded relay in agentbox can bridge events. |
| **Mashlib (solid-pod-rs 0.4.0-alpha.5)** | P2 inspection surface. Enabled in solid-pod-rs-server config when agentbox bumps to alpha.5. Complements the linked objects browser — mashlib serves SolidOS-compatible views, `/lo/` serves agent-centric panes. Operators use it for pod data debugging; users use the Broker Review Pane (G6) for signoff. |
| **Linked Objects Browser (agentbox /lo/)** | P0 host for the Broker Review Pane (G6). The existing pane system (6 panes: VC, provenance, capability, runtime, DCAT, handoff) gains a 7th pane (`enrichment-review-pane.js`). The LOSOS pane contract is the stable extension API. |
| **ServerNostrActor (VisionClaw)** | G7 extends with kind 30300/30301 event emission. Currently handles server identity and NIP-98 signing; gains control-plane event publishing. |
| **Nostr-rs-relay (agentbox embedded)** | G7 uses as the coordination relay between agents and VisionClaw. NIP-42 AUTH gate restricts to known `did:nostr` pubkeys. Already provisioned in agentbox Nix config. |
| **Beads (Nostr-signed audit records)** | Each broker decision is already recorded as a Nostr-signed bead per ADR-049. G3 extends beads into git commit trailers — the commit provenance chain and the bead chain are parallel audit surfaces over the same decision event. |
| **`urn:visionclaw:*` / `urn:agentbox:*` namespaces** | G3 mints `urn:visionclaw:` URIs in commit trailers. G5 maps between `urn:agentbox:*` (agent-side) and `urn:visionclaw:*` (substrate-side) via the BC20 anti-corruption layer. Both namespaces share `did:nostr:<hex>` as the identity primitive. |
| **`did:nostr` identity (ADR-074)** | The universal identity primitive across all components. G2 uses it for remote auth. G3 embeds it in commit trailers. G5 uses agent DIDs for provenance. G6 displays reviewer DIDs. G7 filters relay subscriptions by DID. Every layer of authority is DID-bound. |

---

## Open Questions

1. **Conflict resolution on push.** If the source pod has diverged since our last fetch, do we rebase, merge, or fail? Proposal: fail-and-notify on conflict; the broker re-reviews after manual resolution. Auto-merge is dangerous for knowledge bases.

2. **Enrichment file format standardisation.** Should `.embeddings.json` use a standard format (e.g., ONNX embedding metadata, or a JSON-LD `@type: EmbeddingVector`)? Or is a VisionClaw-specific schema acceptable for MVP?

3. **Multi-remote write-back.** If the same node is ingested from two remotes (e.g., GitHub mirror + Solid pod), which remote receives the write-back? Proposal: the remote marked as `writeback_enabled = true`; if multiple, the one with the most recent `last_sync`.

4. **Precedent-based auto-approval.** The `DecisionOutcome::Precedent` path (ADR-041) could enable auto-approval for enrichment types that have been approved N times. This reduces broker fatigue for routine embedding updates. Deferred to Phase 7.

---

## Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Remotes using git-over-HTTP (vs GitHub REST) | 100% by Phase 7 | Remote registry type distribution |
| Write-back adoption | 30% of remotes with writeback enabled by Phase 4 | Registry config audit |
| Broker enrichment case throughput | 50 cases/day at steady state | BC11 metrics |
| Provenance coverage | 100% of machine-generated commits carry valid trailers | Git log audit |
| Review surface adoption | 80% of broker decisions made via G6 pane (vs REST/CLI) | Pane usage analytics |
| Agent-mediated mutation rate | 100% of data mutations flow through agent + broker | Audit trail gap analysis |
| Nostr control plane coverage | 90% of approval events delivered via relay (Phase 6+) | Relay event count vs broker decision count |
| Time to deprecate GitHub REST path | Phase 7 (Sprint 7) | Code removal commit |
| Mean review time per enrichment case | < 30s for routine, < 5 min for promotions | BC11 decision latency histogram |
| Auto-approval rate (Precedent path) | 40% of routine enrichments by Phase 7 | Precedent-matched case ratio |

---

## Identity and Authority Model

Every layer of the system is bound to `did:nostr:<64-lowercase-hex>` identities (per ADR-074). The authority model is:

```
Pod Owner (did:nostr:<owner>)
    │ WAC ACL grants read/write to ↓
    ▼
VisionClaw Server Identity (did:nostr:<server>)
    │ Signs NIP-98 on git clone/push; delegates to ↓
    ▼
Agentbox Agent Identity (did:nostr:<agent>)
    │ Proposes enrichments; commits carry Proposed-by trailer
    │ Cannot push without approval from ↓
    ▼
Human Broker (did:nostr:<broker>)
    │ Reviews via G6 pane; decision carries Approved-by trailer
    │ Cannot self-review (ADR-041 invariant)
    ▼
Git Commit (signed push via server identity, provenance trailers reference all DIDs)
    │
    ▼
Pod (receives commit; WAC enforces write permission; relay emits audit event)
```

The four DID-bearing participants (owner, server, agent, broker) form a trust chain. Each is independently verifiable via the `did:nostr` DID Document at `/.well-known/did/nostr/<hex>.json`. NIP-26 delegation (per ADR-074 D1) allows the server identity to delegate to agent identities without the pod owner needing to ACL each agent individually.

### Bead chain

Every broker decision produces a bead (per ADR-049): a Nostr-signed JSON object carrying the decision outcome, the case id, the broker pubkey, and the reasoning hash. The git commit trailer and the bead are parallel representations of the same event:

| Representation | Durability | Audience |
|----------------|------------|----------|
| Git commit trailer | Permanent (in repo history) | Anyone with repo access |
| Nostr bead (kind 30300) | Relay-durable (persisted until relay GC) | Relay subscribers |
| Neo4j `DecisionHistoryEntry` | Application-durable | VisionClaw queries |

All three reference the same `case_id` and `decision_id`, so they can be cross-referenced. The git commit is the source of truth for the data change; the bead is the source of truth for the decision event; Neo4j is the queryable index.
