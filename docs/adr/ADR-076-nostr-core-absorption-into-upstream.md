# ADR-076 — Absorb forum `nostr-core` into upstream `nostr` crate

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-010 G9, F25–F30 |
| Companion ADRs | ADR-073, ADR-074, ADR-075 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` |
| Affected repos | `dreamlab-ai-website/community-forum-rs/` (primary), this repo (cross-ref) |

## Context

The forum's `nostr-core` crate (`dreamlab-ai-website/community-forum-rs/crates/nostr-core/`) hand-rolls **7,892 LOC of Nostr protocol implementation** atop RustCrypto primitives. It declares `nostr = "0.44"` as a workspace dep but the dependency is **unused** — `grep "use nostr::"` across the forum tree returns zero hits. Sprint v9 STREAM-D dropped `nostr-sdk` from the workspace and noted "nostr retained: nostr-core/Cargo.toml still declares it" — i.e. the crate was once consumed, then deprecated in favour of the hand-roll.

Module breakdown:

| Module | LOC | What it does |
|--------|-----|--------------|
| `event.rs` | 445 | NIP-01 event id / sign / verify (canonical JSON, BIP-340 Schnorr) |
| `keys.rs` | 369 | secp256k1 keypair + PRF derivation (HKDF) |
| `nip04.rs` | 523 | ECDH + AES-CBC legacy DM |
| `nip19.rs` | 511 | bech32 entities (npub/nsec/note/naddr/nevent/nprofile) |
| `nip26.rs` | 372 | delegated event signing |
| `nip44.rs` | **549** | v2 ChaCha20-Poly1305 DM (carries the C1 critical bug) |
| `nip90.rs` | 610 | DVM job request/result/feedback types |
| `nip98.rs` | 1,075 | HTTP auth + replay-store trait |
| `gift_wrap.rs` | 652 | NIP-59 three-layer wrap (inherits C1) |
| `groups.rs` | 441 | NIP-29 group helpers |
| `calendar.rs` | 382 | NIP-52 calendar |
| `deletion.rs` | 183 | NIP-09 |
| `moderation_events.rs` | 682 | **Project-specific** kinds 30910–30916 + 1984 |
| `signer.rs` | 339 | `Signer` trait abstraction (PRF / NIP-07 / nsec) |
| `wasm_bridge.rs` | 241 | `wasm-bindgen` glue for forum-client WASM target |
| `types.rs` | 446 | shared types |
| `lib.rs` | 72 | public surface |

The C1 finding from `docs/integration-research/05-crypto-gotchas.md` §6 illustrates exactly the failure mode:

```rust
// nostr-core/src/nip44.rs:122-128
let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), &shared_point);
let mut conv_key = [0u8; 32];
hk.expand(&[], &mut conv_key)?;     // produces HMAC-SHA256(PRK, 0x01) — WRONG
                                    // NIP-44 v2 §2.1 mandates conv_key = PRK
```

The comment at line 121 even reads "HKDF-Extract" — the team knew the spec; the API call was mis-mapped. Tests round-trip cleanly because both encrypt and decrypt have the same bug. **Forum DMs are not interoperable with any reference NIP-44 implementation.** This is the textbook bug class an established and widely-tested upstream library prevents.

The other two substrates already consume established crates:

- **VisionClaw** uses `nostr_sdk::Client` (`src/services/server_identity.rs:138-155`) for publish paths and `nostr_sdk::Keys`/`EventBuilder` for signing (`src/services/server_identity.rs:201-204`).
- **Agentbox** uses `nostr-tools` (JS) inside `mcp/nostr-bridge/relay-consumer.js` for `verifyEvent` and `verifyDelegation`. It is already an established-library consumer.

The forum is the outlier. The reason it forked is structural and recoverable:

- Forum needs **`crate-type = ["lib", "cdylib"]`** so the same artefact links into the Leptos `forum-client` (WASM) and the five Cloudflare Workers (also WASM-`cdylib`).
- CF Workers run `wasm32-unknown-unknown` with `?Send` futures and need `getrandom = { features = ["js"] }`.
- Sprint v9 dropped `nostr-sdk` because the SDK pulls Tokio reactor; the forum needs only the lower-level `nostr` crate which is `no_std`-capable.

So the question "should we absorb upstream?" is really "does upstream `nostr` (without `nostr-sdk`) work in the five CF Worker targets and the Leptos client?". The empirical answer per rust-nostr's documentation and JS bindings (which are themselves built from `nostr` on `wasm32-unknown-unknown`) is **yes** — but the forum's specific build matrix needs validation.

PRD-010's Phase 0 currently specifies hand-patching three crypto bugs (C1, C2, C3) in place. C2 lives in agentbox Python and is unaffected by this ADR. C1 + C3 (and the bulk of the protocol-layer surface) are within forum's scope and are exactly what an upstream absorption deletes. Doing the absorption properly is a larger but **lower-risk** path than patching the hand-roll, because it transfers the correctness burden to a community-maintained, test-vector-validated library used by hundreds of Nostr projects.

## Decision

Migrate the forum's `nostr-core` from a hand-rolled NIP implementation to a **thin shim crate over the upstream `nostr` crate** (rust-nostr.org / yukibtc, currently version 0.44 in workspace, target 0.44.x or current stable at migration time).

Concretely:

### D1 — Delete and replace at the protocol layer

The following modules are deleted; their public functions become re-exports of, or thin wrappers over, the upstream `nostr` equivalents:

| Forum module (DELETE) | Upstream replacement |
|-----------------------|----------------------|
| `event.rs` | `nostr::event::{Event, EventBuilder, Kind, Tag}`, `nostr::types::{Timestamp, Filter}` |
| `keys.rs` (signing/verifying primitives) | `nostr::Keys`, `nostr::SecretKey`, `nostr::PublicKey`. Keep only the **PRF→Keys** derivation function (project-specific). |
| `nip04.rs` | `nostr::nips::nip04::{encrypt, decrypt}` |
| `nip19.rs` | `nostr::nips::nip19::{ToBech32, FromBech32, Nip19}` |
| `nip26.rs` | `nostr::nips::nip26::{Conditions, DelegationTag, validate_delegation_tag, sign_delegation, verify_delegation_signature}` |
| `nip44.rs` | `nostr::nips::nip44::{encrypt, decrypt, ConversationKey, Version}` — **fixes C1 by deletion** |
| `nip90.rs` | `nostr::nips::nip90` (DVM types) |
| `nip98.rs` (signing/verify primitives) | `nostr::nips::nip98::HttpData` for token construction; **keep** `Nip98ReplayStore` trait + `KvReplayStore` impl (project-specific persistence) |
| `gift_wrap.rs` | `nostr::nips::nip59::{seal, gift_wrap, extract_rumor, UnsignedEvent}` — **fixes the inherited C1** |
| `groups.rs` | `nostr::nips::nip29` (or hand-rolled if upstream incomplete — re-evaluate at migration time) |
| `calendar.rs` | `nostr::nips::nip52` (calendar events) |
| `deletion.rs` | `nostr::nips::nip09` |

### D2 — Keep as project-specific shim (~500–700 LOC target)

The following stay in `nostr-core` because they are project-specific and not generalisable upstream:

| Module (KEEP) | Why |
|---------------|-----|
| `moderation_events.rs` (682 LOC) | Custom kinds 30910–30916 are DreamLab-specific. **Upstream is not the right home.** Keep verbatim. |
| `signer.rs` (339 LOC, will shrink) | `Signer` trait must accommodate three backends (PRF / NIP-07 / nsec) — a forum-specific composition. Re-implement to delegate event signing to upstream `nostr::Keys::sign_event` via the existing `Nip07Signer`/`PrfSigner`/`LocalSigner` impls; trait shape unchanged. |
| `wasm_bridge.rs` (241 LOC) | `wasm-bindgen` glue for the Leptos client. Forum-specific. Keep verbatim. |
| `keys.rs::derive_from_prf` only | The PRF→nsec HKDF derivation with info string `"nostr-secp256k1-v1"` is forum-specific; upstream has no equivalent. Keep this function (~30 LOC) and the validation. |
| `nip98.rs::Nip98ReplayStore` trait + `KvReplayStore` | The replay-store contract is a forum invention (Sprint v9 STREAM-B). Upstream has no replay store concept. Keep the trait; delete the verifier internals (delegate to upstream). |
| Project-specific kinds 30033 / 30050 (per ADR-074 / ADR-075) | New mesh kinds. Keep type definitions in `nostr-core::mesh`. |
| `wasm_bridge` re-exports of upstream types | Surface the upstream types through the WASM bridge for forum-client consumption. |

Target post-absorption size: **~700–1,000 LOC**, compared to the current 7,892. **~85% reduction in maintained crypto-protocol surface.**

### D3 — Cargo dep set

`nostr-core/Cargo.toml` post-migration:

```toml
[dependencies]
nostr = { version = "0.44", default-features = false, features = [
  "nip04", "nip17", "nip19", "nip26", "nip29", "nip44", "nip52", "nip56",
  "nip59", "nip65", "nip90", "nip98",
  "std",
] }
# RustCrypto primitives kept ONLY for the PRF→Keys derivation
hkdf = { workspace = true }
sha2 = { workspace = true }
zeroize = { workspace = true }
# ... (serde, hex, base64, thiserror, async-trait unchanged)

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = { workspace = true }
js-sys = { workspace = true }
serde-wasm-bindgen = "0.6"
getrandom = { workspace = true, features = ["js"] }
```

The `chacha20poly1305`, `hmac`, `aes`, `cbc`, `bech32`, and direct `k256` deps are **removed** — upstream `nostr` carries them transitively.

### D4 — `nostr-sdk` is NOT reintroduced

`nostr-sdk` provides higher-level facades (relay pool, client orchestration, builder DSLs) on top of `nostr`. The forum's `forum-client/src/relay.rs` already implements its own single-relay manager with the WASM-friendly `SendWrapper<Rc<RefCell<RelayInner>>>` pattern. Reintroducing `nostr-sdk` means rewriting that surface, and `nostr-sdk` historically pulls Tokio (the reason Sprint v9 dropped it).

We take only `nostr` (the crate), not the SDK.

### D5 — WASM/Cloudflare Workers compatibility validation (gating)

Before any module deletion, run a validation spike:

1. Add a fresh test crate `crates/nostr-upstream-canary/` with `crate-type = ["cdylib"]`.
2. Import `nostr` with `default-features = false` + the feature set above.
3. Implement a minimal smoke test exercising: keypair generation, event sign + verify, NIP-04 round-trip, NIP-44 v2 round-trip with paulmillr reference vectors, NIP-19 npub encode/decode, NIP-26 delegation sign + verify, NIP-59 gift-wrap.
4. Build for `wasm32-unknown-unknown` with the same toolchain CF Workers use (Rust 1.84.0 per Sprint v9 STREAM-C).
5. Deploy as a one-shot CF Worker; assert `worker::Response::ok` from each smoke endpoint.

Acceptance criteria:
- All five worker targets (`auth`, `pod`, `relay`, `search`, `preview`) compile against the new dep set.
- Bundle size delta within +200 KiB per worker (CF Workers free tier is 1 MiB; current largest is `relay-worker` at ~700 KiB).
- Cold-start latency delta within +50 ms.
- All paulmillr/nip44 reference vectors pass.
- Forum-client WASM bundle size delta within +500 KiB (current is ~3.5 MiB compressed).

If any criterion fails, the migration is gated. Investigate the specific failure (likely a feature flag issue) before proceeding.

### D6 — Behaviour-preserving migration (per-module, per-PR)

Migration is **module-by-module**, **PR-by-PR**, with full test parity at each step. Order:

1. **Canary spike** (D5).
2. **`nip04.rs`** — simplest module; smallest test surface.
3. **`nip19.rs`** — highest-coverage in tree; many proptest fixtures help validate equivalence.
4. **`nip44.rs`** — fixes C1 critical at this PR.
5. **`gift_wrap.rs`** — depends on nip44; fixes inherited C1.
6. **`event.rs`** — touches every signed event in the codebase; biggest blast radius. Land last among deletions.
7. **`keys.rs`** — keep `derive_from_prf`; replace the rest.
8. **`nip26.rs`, `nip90.rs`, `calendar.rs`, `deletion.rs`, `groups.rs`** — straightforward deletions in any order.
9. **`nip98.rs`** — keep replay store trait + KvReplayStore; delete the rest.
10. **`signer.rs`** — refactor to delegate to upstream; keep three-backend trait surface.
11. **`types.rs`** — re-export upstream types or delete duplicates.
12. **Final cleanup** — drop `chacha20poly1305`/`hmac`/`aes`/`cbc`/`bech32`/`k256` direct deps; verify Cargo.lock shrinks.

Each PR must:
- Pass all existing unit tests (incl. proptest at `tests/nip19_proptests.rs`, `tests/nip04_proptests.rs`, etc.).
- Pass new upstream-vector tests (paulmillr/nip44, NIP-04 reference vectors, NIP-19 reference vectors).
- Build cleanly on `wasm32-unknown-unknown`.
- Show no behaviour change in `forum-client` integration tests + `relay-worker` tests.

### D7 — Project-specific kinds catalogue

`nostr-core::kinds` becomes the registry of project-specific kind allocations:

```rust
// nostr-core/src/kinds.rs
pub const KIND_BAN: u32 = 30910;
pub const KIND_MUTE: u32 = 30911;
pub const KIND_WARNING: u32 = 30912;
pub const KIND_REPORT: u32 = 30913;
pub const KIND_MODERATION_ACTION: u32 = 30914;
pub const KIND_UNBAN: u32 = 30915;
pub const KIND_UNMUTE: u32 = 30916;

// PRD-010 / ADR-074 / ADR-075 mesh kinds
pub const KIND_MESH_SERVICES: u32 = 30033;
pub const KIND_MESH_EVENT:    u32 = 30050;
```

Builders for moderation events stay in `moderation_events.rs` but use `nostr::EventBuilder` underneath:

```rust
pub fn build_ban(admin_keys: &nostr::Keys, target_pubkey: &str, reason: &str) -> nostr::Event {
    nostr::EventBuilder::new(nostr::Kind::Custom(KIND_BAN), reason, [
        nostr::Tag::Custom("d".into(), vec![target_pubkey.to_string()]),
    ])
    .to_event(admin_keys)
    .expect("admin_keys is valid")
}
```

### D8 — Test vector incorporation

The migration adds these reference test vector files to forum CI:

- `tests/vectors/nip44-v2.json` — paulmillr/nip44 canonical vectors (~40 vectors covering encrypt/decrypt, padded length, MAC).
- `tests/vectors/nip04.json` — interop vectors for legacy DM.
- `tests/vectors/nip19.json` — bech32 round-trip vectors.
- `tests/vectors/nip26.json` — delegation sign + verify vectors.
- `tests/vectors/nip59.json` — gift-wrap interop vectors (recipient key → unwrap → rumor).

These run on every PR; failures block merge. Vectors are sourced from upstream `nostr` crate's own test suite where available, and from the canonical NIP repos otherwise.

### D9 — Cross-substrate alignment

VisionClaw already imports `nostr-sdk` (which depends on `nostr`). Post-migration, both substrates consume the same upstream surface — they may even reuse the same workspace dep:

```toml
# /home/devuser/workspace/project/Cargo.toml
[workspace.dependencies]
nostr = "0.44"  # add; track community-forum-rs version
```

VisionClaw's `nostr_sdk::Client` and forum's `nostr-core` (now thin) both speak the same `nostr::Event`, `nostr::Filter`, etc. Cross-system flows in PRD-010 P3-P4 can pass `nostr::Event` instances directly across the BC20 boundary without translation.

Agentbox's JS side continues using `nostr-tools`. The two ecosystems (Rust `nostr`, JS `nostr-tools`) are wire-compatible by spec and validated by paulmillr's cross-language vectors.

### D10 — Migration timeline as part of PRD-010 Phase 0

The migration absorbs into PRD-010 Phase 0 ("crypto correctness, gating"). Phase 0 grows from 1 sprint to **~2 sprints** but covers more ground:

- C1 (NIP-44 conv key) — fixed by `nip44.rs` deletion in step 4.
- C3 (verificationMethod.type) — handled at the DID Document emitter layer, separate fix.
- L20 (NIP-44 reference vectors) — incorporated as D8.
- M14 (bridge re-signing) — addressed at the protocol-consumer layer, separate fix.

This frees Phases 1-5 to focus on identity/auth/bridge/envelope/consolidation without crypto-surface concerns leaking in.

## Consequences

### Positive

- **Correctness**: ~6,500 LOC of hand-rolled crypto-protocol code disappears. The remaining ~700 LOC of project-specific shim is auditable in a few hours. Bug class C1 cannot recur in deleted modules.
- **Community alignment**: forum DMs become interoperable with any reference NIP client. External users can DM forum members from `damus.io` etc. without forum forks.
- **Test surface inheritance**: the upstream `nostr` crate is exercised by hundreds of consumers across rust-nostr ecosystem; the forum gets that battle-testing for free.
- **Cross-substrate symmetry**: VisionClaw + forum both consume `nostr` crate; types flow directly. BC20 ACL is simpler.
- **Maintenance debt reduction**: every future NIP fix or new NIP support arrives via `cargo update` instead of hand-port. Sprint v9-v11 spent ~2 sprints on NIP-04/NIP-44/NIP-26 hand-port; this would have been zero days.
- **Closes PRD-010 risk R5** (solid-pod-rs Storage trait redesign) by demonstrating that "use upstream + thin shim" is the recoverable pattern; same recipe applies to pod-worker's reimplementation when solid-pod-rs 0.5 ships.

### Negative

- **Migration cost**: ~2 engineer-sprints if smooth; ~3 if WASM/CF Workers compat surfaces issues. Cost is real but bounded.
- **Upstream dependency exposure**: forum is now exposed to `nostr` crate breaking changes. Mitigation: pin to `=0.44.x`; explicit cargo update review process; `nostr` crate has a stable release cadence (~quarterly) with reasonable semver discipline.
- **Loss of forum-specific optimisations**: hand-rolled code may have specific allocations or zero-copy patterns the upstream lacks. Cost paid in performance regressions only if benchmarks show them; bench suites at `nostr-core/benches/{bench_keys,bench_nip44,bench_events}.rs` provide the regression guard.
- **Bundle size growth**: `nostr` crate's full feature set is ~250 KiB compiled WASM; current `nostr-core` is ~180 KiB. Net +70 KiB per CF Worker is acceptable.
- **Sprint v9 STREAM-D dropped `nostr-sdk` dep for size reasons**; D4 ensures we don't reintroduce the SDK, but operators may misread "absorb upstream" as "reintroduce SDK". Doc clarity required.

### Neutral

- **`Signer` trait abstraction stays**: forum's three-backend signer (PRF/NIP-07/nsec) is a forum-specific composition; the trait surface doesn't change. Implementations delegate to `nostr::Keys::sign_event` underneath.
- **Project-specific kinds (30910-30916, 30033, 30050) remain in `nostr-core`**: there is no upstream home for them.
- **WASM bridge code is forum-specific glue**, unaffected by the upstream switch.

## Alternatives Considered

### Alt-A — Status quo (Shape C from prior discussion)

Keep the hand-roll; patch the bugs in place; add reference vectors. PRD-010 originally specified this.

*Rejected (now)*: ships C1 fix but carries forward the 7,892 LOC maintenance burden; future bugs of the same class are a `git blame` away. Per-NIP maintenance cost is high; rust-nostr ecosystem moves faster than DreamLab can hand-track.

### Alt-B — Targeted absorption (NIP-44 / NIP-26 / NIP-59 only)

Replace only the bug-bearing modules; leave NIP-01/NIP-04/NIP-19/NIP-98/NIP-09/NIP-29/NIP-52/NIP-90 hand-rolled.

*Rejected*: still maintains ~5,000 LOC of crypto-protocol code; partial absorption creates a hybrid that's harder to reason about than either extreme. Saves migration cost (~1 sprint vs ~2) but loses the test-surface inheritance benefit.

### Alt-C — Migrate to `nostr-sdk` (full SDK absorption)

Replace `nostr-core` AND `forum-client/src/relay.rs` with `nostr-sdk::Client`.

*Rejected*: Sprint v9 STREAM-D dropped `nostr-sdk` for Tokio-runtime concerns. forum-client is `wasm32-unknown-unknown` Leptos with `SendWrapper<Rc<RefCell<...>>>` patterns; the SDK's relay-pool surface assumes a Tokio runtime that doesn't exist in the browser. Could be a P5+ exploration but increases risk substantially.

### Alt-D — Fork rust-nostr and maintain DreamLab-flavoured upstream

Get the test-surface benefit but retain control over CF Workers compatibility tweaks.

*Rejected*: forks bitrot; we'd reintroduce the maintenance burden after one or two upstream releases. The right path if upstream `nostr` truly cannot work on CF Workers is to **upstream the fixes** (PRs to rust-nostr), not maintain a private fork.

### Alt-E — Roll our own DreamLab-internal Nostr crate as a separate workspace

Like solid-pod-rs: extract `nostr-core` to its own workspace, publish to crates.io, share between forum + VisionClaw.

*Rejected*: re-creates rust-nostr but worse-tested. The whole point of upstream absorption is the test surface; rolling DreamLab's own version moves us away from that.

## Implementation notes

### Validation spike timing

The D5 validation spike must complete **before** PRD-010 Phase 0 implementation begins. If the spike fails, PRD-010 falls back to the targeted Phase 0 (Shape C — patch in place); ADR-076 status moves to "Rejected" with the spike findings recorded as the rationale.

Estimated spike duration: 3-5 days. One engineer.

### Cargo workspace alignment

After successful migration, **VisionClaw's** `Cargo.toml` workspace dependencies should add `nostr` at the same version pin to enable potential future direct sharing of types between `nostr-core` and `nostr_bridge.rs`/`mesh_bridge.rs`. This is bookkeeping, not load-bearing.

### Documentation updates

- `dreamlab-ai-website/CLAUDE.md` "Tech Stack" table: replace "Protocol | Nostr (nostr-core crate)" with "Protocol | Nostr (rust-nostr `nostr` crate, with `nostr-core` shim for project-specific kinds)".
- `community-forum-rs/CLAUDE.md` (if exists): document the kept-vs-deleted module map.
- ADR-074 D8 (NIP-26 grammar): cross-reference upstream verifier.
- ADR-075 D6 (signing semantics): cross-reference upstream NIP-59 implementation.

### Reference test vector source

Vector files for D8 are sourced from:
- `nostr` crate's own `crates/nostr/tests/` directory (already CI-tested upstream).
- `paulmillr/nip44` GitHub repo, `javascript/test/vectors.json`.
- `nostr-protocol/nips` GitHub repo where canonical examples exist.

### Per-PR CI gating

Each migration PR adds a CI check:

1. `cargo test -p nostr-core` (existing tests pass).
2. `cargo test -p nostr-core --target wasm32-unknown-unknown` (existing wasm-bindgen-test pass).
3. `cargo build --target wasm32-unknown-unknown -p {auth,pod,relay,search,preview}-worker` (CF Workers still build).
4. `cargo build --target wasm32-unknown-unknown -p forum-client` (Leptos client still builds).
5. New: `cargo test -p nostr-core --test upstream_vectors` (paulmillr + NIP reference vectors pass).

Failures block merge.

## References

- PRD-010 — DID:Nostr Mesh Federation, G9, F25-F30
- ADR-073 — Mesh topology (federation worker uses upstream `nostr::Event` types)
- ADR-074 — DID:Nostr canonicalisation (NIP-26 verifier from upstream per D1)
- ADR-075 — IS-Envelope (signing semantics delegate to upstream NIP-59 per D1)
- DDD §BC-MESH-FORUM aggregates (note `nostr-core` is post-migration thin shim)
- `docs/integration-research/05-crypto-gotchas.md` §6 — C1 NIP-44 conv-key bug
- rust-nostr crate: https://crates.io/crates/nostr — currently `0.44` in workspace Cargo.toml
- paulmillr/nip44 reference vectors: https://github.com/paulmillr/nip44
- Forum's hand-rolled `nostr-core`: `dreamlab-ai-website/community-forum-rs/crates/nostr-core/`
