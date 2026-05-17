# 05 — Phase 2 (solid-pod-rs) + ADR-054 (URN-Solid) Scope Survey

Scope-only confirmation. Test depth for solid-pod-rs is out of scope for
this re-audit — these streams are net-new deliverables, not debt-payoff,
and ADR-055 flags them as P1/P2 rather than P0.

## solid-pod-rs Phase 2

### PARITY-CHECKLIST.md delta

Counted directly from `crates/solid-pod-rs/PARITY-CHECKLIST.md`:

| State | Sprint 1 close | Sprint 2 close |
|-------|---------------:|---------------:|
| present | 27 | **61** |
| partial | 7 | 8 |
| missing | 25 | 11 |

The checklist's own footer at :eof declares `present: 48, partial: 8,
missing: 11` — the 48 refers to "P1 + P2 rows present", which differs
from the raw 61 because some present rows are scoped at P2+ (informational
only per the "Phase 1 → Phase 2 boundary" block). Either accounting puts
the sprint squarely past the ADR-055 target of "48/67 parity."

### LDP conformance (`src/ldp.rs`, 1,384 LoC)

- `Link` header emission via `link_headers()` covers `rel=type`,
  `rel=acl`, `rel=describedby` per the checklist rows at :19-29.
- `Prefer` header parser `PreferHeader::parse`, selects representation
  mode for `PreferMinimalContainer` / `PreferContainedIRIs`.
- `ACCEPT_POST` constant + the Accept-Post header emitter cover the P2
  row at :29.
- SPARQL-Update / N3-Patch entry-points referenced from the "Tests"
  block at :eof (`n3_patch_insert_and_delete`,
  `sparql_insert_data`, `sparql_delete_data`).

### Solid-OIDC (`src/oidc.rs`, 672 LoC)

Feature-gated cleanly:
`#![cfg(feature = "oidc")]` at :22 of `src/oidc.rs`. Cargo feature
`oidc = ["dep:openidconnect", "dep:jsonwebtoken"]` at `Cargo.toml:58`.
Optional deps in the `[dependencies]` block make the default build lean.
DPoP binding (`cnf.jkt` comparison vs SHA-256 of the proof header's JWK),
dynamic client registration (RFC 7591), and discovery document are all
implemented; checklist rows `access_token_binds_to_dpop_jkt` and
`dynamic_registration_returns_client_id` confirm test coverage.

### Solid Notifications (`src/notifications.rs`, 624 LoC)

Both channels in-module:
- `WebSocketChannel2023` — broadcast via
  `broadcast::Sender<StorageEvent>` backed by `Storage::watch()`.
- `WebhookChannel2023` — HTTP POST with Activity Streams 2.0 JSON-LD
  payload (docstring at :10-19 pins the shape).

Discovery advertised both; checklist row `discovery_lists_both_channels`
confirms.

### ACL inheritance corpus

`tests/wac_inheritance.rs` — 735 LoC, 31 `#[test]` functions. Ports the
JSS ACL edge-case corpus referenced in ADR-055 Stream B. Covers the full
"Web Access Control (WAC)" section of the checklist (20+ rows in the
`| present | P1 |` column).

### JSS parity gate (`tests/interop_jss.rs`)

705 LoC, 30 tests. Fixture-driven harness that asserts response-shape
parity against captured JSS outputs (`tests/fixtures/`). This is the
ADR-055 Stream B deliverable "JSS test corpus adapted." Not all tests
are green by default (some checklist rows are still `partial` or
`missing`), but the harness mechanism is in place and trivially
extensible as more features land.

## ADR-054 URN-Solid alignment

### `docs/reference/urn-solid-mapping.md`

51 mapping rows (counted via `grep -c '^| '`). Covers the initial ~50
vocab mappings declared in ADR-055 Stream C.

### `corpus.jsonl` generator hook in saga

Implemented in `src/services/ingest_saga.rs`:

- Public method `IngestSaga::regenerate_corpus_jsonl(owner, None)` at
  :669-765. PUTs the result to
  `{pod_base}/{owner}/public/kg/corpus.jsonl` (URL builder at :793-798).
- Fired from `VisibilityTransitionService::maybe_regenerate_corpus` at
  `src/sovereign/visibility.rs:360-379` on both publish (:458) and
  unpublish (:569). Flag-gated on `URN_SOLID_ALIGNMENT`
  (`urn_solid_alignment_enabled()` imported from the mapping module).
  Best-effort: failures logged, never propagated to the visibility
  transition caller — consistent with the "additive ecosystem surface"
  stance in ADR-055.
- Saga integration confirmed by `IngestSaga::set_urn_solid_mapper(...)`
  at :131-178 (ADR-054 mapper is optional, injected at construction).

### Type manifest + KGNode schema

Implemented inline in `src/handlers/solid_proxy_handler.rs`:

- `render_kg_node_schema_json(pod_base, npub)` at :163-225 emits JSON
  Schema 2020-12 with the `x-urn-solid` extension carrying term,
  status, and lineage. Target URL
  `{pod}/{npub}/public/schema/kg-node.schema.json`.
- `render_manifest_jsonld(pod_base, npub)` at :234-255 emits a
  JSON-LD manifest binding `urn:solid:KGNode` to the schema URL plus
  7 upstream `solid-schema:*` type aliases. Target URL
  `{pod}/{npub}/public/schema/manifest.jsonld`.

The files are not checked into `schema/` in the repo; they are
rendered at request time and PUT to each user's Pod during provisioning.
This matches the ADR-054 design intent (per-Pod manifests are tenant
data, not server data). Tests
`kg_node_schema_carries_x_urn_solid_extension` (:232-276 in
`tests/urn_solid_alignment.rs`) and
`type_manifest_contains_urn_solid_kg_node_binding` (:208-231) pin the
shapes.

## Not-yet-production flags

Nothing in this stream is "stubbed" in the leak-risk sense. Everything
is code-complete behind a flag:

- `URN_SOLID_ALIGNMENT=false` (default) ⇒ no `sameAs`, no corpus, no
  manifest. Pre-Sprint-2 behaviour.
- `oidc` Cargo feature off (default) ⇒ solid-pod-rs compiles without
  `openidconnect`/`jsonwebtoken`; no OIDC endpoints in built binary.

Partials in the parity checklist (8 rows) are feature-level
completeness statements — e.g. "PATCH accepts N3 but not full
SPARQL 1.1 Update grammar" — not privacy or authorisation gaps.

## Verdict

Scope of Phase 2 + ADR-054 is complete as declared in ADR-055. No
stubbed primitives, no silent leaks. The new surface is ready for
`URN_SOLID_ALIGNMENT=true` canary rollout once the debt-payoff findings
(B1-B3, H1-H3) have soaked in staging per ADR-055 compliance criterion #5.
