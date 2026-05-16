# Additional Tensions and Gaps (cross-cutting review)

Reviewer: QE Code Reviewer (cross-cutting pass)
Date: 2026-05-16
Scope: 11 sections + README (8,499 lines). Excludes the seven tensions already triaged.

## Summary
Total found: 18 (5 blockers, 9 important, 4 nits)

Most cross-section boundaries are well-policed by the PRD/ADR/DDD discipline. The
residual tensions cluster around four themes:
1. **Named-graph and IRI identity** — Section 11 owns the persistence layout but
   Sections 7 and 8 reference named-graph IRIs that don't match it exactly.
2. **Domain-event vocabulary collisions** — DDD-01, DDD-07, DDD-08, DDD-11 each
   define event sets; wire↔internal naming maps are implicit.
3. **SQLite schema collisions** — Sections 5, 6, and 11 all declare tables in
   `settings.sqlite3` without a single owner.
4. **Phasing contradictions** — README phasing contradicts at least one "must
   land before" claim in section ADRs.

---

## Tension CC-1: Agent telemetry wire vs internal event name divergence

- **Severity**: blocker
- **Affected sections**: 07 (DDD-07, PRD-07, ADR-07), 10 (ADR-10 D1)
- **Finding**: ADR-10 D1 wire envelope (`10-external-integrations/ADR-10.md`
  lines 44–105) defines 5 wire types: `snapshot`, `delta`, `agent_added`,
  `agent_removed`, `heartbeat`. DDD-07 (`07-bots-telemetry/DDD-07.md` lines
  42–43, 168–215) defines 7 internal events: `AgentJoined`,
  `AgentPositionUpdated`, `AgentStatusChanged`, `AgentCommunicated`,
  `AgentDeparted`, `SwarmSnapshot`, `Heartbeat`. Three problems:
  (a) wire `agent_added` ↔ internal `AgentJoined` rename is silent;
  (b) wire `delta` fans out to internal `AgentPositionUpdated` and/or
  `AgentStatusChanged` per field — no documented dispatch logic;
  (c) internal `AgentCommunicated` has **no wire counterpart**. ADR-07 D5
  line 95 cites a `CommunicationEvent` from telemetry, but ADR-10 D1's
  envelope catalogue omits it. Section 7 depends on a contract Section 10
  never published.
- **Recommended resolution**: Section 10 owns the transport. Add a
  `"type": "communication"` envelope to ADR-10 D1 with payload
  `{ from_agent_id, to_agent_id, weight }`. DDD-07's anti-corruption layer to
  Section 10 (lines 235–253) must include an explicit wire→internal mapping
  table — one row per wire type.

## Tension CC-2: Two incompatible class-bit encodings in Sections 7 and 8

- **Severity**: blocker
- **Affected sections**: 07 (DDD-07 line 48, ADR-07 D3 lines 70–75), 08 (ADR-08 D6)
- **Finding**: ADR-08 D6 (`08-ontology-graph-data/ADR-08.md` lines 168–174)
  declares six discrete class bits: `0x80000000 Agent`, `0x40000000 Page`,
  `0x20000000 OntologyClass`, `0x10000000 OntologyProperty`,
  `0x08000000 LinkedPage`, `0x04000000 Axiom`. DDD-07 line 48 and ADR-07 D3
  line 72 declare a 3-bit mask `0x1C000000` for "ontology subtypes". These
  are incompatible: Section 7's mask covers bits 26–28; Section 8's Axiom
  bit `0x04000000` is bit 26 (overlapping). Section 8's `OntologyClass`
  bit `0x20000000` is bit 29 (outside the mask). The two encodings cannot
  coexist. Adjacent to but distinct from the known tension #1 (mask vs
  value); both need separate resolution.
- **Recommended resolution**: Section 8 owns the domain model and therefore
  the bit layout. ADR-07 and DDD-07 must adopt ADR-08 D6 verbatim. Section 7
  cares only about the `0x80000000` agent flag; ontology bits are out of
  its scope. Drop the `0x1C000000` mask from PRD-07 line 76, ADR-07 D3, and
  DDD-07 line 48.

## Tension CC-3: Physics `LayoutStarted` event not consumed by broadcast layer

- **Severity**: important
- **Affected sections**: 01 (DDD-01 lines 88–96), 02 (ADR-02 D2 lines 36–40)
- **Finding**: DDD-01 emits `LayoutStarted` as a domain event. ADR-02 D2
  references `LayoutDestabilised`, `LayoutSettled`, `LayoutHeartbeat`,
  `PhysicsClamped` but **not** `LayoutStarted`. If a simulation restarts
  (e.g. after panic recovery per ADR-01 D4), the broadcast layer has no
  defined behaviour: should it reset `frame_id`? emit a snapshot? Section 2
  is silent.
- **Recommended resolution**: Add to ADR-02 D2: "On `LayoutStarted` the
  broadcast actor resets `frame_id = 0` for the current epoch, immediately
  emits a snapshot to all connected clients, and transitions to ACTIVE."
  Coordinate with ADR-02 D7 (drop-detection) so the client expects
  `frame_id = 1` next.

## Tension CC-4: Inference named-graph IRI disagreement

- **Severity**: important
- **Affected sections**: 08 (ADR-08 D9 line 198, D2 line 88), 11 (PRD-11 A6 line 121, ADR-11 D2 line 78)
- **Finding**: ADR-08 D9 declares the inference named graph as
  `<urn:visionflow:inference>`. ADR-11 D2 and PRD-11 A6 declare it as
  `<urn:visionflow:graph:ontology:inferred>`. Different IRIs. SPARQL queries
  written against ADR-08's name find an empty graph.
- **Recommended resolution**: Section 11 owns the dataset layout. Section 8
  must use `<urn:visionflow:graph:ontology:inferred>` verbatim. Also update
  ADR-08 D2 line 88 to spell out the two named graphs explicitly.

## Tension CC-5: SQLite schema collision — three sections declare tables, no single owner

- **Severity**: blocker
- **Affected sections**: 05 (ADR-05 D5), 06 (ADR-06 D6), 11 (ADR-11 D5, DDD-11)
- **Finding**: Three SQLite tables are declared by three sections, but
  Section 11 (the persistence owner) does not enumerate all three:
  - **ADR-05 D5** declares `user_settings(pubkey TEXT PK, settings JSON,
    schema_ver, updated_at)` — one row per user, JSON blob
    (`05-settings/ADR-05.md` lines 115–123).
  - **ADR-06 D6** declares `audit_log(...)` + `idx_audit_ts`,
    `idx_audit_pubkey` (`06-auth-security/ADR-06.md` lines 207–222).
  - **ADR-11 D5** declares `settings(key TEXT, owner_pubkey TEXT, value
    JSON, description, updated_at, PRIMARY KEY (key, owner_pubkey))` —
    per-key-per-user (`11-persistence-migration/ADR-11.md` lines 135–146).
    Plus `physics_profiles`, `schema_migrations`. No `audit_log`, no
    `user_settings`.
  Two collisions: (a) Section 5's per-user-blob design is incompatible with
  Section 11's per-key-per-user design (different primary keys, different
  read patterns); (b) Section 6's `audit_log` is invisible to Section 11.
- **Recommended resolution**: Section 11 ADR-11 D5 must enumerate every
  table in `settings.sqlite3`. Add `audit_log` (+ archive tables). Drop the
  competing Section 5 schema; reconcile to Section 11's per-key design
  (Section 5 wraps a "load all for user U" call that returns all rows
  client-side). Section 5 ADR-05 D5 then references Section 11 by name
  rather than redeclaring.

## Tension CC-6: Section 12 introduces `/ws/xr-presence`, Section 6 route table omits it

- **Severity**: important
- **Affected sections**: 06 (ADR-06 D4), 12 (PRD-12 F11, ADR-12 D9)
- **Finding**: ADR-12 D9 introduces a new WebSocket endpoint
  `wss://<host>/ws/xr-presence` (`12-xr-client/ADR-12.md` lines 211–229) and
  asserts it is "documented in ADR-06 as an auth-protected route". ADR-06
  D4's audit table (`06-auth-security/ADR-06.md` lines 113–151) lists no
  presence handler. ADR-12 D9's claim is false.
- **Recommended resolution**: Add a row to ADR-06 D4: `xr_presence_handler`
  with posture `RequireAuth` — Nostr-signed JWT at upgrade only (no
  per-frame signing at 30Hz).

## Tension CC-7: README phasing contradicts ADR-06 phasing claim

- **Severity**: important
- **Affected sections**: README §"Implementation phasing", 06 (ADR-06 §Phasing)
- **Finding**: README lines 113–128 puts all of Section 6 in Phase 7
  (alongside Bots, Ecosystem, External, XR, Settings — in parallel). ADR-06
  §Phasing lines 368–388 states: "D1 + D2 (compile-time gates) must land
  before Phase 3 (binary protocol) because ADR-02 D8 depends on the
  `--allow-skip-auth` flag existing". Following README order strictly puts
  Phase 3 (binary protocol) before the dev-auth gate exists, blocking
  ADR-02 D8.
- **Recommended resolution**: Split Section 6 across two phases. D1+D2 land
  in a new "Phase 2.5: Auth compile-gates" between Persistence and Binary
  Protocol. D3–D10 remain in Phase 7. Renumber README phasing block.

## Tension CC-8: WasmSceneEffects build path undeclared in Section 9 Dockerfile

- **Severity**: important
- **Affected sections**: 04 (PRD-04 F7, ADR-04 D8), 09 (ADR-09 D3)
- **Finding**: PRD-04 F7 and ADR-04 D8 describe the WASM crate at
  `client/crates/scene-effects/` with compiled artefacts in
  `client/src/wasm/scene-effects/` (PRD-04 lines 144–162; ADR-04 lines
  192–208). ADR-09 D3 enumerates Dockerfile stages `base`, `rust-deps`,
  `rust-builder`, `node-deps`, `node-builder`, `development`, `production` —
  no WASM stage. PRD-04 F7 line 161 says the WASM is "versioned alongside
  the client bundle" but the build process producing it is unspecified.
- **Recommended resolution**: ADR-09 D3 add a `wasm-builder` stage (or
  extend `node-builder`) that runs `wasm-pack build` against
  `client/crates/scene-effects/` and outputs to the expected path. Without
  this, every clean build of the production image produces a broken
  client.

## Tension CC-9: Settings persistence — ADR-05 D5 and ADR-11 D5 are structurally incompatible

- **Severity**: blocker
- **Affected sections**: 05 (ADR-05 D5), 11 (ADR-11 D5, DDD-11 lines 455–485)
- **Finding**: Subsumed by CC-5 but worth a dedicated line. ADR-05 D5 design:
  one row per user, JSON blob, point-lookup by pubkey. ADR-11 D5 / DDD-11
  design: one row per (key, owner_pubkey) tuple with global-default
  fallback (`ORDER BY owner_pubkey IS NULL ASC LIMIT 1`). The two designs
  cannot coexist. Section 5 cannot implement against Section 11's schema
  without N queries per page load (unless wrapped). Section 11 cannot
  implement against Section 5's without giving up global-fallback
  semantics.
- **Recommended resolution**: Pick Section 11's design as canonical (it is
  more flexible). Section 5 wraps a "load all settings for user U with
  global fallback" call (one SQL query returning all rows). Update ADR-05
  D5 to reference ADR-11 D5 rather than redeclaring schema.

## Tension CC-10: "Snapshot" is overloaded across three meanings without disambiguation

- **Severity**: important
- **Affected sections**: 02, 07, 10, 11
- **Finding**: The word "snapshot" carries three semantically distinct
  meanings:
  (a) **Position frame snapshot** (ADR-02 D4, PRD-02 §6): full-state V3
      binary frame from `GraphStateActor::current_snapshot()`.
  (b) **Telemetry snapshot** (ADR-07 R1, ADR-10 D1/D2, DDD-07 line 43): a
      `SwarmSnapshot` JSON envelope on agent reconnect.
  (c) **Persistence snapshot** (DDD-11 line 51, ADR-11 D4/D10): either the
      per-60s `:hasX/Y/Z` write to Oxigraph, or the operator's RocksDB tar +
      SQLite VACUUM backup.
  Cross-references between sections leak ambiguity (e.g. ADR-07 R1 line 240
  refers to a `SwarmSnapshot` event "defined by Section 10"; Section 10
  ADR-10 D1 uses `"type": "snapshot"` in lowercase JSON).
- **Recommended resolution**: Add a §"Glossary of overloaded terms" to
  README. Each section qualifies "snapshot" at first use with the
  adjective: position-frame / telemetry / persistence / position-triple.

## Tension CC-11: Section 12 class classification cross-references conflicting bit schemes

- **Severity**: important
- **Affected sections**: 02 (ADR-02 D1), 04 (PRD-04 F1), 08 (ADR-08 D6), 12 (PRD-12 F3, ADR-12 D4)
- **Finding**: PRD-12 F3 lines 110–113 cites "Section 2's 26-bit NODE_ID_MASK
  plus 0x80000000 agent / 0x40000000 knowledge / 0x1C000000 ontology
  subtypes" (Section 7's mask scheme). ADR-12 D4 line 121 cites "the same
  scheme the web client uses (per Section 8 / Section 2)" but Section 8's
  ADR-08 D6 is the 6-bit scheme (CC-2). Section 4 PRD-04 F1 line 68 chains
  vaguely "see Section 8's data model and Section 2's NODE_ID_MASK". Section
  2 ADR-02 line 89 says only "u32 node_id (full id including class flag
  bits)" without enumerating.
- **Recommended resolution**: After CC-2 resolves to Section 8's 6-bit
  scheme, update: PRD-12 F3 cites ADR-08 D6 by name and drops the mask
  phrasing; ADR-12 D4 same; PRD-04 F1 same; ADR-02 D1 adds one sentence
  "`node_id` is a `NodeId` per ADR-08 D6".

## Tension CC-12: Audit log archive table growth not addressed by backup procedure

- **Severity**: nit
- **Affected sections**: 06 (ADR-06 D6), 11 (ADR-11 D10)
- **Finding**: ADR-06 D6 line 232 creates monthly archive tables
  `audit_log_archive_yyyymm`. ADR-11 D10 (backup procedure) treats SQLite
  as a single file; archive tables proliferate over years without an
  operator-driven cleanup path.
- **Recommended resolution**: ADR-11 §Risks add: "Audit archive tables
  grow over time. Operator runbook in `docs/operations/` covers
  `DROP TABLE audit_log_archive_yyyymm` for retired months."

## Tension CC-13: BroadcastChannel naming convention undeclared

- **Severity**: nit
- **Affected sections**: 10 (ADR-10 D3 line 147, D4 line 175)
- **Finding**: ADR-10 uses `BroadcastChannel('visionflow:agent-actions')` (D3)
  and `BroadcastChannel('visionflow:auth')` (D4). The naming convention is
  not declared; future channels need a registry.
- **Recommended resolution**: ADR-10 add a §"BroadcastChannel naming
  convention": prefix `visionflow:`, kebab-case after the prefix. Future
  channels register here.

## Tension CC-14: Worker proxy surface forbids the telemetry path PRD-07 claims to use

- **Severity**: important
- **Affected sections**: 03 (ADR-03 D7 lines 230–250, D9 lines 273–280), 07 (PRD-07 §8 line 184–186)
- **Finding**: ADR-03 D7 declares the worker proxy surface as exactly four
  methods: `attachPositionSAB`, `writeFrame`, `computeEdgeLengths`, `getStats`.
  D9 says non-position WebSocket messages "flow through text-frame handlers
  [...] None of these touch the worker." But PRD-07 §8 "Section 3 (Client
  State)" says "The agent telemetry stream uses the same Comlink-bridged
  worker pipeline as knowledge-graph updates and is subject to the same
  single-flight discipline." Contradictory.
- **Recommended resolution**: ADR-03 is the source of truth on the worker
  surface; PRD-07 §8 must be rewritten. Telemetry goes through the
  WebSocket but is decoded on the main thread (text frames), not in the
  worker. The single-flight discipline applies via the Section-7 coalescer
  (DDD-07 D11), not the worker's frame guard.

## Tension CC-15: GitHub adapter referenced by Section 8 but absent from Section 10

- **Severity**: important
- **Affected sections**: 08 (ADR-08 D10, DDD-08 ACL lines 313–331), 10 (PRD-10 §1, §6, ADR-10)
- **Finding**: ADR-08 D10 and DDD-08 §"Anti-corruption layer to Section 10"
  declare a GitHub adapter producing `ParsedMarkdown` value objects and
  attribute the contract to Section 10. PRD-10 §1 enumerates three external
  systems: agentbox, forum, whelk-rs. **GitHub is not on the list.** PRD-10
  §6 contracts-at-a-glance table has six rows, none for GitHub. ADR-10 has
  decisions D1–D10, none for GitHub. Section 8 implements against a
  phantom contract.
- **Recommended resolution**: PRD-10 §1 add a fourth external system
  (GitHub / Logseq corpus). ADR-10 add a D11 covering: transport (octocrab
  REST), `ParsedMarkdown` value-object shape (delegating to DDD-08), SHA1
  incremental gating contract, `FORCE_FULL_SYNC=1` semantics, parse-error
  envelope.

## Tension CC-16: PRD/ADR heading conventions inconsistent across sections

- **Severity**: nit
- **Affected sections**: all
- **Finding**: PRDs alternate between `## 1. Capability statement` (PRD-01,
  02, 07, 08, 10, 11) and `## Capability` / `## Capability statement` (03,
  04, 05, 06, 09, 12). PRD-04 mixes `F1...F10` functional + `A1...A10`
  acceptance. PRD-08 uses `A3a` for operational scenarios but no other
  letter-suffix anywhere. PRD-12 uses `F1...F12` + `A1...A12` cleanly.
- **Recommended resolution**: Not a blocker. Recommend a fixed template
  for future PRD/ADR authoring; do not retrofit existing docs.

## Tension CC-17: Section 9 ecosystem services have no documented consumers

- **Severity**: important
- **Affected sections**: 09 (ADR-09 D5), 06 (ADR-06 D4 mentions `speech_socket_handler`), 10
- **Finding**: ADR-09 D5 declares three ecosystem services: Kokoro TTS
  (8880), Whisper (8000), Xinference (9997) (`09-ecosystem-services/ADR-09.md`
  lines 125–162). No section documents what VisionFlow code consumes any of
  them. ADR-06 D4 lists `speech_socket_handler` and `inference_handler` and
  `ragflow_handler` but does not say which ecosystem service each connects
  to. Section 10 covers agentbox/forum/whelk, not TTS/STT/embeddings.
- **Recommended resolution**: ADR-09 add §"Consumers" listing which
  VisionFlow handler integrates with each service (Kokoro ↔
  speech_socket_handler? Whisper ↔ ?, Xinference ↔ inference_handler?). If
  consumers are out of scope for this sprint, say so explicitly.

## Tension CC-18: `SwarmSnapshot` internal event vs `snapshot` wire envelope — same as CC-1

- **Severity**: nit
- **Affected sections**: 07 (DDD-07 line 43, ADR-07 R1), 10 (ADR-10 D1)
- **Finding**: ADR-07 R1 line 240 refers to a `SwarmSnapshot` event "defined
  by Section 10". Section 10 ADR-10 D1 defines `"type": "snapshot"` (wire,
  lowercase, no "Swarm" prefix). DDD-07 line 43 lists `SwarmSnapshot` as an
  internal event. Same pattern as CC-1.
- **Recommended resolution**: Fold into CC-1's wire→internal translation
  table. The internal name `SwarmSnapshot` is fine; the wire→internal
  mapping must be explicit.

---

## Coverage gaps

- **Cloudflared tunnel ownership**: ADR-09 D12 says tunnel internals are
  Section 6's concern; ADR-06 has no tunnel section. Pick an owner.
- **Auth endpoint path inconsistency**: PRD-12 F2 / ADR-12 D8 cite
  `/auth/challenge` and `/auth/verify`. ADR-06 D4 lists `nostr_handler` for
  `/challenge` and `/session`. Different path roots (`/auth/*` vs `/*`).
  Reconcile to a single base path (recommend `/api/auth/*`).
- **Migration tool fate post-cutover**: DDD-11 line 509 says
  `migrate-neo4j-to-oxigraph` is "removed from the workspace after the
  cutover is committed". ADR-11 §Implementation order step 8 deletes only
  the `neo4j_*.rs` adapter files, not the tool. Half-specified — pick a
  fate (keep for backup re-runs, or commit to deletion timeline).
- **`computeEdgeLengths` consumer**: ADR-03 D7 surfaces this method on the
  worker proxy "used by layout feedback" (line 240). No section identifies
  what calls it. Section 1 is server-side; Section 4 ADR-04 D2 composes
  edges itself. May be dead surface; verify and prune.
- **NIP-98 canonical URL**: ADR-06 D10 says client and server "share a
  `canonicalise_url` function spec" with the Rust at
  `src/services/nostr_service.rs` and TypeScript at
  `client/src/services/nostrAuthService.ts`. No section says where this
  spec is *documented* — just where the impls live. The spec itself
  (trailing slash policy, query-param order, fragment handling) is in
  prose only at lines 287–293; promote to a cross-referenced anchor.

## Style / structural observations

- **Decision numbering (D1, D2, …)**: consistent across all 11 ADRs. Good.
- **"Rejected from main as buggy / unjustified"** sub-section: present and
  consistently labeled in every ADR. Good.
- **"Bugs and smells at the reset point (41979d33e)"**: present in every PRD
  and most ADRs, consistently labeled. Good.
- **Cross-references** mix `ADR-NN`, `Section N`, and full file paths. Pick
  one short form (`ADR-NN §SS Decision DN`) for in-prose refs; full path
  only in the `Related:` header.
- **README phasing block** lines 113–128 has 7 phases but Phase 7 is
  "parallel" — that's a cluster, not a phase. Rename to "Phase 7+:
  parallel work streams" or split into individual phases per section.
- **Domain event taxonomy** is spread across DDD-01, DDD-07, DDD-08,
  DDD-11. The total across all DDDs is 25+ events. A consolidated event
  reference (one table in README) would make producer↔consumer
  relationships visible at a glance.
- **PRD-11 vs DDD-11 SPARQL translation duplication**: DDD-11 reproduces
  the full SPARQL walkthrough (lines 266–425) that ADR-11 D7 references.
  Authoritative in ADR-11; the DDD walkthrough is supporting detail. Flag
  for future trim, not urgent.
- **No section has a "Test plan" or "Verifiability summary"**. Acceptance
  criteria exist but their mapping to test classes (unit / integration /
  contract / end-to-end / manual) is implicit. The /build-with-quality
  pipeline cannot be wired without this. Recommend a §"Verification" in
  each ADR mapping each A-criterion to a test class and fixture location.
