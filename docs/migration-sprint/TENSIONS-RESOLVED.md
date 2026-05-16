# Tensions Resolved — Migration Sprint Quality Audit

Date     : 2026-05-16
Baseline : `radical-rollback` @ `41979d33e`
Authors  : QE fleet synthesis (6 specialists)

## Audit summary

- Original tensions identified by coordinator: 7
- Additional tensions found by cross-cutting review: 18
- Net unique tensions after dedup: **22** (5 blockers, 12 important, 5 nits)
  - 3 dedups: CC-9 ≡ T5 (merged); CC-2 ↔ T1 (conflict, T1 wins, see TC-1); CC-18 ↔ CC-1 (folded into CC-1)
- Specialists deployed: 6 — code-intelligence (T1), security-auditor (T2), performance-validator (T3), api-contract-validator (T4/T6/T7), requirements-validator (T5), code-reviewer (CC-1..CC-18)
- **Decision**: The resolution set is **ready to drive implementation** for every section except where flagged BLOCKER. The five blockers below have specialist-recommended resolutions with verbatim edits; **one (T1↔CC-2) carries a specialist conflict and requires explicit human confirmation of the recommended path before ADR-08 §D6 is rewritten**. The other four blockers can be applied as proposed. IMPORTANT items can be addressed in their respective implementation phases without gating the sprint kick-off.

## Severity legend

- **BLOCKER** — implementation cannot start until resolved
- **IMPORTANT** — resolve before the affected section's implementation phase begins
- **NIT** — clean up during implementation; not gating

## Resolution index

| ID | Severity | Title | Affected sections | Owning specialist | One-line resolution |
|----|----------|-------|-------------------|-------------------|----------------------|
| TC-1 | BLOCKER | Class-bit encoding (T1 ⊕ CC-2) | 02, 07, 08 | code-intelligence + code-reviewer | Adopt T1 Option A: keep `0x1C000000` mask; reword ADR-08 §D6; CC-2's literal-text recommendation rejected |
| T2 | BLOCKER | Auth-bypass gating mechanism | ADR-02, ADR-06, PRD-06 | security-auditor | Compile-time only (Option A) + boot refusal of dev env vars in release (Option D) |
| TC-5 | BLOCKER | SQLite schema authority (T5 ⊕ CC-5 ⊕ CC-9) | 05, 06, 11 | requirements-validator + code-reviewer | ADR-11 §D5 sole authority; enumerate `audit_log` there; ADR-05 §D5 defers; trait surface frozen |
| T4 | BLOCKER | WebSocket URL space unowned | 02, 06, 07, 10, 12 | api-contract-validator | New ADR-06 §D11 owns 11-row canonical table; ADR-12 D9 and ADR-06 D5 path corrections |
| T7 | BLOCKER | Agent-click envelope underspecified | 07, 10 | api-contract-validator | Full `AgentActionEnvelope` schema in `crates/visionclaw-contracts`; ADR-10 D3 + ADR-07 D8 rewritten |
| T3 | IMPORTANT | Heartbeat: iteration vs wall-clock | 01, 02 | performance-validator | Wall-clock owned by broadcast; physics emits no heartbeat |
| T6 | IMPORTANT | Bots deletion-candidate routes | 07, 10 | api-contract-validator | Two-phase: 410 Gone (Phase 7a) → delete (Phase 7b) |
| CC-1 | IMPORTANT | Telemetry wire ↔ internal event divergence | 07, 10 | code-reviewer | Add `communication` wire envelope; ACL table required; folds CC-18 |
| CC-3 | IMPORTANT | `LayoutStarted` not consumed by broadcast | 01, 02 | code-reviewer | ADR-02 D2 adds explicit reset+snapshot behaviour |
| CC-4 | IMPORTANT | Inference named-graph IRI disagreement | 08, 11 | code-reviewer | Section 11 wins: `<urn:visionflow:graph:ontology:inferred>` |
| CC-6 | IMPORTANT | `/ws/xr-presence` missing from ADR-06 D4 | 06, 12 | code-reviewer | Add row to ADR-06 D4 (subsumed by T4's §D11) |
| CC-7 | IMPORTANT | README phasing contradicts ADR-06 ordering | README, 06 | code-reviewer | Split Section 6: D1+D2 in new Phase 2.5 |
| CC-8 | IMPORTANT | WASM build path missing from Dockerfile | 04, 09 | code-reviewer | ADR-09 D3 adds `wasm-builder` stage |
| CC-10 | IMPORTANT | "snapshot" overloaded across three meanings | 02, 07, 10, 11 | code-reviewer | README glossary; section-local qualifiers at first use |
| CC-11 | IMPORTANT | XR/Section-12 cross-references conflicting bit schemes | 02, 04, 08, 12 | code-reviewer | After TC-1, point all citations at ADR-08 §D6 |
| CC-14 | IMPORTANT | Worker surface forbids telemetry path PRD-07 claims | 03, 07 | code-reviewer | Rewrite PRD-07 §8: telemetry decoded on main thread |
| CC-15 | IMPORTANT | GitHub adapter referenced but absent from Section 10 | 08, 10 | code-reviewer | PRD-10 §1 add GitHub; new ADR-10 D11 |
| CC-17 | IMPORTANT | Ecosystem services have no documented consumers | 06, 09, 10 | code-reviewer | ADR-09 add §Consumers mapping |
| CC-12 | NIT | Audit archive table growth not in backup procedure | 06, 11 | code-reviewer | ADR-11 §Risks note + operator runbook |
| CC-13 | NIT | BroadcastChannel naming convention undeclared | 10 | code-reviewer | ADR-10 §"BroadcastChannel naming" |
| CC-16 | NIT | PRD/ADR heading conventions inconsistent | all | code-reviewer | Template for future docs; no retrofit |
| CC-Gaps | NIT | Coverage gaps (5 unowned surfaces) | 03, 06, 09, 11, 12 | code-reviewer | Tracked as follow-ups (see Cross-cutting follow-ups) |

---

## Resolutions in detail

### TC-1 — Class-bit encoding (T1 ⊕ CC-2 merger and conflict resolution)
**Severity**: BLOCKER
**Affected sections**: 02 (binary protocol), 07 (bots), 08 (ontology)
**Source specialist(s)**: T1 (code-intelligence) AND CC-2 (code-reviewer) — **specialists conflict**

#### Specialist conflict, surfaced
- **T1 recommendation (Option A)**: Keep the existing `ONTOLOGY_TYPE_MASK = 0x1C000000`; assign `OntologyClass=0x04, LinkedPage=0x08, Axiom=0x0C, OntologyProperty=0x10` within that mask; **rewrite ADR-08 §D6 to match**. Rationale: code already encodes this layout (`src/utils/binary_protocol.rs:16-27`); zero wire-format break; no GPU change; CUDA `class_id` buffer is unrelated to wire flag bits.
- **CC-2 recommendation**: Adopt ADR-08 §D6 verbatim (`0x20000000 OntologyClass`); make ADR-07 D3 and DDD-07 follow. Rationale: "Section 8 owns the domain model so its ADR text wins."

#### Conflict adjudication (this document's call)
**Adopt T1 Option A. ADR-08 §D6 text is wrong; the code is right.** Mathematically `0x20000000 & 0x1C000000 == 0`, so ADR-08 D6 literal text cannot coexist with PRD-08 §7 and DDD-07. CC-2's reading honours the words; T1's reading honours the working code, the PRD, the DDD, and the wire ABI. Choosing the words requires breaking the wire and the other two specs; choosing the code requires only rewriting eight lines of ADR-08. Lower-cost option chosen.

**Human-confirmation flag**: The project lead should explicitly accept that ADR-08 §D6 was the inaccurate document. Once accepted, TC-1 unblocks Sections 7 and 8 implementation simultaneously.

#### Current state
`src/utils/binary_protocol.rs:16-27` defines six constants (`AGENT 0x80000000`, `KNOWLEDGE 0x40000000`, `ONTOLOGY_TYPE_MASK 0x1C000000`, `ONTOLOGY_CLASS 0x04000000`, `ONTOLOGY_INDIVIDUAL 0x08000000`, `ONTOLOGY_PROPERTY 0x10000000`, `NODE_ID_MASK 0x03FFFFFF`). CUDA `class_id` (`src/utils/visionflow_unified.cu:276-278`, `src/actors/gpu/gpu_resource_actor.rs:241-249`) is a separately uploaded domain-cluster int, unrelated to flag bits. `NEXT_NODE_ID` (`src/models/node.rs:8`) is plain sequential `u32`.

#### Recommended resolution
Adopt T1 Option A allocation: `Agent 0x80000000` (bit 31), `Page 0x40000000` (bit 30), region mask `ONTOLOGY_TYPE_MASK 0x1C000000` (bits 26-28) carrying `OntologyClass 0x04000000`, `LinkedPage 0x08000000` (repurposed from `ONTOLOGY_INDIVIDUAL`), `Axiom 0x0C000000` (new), `OntologyProperty 0x10000000`. Sequence `0x03FFFFFF` (26 bits). Drop `OntologyIndividual` from the domain model (DDD-08 lists no such aggregate).

#### Proposed edits
- `docs/migration-sprint/08-ontology-graph-data/ADR-08.md` §D6: replace the existing class-bits paragraph (lines 168-174) with:
  ```markdown
  Class bits encode `NodeClass` in the top 6 bits of the `u32` id:
  `0x80000000 = Agent` (bit 31), `0x40000000 = Page` (bit 30). The
  ontology region uses the mask `ONTOLOGY_TYPE_MASK = 0x1C000000`
  (bits 26-28) with these allocations: `0x04000000 = OntologyClass`,
  `0x08000000 = LinkedPage` (placeholder), `0x0C000000 = Axiom`,
  `0x10000000 = OntologyProperty`. Values `0x14000000`, `0x18000000`,
  `0x1C000000` are reserved for future ontology subtypes. The remaining
  26 bits (`NODE_ID_MASK = 0x03FFFFFF`) are the per-class sequence,
  allocated by the atomic counter in `GraphStateActor`. Per DDD-08 §C2
  the sequence is stable across a `LinkedPage → Page` or
  `LinkedPage → OntologyClass` upgrade — only the class bits change.
  ```
- `docs/migration-sprint/07-bots-telemetry/PRD-07.md` line 76 and `ADR-07.md` D3 lines 70-75: drop any redundant restatement of the ontology mask; replace with `See ADR-08 §D6 for the canonical class-bit allocation.`
- `docs/migration-sprint/07-bots-telemetry/DDD-07.md` line 48: same — replace inline mask with citation to ADR-08 §D6.

#### Verification
- Unit test in `src/utils/binary_protocol.rs` asserting `(ONTOLOGY_CLASS_FLAG | LINKED_PAGE_FLAG | AXIOM_FLAG | ONTOLOGY_PROPERTY_FLAG) & !ONTOLOGY_TYPE_MASK == 0` (every assigned ontology subtype lies inside the mask).
- Round-trip test: `set_axiom_flag(set_sequence(1234)) | NODE_ID_MASK & 0x03FFFFFF == 1234 && is_axiom(...) == true`.
- Grep CI: `rg '0x20000000' docs/migration-sprint/` returns zero matches once edits land.

#### Implementation impact
- Files to modify: `src/utils/binary_protocol.rs` (rename `ONTOLOGY_INDIVIDUAL_FLAG` → `LINKED_PAGE_FLAG`; add `AXIOM_FLAG`; update `NodeType` enum + `get_node_type` at lines 183-197); `src/handlers/socket_flow_handler/position_updates.rs:107-172` (LinkedPage/Axiom branches).
- Tests to add: round-trip tests at `src/utils/binary_protocol.rs:1038-1051`; mask-coverage unit test.
- ABI/wire/schema impact: **none** — `OntologyClass/Property` unchanged; `LinkedPage` reuses already-occupied `0x08000000`; `Axiom` newly takes previously-unused `0x0C000000`. No CUDA change. No DB change.

---

### T2 — Auth-bypass gating mechanism
**Severity**: BLOCKER
**Affected sections**: ADR-02 D8, ADR-06 D1+D2, PRD-06 A1+A4
**Source specialist(s)**: T2 (security-auditor)

#### Current state
`src/settings/auth_extractor.rs:39-62, 153-172` carries the bypass code into release binaries, gated by six joint env vars including case-sensitive `APP_ENV`. `src/handlers/socket_flow_handler/http_handler.rs:16-35` repeats the anti-pattern as `ALLOW_INSECURE_DEFAULTS`. Threat model T-A (ops misconfig), T-B (ops-repo supply chain), T-C (FS access) all defeated by compile-time gating with no runtime path. Rust idiom (rustls, tokio, axum-extra) gates dev surfaces with Cargo features; HashiCorp Vault's runtime `-dev` is the documented anti-model.

#### Recommended resolution
Option A (compile-time gating only) + Option D (release-build boot-refusal of suspect dev env vars). ADR-02 D8 defers to ADR-06. PRD-06 A4 corrected. ADR-06 gains a new D11.

#### Proposed edits
- `docs/migration-sprint/02-binary-protocol/ADR-02.md` §D8: replace existing decision body with:
  ```markdown
  ### D8. Auth model

  WebSocket upgrade requires a `?token=<nostr_jwt>` query param in production.
  In dev mode (`?skipAuth=true` to the HTML shell) the client emits no token;
  the server, if launched with `--allow-skip-auth`, accepts.

  The `--allow-skip-auth` flag is gated *exclusively* by the compile-time
  mechanism specified in ADR-06 D2:
  `#[cfg(any(debug_assertions, feature = "dev-auth"))]`. Release binaries
  built without the `dev-auth` feature physically cannot honour the flag —
  the flag-handling code is absent from the binary. There is no runtime
  env-var path. Section 6 owns this surface; this section defers.
  ```
- `docs/migration-sprint/06-auth-security/PRD-06.md` A4: replace `cfg(debug_assertions) || env(VISIONFLOW_DEV_MODE)` with `cfg(any(debug_assertions, feature = "dev-auth"))`. Delete the `VISIONFLOW_DEV_MODE` reference entirely.
- `docs/migration-sprint/06-auth-security/ADR-06.md` add new decision D11 after D10:
  ```markdown
  ### D11. Startup refusal of dev-mode env vars in release

  The release binary, in `main.rs` after `dotenv().ok()` and before binding
  any socket, refuses to start if any of `SETTINGS_AUTH_BYPASS`,
  `VISIONFLOW_DEV_MODE`, `ALLOW_INSECURE_DEFAULTS`, or `NODE_ENV=development`
  with `DOCKER_ENV` set are present. Logs each offending var to stderr,
  exits with status 2. Wrapped in
  `#[cfg(not(any(debug_assertions, feature = "dev-auth")))]` so dev builds
  skip it. The release binary cannot *honour* these vars (no code reads
  them) but their presence is signal of an ops promotion that brought dev
  settings forward.
  ```

#### Verification
- **V1 symbol absence**: `strings target/release/webxr | grep -E 'SETTINGS_AUTH_BYPASS|VISIONFLOW_DEV_MODE|dev-session-token|dev-user'` → zero hits after `cargo build --release`.
- **V2 argv refusal**: `./target/release/webxr --allow-skip-auth` → exit 1, stderr names the flag.
- **V3 D11 boot refusal**: start release binary with `SETTINGS_AUTH_BYPASS=true VISIONFLOW_DEV_MODE=true NODE_ENV=development DOCKER_ENV=1` → exit 2, each offender named.
- **V4 source-lint cargo test**: greps `src/` for `std::env::var\(.*BYPASS|DEV_MODE|INSECURE` outside `#[cfg(...)]` blocks; fails on match.
- **V5 disassembly sanity**: `objdump -d target/release/webxr | grep -c try_dev_bypass` → 0.

#### Implementation impact
- Files to modify: `Cargo.toml` (add `dev-auth = []` feature); `src/main.rs` (~30 lines for D11); `src/settings/auth_extractor.rs:39-62, 153-172` (convert env-gates to `#[cfg]`); `src/handlers/socket_flow_handler/http_handler.rs:16-35` (same gate conversion); `scripts/launch.sh:348-356` (add `dev-auth` to dev `FEATURES=`, omit from prod).
- Tests to add: V1, V3, V4 as CI gates; V2 in startup integration test.
- ABI/wire/schema impact: none.

---

### TC-5 — SQLite schema authority (T5 ⊕ CC-5 ⊕ CC-9 merger)
**Severity**: BLOCKER
**Affected sections**: 05 (ADR-05 D5), 06 (ADR-06 D6), 11 (ADR-11 D5, DDD-11)
**Source specialist(s)**: T5 (requirements-validator) + CC-5/CC-9 (code-reviewer)

#### Merged scope
- T5 / CC-9: ADR-05 D5 (`user_settings`, document-per-user) is structurally incompatible with ADR-11 D5 (`settings(key, owner_pubkey)`, per-key-per-user). Trait surface (17 async fn at `src/ports/settings_repository.rs`) is per-key, so ADR-11's design matches the code.
- CC-5 additional: ADR-06 D6's `audit_log` and `audit_log_archive_yyyymm` tables are invisible to ADR-11's enumeration. Persistence ownership is split across three sections with no single authority.

#### Current state
ADR-05 D5 lines 111-137: `user_settings(pubkey PK, settings JSON, schema_ver, updated_at)`. ADR-06 D6 lines 207-222: `audit_log` + indices + monthly archives. ADR-11 D5 lines 130-172: `settings(key, owner_pubkey, value JSON, …) WITHOUT ROWID` + `physics_profiles` + `schema_migrations` + WAL pragmas + pubkey task-local. ADR-11 line 132 misclaims "44 SettingsRepository methods" (actual: 17).

#### Recommended resolution
**ADR-11 §D5 is the sole authority for the SQLite schema and adapter, for every table in `settings.sqlite3`.** ADR-05 §D5 owns only the domain shape (`AppFullSettings`, `PhysicsSettings`, `SettingValue`, validation, UI, defaults, document-level `schema_version`). ADR-06 §D6 owns the audit-log domain but its tables are catalogued in ADR-11 §D5. The 17-method `SettingsRepository` trait is the contract and is frozen.

#### Proposed edits
- `docs/migration-sprint/05-settings/ADR-05.md` lines 111-137 (D5 body): replace entirely with:
  ```markdown
  ### D5. Persistence: SQLite, schema per ADR-11

  Settings persist in SQLite. **The schema, primary-key layout, per-user
  resolution order, pragmas, and backup mechanics are owned by ADR-11 §D5
  and DDD-11 §"Settings per-user resolution"** — this ADR does not
  duplicate them. Section 5 owns only the contents of what gets stored:

  - The `AppFullSettings` Rust type (generated from `settings.ts` per D1)
    is the domain-level shape. ADR-11's adapter persists it via the
    17-method `SettingsRepository` trait at
    `src/ports/settings_repository.rs`.
  - `AppFullSettings` embeds its own `schema_version: u32` field,
    generated from the `settings.ts` AST. This is distinct from ADR-11's
    `schema_migrations` table: the table tracks which SQL migrations the
    database has applied; the embedded version tracks the shape of an
    individual user's stored document. Section 5 owns the document-shape
    migration ladder (`fn(&mut Value, from: u32) -> u32`); ADR-11's
    adapter invokes it on read when the embedded version is lower than
    current.
  - Anonymous sessions read defaults and receive 401 on save (FR-7).
    Authenticated sessions read and write their own settings; the pubkey
    threads through ADR-11's task-local context, not as a method parameter.
  - The repository trait is frozen by ADR-11 PRD A2. Changes require
    coordinated edits to both ADRs.
  ```
- `docs/migration-sprint/05-settings/ADR-05.md` R3: replace text with:
  ```markdown
  - **R3. Per-document `schema_version` drift.** `AppFullSettings::schema_version`
    (Section 5) and `schema_migrations.id` (Section 11) are independent
    counters answering different questions. Mitigation: distinct names in
    code, plus a unit test asserting they cannot be conflated.
  ```
- `docs/migration-sprint/11-persistence-migration/ADR-11.md` §D5 — insert as first sentence under the heading:
  ```markdown
  This ADR is the sole authority for every table in `settings.sqlite3`,
  including the audit log catalogued from Section 6. ADR-05 (Settings &
  Control Panel) and ADR-06 (Auth & Security) defer to this section for
  all storage and operational concerns; those sections own only their
  domain types (Section 5: `AppFullSettings`, `PhysicsSettings`;
  Section 6: audit event semantics) persisted via the unchanged
  `SettingsRepository` trait surface (17 methods).
  ```
- `docs/migration-sprint/11-persistence-migration/ADR-11.md` — enumerate `audit_log` and `audit_log_archive_yyyymm` in the D5 table list with a back-reference: `See ADR-06 §D6 for audit event semantics and retention policy.`
- `docs/migration-sprint/11-persistence-migration/ADR-11.md` line 132: change "44 SettingsRepository methods" to "17 SettingsRepository methods".
- After the `WITHOUT ROWID` bullet, append the document-version clarification given verbatim in T5 §"Wording check on ADR-11" item 2.

#### Verification
- Trait-surface SHA test: cargo test computes SHA256 over the sorted method signatures of `SettingsRepository` and asserts it equals a constant; any trait edit forces an explicit version bump and an ADR amendment.
- CI check: `rg 'CREATE TABLE' docs/migration-sprint/` returns only inside `11-persistence-migration/` files. Any `CREATE TABLE` in section 5 or 6 docs is a CI failure.
- BDD scenarios (3) defined in T5 §"BDD scenarios" — domain-only change, storage-only change, cross-cutting change.

#### Implementation impact
- Files to modify: `src/adapters/sqlite_settings_repository.rs` (single adapter implementing all 17 methods); `src/config/generated_settings.rs` (embedded `schema_version`); audit-log adapter co-located.
- Tests to add: trait-surface SHA test; three BDD scenarios from T5.
- ABI/wire/schema impact: SQLite schema canonicalised; `audit_log` + archives added to the enumeration; no behaviour change in the audit-log adapter.

---

### T4 — WebSocket URL space ownership
**Severity**: BLOCKER
**Affected sections**: 02, 06, 07, 10, 12
**Source specialist(s)**: T4 (api-contract-validator). CC-6 subsumed.

#### Current state
Baseline registers 7 WS endpoints (`src/main.rs:694-698`, `bots_visualization_handler.rs:507`, `multi_mcp_websocket_handler.rs:925`, `api_handler/analytics/mod.rs:193`, `api_handler/ontology/mod.rs:1366`). Sprint adds 3 (`/ws/xr-presence`, `/ws/agent-telemetry`, `/ws/enterprise-events`). No single section enumerates the lot. ADR-06 D5 cites `wss://<host>/api/ws/...` (wrong; canonical is `/wss`). ADR-12 D9 names `wss://<host>/ws` (wrong; baseline is `/wss`).

#### Recommended resolution
Add new ADR-06 §D11 as the sole authority for the WebSocket URL surface, containing the 11-row canonical table from T4. Cross-link from ADR-02/07/10/12.

#### Proposed edits
- `docs/migration-sprint/06-auth-security/ADR-06.md` — add new decision D11 (after the auth D11 added by T2; renumber if needed) containing the 11-row enumeration table reproduced from the T4-T6-T7 resolution document §T4 "Canonical WebSocket endpoint enumeration", followed by:
  ```markdown
  Cross-section ownership:
  - §D11 owns the URL space and the auth posture per endpoint.
  - Each owning section defines the wire format for its endpoint.
  - Default backpressure is drop-never-queue (ADR-02 D3); deviations
    documented in the owning ADR.
  ```
  (CC-6: the `/ws/xr-presence` row is in this table with `RequireAuth — Nostr-signed JWT at upgrade only`.)
- `docs/migration-sprint/06-auth-security/ADR-06.md` D5 — replace the CSP rationale parenthetical citing `wss://<host>/api/ws/...` with:
  ```markdown
  WebSocket endpoints are enumerated in §D11. All endpoints are same-origin
  (`wss://<host>/wss`, `wss://<host>/ws/...`, `wss://<host>/api/<section>/ws`)
  and fall under `'self'`. External `wss:` is not used.
  ```
- `docs/migration-sprint/12-xr-client/ADR-12.md` D9 — change every occurrence of `/ws` (graph-data path) to `/wss`.

#### Verification
- CI script parses `App::new()` route registrations in `src/main.rs` and asserts every `.route("/ws...")` and `.route("/wss")` appears as a row in ADR-06 §D11. PR fails on drift.
- Grep CI: `rg 'wss?://[^/]+/api/ws' docs/migration-sprint/` returns zero matches (no stale endpoint references).

#### Implementation impact
- Files to modify: only documentation in this sprint. CI route-drift check lives at `scripts/ci/check-ws-route-enumeration.sh`.
- Tests to add: route-enumeration drift check in CI.
- ABI/wire/schema impact: documentary only.

---

### T7 — Agent-click action envelope specification
**Severity**: BLOCKER
**Affected sections**: 07 (ADR-07 D8), 10 (ADR-10 D3)
**Source specialist(s)**: T7 (api-contract-validator)

#### Current state
ADR-07 D8 names an internal intent `RequestAgentControlSurface { agent_id, swarm_id, cursor_world_position }` and defers to Section 10. ADR-10 D3 gives a partial JSON shape and BroadcastChannel hint but no `type` discriminator, no `schema_version`, no origin-verification rules, no shared TS type. Both sides will guess.

#### Recommended resolution
Add the full `AgentActionEnvelope` schema to `crates/visionclaw-contracts/src/agent_action.rs` (Rust source of truth) with `ts-rs`-generated `.d.ts` at `client/src/types/contracts/agent-action.d.ts`. ADR-10 D3 and ADR-07 D8 rewritten to reference the contracts crate.

#### Proposed edits
- `docs/migration-sprint/10-external-integrations/ADR-10.md` D3 — replace the JSON snippet and transport selection prose with:
  ```markdown
  The envelope shape, BroadcastChannel constant, deep-link template, and
  `AgentActionTargetOrigin` allowlist type are defined in
  `crates/visionclaw-contracts/src/agent_action.rs` and generated as
  `client/src/types/contracts/agent-action.d.ts`. The full schema is
  reproduced in `_resolutions/T4-T6-T7-api-contracts.md` §T7. Receivers
  MUST verify `type === "visionflow:agent-action"` and
  `schema_version === 1`; postMessage receivers additionally enforce
  `event.origin` against the allowlist. Unknown `kind` values are no-ops
  (forward-compatible). This envelope supersedes ADR-07 D8's
  `RequestAgentControlSurface` intent.
  ```
- `docs/migration-sprint/07-bots-telemetry/ADR-07.md` D8 — replace the decision body with:
  ```markdown
  Clicking an agent capsule constructs an `AgentActionEnvelope` (ADR-10 D3
  plus `crates/visionclaw-contracts/src/agent_action.rs`) and dispatches it
  via the session's chosen transport. VisionFlow does not render a control
  panel in-process and does not embed an iframe of one; the renderer's
  responsibility ends at envelope dispatch.
  ```

The envelope schema (full TypeScript declaration including `AgentActionEnvelope`, `AGENT_ACTION_CHANNEL`, `AGENT_ACTION_DEEP_LINK_TEMPLATE`, `AgentActionTargetOrigin` and the normative origin-check table) is reproduced verbatim in `_resolutions/T4-T6-T7-api-contracts.md` §T7 and is the source of truth for this resolution.

#### Verification
- `crates/visionclaw-contracts` compiles; `ts-rs` test generates `agent-action.d.ts` byte-identical to the committed file.
- Contract test at `tests/contracts/external-integrations/agent_action.rs`: builds every envelope variant, asserts receiver rejects `type !== "visionflow:agent-action"` and `schema_version !== 1` with structured error; round-trips via BroadcastChannel and postMessage; postMessage path fails closed on unlisted origin.
- Grep: `rg 'RequestAgentControlSurface' src/` returns zero hits after the rewrite.

#### Implementation impact
- Files to create: `crates/visionclaw-contracts/src/agent_action.rs`, generated `client/src/types/contracts/agent-action.d.ts`, `tests/contracts/external-integrations/agent_action.rs`.
- Files to modify: agent-click handler in `client/src/components/...` (one site) to construct the envelope and dispatch by transport.
- ABI/wire/schema impact: the envelope is a new cross-boundary contract; once shipped it is versioned by `schema_version` per ADR-10 D8.

---

### T3 — Heartbeat: iteration vs wall-clock
**Severity**: IMPORTANT
**Affected sections**: 01 (DDD-01, ADR-01 D9), 02 (ADR-02 D2)
**Source specialist(s)**: T3 (performance-validator)

#### Current state
ADR-01 D9 expresses `LayoutHeartbeat` in iteration-counts (300). ADR-02 D2 expresses it in wall-clock (5 s). At 60 Hz they coincide; off 60 Hz they diverge. Physics tick rate is variable: `FastSettle` (default) runs as fast as the GPU produces completions (`src/actors/physics_orchestrator_actor.rs:215-230`); large graphs at 5 Hz → heartbeat every 60 s; paused → never. Heartbeat is semantically a broadcast cadence, not a physics event.

#### Recommended resolution
Wall-clock owned by broadcast actor; physics emits no heartbeat. Physics emits four events: `LayoutStarted`, `LayoutSettled`, `LayoutDestabilised`, `PhysicsClamped`. Broadcast actor holds a `tokio::time::interval(Duration::from_secs(broadcast_heartbeat_secs))` alive only in SETTLED state.

#### Proposed edits
- `docs/migration-sprint/01-physics/ADR-01.md` §D9 — replace decision body with:
  ```markdown
  ### D9. Broadcast cadence governed by settlement, not FPS

  ADR-02 owns this in detail. From the physics-side perspective: the actor
  emits **state-transition events only** — `LayoutStarted`,
  `LayoutSettled { iteration, rms_velocity }`, and
  `LayoutDestabilised { iteration, rms_velocity }`. Settlement is detected
  via RMS-velocity hysteresis across the last N ticks. The physics actor
  emits **no time-based heartbeat**: wall-clock heartbeat cadence is a
  broadcast concern (see ADR-02 D2) and is wholly owned by the broadcast
  actor.
  ```
- `docs/migration-sprint/01-physics/DDD-01.md` §Domain events — remove the `LayoutHeartbeat` bullet entirely. Physics events become four.
- `docs/migration-sprint/02-binary-protocol/ADR-02.md` §D2 — replace the `On LayoutSettled` paragraph with:
  ```markdown
  On `LayoutSettled`: enter SETTLED state. The broadcast actor starts a
  `tokio::time::interval(broadcast_heartbeat_secs)` (default 5 s); each
  tick reads `GraphStateActor::current_snapshot()` and emits a full V3
  frame to all connected clients. The interval is cancelled on
  `LayoutDestabilised` and on shutdown.
  ```
- `docs/migration-sprint/02-binary-protocol/PRD-02.md` §4 — add acceptance criterion A8:
  ```markdown
  A8. **Heartbeat is wall-clock, not iteration-driven.** In the SETTLED
  state, full-position frames emit every `broadcast_heartbeat_secs` of
  wall-clock time (default 5 s), independent of physics tick rate.
  Verified by a test that pauses physics entirely and observes a
  heartbeat within 5.5 s.
  ```

#### Verification
- BDD-1 (paused physics emits heartbeat within 5.5 s), BDD-2 (5-s cadence at 200 Hz physics), BDD-3 (interval cancelled within 100 ms of destabilisation) — all three defined in T3 §"Test scenarios".

#### Implementation impact
- Files to modify: `src/actors/gpu/force_compute_actor.rs:1448-1480, 1528-1560` (remove iter≥300 branches); `src/actors/physics_orchestrator_actor.rs:538-590, 1100-1178` (remove 60 FPS broadcast throttles); `src/actors/client_coordinator_actor.rs:455-525, 748-768` (replace 3 `broadcast_interval` Durations with single `Option<SpawnHandle>`); `src/gpu/broadcast_optimizer.rs` deleted (ADR-02 D5).
- Tests to add: BDD-1/2/3 as integration tests.
- ABI/wire/schema impact: none; same V3 frames, different scheduler.

---

### T6 — Bots deletion-candidate routes
**Severity**: IMPORTANT
**Affected sections**: 07 (ADR-07 D12), 10 (ADR-10 D7)
**Source specialist(s)**: T6 (api-contract-validator)

#### Current state
ADR-07 D12 names 5 deletion candidates with naming drift vs baseline: `POST /api/bots/graph` doesn't exist (handler exposed as `/data` and `/update`); `POST /api/bots/spawn-agent` exists only as a client fallback (`BotsControlPanel.tsx:101`); `POST /api/bots/create-task` and `/stop-task` are not registered routes (internal actor messages only). Consumer audit shows no external on-disk callers but the `agentbox/` directory is gitignored and not provably empty.

#### Recommended resolution
Two-phase deletion. Phase 7a: handlers return `410 Gone` + `Link: <successor>; rel="successor-version"`; route remains registered so callers get structured failure. Phase 7b (~30 days later): route + handler deleted; doc refs removed.

#### Proposed edits
- `docs/migration-sprint/07-bots-telemetry/ADR-07.md` §D12 — replace decision body with the table and `410 Gone` JSON body reproduced from the T4-T6-T7 resolution document §T6 "Proposed wording — revised ADR-07 D12". Key table:
  ```markdown
  | Baseline route | Successor (agentbox) | Phase 7a | Phase 7b |
  |----------------|----------------------|----------|----------|
  | POST /api/bots/initialize-swarm | POST {agentbox}/swarms/initialize | 410+Link | Deleted |
  | POST /api/bots/spawn-agent-hybrid | POST {agentbox}/agents/spawn | 410+Link | Deleted |
  | POST /api/bots/spawn-agent (legacy) | (same) | 410+Link | Deleted |
  | POST /api/bots/data, POST /api/bots/update | (no successor — was inverted client→server graph write; see D3) | 410+Link to D3 | Deleted |
  | DELETE /api/bots/remove-task/{id} | DELETE {agentbox}/tasks/{id} | 410+Link | Deleted |
  ```
  Routes retained (read-only telemetry until `/ws/agent-telemetry` parity): `GET /api/agents/identity/{id}`, `GET /api/bots/{status,agents,data}`. Re-homed at `src/handlers/telemetry_handler.rs`.
- `docs/migration-sprint/10-external-integrations/ADR-10.md` §D7 — append:
  ```markdown
  Additionally, scan for re-introduction of deprecated bots control-plane
  route names (`/initialize-swarm`, `/spawn-agent`, `/spawn-agent-hybrid`,
  `/remove-task`, `/bots/data` POST, `/bots/update` POST) under
  `src/handlers/`. The Phase 7b removal date in ADR-07 D12 is the date
  these become hard CI failures.
  ```

#### Verification
- HTTP-level test: `POST /api/bots/initialize-swarm` returns 410, `Link` header matches successor, body has `successor`, `deprecated_since`, `scheduled_removal`.
- Metric `bots_deprecated_route_calls_total{route}` exposed at `/metrics`; baseline value zero after internal call sites removed.
- CI grep: re-introduction of the deprecated names triggers failure after Phase 7b date.

#### Implementation impact
- Files to modify: `src/handlers/api_handler/bots/mod.rs` (replace handler bodies with `410 Gone` builder); remove client call sites at `client/src/components/BotsControlPanel.tsx:71,101`, `MultiAgentInitializationPrompt.tsx:162-163`, `AgentControlPanel.tsx:110`.
- Tests to add: HTTP-level deprecation tests; CI route-name greps; metric registration test.
- ABI/wire/schema impact: callers see structured 410 with successor link; clean migration window.

---

### CC-1 — Telemetry wire ↔ internal event divergence (folds CC-18)
**Severity**: IMPORTANT
**Affected sections**: 07 (DDD-07), 10 (ADR-10 D1)
**Source specialist(s)**: CC-1, CC-18 (code-reviewer)

#### Current state
ADR-10 D1 wire envelope catalogue: `snapshot`, `delta`, `agent_added`, `agent_removed`, `heartbeat` (5 types). DDD-07 internal events: `AgentJoined`, `AgentPositionUpdated`, `AgentStatusChanged`, `AgentCommunicated`, `AgentDeparted`, `SwarmSnapshot`, `Heartbeat` (7 events). Three failures: (a) wire `agent_added` → internal `AgentJoined` rename silent; (b) wire `delta` → multiple internal events with no dispatch rule; (c) internal `AgentCommunicated` has no wire counterpart but ADR-07 D5 line 95 depends on a `CommunicationEvent`. CC-18 is the same pattern for `SwarmSnapshot` ↔ `snapshot`.

#### Recommended resolution
Section 10 owns the transport. Add a `"type": "communication"` envelope to ADR-10 D1 with payload `{ from_agent_id, to_agent_id, weight }`. DDD-07's anti-corruption layer must contain an explicit wire→internal mapping table — one row per wire type — covering the rename of `agent_added`/`SwarmSnapshot`/etc.

#### Proposed edits
- `docs/migration-sprint/10-external-integrations/ADR-10.md` §D1 — add new wire envelope variant after `heartbeat`:
  ```markdown
  - `"type": "communication"` — emitted when one agent communicates with
    another. Payload: `{ from_agent_id: string, to_agent_id: string,
    weight: number }`. Maps to DDD-07's `AgentCommunicated` internal event.
  ```
- `docs/migration-sprint/07-bots-telemetry/DDD-07.md` §"Anti-corruption layer to Section 10" (lines 235-253) — add at the end of the section:
  ```markdown
  | Wire `type` (ADR-10 D1) | Internal event (this DDD) | Notes |
  |--------------------------|----------------------------|-------|
  | `snapshot` | `SwarmSnapshot` | Full-state, used on connect / reconnect. |
  | `delta` | `AgentPositionUpdated` and/or `AgentStatusChanged` | Dispatch per changed field in payload. |
  | `agent_added` | `AgentJoined` | Pure rename. |
  | `agent_removed` | `AgentDeparted` | Pure rename. |
  | `heartbeat` | `Heartbeat` | Liveness only; no graph mutation. |
  | `communication` | `AgentCommunicated` | New in this sprint. |
  ```

#### Verification
- Contract test instantiates each wire variant, asserts the ACL produces exactly the expected internal event(s); fails on unmapped types.
- Grep CI: `rg 'CommunicationEvent' src/ docs/` returns zero hits after ADR-07 D5 line 95 is corrected to `AgentCommunicated`.

#### Implementation impact
- Files to modify: ADR-10 D1, DDD-07 ACL, ADR-07 D5 line 95 (rename `CommunicationEvent` → `AgentCommunicated`).
- Tests to add: wire→internal mapping contract test.
- ABI/wire/schema impact: new wire variant `communication` (additive, forward-compatible per ADR-10 D8).

---

### CC-3 — `LayoutStarted` not consumed by broadcast layer
**Severity**: IMPORTANT
**Affected sections**: 01 (DDD-01), 02 (ADR-02 D2)
**Source specialist(s)**: CC-3 (code-reviewer)

#### Current state
DDD-01 emits `LayoutStarted`; ADR-02 D2 references `LayoutDestabilised`/`LayoutSettled`/`LayoutHeartbeat`/`PhysicsClamped` only. On panic recovery (ADR-01 D4) broadcast behaviour is undefined.

#### Recommended resolution
ADR-02 D2 explicitly handles `LayoutStarted` by resetting `frame_id` and emitting a snapshot.

#### Proposed edits
- `docs/migration-sprint/02-binary-protocol/ADR-02.md` §D2 — append (after the SETTLED paragraph edited by T3):
  ```markdown
  On `LayoutStarted` the broadcast actor resets `frame_id = 0` for the
  current epoch, immediately emits a position-frame snapshot to all
  connected clients, and transitions to ACTIVE. The next emitted frame
  carries `frame_id = 1`. Coordinated with ADR-02 D7 drop-detection: a
  client whose previous epoch's `frame_id` was non-zero MUST treat the
  reset as a hard reload, not a gap.
  ```

#### Verification
- Integration test: trigger physics restart, assert broadcast actor emits snapshot within one tick and `frame_id` resets.

#### Implementation impact
- Files to modify: broadcast actor state machine.
- Tests to add: restart-snapshot integration test.
- ABI/wire/schema impact: none — same V3 frames; reset signalled by `frame_id` going to 0 then 1.

---

### CC-4 — Inference named-graph IRI disagreement
**Severity**: IMPORTANT
**Affected sections**: 08 (ADR-08 D2, D9), 11 (ADR-11 D2, PRD-11 A6)
**Source specialist(s)**: CC-4 (code-reviewer)

#### Current state
ADR-08 D9 uses `<urn:visionflow:inference>`. ADR-11 D2 and PRD-11 A6 use `<urn:visionflow:graph:ontology:inferred>`. SPARQL queries against the wrong IRI return empty.

#### Recommended resolution
Section 11 owns the persistence layout. Section 8 adopts `<urn:visionflow:graph:ontology:inferred>` verbatim.

#### Proposed edits
- `docs/migration-sprint/08-ontology-graph-data/ADR-08.md` — replace every occurrence of `<urn:visionflow:inference>` with `<urn:visionflow:graph:ontology:inferred>`. Spell out the two named graphs explicitly at D2 line 88:
  ```markdown
  The Oxigraph dataset uses two named graphs:
  `<urn:visionflow:graph:ontology:assert>` for asserted triples and
  `<urn:visionflow:graph:ontology:inferred>` for whelk-rs-derived
  inferences (see ADR-11 §D2).
  ```

#### Verification
- Grep CI: `rg 'urn:visionflow:inference\b' docs/ src/` returns zero matches after edit.
- SPARQL smoke test: `SELECT * WHERE { GRAPH <urn:visionflow:graph:ontology:inferred> { ?s ?p ?o } } LIMIT 1` returns a result after first inference run.

#### Implementation impact
- Files to modify: documentation only; implementation uses Section 11's IRI from inception.
- Tests to add: SPARQL named-graph round-trip test.
- ABI/wire/schema impact: none.

---

### CC-6 — `/ws/xr-presence` missing from ADR-06 D4 (subsumed by T4)
**Severity**: IMPORTANT
**Affected sections**: 06, 12
**Source specialist(s)**: CC-6 (code-reviewer)

#### Resolution
Folded into T4's ADR-06 §D11 enumeration. The `/ws/xr-presence` row is present in the canonical table with posture `RequireAuth — Nostr-signed JWT at upgrade only (no per-frame signing at 30 Hz)`. CC-6 is closed by applying T4's edits; ADR-06 D4 (HTTP table) is unchanged.

#### Verification
- The §D11 table check from T4 covers this.

---

### CC-7 — README phasing contradicts ADR-06 ordering
**Severity**: IMPORTANT
**Affected sections**: README §Phasing, ADR-06 §Phasing
**Source specialist(s)**: CC-7 (code-reviewer)

#### Current state
README lines 113-128 puts all of Section 6 in Phase 7. ADR-06 §Phasing lines 368-388 says "D1+D2 (compile-time gates) must land before Phase 3 (binary protocol) because ADR-02 D8 depends on `--allow-skip-auth` existing." Strict README order blocks ADR-02 D8.

#### Recommended resolution
Split Section 6 across two phases. D1+D2 land in a new Phase 2.5 (between Persistence and Binary Protocol). D3-D11 remain in Phase 7.

#### Proposed edits
- `docs/migration-sprint/README.md` lines 113-128 — restructure the phasing list to insert a new entry between current Phase 2 and Phase 3:
  ```markdown
  **Phase 2.5: Auth compile-gates** — Section 6 ADR-06 D1 (`dev-auth` Cargo
  feature) and D2 (compile-time gating of `--allow-skip-auth` and any
  bypass surface) land. Section 6's remaining decisions D3-D11 ship in
  Phase 7. This split is mandated by ADR-06 §Phasing: ADR-02 D8 depends on
  the `--allow-skip-auth` flag existing in the codebase as of Phase 3.
  ```
  And remove Section 6 from the Phase 7 cluster's full-section enumeration; add a note "Section 6 (D3-D11 only — D1+D2 landed in Phase 2.5)".

#### Verification
- ADR cross-reference check: ADR-06 §Phasing's "must land before Phase 3" claim now matches README chronology.

#### Implementation impact
- Files to modify: `docs/migration-sprint/README.md` only.
- Tests to add: none (documentary).
- ABI/wire/schema impact: none.

---

### CC-8 — WASM build path missing from Dockerfile
**Severity**: IMPORTANT
**Affected sections**: 04 (PRD-04 F7, ADR-04 D8), 09 (ADR-09 D3)
**Source specialist(s)**: CC-8 (code-reviewer)

#### Current state
PRD-04 F7 and ADR-04 D8 expect `client/crates/scene-effects/` compiled to `client/src/wasm/scene-effects/`. ADR-09 D3 enumerates stages `base`/`rust-deps`/`rust-builder`/`node-deps`/`node-builder`/`development`/`production` — no WASM stage. Clean production builds will ship without the WASM artefacts.

#### Recommended resolution
ADR-09 D3 adds a `wasm-builder` stage (or extends `node-builder`) that runs `wasm-pack build` against `client/crates/scene-effects/` and outputs to `client/src/wasm/scene-effects/`.

#### Proposed edits
- `docs/migration-sprint/09-ecosystem-services/ADR-09.md` §D3 — add a new stage entry to the Dockerfile-stage enumeration:
  ```markdown
  - **`wasm-builder`** (after `rust-deps`, before `node-builder`): runs
    `wasm-pack build --target web --release client/crates/scene-effects/`
    and outputs JS/wasm glue to `client/src/wasm/scene-effects/`. The
    `node-builder` stage `COPY --from=wasm-builder` consumes this path
    before `vite build` runs. The `node-builder`'s output therefore
    contains the compiled WASM as part of the static bundle. PRD-04 F7's
    versioning contract holds via the same `client/dist` artefact.
  ```

#### Verification
- Integration test: `docker build --target production .` produces an image whose `/usr/share/nginx/html/wasm/scene-effects/` contains both `.js` and `.wasm` files; file sizes non-zero.

#### Implementation impact
- Files to modify: `Dockerfile` (new `wasm-builder` stage); `ADR-09 D3`.
- Tests to add: production-image WASM-artefact presence check.
- ABI/wire/schema impact: none.

---

### CC-10 — "snapshot" overloaded
**Severity**: IMPORTANT
**Affected sections**: 02, 07, 10, 11
**Source specialist(s)**: CC-10 (code-reviewer)

#### Current state
"Snapshot" carries three meanings: position-frame snapshot (Section 2), telemetry snapshot (Sections 7/10), persistence snapshot (Section 11 — either 60-s `:hasX/Y/Z` write or operator backup). Cross-references leak ambiguity.

#### Recommended resolution
Add a §"Glossary of overloaded terms" to `README.md`. Each section qualifies "snapshot" at first use with an adjective: position-frame / telemetry / persistence / position-triple.

#### Proposed edits
- `docs/migration-sprint/README.md` — append new section:
  ```markdown
  ## Glossary of overloaded terms

  | Term | Disambiguated forms |
  |------|---------------------|
  | snapshot | **position-frame snapshot** (ADR-02 D4 — V3 binary frame from `GraphStateActor::current_snapshot()`); **telemetry snapshot** (ADR-10 D1, DDD-07 — `SwarmSnapshot` JSON on agent reconnect); **persistence snapshot** (ADR-11 D4/D10 — operator backup); **position-triple snapshot** (DDD-11 — per-60 s `:hasX/Y/Z` Oxigraph write). |

  Authoring rule: at first use in any document, write the qualified form.
  Subsequent in-document uses may shorten to "snapshot" if unambiguous.
  ```

#### Verification
- Doc-lint check: each PRD/ADR/DDD's first occurrence of `snapshot` is preceded by `position-frame|telemetry|persistence|position-triple` within 5 words.

#### Implementation impact
- Files to modify: README + first-use qualification across sections.
- Tests to add: doc-lint check.
- ABI/wire/schema impact: none.

---

### CC-11 — Section 12 cross-references conflicting bit schemes
**Severity**: IMPORTANT
**Affected sections**: 02, 04, 08, 12
**Source specialist(s)**: CC-11 (code-reviewer)

#### Current state
PRD-12 F3 cites Section 7's `0x1C000000` scheme; ADR-12 D4 cites "Section 8 / Section 2" with the 6-bit scheme; PRD-04 F1 chains vaguely; ADR-02 D1 says "u32 node_id (including class flag bits)" without enumeration. Multiple co-existing citations to mutually-inconsistent schemes.

#### Recommended resolution
After TC-1 collapses the two schemes onto T1 Option A, point every cross-reference at ADR-08 §D6.

#### Proposed edits
- `docs/migration-sprint/12-xr-client/PRD-12.md` F3 lines 110-113: replace mask phrasing with `See ADR-08 §D6 for the canonical class-bit allocation.`
- `docs/migration-sprint/12-xr-client/ADR-12.md` D4 line 121: replace `"the same scheme the web client uses (per Section 8 / Section 2)"` with `"See ADR-08 §D6."`
- `docs/migration-sprint/04-client-3d-rendering/PRD-04.md` F1 line 68: replace the chained citation with `See ADR-08 §D6.`
- `docs/migration-sprint/02-binary-protocol/ADR-02.md` D1 line 89: append `(a NodeId per ADR-08 §D6).`

#### Verification
- Grep CI: `rg '0x1C000000|0x20000000' docs/migration-sprint/` returns matches **only** in `08-ontology-graph-data/ADR-08.md` and `_resolutions/`.

#### Implementation impact
- Files to modify: four sections, one line each.
- Tests to add: scheme-citation grep gate.
- ABI/wire/schema impact: none.

---

### CC-14 — Worker surface forbids telemetry path PRD-07 claims
**Severity**: IMPORTANT
**Affected sections**: 03 (ADR-03 D7, D9), 07 (PRD-07 §8)
**Source specialist(s)**: CC-14 (code-reviewer)

#### Current state
ADR-03 D7 fixes the worker proxy surface to exactly four methods (`attachPositionSAB`, `writeFrame`, `computeEdgeLengths`, `getStats`); D9 says non-position WS frames flow through text-frame handlers and "none of these touch the worker". PRD-07 §8 claims the agent telemetry stream uses the same Comlink-bridged worker pipeline. Direct contradiction.

#### Recommended resolution
ADR-03 is the source of truth on the worker surface. Rewrite PRD-07 §8: telemetry goes through the WebSocket but is decoded on the main thread. Single-flight discipline applies via the Section-7 coalescer (DDD-07 D11), not the worker frame guard.

#### Proposed edits
- `docs/migration-sprint/07-bots-telemetry/PRD-07.md` §8 lines 184-186 — replace the contested paragraph with:
  ```markdown
  Section 3 (Client State). The agent telemetry stream is delivered via
  WebSocket and decoded on the main thread (text-frame path; see ADR-03
  §D9). The worker proxy surface is closed to four methods per ADR-03 §D7
  and does not handle telemetry. Single-flight discipline for telemetry
  updates is enforced by the Section-7 coalescer (DDD-07 D11), not by the
  worker's frame guard.
  ```

#### Verification
- Cross-reference check: PRD-07 §8 cites ADR-03 §D7 and §D9 correctly; no `worker` references in PRD-07 §8 imply the four-method surface is extended.

#### Implementation impact
- Files to modify: PRD-07 §8 only.
- Tests to add: none (alignment edit).
- ABI/wire/schema impact: none.

---

### CC-15 — GitHub adapter referenced but absent from Section 10
**Severity**: IMPORTANT
**Affected sections**: 08 (ADR-08 D10, DDD-08), 10 (PRD-10 §1+§6, ADR-10)
**Source specialist(s)**: CC-15 (code-reviewer)

#### Current state
ADR-08 D10 and DDD-08 §ACL declare a GitHub adapter producing `ParsedMarkdown` value objects and attribute the contract to Section 10. PRD-10 §1 enumerates three external systems (agentbox, forum, whelk-rs); §6 contracts table has six rows. **GitHub is absent.** Section 8 implements against a phantom contract.

#### Recommended resolution
PRD-10 §1 add a fourth external system (GitHub / Logseq corpus). ADR-10 add a new D11 covering: transport (octocrab REST), `ParsedMarkdown` value-object shape (delegating to DDD-08), SHA1 incremental gating, `FORCE_FULL_SYNC=1` semantics, parse-error envelope.

#### Proposed edits
- `docs/migration-sprint/10-external-integrations/PRD-10.md` §1 — add to the external-systems list:
  ```markdown
  4. **GitHub (Logseq corpus)** — `jjohare/logseq` markdown source-of-truth
     for the knowledge graph. Read-only HTTPS via `octocrab`. Section 8 owns
     the parse; this section owns the transport. See ADR-10 §D11.
  ```
  And add a sixth row to the §6 contracts-at-a-glance table for GitHub.
- `docs/migration-sprint/10-external-integrations/ADR-10.md` — add new decision after D10:
  ```markdown
  ### D11. GitHub adapter (Logseq corpus)

  Transport: `octocrab` REST client. Auth: `GITHUB_TOKEN` from environment.
  Output value object: `ParsedMarkdown` as defined by DDD-08 §"Anti-corruption
  layer to Section 10" — Section 10 is the transport, Section 8 owns the
  parse and the value-object shape.

  Sync gating: `GitHubSyncService::sync_graphs()` SHA1-compares each file's
  blob against the cached hash and skips unchanged files. The
  `FORCE_FULL_SYNC=1` environment variable bypasses gating and forces full
  reparse — used for content-format migrations. Set back to `0` after use.

  Parse-error envelope: errors carry `{ path: string, sha: string,
  error_kind: "yaml" | "wikilink" | "ontology-block" | "io", message: string }`.
  Errors are logged but do not fail the sync; the failed file is retained
  at its previous good version in Neo4j and surfaced as an operator metric
  `github_sync_parse_errors_total{error_kind}`.
  ```

#### Verification
- Cross-reference check: every `Section 10` mention in DDD-08 / ADR-08 resolves to a numbered decision (D11 covers the gap).
- Contract test: `tests/contracts/external-integrations/github_adapter.rs` exercises sync, force-full-sync, parse-error envelope.

#### Implementation impact
- Files to modify: PRD-10 §1 + §6, ADR-10 new D11.
- Tests to add: GitHub adapter contract test.
- ABI/wire/schema impact: documentary; the implementation already exists at `src/services/github_sync_service.rs`.

---

### CC-17 — Ecosystem services have no documented consumers
**Severity**: IMPORTANT
**Affected sections**: 06, 09 (ADR-09 D5), 10
**Source specialist(s)**: CC-17 (code-reviewer)

#### Current state
ADR-09 D5 declares Kokoro TTS (8880), Whisper (8000), Xinference (9997). No section documents which VisionFlow code consumes them. ADR-06 D4 lists `speech_socket_handler`, `inference_handler`, `ragflow_handler` without service mapping.

#### Recommended resolution
ADR-09 add §"Consumers" listing each ecosystem service ↔ VisionFlow handler.

#### Proposed edits
- `docs/migration-sprint/09-ecosystem-services/ADR-09.md` — append new section after D5:
  ```markdown
  ### D5a. Consumers

  | Ecosystem service | VisionFlow consumer | WS endpoint | ADR-06 D4 handler |
  |-------------------|---------------------|-------------|-------------------|
  | Kokoro TTS (8880) | TTS dispatch in speech actor | `/ws/speech` (egress) | `speech_socket_handler` |
  | Whisper STT (8000) | STT dispatch in speech actor | `/ws/speech` (ingress) | `speech_socket_handler` |
  | Xinference (9997) | RAG embeddings + completion | n/a (HTTP) | `inference_handler`, `ragflow_handler` |

  Out-of-scope for this sprint: any expansion of this consumer list.
  Adding a new ecosystem service requires a row here in the same PR.
  ```

#### Verification
- Cross-reference check: every handler named in ADR-06 D4 that touches an ecosystem service appears in the D5a table.

#### Implementation impact
- Files to modify: ADR-09 §D5a (new sub-section).
- Tests to add: none (documentary).
- ABI/wire/schema impact: none.

---

### CC-12 — Audit archive table growth not in backup procedure
**Severity**: NIT
**Affected sections**: 06 (ADR-06 D6), 11 (ADR-11 D10)
**Source specialist(s)**: CC-12 (code-reviewer)

#### Proposed edits
- `docs/migration-sprint/11-persistence-migration/ADR-11.md` §Risks — append:
  ```markdown
  - **Audit archive table growth.** ADR-06 §D6 creates monthly archive
    tables `audit_log_archive_yyyymm` that accumulate indefinitely.
    Mitigation: operator runbook at `docs/operations/audit-log-retention.md`
    covers `DROP TABLE audit_log_archive_yyyymm` for retired months
    (default retention 24 months).
  ```

#### Verification
- Runbook file exists at the cited path.

#### Implementation impact
- Files to modify: ADR-11 §Risks; new operator runbook.

---

### CC-13 — BroadcastChannel naming convention undeclared
**Severity**: NIT
**Affected sections**: 10 (ADR-10 D3, D4)
**Source specialist(s)**: CC-13 (code-reviewer)

#### Proposed edits
- `docs/migration-sprint/10-external-integrations/ADR-10.md` — append a small subsection after D4:
  ```markdown
  ### BroadcastChannel naming convention

  Prefix: `visionflow:`. Suffix: kebab-case noun describing the channel's
  payload. Current channels: `visionflow:agent-actions` (D3),
  `visionflow:auth` (D4). New channels register here in the same PR that
  introduces them.
  ```

#### Verification
- Grep CI: `rg "BroadcastChannel\(['\"]" client/src/` — every literal matches `visionflow:[a-z-]+`.

#### Implementation impact
- Files to modify: ADR-10 (subsection); future channels add a row.

---

### CC-16 — PRD/ADR heading conventions inconsistent
**Severity**: NIT
**Affected sections**: all
**Source specialist(s)**: CC-16 (code-reviewer)

#### Proposed edits
None for existing docs. Add `docs/migration-sprint/_templates/PRD-template.md` and `ADR-template.md` for future authoring, using PRD-12's `F1..Fn` + `A1..An` shape as canonical. Do not retrofit.

#### Verification
- Template files exist; future PRDs reference them.

#### Implementation impact
- Files to create: two templates.

---

### CC-Gaps — Coverage gaps (five unowned surfaces)
**Severity**: NIT (collectively); individual items may surface during implementation
**Affected sections**: 03, 06, 09, 11, 12
**Source specialist(s)**: CC §"Coverage gaps" (code-reviewer)

#### Items
1. **Cloudflared tunnel ownership.** ADR-09 D12 punts to Section 6; ADR-06 has no tunnel section. **Resolution**: ADR-06 add a one-paragraph subsection D12 owning the tunnel config + secret rotation, or explicitly hand back to Section 9. Recommended owner: ADR-06 (auth surface).
2. **Auth endpoint path inconsistency.** PRD-12 F2 / ADR-12 D8 use `/auth/challenge` and `/auth/verify`; ADR-06 D4 has `/challenge` and `/session`. **Resolution**: reconcile to `/api/auth/*` in ADR-06 D4; ADR-12 follows.
3. **Migration tool fate post-cutover.** DDD-11 line 509 says removed from workspace; ADR-11 step 8 deletes only adapter files. **Resolution**: ADR-11 §Implementation order add explicit "delete `tools/migrate-neo4j-to-oxigraph/` 14 days after cutover, after one successful re-run verifies backup path."
4. **`computeEdgeLengths` consumer.** ADR-03 D7 surfaces it; no section calls it. **Resolution**: verify and prune. If dead, drop from D7's four-method surface (now three).
5. **NIP-98 canonical URL spec location.** ADR-06 D10 says client and server share `canonicalise_url` but the spec is in prose only. **Resolution**: ADR-06 D10 promote the trailing-slash / param-order / fragment-handling rules to a numbered list; cite by anchor.

#### Verification
Each item resolved during the affected section's implementation phase; no blocker.

#### Implementation impact
- Files to modify: section-by-section as the items come up.

---

## Cross-cutting follow-ups

These follow-ups are not new tensions; they consolidate engineering work that multiple specialists called out:

1. **`crates/visionclaw-contracts`** — shared crate hosting `AgentActionEnvelope`, `AgentTelemetryEnvelope`, `EnterpriseEventEnvelope`, binary-protocol header constants. `ts-rs` emits `.d.ts`. ADR-10 §D8 versioning gates the crate's semver. Required for T7 closure; benefits CC-1, CC-15.
2. **Contract test harness** at `tests/contracts/external-integrations/` with an agentbox-emulator (ADR-10 R1). Each envelope variant round-trips; receiver structured-error responses asserted. Required for T7, CC-1, CC-15.
3. **CI drift checks**:
   - Route table drift (ADR-06 §D11 vs `App::new()`).
   - SQLite schema drift (`CREATE TABLE` only in ADR-11).
   - Class-bits invariant (mask coverage; TC-1).
   - Settings trait-surface SHA (TC-5).
   - Glossary first-use qualifier (CC-10).
   - Deprecated route names re-introduction (T6 / ADR-10 D7).
4. **Heartbeat-interval cancellation test** (T3 BDD-3).
5. **Class-bits unit test** (TC-1 mask coverage + round-trip).
6. **Trait-surface SHA test for `SettingsRepository`** (TC-5).
7. **Production-image WASM-artefact presence check** (CC-8).
8. **WireMock-style mapping table** for ACL wire→internal events (CC-1, used by Section 7 tests).
9. **Operator runbook** for audit-log retention (CC-12).
10. **PRD/ADR templates** at `docs/migration-sprint/_templates/` (CC-16).

---

## Style and structural observations

From CROSS-CUTTING.md §"Style / structural observations":

- Decision numbering (`D1, D2, …`) is consistent across all 11 ADRs.
- "Rejected from main as buggy / unjustified" and "Bugs and smells at the reset point" sub-sections are present and consistently labelled. Good.
- Cross-references mix `ADR-NN`, `Section N`, and full file paths. **Recommend** standardising on `ADR-NN §SS Decision DN` for in-prose refs; full path only in the `Related:` header.
- README phasing block lines 113-128 calls Phase 7 "parallel" — that is a cluster, not a phase. **Recommend** renaming to "Phase 7+: parallel work streams" or splitting into per-section phases. Combine with CC-7.
- Domain-event taxonomy is spread across DDD-01, DDD-07, DDD-08, DDD-11 (25+ events). **Recommend** a consolidated event reference table in README (producer × consumer × wire correspondence).
- PRD-11 reproduces a full SPARQL walkthrough that ADR-11 §D7 already covers. **Flag** for future trim (not urgent).
- No section has a "Test plan" / "Verifiability summary" mapping A-criteria to test classes. **Recommend** a §"Verification" in each ADR — required for the `/build-with-quality` pipeline to wire cleanly.

---

## Decision checkpoint

This document is the **single authority** for the resolutions above. Where the 12 sprint section docs (PRD/ADR/DDD) conflict with this document, **this document wins** until the proposed edits land in a follow-up commit titled `sprint: apply QE-fleet tension resolutions`.

Implementation MUST NOT begin for any BLOCKER-affected section until the resolutions land in the source docs:

- **TC-1** blocks Sections 2, 7, 8 — **and requires explicit human acceptance** that ADR-08 §D6's literal text was the inaccurate document (specialist conflict; T1's adjudication chosen on cost grounds).
- **T2** blocks Section 6 (Phase 2.5 work) and Section 2 (relies on `--allow-skip-auth`).
- **TC-5** blocks Sections 5, 6, 11.
- **T4** blocks Sections 2, 6, 7, 10, 12 (URL space + path corrections).
- **T7** blocks Sections 7 and 10 simultaneously (envelope is the contract between them).

**IMPORTANT-severity items** can be addressed during their owning section's implementation phase; they are not gating sprint kick-off. **NIT-severity items** clean up during implementation.

Once a project lead has confirmed TC-1's adjudication, this resolution set is ready to drive implementation.

---

## Appendix — Specialist outputs

Preserved unmodified at `docs/migration-sprint/_resolutions/`:

- `T1-class-bits.md` (code-intelligence, 204 lines)
- `T2-auth-gating.md` (security-auditor, 220 lines)
- `T3-heartbeat.md` (performance-validator, 209 lines)
- `T4-T6-T7-api-contracts.md` (api-contract-validator, 381 lines)
- `T5-settings-schema.md` (requirements-validator, 205 lines)
- `CROSS-CUTTING.md` (code-reviewer, 367 lines)

These specialist outputs are the audit-trail source for every resolution above. Where this master document compresses or merges, the original specialist file carries the full reasoning.
