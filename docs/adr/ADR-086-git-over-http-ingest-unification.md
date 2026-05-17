# ADR-086 --- Git-Over-HTTP Ingest Unification

| Field | Value |
|-------|-------|
| Status | Implemented |
| Date | 2026-05-08 |
| Implemented | 2026-05-08 |
| Drives | PRD-013 (G1--G7) |
| Supersedes | GitHub REST API ingest (`src/services/github/`) |
| Companion PRD | `docs/PRD-013-solid-git-ingest-surface.md` |
| Companion ADRs | ADR-041, ADR-049, ADR-051, ADR-074, ADR-075 |
| DDD Contexts | BC2 (Graph Data), BC11 (Judgment Broker), BC13 (Discovery), BC20 (Agentbox Integration) |

## Context

VisionClaw's knowledge graph ingest pipeline is hard-wired to the GitHub REST
API. `GitHubClient` and `EnhancedContentAPI` (under `src/services/github/`)
call `api.github.com` with a personal access token to list files, diff SHAs, and
pull markdown content. This architecture was sufficient when VisionClaw consumed
a single Logseq graph hosted on GitHub, but it now blocks three strategic
directions:

1. **Vendor lock-in.** Knowledge bases hosted on GitLab, Codeberg, Gitea,
   self-hosted bare repos, or Solid pods are unreachable. Every knowledge source
   must be a GitHub repository. The `GITHUB_TOKEN` / `GITHUB_OWNER` /
   `GITHUB_REPO` env vars encode this single-vendor assumption.

2. **No write-back path.** The pipeline is pull-only. Enrichments produced by
   the Discovery Engine (PRD-009) --- embeddings, gap detection, ontology
   promotions --- stay trapped in Neo4j. The knowledge base author never sees the
   system's work committed back to the source of truth. The Insight Migration
   Loop (ADR-049) works around this by opening GitHub PRs, but that path is
   GitHub-specific and does not generalise.

3. **No decentralised identity at the transport layer.** GitHub PATs are bearer
   tokens scoped to a single GitHub account. There is no mechanism for a
   third-party pod owner to grant VisionClaw read or write access using
   `did:nostr` identity (ADR-074). Access control is GitHub's RBAC, not the
   pod owner's WAC policy.

Meanwhile, `solid-pod-rs` 0.4.0-alpha.5 now ships three composable primitives
that dissolve all three limitations:

- **`solid-pod-rs-git`** --- git smart HTTP backend (clone/push/pull against pod
  storage, NIP-98 auth on push).
- **`solid-pod-rs-nostr`** --- `did:nostr` <-> WebID bidirectional resolver +
  embedded NIP-01/11/16 relay.
- **`mashlib`** module --- SolidOS data-browser rendering of RDF resources.

The architectural insight is that any Solid pod with `git-http-backend` enabled
is simultaneously a Git repo, an LDP server, and a mashlib-renderable website.
GitHub becomes just another git remote --- not a special-cased API. A knowledge
base hosted on a Solid pod can be cloned, ingested, enriched, and pushed back
using standard git semantics, with access control mediated by `did:nostr`
pubkeys and NIP-98 signatures rather than platform-specific tokens.

This unification also unlocks bidirectional sync for agentbox: AI agents can
clone a pod's knowledge base, reason over it, commit enrichments with full
URN/DID provenance in each commit, and push those enrichments back --- all gated
through the Judgment Broker (ADR-041) to prevent unreviewed mutations.

## Decision Drivers

- **D1 -- Vendor lock-in removal.** The system must ingest from any git remote,
  not just GitHub. Solid pods, GitLab, Codeberg, Gitea, and bare repos on a VPS
  must all be first-class sources.

- **D2 -- Write-back capability.** Enrichments (embeddings, ontology promotions,
  discovered edges, agent reasoning artefacts) must flow back to the source
  repository as auditable commits, gated by human review.

- **D3 -- Decentralised identity at every layer.** `did:nostr:<hex>` (ADR-074)
  must mediate access control, commit provenance, and transport authentication.
  No platform-specific tokens for pod-hosted knowledge bases.

- **D4 -- Agent-mediated mutation model.** Users never touch data directly. AI
  agents in agentbox are the sole data manipulation surface. Agents understand
  ontology constraints, URN minting rules, RDF shape validation, and commit
  provenance encoding. Humans provide intent and signoff.

- **D5 -- Broker-gated write-back.** No write-back commit reaches the source
  pod without a `DecisionOutcome::Approve` or `DecisionOutcome::Promote` from
  the Judgment Broker (ADR-041). The self-review invariant prevents the
  proposing agent from approving its own enrichment.

- **D6 -- Audit representation.** Every machine-generated commit must carry a
  triple representation: git commit trailer (permanent in repo history), Nostr
  bead (relay-durable, kind 30300), and Neo4j `DecisionHistoryEntry`
  (queryable). All three reference the same `case_id`.

## Considered Options

### Option 1: Git-over-HTTP unification with DID auth (chosen)

Replace the GitHub REST API ingest with a **git-over-HTTP ingest surface** that
treats every knowledge source identically via `git clone` / `git fetch` / `git
push`. Layer `did:nostr` + NIP-98 authentication for Solid pod remotes.
Introduce a Remote Registry for multi-source management. Gate write-back through
the Judgment Broker. Emit provenance as structured git commit trailers. Expose a
Nostr control plane for cross-system coordination.

- **Pros**: Universal ingest surface. Write-back with full provenance. DID
  identity at every layer. Agent autonomy with broker oversight. Existing parsers
  unchanged (they read files from disk; the adapter clones to a local worktree).
  GitHub becomes just another remote with PAT auth, preserving backward
  compatibility.

- **Cons**: Adds `git2` (libgit2) as a new native dependency. Migration period
  where both GitHub REST and git-clone paths coexist. Agentbox Nix hash refresh
  required for the `solid-pod-rs` bump.

### Option 2: Keep GitHub REST API + add Solid LDP client as separate path

Retain the existing `GitHubClient` / `EnhancedContentAPI` pipeline for GitHub
sources. Add a parallel Solid LDP client that reads pod resources via HTTP GET
with `Accept: text/turtle` or `Accept: text/markdown`. Two separate ingest
paths, merged at the parser layer.

- **Pros**: No change to existing GitHub ingest. LDP client is conceptually
  simple.

- **Cons**: Two code paths to maintain, test, and debug. LDP reads individual
  resources; there is no built-in delta/diff mechanism comparable to `git fetch`.
  Incremental sync requires re-implementing SHA-based change detection over LDP
  (duplicating what git does natively). Write-back via LDP PUT is possible but
  lacks commit semantics, history, and merge conflict detection. No unified
  provenance model.

### Option 3: Webhook-based push model

Instead of VisionClaw pulling from remotes, configure GitHub/GitLab/pod webhooks
to push change events to VisionClaw. VisionClaw fetches only changed files on
notification.

- **Pros**: Near-real-time ingest. Efficient (only changed files). Well-
  supported by GitHub and GitLab.

- **Cons**: Requires public-facing webhook endpoint (security surface). Solid
  pods have no standardised webhook mechanism (LDN notifications exist but are
  not widely implemented for git events). Does not solve write-back. Does not
  solve DID-gated access. Does not generalise to bare git repos. VisionClaw
  would still need the git clone path for initial ingest.

## Decision

**Option 1 --- Git-over-HTTP unification with DID auth.**

Seven components implement the decision. Each is detailed in PRD-013; this
section records the architectural rationale and key invariants.

### G1: Git Ingest Adapter --- Implemented 2026-05-08

**Location:** `src/services/git_ingest/`

Replaces `GitHubClient` + `EnhancedContentAPI` with a local-clone-based
pipeline. The adapter uses `git2` (libgit2 Rust bindings) for clone, fetch, and
push operations over the git smart HTTP protocol. For `did:nostr`-authenticated
remotes, NIP-98 auth headers are injected via `git2::RemoteCallbacks::
credentials()`, matching the scheme expected by `solid-pod-rs-git`'s
`BasicNostrExtractor`.

Key invariant: **the adapter clones to a local worktree under
`GIT_INGEST_ROOT` (default `/app/data/git-ingest/<remote-id>/`) and points the
existing parser pipeline at it.** `KnowledgeGraphParser`, `OntologyParser`, and
`block_level_parser` read files from disk --- they do not change. The git layer
is a transport concern, not a parsing concern.

Incremental sync uses `git fetch` + `git diff --name-only <old-sha>..<new-sha>`
to identify changed files, preserving the SHA1-based incremental filtering
semantics of the existing `GitHubSyncService`.

GitHub backward compatibility: GitHub repos are `https://github.com/owner/
repo.git` remotes with PAT auth. The adapter handles them natively. The
`EnhancedContentAPI` tree endpoint becomes an optimisation path (single API call
vs full clone) that is deprecated once operators migrate.

### G2: DID-Gated Remote Registry --- Implemented 2026-05-08

**Location:** `src/services/git_ingest/remote_registry.rs`

A persistent registry of configured knowledge sources, stored in Neo4j.
Each `GitRemote` record carries a URL, auth type (`None` | `Pat` | `DidNostr`),
owner DID, base paths, branch, sync interval, and write-back toggle.

Existing `GITHUB_TOKEN` / `GITHUB_OWNER` / `GITHUB_REPO` / `GITHUB_BASE_PATH`
env vars are read at startup and auto-registered as a PAT-authenticated remote
with `id = "legacy-github"`. New deployments use the REST API
(`POST /api/ingest/remotes`) or a `GIT_REMOTES` JSON env var.

For `did:nostr`-authenticated remotes, the registry uses
`solid-pod-rs-nostr`'s `NostrWebIdResolver` to validate that the configured
DID resolves to a WebID with the expected `verificationMethod.type =
SchnorrSecp256k1VerificationKey2019` (per ADR-074 D1).

### G3: Provenance Commit Encoder --- Implemented 2026-05-08

**Location:** `src/services/git_ingest/provenance.rs`

Every machine-generated commit carries structured provenance as git trailers:

```
Urn: urn:visionclaw:concept:<pubkey>:<slug>
Proposed-by: did:nostr:<agent-hex>
Approved-by: did:nostr:<broker-hex>
Broker-case: case-<date>-<seq>
Decision: approve
Reasoning-hash: sha256:<hex>
Timestamp: <ISO 8601>
Signed-off-by: did:nostr:<server-hex>
```

The encoder mints `urn:visionclaw:` URIs using the existing `src/uri/mint.rs`
infrastructure. It includes the `did:nostr` of the proposing agent (if
agentbox-originated) or the system identity. The broker's reasoning text is
hashed (SHA-256) for tamper-evidence without leaking full text into the public
git history. The `git push` HTTP request itself is signed with NIP-98, binding
the transport to the pushing identity.

Triple audit representation: for every broker-approved enrichment, the system
produces three durable records referencing the same `case_id` and
`decision_id`:

| Representation | Durability | Audience |
|----------------|------------|----------|
| Git commit trailer | Permanent (in repo history) | Anyone with repo access |
| Nostr bead (kind 30300) | Relay-durable (persisted until relay GC) | Relay subscribers |
| Neo4j `DecisionHistoryEntry` | Application-durable | VisionClaw queries |

### G4: Write-Back Saga --- Implemented 2026-05-08

**Location:** `src/services/git_ingest/writeback_saga.rs`

Extends the existing visibility-transition saga pattern (ADR-051) with a
reverse flow: enrichments flow from Neo4j back to the source pod as
broker-approved git commits. The saga has five phases:

1. `git fetch` latest from remote (conflict detection).
2. Apply enrichment to local worktree (file format depends on enrichment type:
   `.ttl` sidecar for ontology promotions, `.embeddings.json` for vectors,
   `.proposals.md` for gap detection).
3. Commit with provenance trailers (G3).
4. `git push` to remote with NIP-98-signed HTTP request.
5. Record push result in Neo4j (audit trail).

Write-back is gated: only `DecisionOutcome::Approve` or
`DecisionOutcome::Promote` from the Judgment Broker triggers the saga. A new
`CaseCategory::KnowledgeEnrichment` variant is added to the broker's case
taxonomy. The `KnowledgeEnrichment` category carries a `SubjectRef` pointing
at the enriched `KGNode` or `OntologyClass`, with `from_state` and `to_state`
representing the enrichment delta.

Conflict policy: if the source pod has diverged since the last fetch, the saga
**fails and notifies** the broker. No auto-merge or auto-rebase. The broker
re-reviews after manual resolution. This is a deliberate conservatism:
auto-merge is dangerous for knowledge bases where semantic conflicts cannot be
detected by line-level diff.

Write-back is disabled by default (`WRITEBACK_ENABLED=false`). Opt-in is
per-remote via the `writeback_enabled` field on `GitRemote`.

### G5: Agentbox Pod Bridge

**Location:** agentbox adapter surface (BC20 anti-corruption layer)

Agentbox agents operate in isolated Nix containers with their own `did:nostr`
keypairs. The BC20 anti-corruption layer (per DDD context map) mediates between
agentbox's `urn:agentbox:*` namespace and VisionClaw's `urn:visionclaw:*`
namespace.

For git ingest, the bridge:

1. Exposes a git clone endpoint to agents via the management API (port 9190).
   Agents request a clone of a registered remote; the bridge clones using
   VisionClaw's credentials and mounts the worktree into the agent's sandbox.
2. Collects agent commits after reasoning completes. The agent commits to a
   local branch; the bridge reads the commits, validates provenance trailers,
   and submits a `BrokerCase` for human review.
3. Relays approval events via the embedded `nostr-rs-relay`. When the broker
   approves, a Nostr event (kind 30300) notifies the agent. The bridge then
   pushes the approved commits to the source remote.

Agent commits must include `Proposed-by: did:nostr:<agent-hex>` and
`Approved-by: did:nostr:<broker-hex>` trailers. No autonomous push is
permitted without broker signoff (human-in-the-loop invariant).

### G6: Broker Review Surface

**Location:** agentbox panes (`enrichment-review-pane.js`) + VisionClaw
WebSocket

The primary human interface for enrichment signoff. A new agentbox viewer pane
renders `KnowledgeEnrichment` broker cases as two-pane visual diffs with
inline approval actions. The pane consumes the same JSON-LD representation that
the existing S05 provenance surface and S01 pods surface produce. No new
linked-data surface is needed --- the pane composes existing surfaces into a
review workflow.

Data flow: VisionClaw `BrokerActor` emits `broker:new_case` /
`broker:case_decided` WebSocket events -> agentbox management API (proxied WS
subscription) -> `enrichment-review-pane.js` renders diff + provenance trailers
+ action buttons -> `POST /api/broker/cases/:id/decide` (VisionClaw REST) ->
`DecisionOrchestrator` -> `WriteBackSaga` (G4).

The reviewer sees: source content (left pane), proposed enrichment (right pane),
provenance trailer block (below), and Approve / Reject / Amend / Delegate /
Promote / Precedent action buttons. Markdown rendering for `.md` changes,
syntax-highlighted Turtle for `.ttl` OWL fragments, tabular display for
`.embeddings.json` vectors.

### G7: Nostr Control Plane

**Location:** agentbox relay (`nostr-rs-relay`) + VisionClaw `ServerNostrActor`

Three event kinds serve the cross-system coordination plane:

| Kind | Purpose | Producer | Consumer |
|------|---------|----------|----------|
| 30300 | Audit event (broker decision recorded) | VisionClaw `ServerNostrActor` | Agentbox agents, external subscribers |
| 30301 | Enrichment proposal (agent submits for review) | Agentbox agent | VisionClaw `BrokerActor` |
| 4 (NIP-17) | Human <-> agent text coordination | Any Nostr client | Agentbox agent, nostr-rust-forum |

IS-Envelope v1 mapping (per ADR-075):

| IS-Envelope kind | Nostr kind | Use case |
|------------------|------------|----------|
| `tool_invoke` | 30301 | Agent submits enrichment for review |
| `tool_result` | 30300 | Broker decision result |
| `chat` | 4 | Human <-> agent coordination |
| `knowledge_link` | 30078 | Cross-pod knowledge graph link announcement |

Relay topology: agentbox embedded relay (NIP-42 AUTH gate, `did:nostr` pubkey
allowlist) <-> VisionClaw `ServerNostrActor` <-> nostr-rust-forum relay
(optional, human-facing). All events NIP-59 gift-wrapped when crossing relay
boundaries; plain signed events within the agentbox <-> VisionClaw trust
boundary.

The Nostr control plane is optional. The system works without it --- the broker
REST API + WebSocket is the primary path. Nostr adds push notifications across
trust boundaries, human feedback via any Nostr client, agent-to-agent
coordination, and relay-durable audit event persistence.

### Identity and Authority Chain

Every layer of authority is bound to `did:nostr:<64-lowercase-hex>` (per
ADR-074). Four DID-bearing participants form the trust chain:

```
Pod Owner (did:nostr:<owner>)
    | WAC ACL grants read/write to:
    v
VisionClaw Server Identity (did:nostr:<server>)
    | Signs NIP-98 on git clone/push; delegates to:
    v
Agentbox Agent Identity (did:nostr:<agent>)
    | Proposes enrichments; commits carry Proposed-by trailer
    | Cannot push without approval from:
    v
Human Broker (did:nostr:<broker>)
    | Reviews via G6 pane; decision carries Approved-by trailer
    | Cannot self-review (ADR-041 invariant)
    v
Git Commit (signed push via server identity, trailers reference all DIDs)
    |
    v
Pod (receives commit; WAC enforces write permission; relay emits audit event)
```

NIP-26 delegation (per ADR-074 D1) allows the server identity to delegate to
agent identities without the pod owner needing to ACL each agent individually.

## Consequences

### Positive

- **Universal ingest surface.** Any git remote --- GitHub, GitLab, Codeberg,
  Gitea, self-hosted bare repos, Solid pods --- is a first-class knowledge
  source. No vendor-specific API code per platform.

- **Write-back with full provenance.** Enrichments flow back to the source
  repository as auditable commits with structured trailers. The knowledge base
  author sees the system's work in their own `git log`.

- **DID identity at every layer.** Access control, commit provenance, transport
  authentication, and audit events all use `did:nostr` as the identity
  primitive. No platform-specific tokens for pod-hosted sources.

- **Agent autonomy with broker oversight.** AI agents can reason over knowledge
  bases and propose enrichments. The Judgment Broker gates write-back, enforcing
  human review. The `DecisionOutcome::Precedent` path enables progressive
  auto-approval for routine enrichment types as trust is established.

- **Triple audit representation.** Every enrichment decision is recorded in
  three complementary durable stores (git commit trailer, Nostr bead, Neo4j
  entry), each serving a different audience and query pattern.

- **Nostr control plane.** Cross-system coordination (broker decisions, agent
  proposals, human feedback) flows through Nostr relays using the same identity
  and messaging infrastructure as the data plane.

### Negative

- **`git2` native dependency.** The `git2` crate (libgit2 bindings) adds a
  non-trivial native dependency to the Rust build. libgit2 requires OpenSSL
  and zlib at build time. The Docker build already carries both, but
  cross-compilation targets may need adjustment.

- **Migration period.** Both GitHub REST (`GitHubSyncService`) and git-clone
  (`GitIngestService`) paths coexist during the rollout (Phases 1--6).
  Operators running the legacy path must migrate before Phase 7 deprecation.
  The auto-registration of `GITHUB_*` env vars as a `legacy-github` remote
  eases this transition.

- **Agentbox Nix hash refresh.** Bumping `solid-pod-rs` from
  `0.4.0-alpha.1+sprint-9` (rev `7f8bc89`) to `0.4.0-alpha.5` (rev `298818e`)
  in the agentbox Nix derivation requires a Nix hash recalculation. This is
  mechanical but blocks agentbox integration (G5) until performed.

- **Conflict resolution conservatism.** The fail-and-notify policy on push
  conflicts is safe but creates manual work when the source pod has diverged.
  Operators with high-velocity knowledge bases may find this friction point
  significant. Auto-merge is deliberately excluded from this ADR; it may be
  reconsidered in a future ADR if conflict patterns are well-understood.

- **Local disk storage.** Each registered remote requires a local clone under
  `GIT_INGEST_ROOT`. For large knowledge bases (thousands of files, significant
  binary assets), storage consumption is approximately 2x raw repo size (clone +
  index). The `GIT_INGEST_ROOT` path must be on a volume with adequate capacity.

### Neutral

- **Existing parsers unchanged.** `KnowledgeGraphParser`, `OntologyParser`, and
  `block_level_parser` continue to read files from disk. The git layer is a
  transport concern below the parser layer. No parser modifications required.

- **IngestSaga pattern preserved.** The pod-first-Neo4j-second saga pattern
  from ADR-051 is reused. G4 extends it with a reverse flow (Neo4j-to-pod
  write-back) but does not alter the forward flow.

- **BrokerActor WebSocket events unchanged.** The existing `broker:new_case`,
  `broker:case_decided`, and `broker:case_claimed` events (ADR-041
  implementation) are reused by G6. No new WebSocket event types required.

- **URN minting infrastructure reused.** G3 uses the existing `src/uri/mint.rs`
  and `src/uri/parse.rs` infrastructure for `urn:visionclaw:concept:` URIs in
  commit trailers. No changes to the URN grammar.

- **`urn:visionclaw:*` / `urn:agentbox:*` namespace mapping.** G5 maps between
  the two namespaces via the existing BC20 anti-corruption layer. Both namespaces
  share `did:nostr:<hex>` as the identity primitive, so the mapping is
  mechanical (kind + scope translation, not identity translation).

## Migration Path

| Phase | Sprint | Components | Key deliverable | Status |
|-------|--------|------------|-----------------|--------|
| 1 | Sprint 1 | G1, G2 | `GitIngestService` with `git2` clone/fetch; Remote Registry; legacy GitHub shim | **Done** (2026-05-08) |
| 2 | Sprint 2 | G2 extension | NIP-98 auth injection; `NostrWebIdResolver` integration; REST API for remote management | **Done** (2026-05-08) |
| 3 | Sprint 3 | G3, G4 | `KnowledgeEnrichment` case category; provenance encoder; write-back saga with broker gating | **Done** (2026-05-08) |
| 4 | Sprint 4 | G5 | Agentbox pod bridge; agent commit collection; Nostr event relay for approvals | Planned |
| 5 | Sprint 5 | G6 | `enrichment-review-pane.js`; WebSocket bridge; two-pane diff rendering | Planned |
| 6 | Sprint 6 | G7 | Kind 30300/30301 event emission; NIP-42 AUTH gate; IS-Envelope mapping | Planned |
| 7 | Sprint 7 | Deprecation | Remove `EnhancedContentAPI`, `GitHubClient`, `GITHUB_*` env vars; migration guide | Planned |

G1 (`GitIngestService`), G2 (`RemoteRegistry`), G3 (`ProvenanceTrailer`), and
G4 (`WriteBackSaga`) are fully implemented as of 2026-05-08. REST routes are
live at `/api/ingest/*`. The git ingest adapter is no longer feature-flagged;
git ingest is the default path. Write-back (G4) is independently gated by
`WRITEBACK_ENABLED` (default `false`, opt-in per remote). G5--G7 remain planned
for subsequent sprints.

## Dependency Changes

### New crate dependencies (VisionClaw)

| Crate | Purpose |
|-------|---------|
| `git2` | libgit2 bindings for clone/fetch/push |
| `solid-pod-rs-git` | Git auth types (`BasicNostrExtractor`, `GitAuth` trait) --- type compatibility only |
| `solid-pod-rs-nostr` | `NostrWebIdResolver` for DID <-> WebID resolution on remote registration |

### solid-pod-rs version bump

| Consumer | Current | Target |
|----------|---------|--------|
| VisionClaw (Cargo.toml) | `0.4.0-alpha.1` | `0.4.0-alpha.5` |
| Agentbox (lib/solid-pod-rs.nix) | `0.4.0-alpha.1+sprint-9` (rev `7f8bc89`) | `0.4.0-alpha.5` (rev `298818e`) |
| nostr-bbs-pod-worker | `workspace = true` (already at alpha.5) | No change |

### Env var changes

| Variable | Status | Purpose |
|----------|--------|---------|
| `GITHUB_TOKEN` | Deprecated | Read at startup for legacy shim; not required for new deployments |
| `GITHUB_OWNER` | Deprecated | Auto-registered as `legacy-github` remote |
| `GITHUB_REPO` | Deprecated | Auto-registered as `legacy-github` remote |
| `GITHUB_BASE_PATH` | Deprecated | Auto-registered as `legacy-github` remote |
| `GIT_REMOTES` | New (optional) | JSON array of `GitRemote` configs for bootstrapping |
| `GIT_INGEST_ROOT` | New | Local clone storage path (default: `/app/data/git-ingest/`) |
| `GIT_INGEST_ENABLED` | New (Phase 1 only) | Feature flag; removed when git ingest becomes default |
| `WRITEBACK_ENABLED` | New | Global kill-switch for write-back (default: `false`) |

## Open Questions

1. **Enrichment file format standardisation.** Should `.embeddings.json` use a
   standard format (JSON-LD with `@type: EmbeddingVector`, ONNX embedding
   metadata), or is a VisionClaw-specific schema acceptable for MVP? Currently
   uses VisionClaw-specific schema; standardisation deferred to a future ADR.

2. **Multi-remote write-back.** If the same node is ingested from two remotes
   (e.g., GitHub mirror + Solid pod), which remote receives the write-back?
   Current proposal: the remote marked as `writeback_enabled = true`; if
   multiple, the one with the most recent `last_sync`. Needs validation.

3. **Precedent-based auto-approval threshold.** The
   `DecisionOutcome::Precedent` path (ADR-041) enables auto-approval for
   enrichment types approved N times. The threshold N and the similarity
   matching algorithm are deferred to Phase 5.

4. **`git verify-commit` with NIP-98 Schnorr signatures.** Embedding Schnorr
   signatures in git commits for `git verify-commit` compatibility is a stretch
   goal. The NIP-98 transport signature already binds the push to a DID, but
   per-commit verification is not yet specified.

## Related Decisions

- **ADR-041** (Judgment Broker Workbench): G4 adds
  `CaseCategory::KnowledgeEnrichment`. The six decision outcomes all apply:
  Approve triggers push, Reject blocks, Amend modifies the enrichment, Delegate
  routes to a domain expert, Promote elevates and pushes, Precedent flags for
  future auto-approval.

- **ADR-049** (Insight Migration Broker Workflow): The promotion tutorial's
  "opens a GitHub pull request" step becomes "commits to the source pod via
  write-back saga". No PR needed when the source is a pod --- the commit is the
  mutation. For GitHub-hosted sources, the PR path remains available.

- **ADR-051** (Visibility Transitions): The pod-first-Neo4j-second saga pattern
  is reused by G4. The write-back saga is the reverse flow (Neo4j-first-pod-
  second), maintaining the same two-phase-commit semantics.

- **ADR-074** (Cross-System DID:Nostr Canonicalisation): `did:nostr:<64-hex>`
  is the universal identity primitive across all G1--G7 components. NIP-26
  delegation enables server-to-agent trust chains without per-agent ACL
  entries on the pod.

- **ADR-075** (IS-Envelope Message Contract): Enrichment proposals and broker
  decisions map to IS-Envelope v1 kinds (`tool_invoke`, `tool_result`, `chat`,
  `knowledge_link`). NIP-59 gift-wrap on the wire when crossing relay
  boundaries.

- **PRD-009** (Discovery Engine): Embeddings, gap detection, and related-node
  proposals become write-back candidates. Each discovery output can generate a
  `BrokerCase` for review.

- **PRD-010** (Mesh Federation): The Nostr control plane (G7) uses the mesh
  relay topology (ADR-073) and DID canonicalisation (ADR-074) for cross-system
  event federation.

- **PRD-013** (Solid Pod Git Ingest Surface): The companion PRD specifying user
  stories, acceptance criteria, non-functional requirements, and full component
  architecture for all seven components.

## References

- PRD-013: `docs/PRD-013-solid-git-ingest-surface.md`
- ADR-041: `docs/adr/ADR-041-judgment-broker-workbench.md`
- ADR-049: `docs/adr/ADR-049-insight-migration-broker-workflow.md`
- ADR-051: `docs/adr/ADR-051-visibility-transitions.md`
- ADR-074: `docs/adr/ADR-074-cross-system-did-nostr-canonicalisation.md`
- ADR-075: `docs/adr/ADR-075-is-envelope-message-contract.md`
- DDD Context Map: `docs/ddd-mesh-federation-context.md`
- Binary Protocol Spec: `docs/binary-protocol.md`
- `solid-pod-rs` changelog: `solid-pod-rs/CHANGELOG.md`
- `git2` crate: `https://docs.rs/git2`
