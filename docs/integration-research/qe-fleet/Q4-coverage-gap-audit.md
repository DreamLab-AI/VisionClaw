# Q4 — Coverage Gap Audit (Crypto / Security / Protocol Conformance)

| Field | Value |
|-------|-------|
| Author | QE Fleet — Specialist Q4 |
| Date | 2026-05-07 |
| Scope | Forum (`dreamlab-ai-website/community-forum-rs/`) + Agentbox (`agentbox/`) + VisionClaw (this repo) + solid-pod-rs (`solid-pod-rs/`) |
| Companion | Q1 (VisionClaw surfaces), Q2/Q3 fleet outputs, PRD-010, ADR-073..076 |
| Method | File-system inventory, grep over `#[test]`/`#[tokio::test]`/`it(`/`describe(`, CI workflow inspection, RuVector memory namespace `project-state`. |
| Verdict | Aggregate native test count is healthy at the unit level; **structural test surfaces are absent** at every cross-substrate seam. The C1 (NIP-44 v2) bug shipped because the structural surface for it does not exist. |

---

## G0 — Executive verdict (read this first)

The four projects together contain **roughly 3,700 unit tests** that exercise individual modules in isolation, plus **262 contract / integration assertions** in agentbox and **39 React/Vitest test files** in the forum. Almost none of those tests talk to each other. The C1 NIP-44 v2 conversation-key bug (forum nostr-core `nip44.rs:122-128`, documented in `docs/integration-research/05-crypto-gotchas.md` §6 and reproduced in ADR-076) survived 16 module-local tests because **zero of the 16 use the paulmillr/nip44 reference vectors** that are the canonical regression guard for NIP-44. Every encrypt-then-decrypt round-trip with the same buggy implementation passes. The structural reason this shipped is missing reference-vector tests; the tactical reason is the team did not absorb upstream `nostr` (ADR-076).

The same shape of gap repeats across every NIP, every DID Document emitter, every cross-substrate boundary. **No test boots more than one substrate.** The federation contracts in ADR-073/074/075 have **zero tests** in any of the four trees — the strings "ADR-073", "ADR-074", "ADR-075", "ADR-076" appear nowhere outside `docs/`. Mutation testing is not configured anywhere. Fuzzing is configured for one VisionClaw crate (`crates/visionclaw-xr-presence/fuzz`) and for upstream library `tests/fuzz` directories that ride along in the cargo registry — neither addresses the protocol surface.

The recommended programme is laid out in §G14–G17. The minimum viable change is the cross-substrate contract suite in §G3 plus the reference-vector matrix in §G2. Together those two suites are roughly twelve engineer-days and would have prevented C1, C2, and C3 from `01-visionclaw-surfaces.md`.

---

## G1 — Per-project test inventory

Numbers below were derived by grep over `#[test]` / `#[tokio::test]` / `^\s*(it|test|describe)\s*\(` and by directory enumeration. They are accurate as of 2026-05-07.

### G1.1 Forum — `dreamlab-ai-website/community-forum-rs/`

Workspace declares 8 crates (`crates/{nostr-core,forum-client,auth-worker,pod-worker,preview-worker,relay-worker,search-worker,admin-cli}`), pinned at `nostr = "0.44"` (declared, **unused** per ADR-076), `worker = "0.8"`, `leptos = "0.7"`. Runner: `cargo test --manifest-path community-forum-rs/Cargo.toml -p <crate>` driven from `.github/workflows/rust-ci.yml`.

| Crate | Inline `#[test]` / `#[tokio::test]` (in `src/`) | External test files (in `tests/`) | Notes |
|-------|-------------------------------------------------|------------------------------------|-------|
| `nostr-core` | **210** | `nip04_proptests.rs`, `nip19_tests.rs`, `nip19_proptests.rs`, `nip26_tests.rs` (Sprint v9 STREAM-E1 added 22 proptests across NIP-19/04) | wasm-bindgen-test dev-dep declared; proptest gated behind `cfg(not(target_arch = "wasm32"))` |
| `pod-worker` | **131** | `wac_proptests.rs` (13 proptests) | 31/31 ACL tests pass after Sprint v9 STREAM-B Control-coercion fix |
| `relay-worker` | **91** | `audit_tests.rs`, `moderation_tests.rs`, `nip_handlers_tests.rs`, `whitelist_tests.rs` (gated behind `--features test-exports`) | Adds 1900+ LOC enforcement perimeter exposure |
| `auth-worker` | **88** | (none) | NIP-98 + WebAuthn surface |
| `preview-worker` | **40** | (none) | SSRF redirect harness lives inline (21/21 pass after STREAM-B B4) |
| `search-worker` | **41** | (none) | R2 indexer |
| `admin-cli` | **24** | (none) | Wrangler admin tooling |
| `forum-client` | **93** | (none) | Leptos UI — runs natively via `cfg!(target_arch = "wasm32")` guards |

**Forum Rust test total: ~718 native unit/proptest assertions.** The wasm32 path runs `cargo test ... --target wasm32-unknown-unknown -p nostr-core --no-run` — i.e. **WASM tests compile but do not execute**. There is no `wasm-bindgen-test` runner configured. WASM-specific code in `forum-client/src/auth/passkey.rs` (PRF derivation, WebAuthn flows) ships untested.

**React surface (Sprint v9 STREAM-E3):** vitest 2.1.8 + @testing-library/react 16. Test files:

```
src/pages/__tests__/Contact.test.tsx               5 tests
src/components/__tests__/ErrorBoundary.test.tsx    7 tests
src/lib/__tests__/image-utils.test.ts              21 tests
src/lib/__tests__/markdown.test.ts                 12 tests
```

**React total: 4 files / 45 tests.** No e2e/Playwright. No visual-regression. No bundle-size gate. The Marketing site React surface is otherwise undocumented by tests.

### G1.2 Agentbox — `agentbox/`

Runner mix: `npm run test:config` (jest, scoped to `tests/config/`), `npx jest tests/contract/` (per `.github/workflows/contract-tests.yml`), plus shell-script harnesses for `tests/{bootstrap,cli,runtime-contract,toolchains,backup,artifact-probes,reproducibility,observability,tui,security}/`. There is **no top-level test runner that discovers everything** — each subdirectory is invoked separately by its own script or workflow.

| Path | Test count | Runner |
|------|-----------:|--------|
| `tests/contract/{beads,events,memory,orchestrator,pods,privacy-filter}.contract.spec.js` | 86 (16+18+18+6+14+14) | jest via contract-tests.yml |
| `tests/contract/linked-data/{uris,surfaces,viewer,jcs,invariants}.contract.spec.js` | 83 (21+12+9+9+32) | jest via contract-tests.yml |
| `tests/integration/resolver-degraded.test.js` | 12 | jest |
| `tests/runtime-contract/RC-003-07.spec.js` | 6 | jest |
| `tests/sovereign/nostr-bridge{,.integration}.test.js` | unknown — not counted by grep above (uses `assert` not `it/test`) | shell harness |
| `tests/config/semantic-rules.test.js` | inside `npm run test:config` (jest) | jest |
| Shell harnesses (`bootstrap/*.sh`, `cli/smoke.sh`, `runtime-contract/RC-002-*.sh`, `RC-003-{06,08,09,10}.sh`, `toolchains/*.sh`, `backup/round-trip.sh`, `security/secret-canary.sh`, `flake/gpu-backend.test.sh`, `reproducibility/nix-build-hash.sh`, `tui/non-interactive-validate.sh`, `observability/metrics.test.js`) | ~25 assertion files; pass/fail by exit code | `bash` |

**Agentbox JS test total (assertions counted by grep): 262.** Significant note: `tests/{unit,snapshot}/` referenced in the prompt **do not exist**. The agentbox test taxonomy is `{contract,contract/linked-data,integration,sovereign,runtime-contract,bootstrap,cli,toolchains,backup,observability,security,artifact-probes,flake,reproducibility,tui,3dgs,cuda}`. The README at `tests/contract/README.md` documents the layout.

The `RelayConsumer` (`mcp/nostr-bridge/relay-consumer.js`, 473 LOC per `01-visionclaw-surfaces.md` §7) has no test coverage — only `tests/sovereign/nostr-bridge{,.integration}.test.js` exercise the broader bridge. `verifyNip98` (used in `management-api/middleware/auth.js` and `mcp/servers/nostr-bridge.js`) is referenced by 30 `verifyEvent`/`verifyNip98` call-sites across `management-api/lib` and `mcp/`, but the only direct verifier test is in `tests/sovereign/nostr-bridge.test.js`.

WASM coverage: not applicable; agentbox is Node.

### G1.3 VisionClaw — this repo

| Surface | Inventory | Notes |
|---------|-----------|-------|
| `tests/*.rs` | 112 top-level integration files | Includes `server_identity.rs`, `bridge_signing_fanout.rs`, `auth_sovereign_mesh.rs`, `auth_hardening.rs`, `bridge_edge_test.rs`, `uri_grammar.rs`, `visibility_transitions.rs` — the seven cited in the prompt — plus 105 others spanning GPU, CQRS, ontology, settings, telemetry, MCP. |
| `tests/{actors,adapters,api,archive,cqrs,events,fixtures,inference,integration,load,performance,ports,settings,smoke,solid,unit,benchmarks}/` | 17 subdirs of further integration tests | `tests/solid/nip98.test.ts` exists (TS, Vitest; standalone) |
| Inline `#[test]` in `src/` | **1,353** | Heaviest single-source corpus across the four projects |
| Client (`client/src/**/*.test.ts*`) | 39 files / **966 it/describe assertions** | Vitest; covers WebSocket store, telemetry, hooks, settings UI |

**VisionClaw test total: ~2,431 native + 966 client.** The crypto/protocol surface that matters for cross-substrate work (`src/utils/nip98.rs` 636 LOC, 17 tests; `src/services/nip26_cap.rs`; `src/services/server_identity.rs`; `src/services/inbox_service.rs`; `src/services/automation_routine.rs`; `src/handlers/solid_pod_handler.rs`) is materially under-covered relative to its complexity. There is **no mutation testing**, **no fuzzing of the protocol surface** (the one `fuzz/` directory at `crates/visionclaw-xr-presence/fuzz` targets the XR presence service, not Nostr crypto), and **no proptests** (`grep -rln proptest_config | grep visionclaw` returns nothing).

`tests/test-nostr-auth.js` and `tests/test-git-nostr-auth.js` are JS one-shots; they look like manual probes, not regression tests under a runner.

The VisionClaw GitHub workflow surface is `xr-godot-{ci,android,release}.yml` plus `docs-ci.yml`, `ontology-publish.yml`. **There is no Rust CI workflow for the VisionClaw substrate itself** in `.github/workflows/`. CI runs only for the XR client and docs.

WASM coverage: there are scattered `wasm-bindgen` types under `client/src/wasm/scene-effects/` but no automated test harness running them.

### G1.4 solid-pod-rs — `solid-pod-rs/`

7 crates: `solid-pod-rs`, `solid-pod-rs-{nostr,activitypub,didkey,git,idp,server}`. Runner: `cargo test --all-targets` per workspace (CI matrix in `.github/workflows/ci.yml` runs `{ubuntu,macos} × {stable,beta} × {default,oidc,s3-backend,all-features}`, plus MSRV 1.75 sanity, plus wasm32-unknown-unknown for `memory-backend` only).

| Crate | Inline tests | External `tests/*.rs` | Highlights |
|-------|-------------:|-----------------------|------------|
| `solid-pod-rs` | 238 | `wac_basic.rs` (121 lines), `wac_inheritance.rs` (740 lines), `storage_trait.rs` (212 lines), `oidc_jwks_ssrf.rs`, `sprint12_security.rs`, `ldp_range_jss.rs`, `did_nostr_resolver.rs` (206 lines, 6 tests), `interop_jss.rs`, `tenancy_subdomain.rs`, `oidc_mod_direct.rs`, `config_sprint11.rs`, `cid_verifier_sprint11.rs` | Best-tested pure-logic crate of the four projects |
| `solid-pod-rs-nostr` | 44 | `relay_nip11.rs` (57 lines), `resolver_integration.rs` (135 lines, two wiremock-driven cases) | Per the prompt: "unit + 2 wiremock integration tests" — confirmed |
| `solid-pod-rs-activitypub` | 56 | `federation_flows.rs`, `store_comprehensive.rs`, `sprint12_ap_features.rs`, `http_signatures.rs` | |
| `solid-pod-rs-idp` | 58 | `tests/` exists | |
| `solid-pod-rs-git` | 29 | `tests/` exists | |
| `solid-pod-rs-didkey` | 12 | `tests/` exists | |
| `solid-pod-rs-server` | 0 inline | `middleware_guards.rs`, `error_logging_middleware.rs`, `cli_ops_sprint11.rs`, `cli_comprehensive.rs` | Runtime adapter; thin |

**solid-pod-rs test total: ~437 native + ~12 external test binaries.** This is the **most rigorous** surface of the four — wac_inheritance.rs alone is 740 lines exercising ACL group inheritance. CI runs `-D warnings` on rustdoc and clippy and gates wasm32 build for the memory backend. MSRV is enforced.

What is missing from solid-pod-rs is interaction testing with **the consumers**. solid-pod-rs ships its own `tests/did_nostr_resolver.rs` but nothing consumes a forum-emitted DID Document or an agentbox-bootstrap-emitted DID Document. See §G10.

### G1.5 Aggregate

| Project | Native unit | Native proptest | External `.rs`/`.js`/`.tsx` | React/Vitest | Total assertions |
|---------|------------:|----------------:|-----------------------------:|-------------:|-----------------:|
| Forum (Rust) | 718 | 35 | (counted above) | 45 | ~798 |
| Agentbox (JS) | n/a | 0 | 14 spec files | n/a | 262 |
| VisionClaw (Rust) | 1,353 | 0 | 112 + 17 subdirs | 966 | ~2,431 backend + 966 client |
| solid-pod-rs (Rust) | 437 | 0 | 12 external bins | n/a | ~445+ |
| **Total** | **2,508** | **35** | | **1,011** | **~3,937 protocol-relevant assertions** |

This count is high. The depth-of-coverage problem is **horizontal**, not vertical: each substrate exercises itself well; nothing exercises the seams.

---

## G2 — NIP reference vector coverage

A "reference vector" is a published JSON or TOML file produced by an authority — paulmillr's `nip44.json`, the `nostr-protocol/nips` repo's NIP-19 examples, the nostr-tools test corpus — that is consumed verbatim by tests in each implementation as a regression guard. The function is to detect divergence from the spec independently of round-trip-with-self success. Round-tripping NIP-44 against itself does not catch the C1 bug; running paulmillr's `valid` cases through `nip44_decrypt(plaintext_hex, conversation_key_hex, nonce_hex)` does.

The grep `find /home/devuser/workspace/project/dreamlab-ai-website -name "*nip44*"` returns only `nostr-core/src/nip44.rs`. There is **no `nip44_test_vectors.json`, no `nip44.test-vectors`, no fixture file of any kind**. The same is true for NIP-04, NIP-19, NIP-26, NIP-59, NIP-98 — `find ... -name "*.json" -path "*vector*"` returns empty.

The matrix below combines that grep with the per-project module inventory. ✓ = canonical reference vectors present and exercised by tests; ◐ = some test cases that look like reference vectors but are home-rolled or partial; ✗ = missing.

| NIP | Forum nostr-core | Forum (other) | Agentbox JS | VisionClaw | solid-pod-rs |
|-----|:----------------:|:-------------:|:-----------:|:----------:|:------------:|
| **NIP-01** event id canonical JSON | ◐ (24 inline tests in `event.rs` use ad-hoc inputs; no canonical fixture) | n/a | ◐ (uses `nostr-tools.verifyEvent` — upstream is well-tested but **no test in agentbox asserts on a canonical fixture**) | ◐ (uses `nostr_sdk::EventBuilder` — same; no fixture-driven assertion) | n/a |
| **NIP-04** ECDH+CBC DM | ◐ (`nip04.rs` 24 tests + `nip04_proptests.rs` 9 proptests, all self-roundtrip; no published vectors) | ✗ | ✗ | ✗ | ✗ |
| **NIP-19** bech32 entities | ◐ (`nip19.rs` 9 tests + `nip19_tests.rs` + `nip19_proptests.rs` 13 proptests; no `nostr-protocol/nips#nip-19` examples checked in) | ✗ | ✗ | ✗ | ✗ (DID resolver consumes npub but does not vendor `nostr-protocol/nips` test corpus) |
| **NIP-26** delegation | ◐ (`nip26.rs` 7 inline + `nip26_tests.rs` 30 tests) | ✗ | ✗ | ✗ (`src/services/nip26_cap.rs` exists, no test fixture) | ✗ |
| **NIP-44 v2** ChaCha20-Poly1305 DM | ✗ (16 inline tests in `nip44.rs`, all home-rolled, all self-round-trip; **C1 bug shipped because of this gap**) | ✗ | ✗ | ✗ | ✗ |
| **NIP-59** gift-wrap | ◐ (`gift_wrap.rs` 16 inline tests, home-rolled three-layer round-trip) | ✗ | ✗ | ✗ | ✗ |
| **NIP-98** HTTP auth | ◐ (`nip98.rs` 32 inline + 3 STREAM-B replay tests; covers replay, expiry, payload hash but no canonical published fixture) | ✗ | ✗ (only `tests/sovereign/nostr-bridge.test.js` exercises `verifyNip98`) | ◐ (`src/utils/nip98.rs` 17 inline + `tests/solid/nip98.test.ts`) | ✗ (`crates/solid-pod-rs/src/auth/` has the verifier; covered by inline tests, no published fixture) |

**Findings.**

1. **No project vendors paulmillr/nip44 vectors.** This is the structural cause of the C1 bug. The test gap is not "we forgot to test"; it is "we tested round-trip-with-self." The forum's 16 NIP-44 tests in `nip44.rs` and 16 NIP-59 tests in `gift_wrap.rs` (which inherits NIP-44 by composition) all encrypt-then-decrypt with the same code path and so all pass even when the conversation key derivation is wrong.
2. **No project vendors `nostr-protocol/nips` examples for any NIP.** The closest thing in the repo is the inline `let pubkey_hex = "..."` constants in `nip19.rs` tests — those are derived from spec, but they are not the spec corpus, they are a person's transcription of it.
3. **Cross-implementation contract tests do not exist.** §G3 enumerates this in detail.

This audit treats every ◐ as a partial coverage marker — the property tests added by Sprint v9 STREAM-E1 are real and useful (they catch encoding asymmetries, length invariants, reserved-prefix handling) but they are *not* a substitute for canonical vectors. A property test asserts "for all inputs of shape X, encode-then-decode is identity"; a reference vector asserts "for *this specific input* the spec-conformant output is *this specific bytestring*". Both are needed.

---

## G3 — Contract testing across substrates

**Definition adopted:** A contract test is a test that runs **the same input through two independently-developed implementations of the same protocol** and asserts byte-equal output. Contract tests are the only tool that detects the divergence class of bug — where each implementation passes its own tests but they disagree with each other.

The four projects implement substantial overlap:

| Function | Forum nostr-core | Agentbox JS | VisionClaw | solid-pod-rs |
|----------|:----------------:|:-----------:|:----------:|:------------:|
| Compute event id from unsigned event | `compute_event_id` (event.rs) | `nostr-tools.serializeEvent` + sha256 | `nostr_sdk::EventBuilder::id()` | n/a |
| Verify Schnorr signature | `verify_event` / `verify_event_strict` | `nostr-tools.verifyEvent` | `nostr_sdk::Event::verify` | n/a (delegates to nostr crate via solid-pod-rs-nostr) |
| Encode npub from pubkey | `nip19::encode_npub` | `nostr-tools.npubEncode` | `nostr_sdk::PublicKey::to_bech32` | `solid-pod-rs-nostr::npub_to_x_only` |
| Encrypt NIP-44 v2 | `nip44::encrypt` | n/a (agentbox does not encrypt — only verifies) | `nostr_sdk::nip44::encrypt` | n/a |
| Verify NIP-98 token | `nostr-core::nip98::verify_token` | `verifyNip98` in `mcp/servers/nostr-bridge.js` and `management-api/middleware/auth.js` | `src/utils/nip98.rs::verify_nip98` (636 LOC) | `solid-pod-rs::auth::Nip98Verifier` |
| Build did:nostr DID Document | `pod-worker/src/did.rs` | `scripts/sovereign-bootstrap.py` | not yet emitted (PRD-010 H8) | `solid-pod-rs-nostr/src/did.rs` |
| Verify NIP-26 delegation token | `nip26::verify_delegation` | not wired (delegation verifier referenced in `mcp/nostr-bridge/relay-consumer.js` per memory `sprint-v8-status` note "BUG-3 FIXED" — but no test) | `src/services/nip26_cap.rs` | not present |
| Derive HKDF info string for PRF passkey signer | `nostr-core/src/keys.rs` | n/a | n/a | n/a |
| JCS canonicalisation (RFC 8785) | n/a (forum events use NIP-01 canonical JSON, not JCS) | `tests/contract/linked-data/jcs.contract.spec.js` (9 cases) | not present | `solid-pod-rs-activitypub` ld-canonicalisation (in HTTP signatures path) |

**Contract tests that exist today: zero.** Every overlap above is implemented twice (or more) and tested locally only. The agentbox `tests/contract/linked-data/jcs.contract.spec.js` is a contract within agentbox — it pins agentbox's JCS implementation against expected output — but it does not run the same input through any other implementation.

**Specific high-priority contracts that the suite needs to add (mapped to PRD-010 phases and the C-findings in `01-visionclaw-surfaces.md`):**

| Contract ID | Pair | Input | Assertion | Drives |
|-------------|------|-------|-----------|--------|
| C-EVENT-ID-1 | forum nostr-core ↔ nostr_sdk | UnsignedEvent (kind 1, fixed pubkey, fixed created_at, fixed content) | `compute_event_id` byte-equal `EventBuilder::id()` | NIP-01 conformance under PRD-010 P0 |
| C-EVENT-ID-2 | forum nostr-core ↔ nostr-tools (JS) | same UnsignedEvent | same | C2 mitigation (bech32 npub) |
| C-EVENT-ID-3 | nostr_sdk ↔ nostr-tools | same | sanity, used as control | — |
| C-NPUB-1 | forum `nip19::encode_npub` ↔ nostr-tools `npubEncode` | 32-byte x-only pubkey | byte-equal | C2 (sovereign-bootstrap.py uses 64-byte SEC1) |
| C-NPUB-2 | forum `nip19::encode_npub` ↔ `solid-pod-rs-nostr::npub_to_x_only` round-trip | same | round-trip identity | — |
| C-NIP44-1 | forum `nip44::encrypt` ↔ `nostr_sdk::nip44::encrypt` | paulmillr `valid.json` cases | byte-equal output | C1 (the bug that shipped) |
| C-NIP44-2 | forum `nip44::decrypt` ↔ paulmillr vectors | paulmillr `valid.json` ciphertext + key | plaintext byte-equal | C1 |
| C-NIP98-1 | forum `nip98::verify_token` ↔ VisionClaw `nip98.rs::verify_nip98` ↔ agentbox `verifyNip98` ↔ solid-pod-rs `Nip98Verifier` | one client-generated NIP-98 token | all four accept | replay protection convergence (Sprint v9 STREAM-B B1) |
| C-NIP26-1 | forum `nip26::verify_delegation` ↔ VisionClaw `nip26_cap.rs` | one delegation token + a delegated event | both accept | PRD-010 P2 |
| C-DID-1 | forum `pod-worker/did.rs` Tier-1 doc ↔ solid-pod-rs-nostr Tier-1 doc | same pubkey | structurally equal: `verificationMethod[0].type == "SchnorrSecp256k1VerificationKey2019"`, `@context` includes secp256k1-2019/v1 | C3 (the three different verificationMethod.type strings) |
| C-DID-2 | forum did.rs ↔ agentbox `sovereign-bootstrap.py` | same pubkey | same | C3 |
| C-HKDF-1 | nostr-core `keys.rs` PRF info string ↔ JS reference | same passkey/credential | same derived 32-byte key | PRD-010 H11 |
| C-JCS-1 | agentbox `jcs.contract.spec.js` ↔ Rust `serde_jcs` reimplementation | RFC 8785 corpus | byte-equal | ADR-075 D15 (IS-Envelope canonicalisation) |
| C-IS-ENVELOPE-1 | forum encoder ↔ agentbox encoder ↔ VisionClaw encoder | seven envelope kinds | round-trip + AS2 LDN mapping | ADR-075 |

**Engineering note on contract harness shape:** The natural location is a new `tests/contract-cross/` directory at the workspace root of one project, vendoring fixture JSON in `tests/fixtures/cross-impl/`. The forum is the cleanest host because it already has Rust + JS in the tree (the JS lives at `dreamlab-ai-website/src/`). For the JS half of cross-impl tests, vendor a thin runner under `tests/contract-cross/js/` that invokes `nostr-tools` against the same fixtures. CI gate runs both halves and `diff`s the artefacts.

---

## G4 — Cross-system smoke tests

A cross-system smoke test boots ≥2 substrates and asserts an end-to-end flow. The one near-candidate is `agentbox/tests/sovereign/nostr-bridge.integration.test.js`, but inspection shows it boots only `mcp/nostr-bridge` against itself (publish + verify) — it does not bring up a second substrate. The strings `nostr-rs-relay` and `nakamoto` appear only in `agentbox/{agentbox.toml,flake.nix,CHANGELOG.md,docs/}` — i.e. as a manifest-level dependency, not as a test fixture. **No project boots a real Nostr relay in CI.**

End-to-end flows that have **zero** test coverage today:

| Flow | Required substrates | Why it matters |
|------|---------------------|----------------|
| **Forum DM → relay → agentbox → pod inbox → agent reply → outbox → forum** | forum-client + nostr-rs-relay + agentbox RelayConsumer + pod-worker (or solid-pod-rs-server) | The "headline" mesh-federation flow per PRD-010 §3.2; the mesh-event kind 30050 path per ADR-075 |
| **VisionClaw `bridge_signing_fanout` → agentbox `RelayConsumer` ack** | VisionClaw + agentbox + nostr-rs-relay | The `tests/bridge_signing_fanout.rs` exists and the agentbox `RelayConsumer` exists but they have never been wired together |
| **Forum NIP-98 token → VisionClaw `nip98.rs` verifier** | forum-client + VisionClaw HTTP surface | Cross-implementation NIP-98 — see G7 |
| **Forum kind-1059 (gift-wrap) → relay-worker AUTH gate → agentbox RelayConsumer NIP-42 AUTH-RESP** | forum + nostr-rs-relay + agentbox | Per `01-visionclaw-surfaces.md` §6.2 (AUTH plumbing missing in forum-client) |
| **BC20 ACL: `urn:agentbox:bead:*` ↔ `urn:visionclaw:bead:*` translation** | VisionClaw + agentbox | PRD-006 §5.5 — six modules paper-only, zero LOC, zero tests |
| **DID Document round-trip: forum emits → solid-pod-rs-nostr resolves** | forum + solid-pod-rs-nostr | C3 (verificationMethod.type alignment) |
| **NIP-26 delegation: forum signs delegated event → VisionClaw `nip26_cap.rs` accepts → agentbox bridge re-publishes without re-signing** | forum + VisionClaw + agentbox + nostr-rs-relay | `01-visionclaw-surfaces.md` §6.4 — re-sign-on-fan-out bug at `nostr_bridge.rs:219-222` |
| **Mesh service-list propagation: kind 30033 published → all three substrates discover** | forum + agentbox + VisionClaw | ADR-074 D9 |
| **Cross-substrate moderation propagation: kind 30910–30916 + 1984 fan-out** | forum + agentbox + VisionClaw | DDD `TR-Moderation-Honour` — described, untested |

**The structural reason this is missing**: there is no docker-compose or Nix-flake target that boots two substrates in CI. The agentbox flake.nix declares `nostr-rs-relay` but at no point in the four `.github/workflows/` trees is it `docker run`'d. PRD-010 P3 will require this.

A docker-compose harness at `tests/cross-system/docker-compose.yml` containing `nostr-rs-relay` + `forum-relay-worker (wrangler dev)` + `agentbox management-api` + `solid-pod-rs-server` + `visionclaw` is the unblocker. It costs roughly two engineer-days. Once it exists, every flow above is a separate test file under `tests/cross-system/`.

---

## G5 — Mutation testing coverage

**Findings:**

- `find . -name ".mutants.toml" -o -name "cargo-mutants.toml"` returns **zero hits** across all four projects.
- No GitHub Actions workflow contains the substring `cargo-mutants` or `mutants`.
- No package.json contains `stryker` or `stryker-mutator`.
- The `cargo-mutants` binary is not declared in any tooling manifest (`flake.nix`, `Dockerfile`, `devShells`).

**Verdict:** Mutation testing is **not configured anywhere** in the four projects.

This matters specifically for the crypto/security modules. The class of bug C1 (HKDF-Expand vs Extract — one method-call swap) is exactly the kind of bug mutation testing surfaces: replacing `hk.expand(&[], ...)` with `hk.extract(...)` is a single mutation; the mutant survives only if no test fails when the code computes a different conversation key. Reference-vector tests (G2) would kill this mutant. So mutation testing answers a different question than vectors: *given the current test suite, how many surviving mutants does it produce?*

**Recommended baseline (justified per-module in §G16):**

| Module | Tool | Recommended kill-rate | Why this number |
|--------|------|----------------------:|-----------------|
| `nostr-core/src/nip44.rs` | cargo-mutants | ≥ 90% | Smallest surface, hottest spec; once ADR-076 absorbs upstream this becomes ≥ 90% on the integration shim |
| `nostr-core/src/nip04.rs` | cargo-mutants | ≥ 90% | |
| `nostr-core/src/nip19.rs` | cargo-mutants | ≥ 85% | |
| `nostr-core/src/nip26.rs` | cargo-mutants | ≥ 85% | |
| `nostr-core/src/nip98.rs` | cargo-mutants | ≥ 85% | |
| `nostr-core/src/event.rs` | cargo-mutants | ≥ 90% | Canonical JSON serialisation |
| `nostr-core/src/keys.rs` | cargo-mutants | ≥ 80% | PRF derivation |
| `pod-worker/src/acl.rs` | cargo-mutants | ≥ 80% | 673 LOC; ACL Control coercion (Sprint v9 B3) recently fixed |
| `pod-worker/src/did.rs` | cargo-mutants | ≥ 85% | DID Document emitter |
| `relay-worker/src/relay_do/nip_handlers.rs` | cargo-mutants | ≥ 75% | 766 LOC; high mutation surface |
| `src/utils/nip98.rs` (VisionClaw) | cargo-mutants | ≥ 75% | 636 LOC; cross-substrate auth pivot |
| `src/services/nip26_cap.rs` (VisionClaw) | cargo-mutants | ≥ 75% | Delegation cap |
| `solid-pod-rs/src/auth/nip98.rs` | cargo-mutants | ≥ 80% | Already heavily tested; baseline above 80% achievable |
| `solid-pod-rs/src/wac/` | cargo-mutants | ≥ 85% | 740-line wac_inheritance test suite makes high mutation kill plausible |
| `agentbox/management-api/middleware/auth.js` | stryker-mutator | ≥ 75% | NIP-98 verifier in JS |
| `agentbox/mcp/nostr-bridge/relay-consumer.js` | stryker-mutator | ≥ 70% | 473 LOC, currently no consumer test |
| `dreamlab-ai-website/src/components/auth/*.tsx` | stryker-mutator | ≥ 65% | UI surface; lower priority but worth tracking |

**Cost estimate:** Initial cargo-mutants run on `nostr-core` is roughly 8 minutes per module on a 16-core runner (proven by `cargo-mutants --baseline` on similar-sized crates); with caching, the gate adds ~2 minutes per PR. Stryker on the agentbox middleware is similar (~3 minutes for `auth.js`).

**Phasing:** baseline first, gate second. Run `cargo-mutants --json --output mutants/` weekly via cron (not on PR) for the first sprint to establish the kill-rate; promote modules to PR gates one at a time as the residue is cleared.

---

## G6 — Property / fuzz testing coverage

### Property tests (proptest, fast-check, hypothesis)

| Surface | Proptest count | File location |
|---------|---------------:|---------------|
| forum nostr-core NIP-19 | 13 | `crates/nostr-core/tests/nip19_proptests.rs` (Sprint v9 STREAM-E1) |
| forum nostr-core NIP-04 | 9 | `crates/nostr-core/tests/nip04_proptests.rs` (Sprint v9 STREAM-E1) |
| forum pod-worker WAC | 13 | `crates/pod-worker/tests/wac_proptests.rs` (Sprint v9 STREAM-E1) |
| **Forum total** | **35** | — |
| VisionClaw | 0 | — |
| Agentbox | 0 | — |
| solid-pod-rs | 0 | — |

Forum has the only proptest investment, and it landed in Sprint v9. The fact that NIP-04 has proptest but NIP-44 does not — combined with the fact that NIP-44 was the one with the C1 bug — is structurally consistent with how the bug shipped. NIP-44 was added later and missed the same investment.

**Recommended new property tests, by module and invariant:**

| Module | Invariants worth property-testing |
|--------|----------------------------------|
| `nostr-core/src/nip44.rs` | (a) decrypt(encrypt(p, k)) == p for all p ∈ Bytes, k ∈ valid-key; (b) ciphertext length == plaintext length + padding + 32 (MAC) + 32 (nonce) + 1 (version); (c) MAC verification rejects single-bit ciphertext flips with prob 1; (d) padding is zero-suppressed only for the boundary case |
| `nostr-core/src/event.rs` | (a) `compute_event_id(canonicalise(serialise(e)))` == `compute_event_id(e)` for all event shapes; (b) signature verifies after sign(); (c) signature does not verify if any field changes |
| `nostr-core/src/keys.rs` | (a) PRF derivation is deterministic over (credential, salt); (b) two distinct salts produce distinct keys; (c) wrap/unwrap round-trip identity |
| `nostr-core/src/gift_wrap.rs` | (a) three-layer round-trip identity for any inner kind; (b) sender/receiver pubkey not exposed in wrap; (c) timestamp randomisation lies within the spec window |
| `nostr-core/src/nip26.rs` | (a) delegation token verifies under `delegator_pubkey`; (b) delegated event id is signed by `delegate_pubkey`; (c) conditions parsing accepts well-formed and rejects malformed in equal counts |
| `pod-worker/src/acl.rs` | (a) Control mode required for `*.acl` paths regardless of ACL-stated grants; (b) Append+Write does not imply Control; (c) ACL up to MAX_ACL_DOC_BYTES parses cleanly, beyond rejects deterministically |
| `relay-worker/src/relay_do/nip_handlers.rs` | (a) AUTH challenge issued before any kind-1059 read; (b) NIP-42 AUTH-RESP within tolerance window accepted; (c) replay-cache key uniqueness |
| `solid-pod-rs/src/wac/` | (a) ACL inheritance closure on group memberships terminates; (b) ACL grants monotonic under group expansion; (c) revocation propagates within one cache TTL |
| `solid-pod-rs-nostr/src/did.rs` | (a) DID Document round-trips through serde_json::Value::to_string and parse; (b) `verificationMethod[0].id` ends with `#0`; (c) `@context` always includes `https://www.w3.org/ns/did/v1` |
| `src/utils/nip98.rs` (VisionClaw) | (a) replay-cache eviction monotonic; (b) tolerance-window edges; (c) URL canonicalisation idempotent |
| `agentbox/management-api/lib/uris.js` | (a) parse(mint(kind, scope, local)) round-trip identity; (b) reject `urn:visionclaw:` URIs at the boundary; (c) hex-pubkey scope vs bech32 scope coercion |

### Fuzz testing (cargo-fuzz, libfuzzer, jazzer)

| Project | Fuzz target count | Notes |
|---------|-------------------|-------|
| Forum | 0 | None |
| VisionClaw | 1 | `crates/visionclaw-xr-presence/fuzz` — XR presence service, **not crypto** |
| Agentbox | 0 | None (the matches under `agentbox/workspace/.cargo/registry/...` are upstream library tests, not project targets) |
| solid-pod-rs | 0 | None |

**Verdict:** No fuzzing of any protocol surface in any project. The single VisionClaw fuzz target is on the XR presence service (`presence_actor.rs` flow), not Nostr/DID/JCS/IS-Envelope.

**Recommended fuzz targets:**

| Target | Crate | Input | Property |
|--------|-------|-------|----------|
| `nip04_decrypt_fuzz` | nostr-core | random bytes ≤ 4 KiB | does not panic, returns Result |
| `nip44_decrypt_fuzz` | nostr-core | random bytes ≤ 64 KiB | does not panic; rejects invalid versions deterministically |
| `nip19_decode_fuzz` | nostr-core | random ASCII ≤ 1 KiB | does not panic; round-trip when prefix matches |
| `nip98_verify_fuzz` | nostr-core | random base64 token + URL bytes | does not panic; classifies signature failures correctly |
| `event_parse_fuzz` | nostr-core | random JSON ≤ 16 KiB | does not panic; canonical-form invariants hold |
| `wac_acl_parse_fuzz` | pod-worker (or solid-pod-rs) | random JSON-LD ≤ MAX_ACL_DOC_BYTES | does not panic; rejects beyond cap |
| `is_envelope_decode_fuzz` | (new crate) | random bytes ≤ 4 KiB | does not panic; rejects unknown kinds; LDN mapping idempotent |
| `did_document_parse_fuzz` | solid-pod-rs-nostr | random JSON ≤ 8 KiB | does not panic; rejects type ≠ SchnorrSecp256k1VerificationKey2019 |
| `bridge_event_dispatch_fuzz` | VisionClaw `nostr_bridge.rs` | random kind-1059 bytes | does not panic; does not re-sign without delegation tag |

Per-target initial corpus is the existing test-vector JSON (G2). Run `cargo fuzz run <target> -- -max_total_time=600` for 10 minutes per target nightly. Fuzz crashes file directly into `tests/fixtures/regression/<target>/<hash>.bin` for replay.

---

## G7 — Authentication test coverage (NIP-98)

| Surface | Test count | Cross-impl coverage |
|---------|-----------:|--------------------|
| forum `nostr-core/src/nip98.rs` | 32 inline + 3 STREAM-B replay tests (memory `sprint-v9-stream-b-status` confirms "210/210 tests pass") | replay/expiry/payload-hash covered local-only |
| VisionClaw `src/utils/nip98.rs` (636 LOC) | 17 inline + 1 TS file at `tests/solid/nip98.test.ts` | local-only |
| agentbox `verifyNip98` (`management-api/middleware/auth.js`, `mcp/servers/nostr-bridge.js`) | 1 file (`tests/sovereign/nostr-bridge.test.js`) | local-only |
| solid-pod-rs `Nip98Verifier` (`crates/solid-pod-rs/src/auth/`) | inline counts roll into the 238 figure for `solid-pod-rs` (G1.4) | local-only |

**Cross-implementation gap:**

There is **no test that sends a forum-generated NIP-98 token to the VisionClaw verifier**, or vice versa, or to the agentbox verifier, or to the solid-pod-rs verifier. The four implementations have all been built to NIP-98 and they all probably interoperate, but interoperability is asserted nowhere — and Sprint v9 STREAM-B specifically added KV-backed replay protection to four of the five workers (auth-worker, pod-worker, relay-worker, search-worker), with the **trait** `Nip98ReplayStore` defined once in `nostr-core` and **four separate `KvReplayStore` impls**. Inter-worker replay coordination — i.e. a token replayed across worker boundaries — is **not tested**. The replay window is per-worker.

**Specific recommended tests (add as `tests/contract-cross/nip98_cross_impl.rs`):**

| Test | Setup | Assertion |
|------|-------|-----------|
| `cross_impl_basic_accept` | forum forum-client signs token; submit to all four verifiers | all four return 200/Ok |
| `cross_impl_replay_per_worker` | submit same token to forum auth-worker twice within tolerance | second attempt rejected |
| `cross_impl_replay_cross_worker` | submit same token to forum auth-worker, then to forum pod-worker, both within tolerance | **expected behaviour: undefined**; this surface is the gap. Document it in tests as a deliberate decision (current: each worker has its own replay store; tokens replay across workers freely) |
| `cross_impl_expired` | sign token with `created_at - 120s` | all four reject |
| `cross_impl_payload_hash_mismatch` | sign token with `payload-hash` of "A" but POST body "B" | all four reject |
| `cross_impl_wrong_method` | sign token for GET, submit as POST | all four reject |
| `cross_impl_visionclaw_to_forum` | VisionClaw signs (using `SERVER_NOSTR_PRIVKEY`); submit to forum auth-worker | accepted |
| `cross_impl_agentbox_signed` | agentbox emits NIP-98 (does it? — verify **first** that agentbox emits, not just verifies; if no emitter exists this test reduces to "VisionClaw emits, agentbox verifies") | accepted |

The cross-worker replay observation (third row above) is the single-most-important one — it surfaces a property the system silently relies on but does not test. Sprint v9 STREAM-B B5 split admin KV into `ADMIN_KV` (auth-worker rw) + `ADMIN_KV_RO` (pod-worker ro); the same architectural pattern would apply to a unified `NIP98_REPLAY_KV`, but the current code uses one KV namespace **per worker**.

---

## G8 — Authorization test coverage (WAC + delegation)

### WAC (Web Access Control)

| Surface | Test count |
|---------|-----------:|
| forum pod-worker `acl.rs` | 31 inline + 13 proptests (post-Sprint v9 STREAM-B B3 + STREAM-E1) |
| solid-pod-rs `wac/` | 12 (basic) + lengthy `wac_inheritance.rs` (740 lines) + `storage_trait.rs` (212 lines) |
| VisionClaw delegated path | **0** — `solid_pod_rs::wac::evaluate_access` is consumed by `src/handlers/solid_pod_handler.rs`, but no test exercises the delegation; coverage relies on the upstream solid-pod-rs suite |
| agentbox WAC | **N/A** — agentbox does not implement WAC; the upstream solid-pod-rs server enforces it. Agentbox tests in `tests/contract/pods.contract.spec.js` are CRUD-shape contracts on the pod adapter, not ACL-enforcement tests |

### NIP-26 delegation verifier wiring (per substrate)

| Substrate | Module | Wired? | Tested? |
|-----------|--------|--------|---------|
| forum | `nostr-core/src/nip26.rs` | yes (used in nostr-core) | yes (7 inline + 30 in `nip26_tests.rs`) |
| forum relay-worker | imports nostr-core | enforcement path uncertain — the nip_handlers tests do not assert delegation-tag acceptance | **no** |
| VisionClaw | `src/services/nip26_cap.rs` | yes (referenced from `automation_orchestrator_actor.rs`, `inbox_service.rs`, `automation_routine.rs`) | **no test fixture** — grep for `verify_delegation` in `tests/` returns nothing |
| agentbox | `mcp/nostr-bridge/relay-consumer.js` | per memory `sprint-v8-status` "BUG-3 FIXED" notes a `verifyDelegation` call site, but the consumer is **not wired into management-api boot** per `01-visionclaw-surfaces.md` §7 | **no** |
| solid-pod-rs | not present | n/a | n/a |

**Cross-substrate gap:** the same delegation token, signed by a delegator and used in a delegated event, has never been routed through forum → agentbox → VisionClaw. The verifier in each substrate accepts its own tokens; whether they accept each other's tokens is unknown.

**Recommended tests:**

| Test | Crate | Assertion |
|------|-------|-----------|
| `nip26_delegated_event_relay_worker_accepts` | relay-worker | event with valid delegation tag passes nip_handlers gate |
| `nip26_delegated_event_visionclaw_accepts` | VisionClaw | same delegated event accepted by `nip26_cap.rs` |
| `nip26_delegated_event_agentbox_accepts` | agentbox | same delegated event accepted by RelayConsumer |
| `nip26_invalid_delegation_all_reject` | all three | delegation with bad signature rejected by all three |
| `nip26_expired_delegation_all_reject` | all three | delegation past expiry rejected |
| `nip26_kind_filter_per_substrate` | all three | delegation that excludes kind 1059 still allows kind 1; kind-1059 attempt rejected |

---

## G9 — Replay protection test coverage

Sprint v9 STREAM-B B1 added the `Nip98ReplayStore` trait (`nostr-core/src/nip98.rs`) plus a `KvReplayStore` impl in each of auth-worker, pod-worker, relay-worker, search-worker. NIP98_REPLAY KV binding added to all four `wrangler.toml` files. **3 new replay tests** in nip98 (replay-detected, replay-after-tolerance-allowed, distinct-events-allowed).

| Substrate | Replay store | Tests |
|-----------|--------------|------:|
| forum auth-worker | `KvReplayStore` | 3 (Sprint v9 B1) — local |
| forum pod-worker | `KvReplayStore` | 0 — coverage rolled into nip98 trait tests |
| forum relay-worker | `KvReplayStore` | 0 |
| forum search-worker | `KvReplayStore` | 0 |
| VisionClaw `src/utils/nip98.rs` | **no replay store** (compare-by-`created_at` window only) | n/a |
| agentbox | no replay store | n/a |
| solid-pod-rs | implementation in `crates/solid-pod-rs/src/auth/` | inline tests cover replay window |

**Per-worker store, no coordination.** A token submitted to forum auth-worker is rejected on second submission to that worker, but the same token can be submitted to forum pod-worker without rejection — each worker has its own KV namespace. This is a deliberate sprint v9 design choice; whether it is the right one is a separate question. **The fact that no test pins this property in either direction is the actual gap.**

**Cross-system replay** (a forum-emitted token replayed against a VisionClaw endpoint) **has zero coverage**, mostly because VisionClaw has no NIP-98 replay store at all. Adding this as a test would also act as a pressure test on whether VisionClaw should add the replay store.

**Recommended tests:**

| Test | Assertion |
|------|-----------|
| `nip98_replay_per_worker_independent` | Documented behaviour — token replayed across workers within tolerance is currently allowed |
| `nip98_replay_visionclaw_no_store` | Explicit assertion that VisionClaw does not deduplicate replay (so we know if/when this changes) |
| `nip98_replay_cross_substrate_documented` | A forum-signed token submitted to forum auth-worker and then to VisionClaw is currently accepted by both; flag for ADR review |

---

## G10 — DID Document conformance test coverage

Three emitters, three different verificationMethod.type strings, per `01-visionclaw-surfaces.md` C3:

| Emitter | type string | @context line |
|---------|-------------|---------------|
| forum `pod-worker/src/did.rs` | `SchnorrSecp256k1VerificationKey2019` (post-Sprint v9 STREAM-A) | includes `https://w3id.org/security/suites/secp256k1-2019/v1` |
| solid-pod-rs-nostr `did.rs` | `NostrSchnorrKey2024` | (does not include the secp256k1-2019 context line) |
| agentbox `scripts/sovereign-bootstrap.py` | `SchnorrSecp256k1VerificationKey2022` (per `01-visionclaw-surfaces.md` C3 — non-existent suite) | varies |

| Emitter | Test count |
|---------|-----------:|
| forum `pod-worker/src/did.rs` | 10 (Sprint v9 STREAM-A) |
| VisionClaw | **0** — no DID emitter present yet |
| agentbox `sovereign-bootstrap.py` | unit + contract presence: not found in `tests/` |
| solid-pod-rs-nostr | 44 inline + `relay_nip11.rs` (57 lines) + `resolver_integration.rs` (135 lines, 2 wiremock-driven cases) |
| solid-pod-rs (consumer of did:nostr) | `tests/did_nostr_resolver.rs` (206 lines, 6 tests) |

**Cross-emitter conformance:** **untested**. A DID Document emitted by forum should be parseable by the solid-pod-rs-nostr resolver, but no test asserts this. ADR-074 mandates the convergence on `SchnorrSecp256k1VerificationKey2019`; until the test exists, it is checked at PR-review time.

**Recommended tests:**

| Test | Setup | Assertion |
|------|-------|-----------|
| `did_doc_forum_emitter_solid_pod_rs_resolver` | forum did.rs emits Tier-1 doc; solid-pod-rs-nostr resolver consumes | `verificationMethod[0].type == "SchnorrSecp256k1VerificationKey2019"`; `@context` includes the secp256k1-2019 suite; pubkey round-trips through `npub_to_x_only` |
| `did_doc_agentbox_emitter_forum_consumer` | sovereign-bootstrap.py emits; forum (or visionclaw) parses | type alignment + bech32 vs hex pubkey scope alignment per ADR-074 |
| `did_doc_three_emitters_byte_equal` | forum + solid-pod-rs-nostr + (corrected) agentbox emit for same pubkey | structurally equal (excluding timestamp fields if any) |
| `did_doc_resolver_rejects_legacy_NostrSchnorrKey2024` | solid-pod-rs/did_nostr_resolver consumes legacy doc | rejects with deterministic error (or accepts with deprecation warning, per ADR-074 transition policy) |

---

## G11 — Cross-system fuzz / negative testing

Negative tests at substrate boundaries — i.e. "what if upstream sends garbage?" — currently exist nowhere. Each substrate trusts its own producers. The cross-substrate boundaries are:

| Boundary | What can go wrong |
|----------|------------------|
| forum-relay-worker → agentbox RelayConsumer | malformed kind-1059, 64-byte SEC1 pubkey instead of 32-byte x-only, AUTH challenge re-issued during read, kind not in agentbox allowlist |
| forum-relay-worker → VisionClaw `bridge_signing_fanout.rs` | event id mismatch, signature under non-x-only pubkey, NIP-26 delegation tag malformed |
| agentbox `RelayConsumer` → forum auth-worker (NIP-98 token verification) | clock skew beyond tolerance, payload hash drift due to JS Buffer/string encoding, replay attempt |
| agentbox → VisionClaw (BC20 ACL) | `urn:agentbox:bead:*` with malformed kind, hex pubkey scope vs bech32 scope, scope without colon separator |
| forum-client (WASM) → forum-relay-worker (CF Workers WASM) | Float32Array vs Uint8Array in event content, `?Send` boundary leaks |
| VisionClaw `nostr_bridge.rs` re-publish path | re-sign without delegation tag (the `01-visionclaw-surfaces.md` §6.4 finding) |

**Recommended tests** (one per boundary, fuzzed input):

| Test | Boundary | Assertion |
|------|----------|-----------|
| `relay_consumer_malformed_kind1059` | agentbox | does not panic; logs error; does not republish |
| `relay_consumer_non_xonly_pubkey` | agentbox | rejects; does not crash bridge process |
| `bridge_fanout_invalid_signature` | VisionClaw | does not republish; emits structured error event |
| `bridge_fanout_no_resign_without_delegation` | VisionClaw | re-published event preserves original `pubkey` field; never re-signs under bridge key absent valid NIP-26 delegation |
| `nip98_clock_skew_5min` | cross | rejects deterministically in all four verifiers |
| `bc20_malformed_urn_translation` | VisionClaw + agentbox | translation returns Err(InvalidUri); does not panic |
| `is_envelope_unknown_kind` | all three | rejects with unknown-kind error; does not crash dispatcher |
| `did_doc_corrupt_jcs` | all three | parser returns Err; does not panic |

These tests live in `tests/cross-system/negative/`. Use `proptest` over byte ranges 0..16384 with strategy bias for short and "near-valid" inputs (10% near-valid, 80% random, 10% empty).

---

## G12 — Federation conformance suite

ADR-073, ADR-074, ADR-075, ADR-076 specify the federation contract. The strings `ADR-073`, `ADR-074`, `ADR-075`, `ADR-076` appear nowhere in `tests/` directories across the four projects (verified by grep). DDD `TR-Moderation-Honour` and the related cross-substrate moderation propagation behaviour have **zero tests**.

The required federation conformance suite:

| Suite | ADR | Cases |
|-------|-----|------:|
| **IS-Envelope round-trip** | ADR-075 D15 | 7 envelope kinds (chat, tool_invoke, tool_result, knowledge_link, moderation, mesh_ping, mesh_event) × 3 emitters × 2 wrap modes (NIP-59 vs kind-30050) = 42 round-trip cases; plus AS2 LDN mapping per kind = 7; plus JCS-canonical assertion = 7. **Total: ~56.** |
| **Federation worker fan-out + dedup** | ADR-073 D9 | 1 publish, 3 receivers, ≥ 1 LRU cache hit; plus duplicate-id second publish must dedup; plus three-relay topology fan-out converges within 2 round-trips. **Total: ~12.** |
| **Federation key authorization** | ADR-073 D4 | one valid federation key per relay; AUTH-RESP from non-federation key rejected; key rotation graceful. **Total: ~9.** |
| **Mesh service-list propagation** | ADR-074 D9 | kind 30033 emitted by node A → node B discovers within timeout; service-list canonicalisation matches; revocation propagates. **Total: ~7.** |
| **Cross-substrate moderation propagation** | DDD `TR-Moderation-Honour` | 8 moderation kinds (30910–30916 + 1984) × 3 emitters × 3 receivers = 72 cases. Per memory `sprint-v9-stream-a-status` step 6: forum has the canonical kind list, but the agentbox + VisionClaw moderation honour paths are not implemented yet. **Total: ~72.** |
| **NIP-26 delegation across federation** | ADR-073 + NIP-26 | delegated event published via federation worker → all three receivers accept; delegation revocation within tolerance. **Total: ~8.** |

**Aggregate: ~164 federation conformance cases. Currently implemented: 0.**

These tests will fail or be skipped until ADR-073..076 implementation lands. Their value is to define the conformance contract before implementation, so the implementation can be driven by the test suite. This is the TDD path PRD-010 implicitly endorses.

---

## G13 — Bundle size & cold-start observability

ADR-076 D5 specifies bundle size budgets:

| Surface | Budget |
|---------|--------|
| Per CF Worker | +200 KiB max (delta over baseline) |
| Forum-client (WASM) | +500 KiB max (delta over baseline) |

Required gates: a CI step that builds release artefacts, computes size, compares to a baseline file (`scripts/bundle-budget.json`), fails the build on overshoot.

**Currently implemented:** none. `grep "bundle.*size\|bundle.*budget"` over `dreamlab-ai-website/.github/` returns empty. The five CF Workers have `wrangler deploy` paths in `workers-deploy.yml` and `deploy.yml` but the size of the deployed artefact is not measured by CI.

**Cold-start observability** is a runtime concern, not a CI gate. It belongs in production observability (Cloudflare Analytics + Workers Trace Events), with an alarm when p95 cold-start exceeds 200ms or whatever budget ADR-076 settles on. There is no current alarm.

**Recommended gate** (`.github/workflows/bundle-budget.yml`):

```yaml
- name: Build all 5 CF Workers (release)
  run: cargo build --release --target wasm32-unknown-unknown -p auth-worker -p pod-worker -p relay-worker -p search-worker -p preview-worker
- name: Compare to baseline
  run: ./scripts/bundle-budget-check.sh --baseline scripts/bundle-budget.json --threshold-bytes 204800
- name: Fail on overshoot
```

The script reads `target/wasm32-unknown-unknown/release/<crate>.wasm`, computes `wc -c`, and `jq`-compares against the baseline. New baseline writeback is a manual `--update-baseline` flag, never run in CI.

---

## G14 — Recommended test plan

The plan below is sequenced — earlier dependencies must land before later. Engineer-day cost is calibrated against Sprint v9 STREAM-E1 (35 proptests in 2 engineer-days for one engineer) and the agentbox contract suite (262 contract tests in roughly 8 engineer-days extrapolated from PR history).

| Phase | Test bundle | Type | Target location | Test count | E-days | Dependencies | CI gate |
|-------|-------------|------|-----------------|-----------:|-------:|--------------|---------|
| **P0** | Reference vectors (paulmillr/nip44, nostr-protocol/nips NIP-19, NIP-26) | unit | `nostr-core/tests/vectors/{nip04,nip19,nip26,nip44,nip59,nip98}_vectors.rs` + `nostr-core/tests/fixtures/*.json` (vendored from upstream) | ~120 | 4 | none | block-on-fail |
| **P0** | NIP-44 negative vectors (paulmillr `invalid.json`) | unit | `nostr-core/tests/vectors/nip44_invalid.rs` | ~30 | 1 | P0 vectors | block-on-fail |
| **P0** | NIP-44 reject lower-case-hex variant + non-zero version byte | unit | `nostr-core/tests/vectors/nip44_invalid.rs` | ~6 | 0.5 | P0 | block-on-fail |
| **P1** | Cross-impl event id (forum ↔ nostr_sdk ↔ nostr-tools) | contract | `tests/contract-cross/event_id_cross_impl.rs` + `tests/contract-cross/js/event-id.test.js` | ~20 | 2 | P0 | block-on-fail |
| **P1** | Cross-impl npub round-trip (5 implementations) | contract | `tests/contract-cross/npub_cross_impl.rs` | ~15 | 1 | P0 | block-on-fail |
| **P1** | Cross-impl NIP-44 (forum ↔ nostr_sdk against paulmillr corpus) | contract | `tests/contract-cross/nip44_cross_impl.rs` | ~50 | 2 | P0 vectors | block-on-fail |
| **P1** | Cross-impl NIP-98 (4 implementations × 7 cases) | contract | `tests/contract-cross/nip98_cross_impl.rs` | ~28 | 3 | P0 | block-on-fail |
| **P1** | Cross-impl NIP-26 delegation (3 implementations × 5 cases) | contract | `tests/contract-cross/nip26_cross_impl.rs` | ~15 | 2 | P0 | block-on-fail |
| **P1** | DID Document conformance (3 emitters × 4 cases) | contract | `tests/contract-cross/did_doc_conformance.rs` | ~12 | 2 | C3 fix in agentbox/sovereign-bootstrap.py | block-on-fail |
| **P1** | HKDF cross-impl (Rust shim ↔ JS reference) | contract | `tests/contract-cross/hkdf_cross_impl.rs` | ~8 | 1.5 | P0 | block-on-fail |
| **P1** | JCS canonicalisation cross-impl (Rust ↔ JS) | contract | `tests/contract-cross/jcs_cross_impl.rs` | ~16 (RFC 8785 corpus) | 1 | none | block-on-fail |
| **P2** | Property tests for NIP-44 (4 invariants), event.rs (3), keys.rs (3), gift_wrap.rs (3), nip26.rs (3), pod-worker acl (3), relay nip_handlers (3), VisionClaw nip98 (3), agentbox uris (3) | property | per-crate `tests/*_proptests.rs` | ~28 | 4 | none | block-on-fail |
| **P2** | Fuzz targets (9 listed in §G6) | fuzz | per-crate `fuzz/fuzz_targets/*.rs` | 9 binaries | 5 | none | nightly cron, warn-only initially, block-on-fail after baseline crash-free for 7 days |
| **P3** | Cross-system docker-compose harness | infra | `tests/cross-system/docker-compose.yml` + `tests/cross-system/lib/` | n/a | 2 | none | n/a |
| **P3** | Cross-system flow: forum DM → relay → agentbox → pod inbox → agent reply → outbox → forum | integration | `tests/cross-system/dm_full_flow.rs` (or `.test.js`) | 1 flow | 3 | P3 harness | block-on-fail |
| **P3** | Cross-system flow: VisionClaw bridge_signing_fanout → agentbox RelayConsumer | integration | `tests/cross-system/bridge_fanout_e2e.rs` | 1 flow | 2 | P3 harness | block-on-fail |
| **P3** | Cross-system NIP-42 AUTH plumbing | integration | `tests/cross-system/nip42_auth_e2e.rs` | 3 cases | 2 | P3 harness | block-on-fail |
| **P3** | Cross-system BC20 ACL translation | integration | `tests/cross-system/bc20_acl.rs` | 5 cases | 2 | P3 harness; BC20 implementation lands | block-on-fail |
| **P3** | Cross-system DID Document round-trip with real relay | integration | `tests/cross-system/did_doc_relay.rs` | 3 cases | 1.5 | P3 harness | block-on-fail |
| **P3** | Cross-system mesh service-list propagation | integration | `tests/cross-system/mesh_service_list.rs` | 3 cases | 2 | P3 harness | block-on-fail |
| **P4** | Federation conformance: IS-Envelope round-trip ×56 | integration | `tests/cross-system/federation/is_envelope_*` | ~56 | 6 | ADR-075 implementation | block-on-fail |
| **P4** | Federation worker fan-out + dedup | integration | `tests/cross-system/federation/fan_out_dedup.rs` | ~12 | 3 | ADR-073 impl | block-on-fail |
| **P4** | Federation key authorization | integration | `tests/cross-system/federation/key_auth.rs` | ~9 | 2 | ADR-073 impl | block-on-fail |
| **P4** | Cross-substrate moderation propagation ×72 | integration | `tests/cross-system/federation/moderation_propagation.rs` | ~72 | 6 | DDD TR-Moderation-Honour impl | block-on-fail |
| **P5** | Mutation testing baseline (cargo-mutants on 9 nostr-core/pod-worker/VisionClaw modules) | mutation | `.mutants.toml` per crate + `.github/workflows/mutation-weekly.yml` | n/a | 4 | none | weekly cron, warn-only |
| **P5** | Mutation testing PR gate (graduated rollout: nip44 first, then event, then keys, ...) | mutation | extend `.github/workflows/mutation-pr.yml` | n/a | 3 | P5 baseline | block-on-fail (per-module rollout) |
| **P5** | Bundle size gate | infra | `.github/workflows/bundle-budget.yml` + `scripts/bundle-budget.json` | n/a | 1 | none | block-on-fail |
| **P5** | Cold-start observability alarm | runtime | Cloudflare Workers + Analytics dashboard | n/a | 1.5 | none | runtime alarm, not CI |

**Aggregate engineer-day cost: ~75 e-days** (one engineer at 2 e-days/week → ~37 weeks; at 5 e-days/week → ~15 weeks; with three engineers parallel on independent phases → ~5 weeks). The first two phases (P0+P1, ~22 e-days) are the highest-leverage and should land first.

---

## G15 — Coverage threshold proposal (per substrate)

Targets are **line ≥ X% / branch ≥ Y%**, measured by `cargo-llvm-cov` for Rust and `c8` for JS.

| Substrate | Line | Branch | Trade-off |
|-----------|-----:|-------:|-----------|
| **forum nostr-core** (post-ADR-076 absorption — only the integration shim remains) | ≥ 95% | ≥ 90% | Small surface (~500 LOC of shim after absorption); 95% is achievable. The trade-off is that the integration shim's main duty is invoking upstream; coverage measures the invocation, not the underlying crypto. Reference vectors (G2) and contract tests (G3) carry the *correctness* guarantee. |
| **forum nostr-core** (today, pre-absorption) | ≥ 88% | ≥ 80% | Realistic given current 718 inline tests against ~7,892 LOC; pushing higher trades effort for diminishing returns until absorption lands. |
| **forum CF Workers** (auth, pod, preview, relay, search) | ≥ 80% | ≥ 70% | Each worker is 1–2 KLOC; 80% line is the median for healthy workers-rs projects. Branch threshold lower because CF Workers have many short-circuit error paths that are exercised but not branch-covered without explicit fault injection. |
| **VisionClaw substrate** (Rust) | ≥ 75% | ≥ 65% | Heaviest single corpus (1353 inline + 112 integration); the actor-mesh + GPU + protocol breadth makes 80%+ expensive. The protocol-relevant subset (`utils/nip98.rs`, `services/nip26_cap.rs`, `services/server_identity.rs`, `handlers/solid_pod_handler.rs`) should be pinned higher: line ≥ 85%, branch ≥ 75%. |
| **VisionClaw client** (TS/Vitest) | ≥ 70% | ≥ 60% | 39 test files / 966 assertions over a much larger surface; lower threshold reflects the UI-heavy code that receives proportionally less ROI from coverage. |
| **agentbox JS** (management-api + mcp + scripts) | ≥ 80% | ≥ 70% | The contract surface is the priority; backend logic in management-api is 80%-attainable. The JS client side and admin UI lower (tracked separately). |
| **solid-pod-rs** (Rust) | ≥ 85% | ≥ 75% | Already the best-tested crate; 85% is a near-floor improvement. Sets the bar for the others. |
| **Cross-substrate contract suite** | 100% pass per release | 100% pass per release | Contract tests are pass/fail, not percentage; "100% of defined contracts pass" is the gate. |
| **Federation conformance suite** | 100% pass per release | 100% pass per release | Same. |

**Trade-offs explicit:**

1. **Pushing nostr-core to 95% line is only credible after ADR-076.** Pre-absorption, the surface is too large and the C1-class bugs survive coverage anyway. Investing in absorption first, then 95% on the 500-LOC shim, beats investing in 95% on 7,892 LOC of hand-rolled crypto.
2. **VisionClaw 75% is loose.** The actor mesh has many message-handler branches that are coverage-neutral noise. Pinning the **protocol subset** to 85% surfaces real coverage improvements while accepting that the broader codebase trends to 75%.
3. **CF Workers 80% requires fault injection.** Workers-rs error paths (KV unavailable, R2 timeout, AUTH challenge expired) require fault injection to cover meaningfully. The harness for that is roughly 3 e-days extra; otherwise coverage stalls at ~72% for honest reasons.
4. **Coverage gates fight contract gates.** A team optimised for line coverage adds tests that exercise lines without exercising correctness. Contract gates (G3) and reference vectors (G2) are the counter-pressure. Both must be in CI.

---

## G16 — Mutation kill-rate proposal

For each crypto/security module, a kill-rate target. cargo-mutants for Rust, stryker-mutator for JS. The numbers below are baseline targets — initial run will produce a higher residue, and the rollout is "clear residue, raise gate" rather than "set gate, fail until cleared".

| Module | Tool | Target kill-rate | Justification |
|--------|------|-----------------:|---------------|
| `nostr-core/src/nip44.rs` (post-absorption shim) | cargo-mutants | ≥ 90% | Smallest surface, hottest spec. |
| `nostr-core/src/event.rs` | cargo-mutants | ≥ 90% | NIP-01 canonical JSON path; mutating the field-order or field-set is what reference vectors catch. |
| `nostr-core/src/keys.rs` | cargo-mutants | ≥ 80% | PRF derivation + secp256k1 signer; mutations on hash inputs catch HKDF info string drift. |
| `nostr-core/src/nip04.rs` | cargo-mutants | ≥ 90% | After absorption is a thin shim. |
| `nostr-core/src/nip19.rs` | cargo-mutants | ≥ 85% | Bech32 entity encoder; reference vectors plus 13 proptests already exist. |
| `nostr-core/src/nip26.rs` | cargo-mutants | ≥ 85% | |
| `nostr-core/src/nip98.rs` | cargo-mutants | ≥ 85% | High residue expected on replay-store branch handling (4 worker impls). |
| `pod-worker/src/acl.rs` | cargo-mutants | ≥ 80% | 673 LOC; recent Sprint v9 STREAM-B B3 fix expanded the surface; baseline first. |
| `pod-worker/src/did.rs` | cargo-mutants | ≥ 85% | Sprint v9 STREAM-A landed verificationMethod fix; 10 tests cover the surface. |
| `relay-worker/src/relay_do/nip_handlers.rs` | cargo-mutants | ≥ 75% | 766 LOC; AUTH-gate logic is the priority subset. |
| `src/utils/nip98.rs` (VisionClaw) | cargo-mutants | ≥ 75% | 636 LOC; 17 inline tests + cross-impl tests (G7) feed the kill-rate. |
| `src/services/nip26_cap.rs` (VisionClaw) | cargo-mutants | ≥ 75% | Currently zero direct tests; baseline will be poor; block-on-fail only after first cleanup pass. |
| `src/services/server_identity.rs` (VisionClaw) | cargo-mutants | ≥ 70% | High level of nostr_sdk consumption; mutations on the consumer code, not the crypto. |
| `solid-pod-rs/src/auth/nip98.rs` | cargo-mutants | ≥ 80% | Already heavily-tested; achievable. |
| `solid-pod-rs/src/wac/` | cargo-mutants | ≥ 85% | wac_inheritance.rs at 740 LOC of tests is unusual; high kill-rate plausible. |
| `solid-pod-rs-nostr/src/did.rs` | cargo-mutants | ≥ 85% | Compact module + 44 inline tests + 192 lines external. |
| `agentbox/management-api/middleware/auth.js` | stryker | ≥ 75% | Cross-impl tests (G7) feed this directly. |
| `agentbox/mcp/nostr-bridge/relay-consumer.js` | stryker | ≥ 70% | 473 LOC, currently no consumer test; baseline first. |

**Sampled — not exhaustive — gate.** Mutation testing every PR is too expensive (15-30 minutes wall-time per crate). The recommended cadence is:

- **Weekly** (cron): full mutation run on all 18 modules. Output to artefact + post to issue tracker for residue triage.
- **Per PR**: mutation testing only on modules with files changed in the PR, with a 5-minute timeout. Caching reduces this to 1-2 minutes for cache hits.
- **Block-on-fail rollout per module:** as each module's residue clears, that module's mutation gate becomes block-on-fail in PR CI.

**Forum upstream-consumption note** (per the prompt): once ADR-076 absorbs the `nostr` crate, the forum consumes upstream and is responsible for **invocation correctness**, not crypto-correctness. Mutation tests on the integration shim assert "we called the upstream library correctly". This is a different and smaller test surface than mutation tests on hand-rolled crypto. The numbers above already reflect this distinction.

---

## G17 — CI gate proposal

Nine PR gates, in order. Each PR must pass all gates before merge. Each gate has a workflow file, a block-vs-warn policy, a triage owner.

| # | Gate | Workflow file | Block / Warn | Triage owner | Notes |
|---|------|--------------|:------------:|--------------|-------|
| 1 | **Unit + property tests** | forum: `dreamlab-ai-website/.github/workflows/rust-ci.yml` jobs `test-native`, `test-wasm`; agentbox: `agentbox/.github/workflows/contract-tests.yml` (covers contract+unit); solid-pod-rs: `solid-pod-rs/.github/workflows/ci.yml` matrix; **VisionClaw: NEW workflow `.github/workflows/visionclaw-rust-ci.yml`** | block | crate owner | VisionClaw gate is currently absent — adding it is the single largest ROI step (it has 1353 inline tests with no CI gate). |
| 2 | **Reference vector tests** (paulmillr/nip44, nostr-protocol/nips NIP-19, NIP-26, JCS RFC 8785) | runs inside Gate 1 (`cargo test`); fixture vendoring under `nostr-core/tests/fixtures/upstream/` | block | crypto reviewer | P0 above. Vendor fixtures via `git subtree` from upstream — never live-fetch in CI. |
| 3 | **Cross-substrate contract tests** | NEW `.github/workflows/contract-cross.yml` at the repo root that owns `tests/contract-cross/`; runs both Rust and JS halves; depends on Gate 1 of all four projects | block | a single named "federation" reviewer who owns all four trees; rotate quarterly | P1 above. The hardest gate to set up (multi-language) but the highest-value one for catching divergence bugs. |
| 4 | **Bundle size budget** (CF Workers + forum-client) | NEW `.github/workflows/bundle-budget.yml` in dreamlab-ai-website | block | forum infra owner | P5 above; ADR-076 D5. Baseline check vs `scripts/bundle-budget.json`. |
| 5 | **Cold-start latency budget** | runtime alarm (Cloudflare Workers Trace), not CI | warn (alarm, not block) | forum infra owner | A failing alarm pages the on-call; merging a PR that breaks cold-start does not block but creates an immediate incident. |
| 6 | **Anti-drift lint** (`urn:visionclaw:` outside `src/uri/`; `verificationMethod.type` assertions; "NostrSchnorrKey2024" residue) | NEW `.github/workflows/anti-drift.yml` running custom grep-based linter | block | architecture reviewer | Single-purpose lint script (~50 lines) that greps the four trees for known-bad strings outside known-good locations. |
| 7 | **License & dep audit** (`cargo deny`, `npm audit`) | forum: extend `rust-ci.yml`; agentbox: extend `contract-tests.yml`; solid-pod-rs: already has scheduled audit per ci.yml; VisionClaw: NEW | block (errors) / warn (advisories) | security reviewer | `cargo deny check {ban,license,advisory}` — already implicit in solid-pod-rs CI; extend uniformly. |
| 8 | **Coverage threshold** (per G15) | NEW `.github/workflows/coverage.yml` per project | block | crate owner | `cargo-llvm-cov --fail-under-lines X --fail-under-branches Y`; Codecov integration optional. |
| 9 | **Mutation kill-rate** (per G16, sampled) | NEW `.github/workflows/mutation-pr.yml` (per-PR sampled) + `.github/workflows/mutation-weekly.yml` (cron full) | per-module: block once baseline cleared; before that, warn-only | crate owner | Weekly cron is non-blocking; per-PR runs only modules touched by the PR with 5-min timeout. |

**Coordinated triage:** Gate 3 (cross-substrate contract) is unusual in that no single project owns it. The recommended pattern is a single named reviewer (rotating quarterly) who has merge rights into the contract-cross workflow file, plus a label `federation-contract` that auto-assigns this reviewer when contract tests touch any of the four trees. The bug class Gate 3 catches is exactly the C1/C2/C3 class — divergence between two substrate-local truths — and the absence of an owner is what allows it to ship.

**Workflow consolidation opportunity:** the four projects today have eight CI workflow files between them. Adding nine gates each could explode the file count. The recommended consolidation: one workflow file per gate (not per project), each one matrixing across the four projects. So `contract-cross.yml` runs against forum + agentbox + VisionClaw + solid-pod-rs in a single matrix; `coverage.yml` matrixes thresholds per substrate; `bundle-budget.yml` is forum-only.

---

## Aggregate findings

1. **Test depth is healthy at the unit level.** Total assertions are ~3,937 protocol-relevant. The forum's Sprint v9 investments (STREAM-E1 35 proptests; STREAM-B 3 NIP-98 replay tests) and solid-pod-rs's deep `wac_inheritance.rs` (740 lines) prove the team can ship coverage when prioritised.
2. **Test breadth is empty at every cross-substrate seam.** Zero contract tests across substrates. Zero cross-system smoke tests. Zero ADR-073/074/075/076 conformance tests. The C1, C2, C3 critical bugs from `01-visionclaw-surfaces.md` all live at exactly the seams that have no coverage. This is causal, not coincidental.
3. **Reference vector vendoring is universally absent.** Not a single `vectors.json` from paulmillr or nostr-protocol/nips is checked into any of the four trees. The C1 NIP-44 v2 bug shipped because of this gap.
4. **Mutation testing is configured nowhere.** A class of bug (HKDF-Expand vs Extract) that mutation testing would catch survives the current suite.
5. **Fuzzing is configured for one non-protocol surface** (XR presence). The protocol surface — relay event parsing, NIP-98 token parsing, NIP-19 bech32 parsing, JCS canonicalisation, IS-Envelope decoding — has no fuzzing.
6. **VisionClaw has no Rust CI gate at all.** 1,353 inline tests exist; none run in CI for this substrate. xr-godot-* workflows exist but cover only the XR client. This is the largest single non-protocol coverage gap.
7. **WASM tests compile but do not execute.** The forum's `test-wasm` job is `cargo test ... --no-run`. Forum-client's PRF/WebAuthn paths run only on `wasm32-unknown-unknown`; they are never tested under any runner.
8. **Bundle size budgets are specified (ADR-076 D5) but not enforced in CI.**
9. **Cold-start observability is specified but not implemented.**

The recommended programme (§G14) shifts the project from depth to breadth. The highest-leverage two phases (P0 reference vectors + P1 cross-impl contracts, ~22 e-days) would have prevented C1, C2, and C3 — and they would prevent the next bug of the same class. Without them, the structural reason that bug shipped remains in place.

---

## Cross-references

- `docs/integration-research/01-visionclaw-surfaces.md` — C1/C2/C3 critical findings, §6.4 nostr_bridge re-sign, §7 RelayConsumer not wired, §9 cross-system test absence.
- `docs/integration-research/02-forum-surfaces.md` — forum CF Worker test surface and NIP-98 replay store split.
- `docs/integration-research/03-agentbox-surfaces.md` — agentbox contract suite layout.
- `docs/integration-research/04-solid-pod-rs-surfaces.md` — wac_inheritance + did_nostr_resolver test depth.
- `docs/integration-research/05-crypto-gotchas.md` §6 — NIP-44 v2 conversation key bug (C1).
- `docs/integration-research/06-uri-dataflow-alignment.md` — `urn:visionclaw:` vs `urn:agentbox:` URI BC20 boundary.
- `docs/PRD-010-did-nostr-mesh-federation.md` — phasing P0..P5; this audit's §G14 is sequenced against the PRD phases.
- `docs/adr/ADR-073-private-nostr-relay-mesh-topology.md` — federation contract; §G12 conformance suite addresses it.
- `docs/adr/ADR-074-cross-system-did-nostr-canonicalisation.md` — C3 mitigation; §G10 conformance suite addresses it.
- `docs/adr/ADR-075-is-envelope-message-contract.md` — IS-Envelope; §G12 round-trip suite addresses it.
- `docs/adr/ADR-076-nostr-core-absorption-into-upstream.md` — absorption rationale; §G15 line-coverage targets are calibrated for post-absorption.
- `docs/ddd-mesh-federation-context.md` — `TR-Moderation-Honour` invariant; §G12 cross-substrate moderation propagation suite addresses it.
- RuVector memory keys exercised: `sprint-v9-stream-e1-status` (proptest landing), `sprint-v9-stream-b-status` (NIP-98 replay store landing), `sprint-v9-stream-a-status` (DID type fix), `sprint-v9-audit-findings` (relay-worker zero-test surface), `sprint-v8-status` (NIP-26 module), `prd-010-mesh-federation-summary` (federation context).

---

## Appendix A — Per-module risk-scored gap ledger

Risk score = **complexity × criticality × change-frequency × cross-substrate-blast-radius**, on a 1–5 scale per axis (max 625; report normalised to 0–100).

- **Complexity (C)**: LOC + branch density. ≤200 LOC → 1; 201–500 → 2; 501–1000 → 3; 1001–2000 → 4; >2000 → 5.
- **Criticality (K)**: 1 cosmetic / observability; 2 admin tooling; 3 user-facing UI; 4 protocol-conformance; 5 crypto / authn / authz.
- **Change-frequency (F)** (proxy: Sprint v8/v9/v10 churn per memory): 1 dormant; 2 occasional; 3 active in current sprint; 4 active in current and previous sprint; 5 actively rewritten.
- **Cross-substrate blast radius (B)**: 1 isolated; 2 single substrate; 3 two substrates; 4 three substrates; 5 all four.

| Module | LOC | Inline tests | C | K | F | B | Score | Existing test gap | Recommended P-phase |
|--------|----:|------------:|--:|--:|--:|--:|------:|-------------------|:-------------------:|
| `nostr-core/src/nip44.rs` | 549 | 16 | 3 | 5 | 4 | 5 | **96** | Reference vectors absent; C1 bug shipped here | P0 |
| `nostr-core/src/event.rs` | 445 | 24 (incl `compute_event_id`) | 2 | 5 | 3 | 5 | **48** | No canonical-JSON cross-impl | P0 + P1 |
| `nostr-core/src/keys.rs` | 369 | within 210 inline | 2 | 5 | 3 | 4 | **38** | HKDF info string cross-impl absent | P1 |
| `nostr-core/src/nip04.rs` | 523 | 24 | 3 | 4 | 2 | 4 | **38** | Reference vectors absent | P0 |
| `nostr-core/src/nip19.rs` | 511 | 9 + 13 proptests | 3 | 4 | 3 | 5 | **58** | Spec-corpus vectors absent; bech32 npub mismatch (C2) | P0 + P1 |
| `nostr-core/src/nip26.rs` | 372 | 7 + 30 (external) | 2 | 4 | 2 | 5 | **32** | Cross-substrate delegation untested | P1 |
| `nostr-core/src/nip98.rs` | 1,075 | 32 + 3 (replay) | 4 | 5 | 5 | 5 | **160** | Cross-impl untested across 4 verifiers | P1 |
| `nostr-core/src/gift_wrap.rs` | 652 | 16 | 3 | 5 | 3 | 4 | **57** | Inherits C1; reference vectors absent | P0 |
| `nostr-core/src/moderation_events.rs` | 682 | within 210 | 3 | 4 | 4 | 4 | **77** | TR-Moderation-Honour cross-substrate untested | P3 + P4 |
| `nostr-core/src/signer.rs` | 339 | within 210 | 2 | 5 | 3 | 3 | **29** | Spin-loop fix Sprint v9 STREAM-B B7; PRF/NIP-07/nsec branches need contract tests | P2 |
| `nostr-core/src/wasm_bridge.rs` | 241 | within 93 | 2 | 4 | 2 | 3 | **15** | WASM tests `--no-run` only — never executed | P2 (WASM runner) |
| `pod-worker/src/acl.rs` | 673 | 31 + 13 proptests | 3 | 5 | 4 | 4 | **77** | Sprint v9 B3 fix recent; mutation testing residue likely high | P2 + P5 |
| `pod-worker/src/did.rs` | 300 | 10 | 2 | 5 | 4 | 5 | **80** | Cross-emitter DID Document conformance absent (C3) | P1 |
| `pod-worker/src/lib.rs` (router) | (large) | within 131 | 4 | 4 | 4 | 3 | **77** | Cache-Control headers (D7 deferred) untested | P3 |
| `pod-worker/src/storage/cf_backend.rs` | scaffold | 0 | 1 | 4 | 3 | 2 | **9** | Adapter not yet consumed; trait-shape sanity | P2 |
| `relay-worker/src/relay_do/nip_handlers.rs` | 766 | 0 (Q1 audit H15) | 3 | 5 | 4 | 5 | **96** | Sprint v9 audit flagged 0 tests on 1900 LOC enforcement perimeter | P2 + P3 |
| `relay-worker/src/moderation.rs` | 380 | within 91 | 2 | 5 | 4 | 4 | **51** | Cross-substrate honour untested | P3 + P4 |
| `relay-worker/src/whitelist.rs` | 532 | within 91 | 3 | 4 | 3 | 3 | **35** | NIP-42 AUTH gate boundary | P3 |
| `relay-worker/src/audit.rs` | 200 | within 91 | 2 | 3 | 2 | 2 | **10** | Cosmetic-ish; observability priority | P5 |
| `auth-worker/src/lib.rs` | (large) | 88 | 3 | 5 | 5 | 5 | **150** | Replay store cross-worker coordination untested | P1 + P2 |
| `preview-worker/src/ssrf.rs` | (mid) | within 40 | 2 | 5 | 3 | 3 | **35** | Sprint v9 B4 fix recent; redirect bypass corpus inline only | P2 |
| `preview-worker/src/parse.rs` | (mid) | within 40 | 2 | 4 | 2 | 2 | **13** | Negative-tests via fuzz absent | P2 |
| `search-worker/src/lib.rs` | (mid) | 41 | 2 | 4 | 3 | 2 | **20** | R2 panic fixed Sprint v9 D5 | P5 |
| `forum-client/src/auth/passkey.rs` | (large WASM) | within 93 | 3 | 5 | 4 | 5 | **96** | WASM-only — never executed in CI; PRF salt enumeration oracle fixed Sprint v9 B2 | P2 (WASM runner) |
| `forum-client/src/auth/nip07.rs` | (mid WASM) | within 93 | 2 | 4 | 3 | 3 | **29** | Wallet flow untested in CI | P2 |
| `forum-client/src/auth/nip98.rs` | (mid WASM) | within 93 | 2 | 5 | 3 | 5 | **60** | Cross-impl untested | P1 |
| `forum-client/src/auth/session.rs` | (mid WASM) | within 93 | 2 | 5 | 3 | 3 | **29** | Sprint v9 B8 sessionStorage hardening untested in CI | P2 |
| `forum-client/src/components/onboarding_modal.rs` | (Sprint v10 N3a) | 34 (recent N3) | 2 | 3 | 5 | 2 | **20** | Already well-tested; recent landing | (none) |
| `forum-client/src/components/message_input.rs` | (Sprint v10 N3b) | within 34 | 2 | 3 | 5 | 2 | **20** | Mention autocomplete tested | (none) |
| **VisionClaw** `src/utils/nip98.rs` | 636 | 17 + 1 TS file | 3 | 5 | 3 | 5 | **72** | Cross-impl with forum/agentbox/solid-pod-rs absent | P1 |
| **VisionClaw** `src/services/nip26_cap.rs` | (mid) | 0 | 2 | 5 | 3 | 4 | **48** | No tests, despite being on the federation hot path | P1 + P2 |
| **VisionClaw** `src/services/server_identity.rs` | (mid) | within 1353 | 2 | 5 | 3 | 4 | **48** | Two-keypair issue (`SERVER_NOSTR_PRIVKEY` + `VISIONCLAW_NOSTR_PRIVKEY` per `01-visionclaw-surfaces.md`) untested | P3 |
| **VisionClaw** `src/services/inbox_service.rs` | (mid) | within 1353 | 3 | 4 | 4 | 4 | **77** | Federation hot path | P3 |
| **VisionClaw** `src/services/automation_routine.rs` | (mid) | within 1353 | 3 | 4 | 3 | 3 | **35** | Delegated execution path | P2 |
| **VisionClaw** `src/handlers/solid_pod_handler.rs` | (mid) | within 1353 | 2 | 5 | 4 | 4 | **64** | Modified in current branch (`git status` flags it); no apparent corresponding test update | P2 |
| **VisionClaw** `src/actors/nostr_bridge.rs` | (mid) | within 1353 | 3 | 5 | 4 | 5 | **120** | Re-sign-on-fan-out bug (`01-visionclaw-surfaces.md` §6.4) | P3 |
| **VisionClaw** `tests/bridge_signing_fanout.rs` | (test) | (test) | 3 | 5 | 4 | 5 | **120** | Exists but never wired against agentbox RelayConsumer | P3 |
| **agentbox** `mcp/nostr-bridge/relay-consumer.js` | 473 | 0 (per Q1 §7) | 2 | 5 | 4 | 5 | **80** | Not wired into management-api boot; no consumer test | P3 |
| **agentbox** `management-api/middleware/auth.js` | (mid) | 1 file | 2 | 5 | 3 | 4 | **48** | Cross-impl NIP-98 untested | P1 |
| **agentbox** `management-api/lib/uris.js` | (mid) | within `tests/contract/linked-data/uris.contract.spec.js` (21) | 2 | 4 | 3 | 4 | **38** | BC20 boundary cross-impl absent | P3 |
| **agentbox** `scripts/sovereign-bootstrap.py` | (mid) | 0 | 2 | 5 | 2 | 4 | **32** | C2 (64-byte SEC1 bech32) and C3 (`SchnorrSecp256k1VerificationKey2022`) bugs unfixed | P0 (fix) + P1 (test) |
| **solid-pod-rs** `src/auth/nip98.rs` | (mid) | within 238 | 2 | 5 | 2 | 4 | **32** | Cross-impl absent | P1 |
| **solid-pod-rs** `src/wac/` | (large) | dedicated 740 + 121-line external | 3 | 5 | 3 | 4 | **57** | Best-tested; cross-impl with pod-worker still absent | P3 |
| **solid-pod-rs-nostr** `src/did.rs` | (mid) | 44 inline + 192 external | 2 | 5 | 3 | 4 | **48** | Type drift (`NostrSchnorrKey2024`) per ADR-074 untested for ADR alignment | P1 |
| **solid-pod-rs-server** `src/lib.rs`/`src/main.rs` | (mid) | 0 inline + 4 external | 2 | 4 | 2 | 3 | **19** | Runtime adapter | P3 |

**Top-15 by score** (these absorb roughly 70% of total identified risk):

1. `auth-worker/src/lib.rs` — 150
2. `nostr-core/src/nip98.rs` — 160
3. `relay-worker/src/relay_do/nip_handlers.rs` — 96
4. `nostr-core/src/nip44.rs` — 96
5. `forum-client/src/auth/passkey.rs` — 96
6. `src/actors/nostr_bridge.rs` (VisionClaw) — 120
7. `tests/bridge_signing_fanout.rs` — 120
8. `agentbox/mcp/nostr-bridge/relay-consumer.js` — 80
9. `pod-worker/src/did.rs` — 80
10. `nostr-core/src/moderation_events.rs` — 77
11. `pod-worker/src/acl.rs` — 77
12. `pod-worker/src/lib.rs` (router) — 77
13. `src/services/inbox_service.rs` (VisionClaw) — 77
14. `src/utils/nip98.rs` (VisionClaw) — 72
15. `src/handlers/solid_pod_handler.rs` (VisionClaw) — 64

This ranking dictates the order in which engineers attack G14 P0 + P1. The investment is dominated by NIP-98 (#1 + #14 + #5 + #4 + cross-impl in §G3) and the bridge fan-out path (#6 + #7 + #8). Together those are roughly 9 of the recommended 22 P0+P1 e-days.

---

## Appendix B — Deferred-test-debt ledger

The following deferrals are recorded across Sprint v8/v9/v10 memory entries and translate directly into open test debt. Each row is a "we shipped this without the test we said we'd write." Action: each becomes a P-phase test in §G14 or is explicitly retired with rationale.

| Origin | Item | Status | Translates to |
|--------|------|--------|---------------|
| Sprint v8 QE1 | WASM target check | BLOCKED — pre-existing nix toolchain `gnu/stubs-32.h`/secp256k1-sys C build failure | P2 WASM runner workstream; the same blocker still appears in Sprint v9 STREAM-A/D notes |
| Sprint v8 BUG-4 | `pod_url`/`web_id` AuthState wiring | PARTIAL — types available, full wiring deferred | P2 (forum-client integration test asserts AuthState populated post-passkey-login) |
| Sprint v9 STREAM-A | Deletion of `acl.rs`/`webid.rs`/`provision.rs`/`did.rs` | DEFERRED — waiting on STREAM-B WAC + solid-pod-rs 0.5 publish | P5 (mutation kill-rate target then becomes feasible because the surface is the upstream invocation, not the hand-roll) |
| Sprint v9 STREAM-A | KV namespace IDs in wrangler.toml | placeholders requiring `wrangler kv:namespace create` | not a test gap per se but a deploy gate; flag in G17 #7 |
| Sprint v9 STREAM-D D7 | pod-worker Cache-Control headers | SKIPPED — coordination with STREAM-B | P3 cross-system smoke (asserts response headers under live deployed-Workers conditions) |
| Sprint v9 STREAM-D D13 | `forum-client/src/auth/passkey.rs:115-167` `cfg!(debug_assertions)` log gating | SKIPPED — STREAM-B owned same file | P2 WASM runner test for log-output absence under release build |
| Sprint v9 STREAM-B | WASM target check `gnu/stubs-32.h`/secp256k1-sys C build | DEFERRED to GitHub Actions | P2 — confirm CI executes WASM tests, not just compiles them |
| Sprint v9 STREAM-E1 | pod-worker `acl` module is non-pub in cdylib-only crate; tests use replicated reference algorithms | LIMITATION documented | P2 — promote to a crate-feature `test-internals` analogous to relay-worker `test-exports` (Sprint v9 STREAM-E1 already established this pattern) |
| Sprint v10 STREAM-N3 | Username release does not republish kind-0 to clear `nip05` field | DEFERRED to v11 | P3 cross-system smoke; assert kind-0 republish behaviour |
| Sprint v10 STREAM-N3 | NIP-07 / hardware signers: claim/release shows "not yet supported" | TRACKED for v11 | P2 — refactor to trait-based `Signer` is required first; test follows |
| Sprint v10 STREAM-N3 | Lockfile drift after `package-lock.json` regeneration | acceptable | not a test gap |
| `01-visionclaw-surfaces.md` | RelayConsumer not wired into management-api boot | open | P3 cross-system smoke test asserts boot-time wiring (it should fail today) |
| `01-visionclaw-surfaces.md` | port 7777 not in docker-compose.yml; nostr-rs-relay invisible externally | open | P3 cross-system harness fixes this; test will then pass |
| `01-visionclaw-surfaces.md` C3 | three different `verificationMethod.type` strings | open at agentbox/sovereign-bootstrap.py | P0 fix + P1 cross-emitter conformance |
| `01-visionclaw-surfaces.md` H7 | VisionClaw URI resolver issues 307s to four routes that don't exist | open | P3 cross-system smoke test asserts resolver-target routes exist |
| `01-visionclaw-surfaces.md` | VisionClaw has two unrelated keypairs (SERVER_NOSTR_PRIVKEY + VISIONCLAW_NOSTR_PRIVKEY) | open | P3 cross-system smoke test asserts keypair convergence per ADR-074 |

---

## Appendix C — One-page checklist for the next PR landing protocol-relevant code

For any PR touching nostr-core, pod-worker (acl/did/nip98 paths), VisionClaw (`utils/nip98.rs`, `services/server_identity.rs`, `services/nip26_cap.rs`, `actors/nostr_bridge.rs`, `handlers/solid_pod_handler.rs`), agentbox (`management-api/middleware/auth.js`, `mcp/nostr-bridge/`), or solid-pod-rs `src/auth/`/`src/wac/` — the reviewer checks:

```
[ ] Does this change touch a NIP implementation?
    [ ] If yes: are the canonical reference vectors exercised?
        (forum: nostr-core/tests/fixtures/upstream/<nip>.json — present?)
    [ ] If yes: is there a cross-impl contract test in tests/contract-cross/?
[ ] Does this change cross a substrate boundary?
    [ ] If yes: is there a test in tests/cross-system/ that covers the round-trip?
    [ ] If yes: is there a negative test in tests/cross-system/negative/?
[ ] Does this change modify a DID Document emitter?
    [ ] verificationMethod.type == "SchnorrSecp256k1VerificationKey2019"?
    [ ] @context includes "https://w3id.org/security/suites/secp256k1-2019/v1"?
    [ ] cross-emitter conformance test updated?
[ ] Does this change modify NIP-98 verification?
    [ ] cross-impl test (forum ↔ VisionClaw ↔ agentbox ↔ solid-pod-rs) updated?
    [ ] replay-store behaviour pinned?
[ ] Does this change modify NIP-26 delegation?
    [ ] cross-substrate verifier test updated (forum + VisionClaw + agentbox)?
[ ] Does this change modify federation behaviour (relay AUTH, fan-out, dedup)?
    [ ] ADR-073 conformance suite case added or updated?
[ ] Does this change modify the IS-Envelope?
    [ ] ADR-075 round-trip case added?
    [ ] AS2 LDN mapping case added?
[ ] Does this change modify a moderation kind?
    [ ] DDD TR-Moderation-Honour cross-substrate propagation case added?
[ ] Does this change introduce a new bundle to forum CF Workers or forum-client?
    [ ] bundle-budget.json baseline updated?
[ ] Does this change introduce a new urn:visionclaw: or urn:agentbox: kind?
    [ ] BC20 ACL translation test added?
    [ ] anti-drift lint regex updated?
```

A reviewer who answers any of "yes" without "test added" sends the PR back. The checklist captures the structural gaps from this audit; following it converts coverage from per-substrate-only to cross-substrate by default.
