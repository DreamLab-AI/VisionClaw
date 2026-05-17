# Q1 — Cryptographic Protocol Audit, Full Ecosystem Sweep

> **Scope.** Every place across the four DreamLab codebases where crypto-protocol
> code is hand-rolled atop low-level primitives, when a community-maintained
> library could carry the surface. Each finding is cited at file:line, classed by
> risk, costed in engineer-days, and matched to a concrete upstream replacement.
>
> **Codebases audited.**
>
> | # | Repo | Path | Language |
> |---|------|------|----------|
> | 1 | VisionClaw substrate | `/home/devuser/workspace/project/` | Rust |
> | 2 | Forum (`community-forum-rs`) | `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/` | Rust (CF Workers + Leptos WASM) |
> | 3 | Agentbox | `/home/devuser/workspace/project/agentbox/` | Python + Node.js |
> | 4 | solid-pod-rs | `/home/devuser/workspace/project/solid-pod-rs/` | Rust |
>
> **Companion docs.** Builds on `docs/integration-research/05-crypto-gotchas.md`
> (C1 NIP-44 conv-key, C2 broken npub, C3 verificationMethod.type drift) and
> ADR-076 (nostr-core absorption into upstream `nostr` crate).
> Where 05-crypto-gotchas already classified a finding, this document repeats
> the LOC + library mapping but does not re-litigate the bug.
>
> **Audit lens.** "If a competent community-maintained library exists for this
> exact protocol surface, why aren't we using it?" Project-specific glue
> (PRF→nsec, project URI grammars, custom event kinds, replay store
> persistence) is exempt — only protocol layer wrapping is in scope.

---

## 0. Executive summary — duplication tally

The same protocols are reimplemented up to **four times** across the ecosystem.
Total hand-rolled crypto-protocol surface across all four repos is **~14,000
LOC**, of which **~9,500 LOC can be deleted** by routing to upstream libraries
without losing functionality. ADR-076 already accounts for ~6,500 LOC of that
inside forum's `nostr-core`. This report finds another **~2,200 LOC** of
removable duplication outside ADR-076's scope.

| Protocol surface | Implementations | Total LOC | Replaceable LOC | Established library |
|------------------|-----------------|-----------|-----------------|---------------------|
| **NIP-98 HTTP auth** | **4** (forum, VisionClaw, agentbox, solid-pod-rs) | **2,286** | **~1,650** | `nostr` 0.44 (`nostr::nips::nip98`) + project replay store |
| **NIP-19 bech32 entities** | **2** (forum nostr-core, agentbox sovereign-bootstrap) | 511 + 56 | 511 + 56 | `nostr` 0.44 (`nostr::nips::nip19`) + Python `bech32` |
| **NIP-01 event id / Schnorr sign+verify** | **3** (forum nostr-core, VisionClaw nostr_identity_verifier, solid-pod-rs nip98 schnorr) | 445 + 92 + 30 | 445 + ~50 | `nostr` 0.44 (`nostr::Event::verify_signature`) |
| **NIP-04 ECDH+AES-CBC DM** | **1** (forum nostr-core) | 523 | 523 | `nostr` 0.44 (`nostr::nips::nip04`) |
| **NIP-44 v2 ChaCha20-Poly1305 DM** | **1** (forum nostr-core, **carries C1 critical bug**) | 549 | 549 | `nostr` 0.44 (`nostr::nips::nip44`) |
| **NIP-26 delegation** | **1** (forum nostr-core) + **1 verifier missing** (VC, agentbox) | 372 | 372 | `nostr` 0.44 (`nostr::nips::nip26`) |
| **NIP-59 gift-wrap** | **1** (forum nostr-core, **inherits C1**) | 652 | 652 | `nostr` 0.44 (`nostr::nips::nip59`) |
| **WAC (ACL) evaluator** | **2** (forum pod-worker, solid-pod-rs) | 821 + ~900 | 821 | `solid-pod-rs::wac` (already used by VisionClaw) |
| **WebID derivation+verify** | **2** (forum pod-worker, solid-pod-rs) | 86 + ~140 | 86 | `solid-pod-rs::interop::did_nostr` |
| **Solid LDP method dispatch** | **2** (forum pod-worker, solid-pod-rs) | ~700 | ~700 | `solid-pod-rs::ldp` |
| **DID document emission** | **3** (forum pod-worker, solid-pod-rs-nostr, agentbox s04-did.js) | 315 + ~140 + 91 | 315 + 91 | `solid-pod-rs-nostr::did` |
| **RFC 8785 JCS canonicalisation** | **1** (agentbox jcs.js) | 166 | ~30 | `npm @web5/json-canonicalization` or `npm canonicalize` |
| **JSON-LD encoder + 11 surface emitters** | **1** (agentbox) | 2,206 | 0 (project glue, but uses upstream `jsonld@^8`) | already on `jsonld@^8.3.2` |
| **AS2 ActivityStreams encoders** | **1** (agentbox s02-nostr.js, embedded across surfaces) | ~150 | 50 | `npm activitystrea.ms` or upstream `activitypub-types` |
| **content-hash-12** (sha256 first 6 bytes) | **2** (VisionClaw + agentbox) | 35 + 18 | 0 (trivial, kept) | n/a |
| **WebAuthn ceremony** | **2** (forum auth-worker hand-roll, solid-pod-rs-idp) | 904 + 507 | 700 | `webauthn-rs` 0.5 (already used by solid-pod-rs-idp) |
| **HTTP Signatures (Cavage)** | **1** (solid-pod-rs-activitypub) | 563 | 200 | `npm http-signature` Rust eq.: `sigh` or `httpsig` |
| **bech32 (low-level)** | **2** (forum via `bech32` crate, agentbox Python) | uses lib + 56 | 56 | `bech32` PyPI |

The cross-cutting headline is the **NIP-98 quadruple duplication** — the same
27235-event verifier with the same timestamp tolerance, URL normalisation, and
payload-hash check exists four separate times. Three of those four have
test parity but diverge on edge cases:

- forum `nostr-core/src/nip98.rs` — full impl with replay store and Schnorr
  verify, 1,075 LOC.
- VisionClaw `src/utils/nip98.rs` — token *creation* only, delegates to
  `nostr_sdk` for signing, 636 LOC.
- agentbox `mcp/servers/nostr-bridge.js::verifyNip98` (lines 321-383, 63 LOC)
  + `management-api/middleware/auth.js::verifyNip98Header` (150 LOC) —
  delegates to `nostr-tools` for signature verify, hand-rolls everything else.
- solid-pod-rs `crates/solid-pod-rs/src/auth/nip98.rs` — full impl with
  optional `nip98-schnorr` feature, 484 LOC.

VisionClaw already imports the solid-pod-rs version (`use solid_pod_rs::auth::nip98;`
in `src/handlers/solid_pod_handler.rs:16`) for the Solid-handler path, but has
its **own separate** NIP-98 token *generator* in `src/utils/nip98.rs` that it
uses to authenticate outbound calls to the Solid backend — i.e. VisionClaw
already has two NIP-98 surfaces in one repo.

**Risk-class breakdown.**

| Class | Count | Cumulative LOC delete |
|-------|-------|----------------------|
| HAND-ROLL CRITICAL | 4 | ~3,400 |
| HAND-ROLL HIGH | 11 | ~4,200 |
| HAND-ROLL MED | 9 | ~1,500 |
| PRIMITIVE USE OK | 7 | n/a |
| LIBRARY USE OK | 6 | n/a |

---

## 1. Per-finding inventory — VisionClaw substrate (`/home/devuser/workspace/project/`)

VisionClaw is the **best-behaved** of the four substrates. It already routes
all Nostr signing/verifying through `nostr_sdk` (which itself wraps the upstream
`nostr` crate), and routes Solid/NIP-98 verification through `solid-pod-rs`.
The remaining hand-roll is a thin generator and one direct-secp256k1 verifier.

### F1.1 — `src/utils/nip98.rs` — NIP-98 token *generator* (HAND-ROLL HIGH)

- **File:line.** `/home/devuser/workspace/project/src/utils/nip98.rs:1-636`.
- **LOC.** 636.
- **Protocol.** NIP-98 HTTP auth — outbound token creation (sign + base64).
- **Risk class.** HAND-ROLL HIGH. The signing path goes through
  `nostr_sdk::EventBuilder::sign_with_keys` (line 83-86), so the cryptographic
  core is library code. But the **wrapper protocol logic** — tag construction
  (line 65-80), payload-hash computation via `sha2::Sha256` (line 75), the
  `Nip98Event` serialisation shim (line 89-101), and the base64-wrapped
  authorization header (line 105) — is all hand-rolled.
- **What's hand-rolled.** Tag building (`["u", url]`, `["method", METHOD]`,
  optional `["payload", hex]`); event-to-wire serialisation; base64 envelope.
- **Recommended replacement.** `nostr` 0.44 ships
  `nostr::nips::nip98::HttpData` which builds the exact same payload. The
  upstream rust-nostr crate produces a base64-encoded authorization-header
  string in three lines.
- **Estimated LOC reduction.** 636 → ~80 (replace 90% of the file with a
  wrapper that calls `HttpData::new(url, method).to_authorization()`).
- **Migration effort.** 0.5 engineer-day. Drop-in replacement; keep the
  `Nip98Config` struct as a forward-compatible adapter.
- **Risk.** Low. The sig surface is already `nostr_sdk`; we replace only the
  shape layer, which has unit tests at the bottom of the file (visible).
  Mitigation: regression-test against existing `solid_pod_rs::auth::nip98`
  verifier paths.
- **Blocked by.** None. VisionClaw's `Cargo.toml` already pulls
  `secp256k1 = "0.29"` and `nostr-sdk = "0.43.0"` (`Cargo.toml:100-103`).
  Adding `nostr = "0.44"` direct is bookkeeping per ADR-076 D9.

### F1.2 — `src/services/nostr_identity_verifier.rs` — XR Schnorr verifier (PRIMITIVE USE OK)

- **File:line.** `/home/devuser/workspace/project/src/services/nostr_identity_verifier.rs:1-92`.
- **LOC.** 92.
- **Protocol.** Bespoke `(nonce || timestamp_us)` Schnorr challenge-response
  for the XR presence handshake. **Not a Nostr event** — a custom binary
  challenge under BIP-340 Schnorr.
- **Risk class.** PRIMITIVE USE OK. The construction is project-specific
  (PRD-008 §T-WS-1) and intentionally bypasses event synthesis: hash a
  fixed-shape buffer and verify Schnorr. No matching upstream exists.
- **What's hand-rolled.** Hex parsing (lines 36-40), `XOnlyPublicKey::from_slice`
  (line 41), `Signature::from_slice` (line 50), SHA-256 of nonce ‖ timestamp
  little-endian (lines 52-55), `SECP256K1.verify_schnorr` (line 58).
- **Recommended replacement.** None. Direct `secp256k1` use is correct here:
  this is not an event signature, it's a challenge-response over a custom
  message, and `secp256k1::SECP256K1` is the reference Schnorr implementation.
- **Note.** Doc-comment at line 5 explicitly chose `secp256k1` over
  `nostr_sdk::Event` synthesis to avoid fake-event ceremony. Correct call.
- **Estimated LOC reduction.** 0.
- **Migration effort.** 0.

### F1.3 — `src/utils/opaque_id.rs` — HMAC-PRF opaque-id derivation (PRIMITIVE USE OK)

- **File:line.** `/home/devuser/workspace/project/src/utils/opaque_id.rs:1-282`.
- **LOC.** 282.
- **Protocol.** ADR-050 session-scoped private-node opaque ids:
  `hex(HMAC-SHA256(salt, owner_pubkey || '|' || canonical_iri))[..24]`.
- **Risk class.** PRIMITIVE USE OK. This is a project-specific PRF
  construction, not a standard protocol. Uses `hmac::Hmac<Sha256>` (line 30,
  39) and a salt-rotation token via `sha2::Sha256` (line 150-158). The
  surrounding `SessionSalt` rotation machinery (lines 49-145) is project
  application code.
- **What's hand-rolled.** HMAC PRF (correct primitive use); manual hex encoding
  (lines 174-188 — could use `hex::encode` but the manual loop avoids one dep
  and is byte-identical).
- **Recommended replacement.** None. The construction is bespoke.
- **Estimated LOC reduction.** ~12 (replace `nibble_to_hex` with `hex::encode`).
- **Migration effort.** 0.1 engineer-day. Cosmetic.

### F1.4 — `src/utils/canonical_iri.rs` — npub-scoped IRI builder (LIBRARY USE OK)

- **File:line.** `/home/devuser/workspace/project/src/utils/canonical_iri.rs:1-161`.
- **LOC.** 161.
- **Protocol.** NIP-19 npub encoding — delegates correctly to
  `nostr_sdk::PublicKey::to_bech32` (line 65-66).
- **Risk class.** LIBRARY USE OK. The bech32 step is library code; the path
  hashing (lines 71-93) is project-specific content addressing. The whole
  file is `#[deprecated]` in favour of the 12-hex form (line 46-50).
- **Recommended action.** None. Replacement landed already (`src/uri/mint.rs`).
- **Estimated LOC reduction.** 0 (already deprecated; will delete on data
  migration per PRD-006 §5.10).

### F1.5 — `src/uri/parse.rs` + `src/uri/legacy.rs` — URN parsing (LIBRARY USE OK)

- **File:line.** `parse.rs:1-291` + `legacy.rs:1-92`.
- **LOC.** 291 + 92 = 383.
- **Protocol.** `urn:visionclaw:*` URN grammar (RFC 8141) and CURIE
  rewriting. NIP-19 npub decode delegated to `nostr_sdk::PublicKey::from_bech32`
  (parse.rs line 257-260). Content-address `sha256-12-...` uses `sha2::Sha256`
  (parse.rs line 269-281).
- **Risk class.** LIBRARY USE OK. RFC 8141 is a string grammar, not a crypto
  protocol — hand-parsing is correct. The crypto bits delegate to libraries.
- **Estimated LOC reduction.** 0.

### F1.6 — `src/services/server_identity.rs` (LIBRARY USE OK)

- **File:line.** `server_identity.rs:1-400`.
- **LOC.** 400.
- **Protocol.** Server Nostr identity. Loads `SERVER_NOSTR_PRIVKEY` (nsec or
  hex), constructs `nostr_sdk::Keys`, signs server-issued events via
  `EventBuilder::sign_with_keys`, broadcasts via `nostr_sdk::Client`. **All
  through library code.**
- **Risk class.** LIBRARY USE OK. Correct delegation throughout. Lines
  171-180 + 397 use `nostr_sdk` parsing/signing; aligned with forum and ADR-076.
- **Estimated LOC reduction.** 0.

### F1.7 — `src/services/nostr_bridge.rs` — bead bridge (HAND-ROLL MED)

- **File:line.** `nostr_bridge.rs:1-487`.
- **LOC.** 487.
- **Protocol.** Forwards bead-stream events from JSS relay → forum, signing
  with the server's identity. Uses `nostr_sdk::Event::verify` for inbound
  signature check (line 192) — correct.
- **Risk class.** HAND-ROLL MED. The signature path is library code. What's
  hand-rolled is the **WebSocket envelope handling** for the relay protocol
  (`["EVENT", event]` JSON wrap at lines 257-258, NIP-01 wire framing) — but
  this is the relay-client surface, not crypto. Direct `tokio_tungstenite`
  use is appropriate.
- **Open gap (per gotchas-05 §15.7).** No `Event` delegation-tag verification
  before re-signing under bridge identity. **HIGH** severity — flagged
  separately as M14.
- **Recommended replacement.** None for the WS framing. For delegation-tag
  verification: import `nostr::nips::nip26::verify_delegation_signature` once
  ADR-076 lands; until then, port forum's `nostr-core::nip26` helper.
- **Estimated LOC reduction.** 0 for crypto surface; +30 for delegation
  verification.
- **Migration effort.** 0.5 engineer-day for delegation verifier wiring.

### F1.8 — `src/services/nostr_service.rs` and `nostr_bead_publisher.rs` (LIBRARY USE OK)

- **File:line.** `nostr_service.rs:1-668`, `nostr_bead_publisher.rs:1-423`.
- **LOC.** 668 + 423 = 1091.
- **Risk class.** LIBRARY USE OK. Both go through `nostr_sdk::prelude::*`
  for events, builders, signing. No hand-roll.

### F1.9 — `src/handlers/solid_pod_handler.rs` (LIBRARY USE OK)

- **File:line.** `solid_pod_handler.rs:16` imports `solid_pod_rs::auth::nip98`.
- **Risk class.** LIBRARY USE OK. Correctly delegates NIP-98 verification to
  the upstream `solid-pod-rs` crate. **This is the canonical example** of
  what the rest of the ecosystem should look like.

### VisionClaw subtotal

- **Hand-roll surface to delete.** ~616 LOC (F1.1) + ~12 LOC cosmetic (F1.3).
- **Library usage already correct.** F1.4, F1.5, F1.6, F1.8, F1.9.
- **Total finding count.** 9. CRITICAL: 0; HIGH: 1 (F1.1); MED: 1 (F1.7);
  PRIMITIVE OK: 2; LIBRARY OK: 5.

---

## 2. Per-finding inventory — Forum (`community-forum-rs/`)

### F2.0 — Pre-existing scope: ADR-076 absorbs ~7,200 LOC

ADR-076 already specifies that **6,500 LOC of `nostr-core/`** (namely
`event.rs`, `keys.rs` minus `derive_from_prf`, `nip04.rs`, `nip19.rs`,
`nip26.rs`, `nip44.rs`, `nip90.rs`, `gift_wrap.rs`, `groups.rs`, `calendar.rs`,
`deletion.rs`, plus the verifier internals of `nip98.rs`) collapses into
re-exports of `nostr` 0.44. **This audit does NOT relitigate those modules** —
the ADR-076 mapping table is reproduced below for completeness.

| nostr-core module | LOC | ADR-076 disposition | Upstream | Risk lens |
|-------------------|-----|---------------------|----------|-----------|
| `event.rs` | 445 | DELETE | `nostr::event::*` | CRITICAL (signing surface) |
| `keys.rs` | 369 | KEEP only `derive_from_prf` (~30 LOC) | `nostr::Keys` | HIGH (aux_rand hard-zero at line 63 — see §5.4 below) |
| `nip04.rs` | 523 | DELETE | `nostr::nips::nip04` | HIGH |
| `nip19.rs` | 511 | DELETE | `nostr::nips::nip19` | HIGH |
| `nip26.rs` | 372 | DELETE | `nostr::nips::nip26` | HIGH |
| **`nip44.rs`** | **549** | **DELETE — fixes C1 by deletion** | `nostr::nips::nip44` | **CRITICAL** |
| `nip90.rs` | 610 | DELETE | `nostr::nips::nip90` | MED |
| `nip98.rs` | 1,075 | KEEP `Nip98ReplayStore` trait + `KvReplayStore` (~150 LOC); DELETE rest | `nostr::nips::nip98` | HIGH |
| `gift_wrap.rs` | 652 | DELETE — inherits C1 | `nostr::nips::nip59` | CRITICAL (inherits C1) |
| `groups.rs` | 441 | DELETE if upstream NIP-29 ready, else KEEP | `nostr::nips::nip29` | MED |
| `calendar.rs` | 382 | DELETE | `nostr::nips::nip52` | LOW |
| `deletion.rs` | 183 | DELETE | `nostr::nips::nip09` | LOW |
| `moderation_events.rs` | 682 | KEEP — DreamLab kinds | n/a | MED (project-specific) |
| `signer.rs` | 339 | KEEP, refactor — composition | uses `nostr::Keys` | MED |
| `wasm_bridge.rs` | 241 | KEEP | re-exports upstream | LOW |
| `types.rs` | 446 | KEEP partial; re-export rest | mixed | LOW |
| `lib.rs` | 72 | KEEP | n/a | LOW |
| **TOTAL** | **7,892** | **post-migration target ~700-1,000 LOC** | | **~85% reduction** |

### F2.1 — `nostr-core/src/nip44.rs` C1 critical bug (HAND-ROLL CRITICAL)

- **File:line.** `nostr-core/src/nip44.rs:122-128`.
- **LOC affected.** 549 (whole module replaced; 7 LOC bug).
- **Risk class.** **HAND-ROLL CRITICAL.** Already documented as C1 in
  `docs/integration-research/05-crypto-gotchas.md` §6. Forum NIP-44
  conversation key uses `Hkdf::expand(empty info, …)` instead of
  `HKDF-Extract` PRK. **All forum DMs are non-interoperable with
  reference NIP-44 implementations.**
- **Replaced by ADR-076 D6 step 4.** Migration effort: 1 engineer-day for
  this module specifically; net gain: deletion of 549 LOC + 100 LOC of test
  scaffold + bug-class disappearance.

### F2.2 — `nostr-core/src/nip98.rs` (1,075 LOC) — partial keep (HAND-ROLL HIGH)

- **File:line.** `nostr-core/src/nip98.rs:1-1075`.
- **LOC.** 1,075.
- **Protocol.** NIP-98 verifier with replay store.
- **Risk class.** HAND-ROLL HIGH. The verifier internals (event-id recompute,
  Schnorr signature verify, timestamp tolerance, URL/method matching) are all
  upstream-redundant. The `Nip98ReplayStore` trait and `KvReplayStore`
  implementation are forum-specific because they target Cloudflare KV.
- **Replacement.** `nostr::nips::nip98::HttpData::verify`. Keep
  `Nip98ReplayStore` trait + `KvReplayStore` implementation
  (lines roughly 851-1075 in the existing file).
- **LOC reduction.** 1,075 → ~150 (replay store + thin wrapper).
- **Migration effort.** 1 engineer-day. Test surface: 100 lines of unit test
  exist (lines 871-958) covering replay first/second-use; preserve verbatim.
- **Risk.** Low. Behaviour-preserving by design.

### F2.3 — `nostr-core/src/nip04.rs:104-106` — x-coordinate recovery duplication (HAND-ROLL MED)

- **File:line.** `nostr-core/src/nip04.rs:59-79`, also seen at
  `nostr-core/src/nip44.rs:104-119`.
- **LOC.** ~30 each, ~60 LOC total duplication.
- **Protocol.** Reconstructing a compressed secp256k1 point from a 32-byte
  x-only pubkey by prefixing `0x02` (assume even y) and parsing via
  `PublicKey::from_sec1_bytes`. Identical pattern in both files.
- **Risk class.** HAND-ROLL MED. Functional, but error-prone if the y parity
  ever needs to be even. Upstream `nostr` already has this hidden inside
  `nostr::PublicKey::from_xonly`.
- **Replacement.** `nostr::PublicKey::from_xonly`. Disappears under ADR-076.
- **LOC reduction.** ~60 (deleted with parent files).

### F2.4 — `nostr-core/src/keys.rs:63` — aux_rand hardcoded zero (HAND-ROLL HIGH)

- **File:line.** `nostr-core/src/keys.rs:60-70`.
- **LOC.** 11.
- **Protocol.** BIP-340 Schnorr signing. `SecretKey::sign` calls
  `sign_raw(&message, &[0u8; 32])`. **Per BIP-340 §3.3 this is allowed but
  removes the synthetic-randomness defence against fault attacks.**
- **Risk class.** HAND-ROLL HIGH. Production code paths
  (`event::sign_event`) override this with `getrandom`, but `keys.rs::sign`
  is a public function reachable from `Keypair::sign` test helpers and
  potentially from any future direct call site. **Under ADR-076 the whole
  function disappears** — `nostr::Keys::sign_event` (and the lower-level
  `nostr::SecretKey::sign_schnorr`) randomise aux_rand by default.
- **Replacement.** `nostr::Keys::sign_event`. Eliminated by ADR-076 D6 step 7.
- **Migration effort.** 0 incremental — covered by ADR-076.
- **Lint mitigation L19** (already in 05-gotchas): deny direct
  `SecretKey::sign(...)` outside `event::sign_event`.

### F2.5 — `pod-worker/src/acl.rs` — JSON-LD WAC evaluator (HAND-ROLL HIGH)

- **File:line.** `pod-worker/src/acl.rs:1-821`.
- **LOC.** 821.
- **Protocol.** WAC (Web Access Control) — evaluates ACL documents stored as
  JSON-LD against incoming requests. Mode mapping (`acl:Read`, `acl:Write`,
  `acl:Append`, `acl:Control`) at lines 90-100; `IdOrIds` shape parsing
  lines 68-81; access-mode evaluation lines 162-188.
- **Risk class.** HAND-ROLL HIGH. The doc-comment (line 4-8) explicitly
  chose to "use direct JSON parsing instead of a full RDF library" to keep
  the WASM bundle small. But:
  - A full WAC evaluator already exists at
    `solid-pod-rs/crates/solid-pod-rs/src/wac/{evaluator,parser}.rs` —
    349 + 559 = 908 LOC of battle-tested implementation.
  - VisionClaw uses the solid-pod-rs version directly
    (`use solid_pod_rs::wac::evaluate_access` at
    `src/handlers/solid_pod_handler.rs:21`).
  - Forum's WAC evaluator does not normalise `did:nostr:<HEX>` case (gotchas
    §14) — a known live bug requiring a separate fix.
- **Replacement.** `solid-pod-rs::wac::*` once forum migrates to native
  pod-rs storage (per `04-solid-pod-rs-surfaces.md` §13). Until then, port
  the case-normalisation + `coerce_required_mode_for_acl` logic from forum
  into solid-pod-rs as **upstream PRs**, then delete forum's copy.
- **LOC reduction.** 821 → 0 (consume `solid-pod-rs::wac::evaluate_access`).
- **Migration effort.** 5 engineer-days. WASM-target compat needs
  validation (solid-pod-rs assumes std + tokio; CF Workers need
  `wasm32-unknown-unknown` + `?Send` futures).
- **Risk.** MEDIUM. CF Workers WASM target is the gating concern. If
  solid-pod-rs cannot compile to `wasm32-unknown-unknown` cleanly (likely
  given its `tokio + actix-web` deps), the migration target is to
  **re-export the pure ACL evaluation core** from solid-pod-rs as a
  no_std-compatible sub-crate (`solid-pod-rs-core` or `solid-pod-rs-wac`).

### F2.6 — `pod-worker/src/did.rs` — DID document emitter (HAND-ROLL MED)

- **File:line.** `pod-worker/src/did.rs:1-315`.
- **LOC.** 315.
- **Protocol.** `did:nostr` DID Document generation, Tier-1 + Tier-3.
  Doc comment (line 4) says "Mirrors solid-pod-rs-nostr v0.4.0-alpha.2 `did`
  module logic, adapted for WASM Workers (no tokio dependency)."
- **Risk class.** HAND-ROLL MED. The implementation is **already a copy** of
  `solid-pod-rs-nostr::did::render_did_document_tier1`
  (`solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs`), forked because
  pod-worker is a Cloudflare Worker (WASM, no tokio). The fork has correctly
  picked `SchnorrSecp256k1VerificationKey2019` per ADR-027 (line 88).
- **Replacement.** Same as F2.5 — extract `solid-pod-rs-nostr::did` into a
  no_std `solid-pod-rs-nostr-core` so pod-worker can consume it. Until then
  this is a sanctioned fork.
- **LOC reduction.** 315 → ~50 if shared upstream.
- **Migration effort.** 3 engineer-days (depends on F2.5 success).

### F2.7 — `pod-worker/src/webid.rs` — WebID profile handler (HAND-ROLL MED)

- **File:line.** `pod-worker/src/webid.rs:1-86`.
- **LOC.** 86.
- **Protocol.** WebID profile JSON-LD generation + verification of the
  `["webid", uri]` tag in NIP-98 events. The bidirectional carrier
  enforcement (DID Doc `alsoKnownAs` ↔ WebID `schema:identifier`) lives at
  `did.rs:50-60`.
- **Risk class.** HAND-ROLL MED. Same pattern as F2.6 — already a fork of
  `solid-pod-rs::interop::did_nostr`. Forum's variant hardcodes
  `https://pods.dreamlab-ai.com/<hex>/...` (gotchas §13 hazard).
- **Replacement.** `solid-pod-rs::interop::did_nostr` after WASM extraction.
- **LOC reduction.** 86 → ~20.

### F2.8 — `pod-worker/src/{lib.rs,container.rs,patch.rs,content_negotiation.rs,conditional.rs,quota.rs,provision.rs,notifications.rs}` — Solid LDP+pod machinery (HAND-ROLL HIGH)

- **File:line.** Across all eight files: `lib.rs:1-1409`, `container.rs:1-162`,
  `patch.rs:1-319`, `content_negotiation.rs:1-243`,
  `conditional.rs:1-110`, `quota.rs:1-107`, `provision.rs:1-425`,
  `notifications.rs:1-167`.
- **LOC.** 1,409 + 162 + 319 + 243 + 110 + 107 + 425 + 167 = **2,942 LOC**.
- **Protocol.** Solid LDP method dispatch (`GET`/`PUT`/`POST`/`DELETE`/
  `PATCH`/`HEAD`/`OPTIONS`), container handling, content negotiation,
  conditional-request handling, quota enforcement, pod provisioning
  (TypeIndex bootstrap), Solid Notifications webhooks.
- **Risk class.** HAND-ROLL HIGH. The whole pod-worker re-implements
  solid-pod-rs LDP semantics. `provision.rs` doc-comment (line 5) explicitly
  says "Mirrors the logic published in solid-pod-rs v0.4.0-alpha.2
  `provision.rs`."
- **Replacement.** `solid-pod-rs::ldp::*`, `solid-pod-rs::storage::*`,
  `solid-pod-rs::quota::*`, `solid-pod-rs::wac::*` after WASM extraction.
- **LOC reduction.** 2,942 → ~600 (Cloudflare-specific KV/R2 storage adapter
  + WS upgrade for Solid Notifications + project-specific tag-bridge).
- **Migration effort.** **15-20 engineer-days.** Big-bang absorption
  candidate; should be staged after the smaller F2.5/F2.6/F2.7 wins prove
  the WASM extraction pattern works.

### F2.9 — `relay-worker/src/relay_do/nip_handlers.rs` (PRIMITIVE USE OK)

- **File:line.** `relay-worker/src/relay_do/nip_handlers.rs:1-762`.
- **LOC.** 762.
- **Risk class.** PRIMITIVE USE OK. NIP-01 relay protocol implementation
  (REQ/EVENT/AUTH/CLOSE/EOSE wire framing). Uses `nostr-core::Event::verify`
  (which post-ADR-076 is `nostr::Event::verify`). Wire framing and
  durable-object session state are project-specific.

### F2.10 — `auth-worker/src/webauthn.rs` — WebAuthn ceremony (HAND-ROLL CRITICAL)

- **File:line.** `auth-worker/src/webauthn.rs:1-904`.
- **LOC.** 904.
- **Protocol.** WebAuthn registration + authentication ceremony, including
  attestation-object parsing (line 121), client-data-JSON parsing (line 145,
  604-635), origin/challenge/ceremony-type checks, signCount handling
  (line 687).
- **Risk class.** HAND-ROLL CRITICAL. The Cargo.toml at workspace level
  declares `passkey-types = "0.3"` (forum-rs/Cargo.toml:35) — but the
  auth-worker Cargo.toml does **not** import it. The whole 904 LOC is a
  manual ceremony implementation against `web-sys` shapes. Constant-time
  comparison is hand-coded at lines 31-39.
- **Replacement.**
  - **Best**: `webauthn-rs` 0.5 — already used by `solid-pod-rs-idp` per
    `solid-pod-rs/crates/solid-pod-rs-idp/Cargo.toml:56` (`webauthn-rs = "0.5"`).
  - **WASM-compatible alternative**: `passkey-types = "0.3"` (already declared)
    + project-specific challenge-state store. The crate is no_std-friendly
    and is what the forum's workspace already pulls — but is not consumed.
- **LOC reduction.** 904 → ~200. The PRF-extension-output extraction and
  Nostr-key derivation (the parts that are **not** WebAuthn ceremony but
  forum-specific PRF→nsec wiring) stay; everything else is upstream.
- **Migration effort.** 8-10 engineer-days. WebAuthn is sprawling; even
  `webauthn-rs` has 30+ knobs; map carefully.
- **Risk.** HIGH. Drop-in is non-trivial because the existing implementation
  pre-dates the `webauthn-rs` 0.5 trait redesign. Mitigation: parallel
  rollout with feature flag, full reference-vector test set
  (FIDO conformance suite vectors).

### F2.11 — `auth-worker/src/crypto.rs` — ChaCha20Poly1305 nsec at-rest (LIBRARY USE OK)

- **File:line.** `auth-worker/src/crypto.rs:1-222`.
- **LOC.** 222.
- **Protocol.** AEAD encryption for the welcome-bot's nsec at rest. Uses
  `chacha20poly1305::ChaCha20Poly1305` correctly with random 12-byte nonces
  (line 78) and base64url envelope.
- **Risk class.** LIBRARY USE OK. Direct `ChaCha20Poly1305` use is the
  reference pattern; the wrapping is project-specific. Constant-time
  AEAD verification is built into the crate.

### F2.12 — `auth-worker/src/delegation.rs` — NIP-26 verifier endpoint (HAND-ROLL MED)

- **File:line.** `auth-worker/src/delegation.rs:1-288`.
- **LOC.** 288.
- **Protocol.** NIP-26 delegation token verification — server endpoint.
  Computes SHA-256 of `"nostr:delegation:{delegatee}:{conditions}"` (line 23)
  via `sha2::Sha256`, verifies Schnorr via `k256::schnorr::VerifyingKey`.
- **Risk class.** HAND-ROLL MED. Reimplements logic from
  `nostr-core/src/nip26.rs::DelegationToken::verify` — same module that
  ADR-076 deletes. After ADR-076 lands, this endpoint can call
  `nostr::nips::nip26::verify_delegation_signature` directly.
- **Replacement.** `nostr::nips::nip26::verify_delegation_signature`.
- **LOC reduction.** 288 → ~120 (HTTP wrapping kept; verification body
  becomes one call).
- **Migration effort.** 1 engineer-day, gated on ADR-076 D6 step 8.

### F2.13 — `forum-client/src/auth/nip98.rs` — client-side NIP-98 token (HAND-ROLL MED)

- **File:line.** `forum-client/src/auth/nip98.rs:1-334`.
- **LOC.** 334.
- **Protocol.** Client-side NIP-98 token creation via WASM, calls
  `nostr_core::nip98::create_token_at` (line 40).
- **Risk class.** HAND-ROLL MED. Thin wrapper over `nostr-core`. Uses
  `js_sys::Date::now()` (line 39) because `SystemTime` is unavailable in
  WASM. After ADR-076, calls go through `nostr::nips::nip98::HttpData`.
- **Replacement.** `nostr::nips::nip98::HttpData::to_authorization` after
  ADR-076 lands.
- **LOC reduction.** 334 → ~80.

### F2.14 — `forum-client/src/auth/passkey.rs` — passkey ceremony client (HAND-ROLL MED)

- **File:line.** `forum-client/src/auth/passkey.rs:1-480`.
- **LOC.** 480.
- **Protocol.** WebAuthn PRF passkey registration + auth via `web-sys`.
  Uses the underlying `webauthn` module helpers (line 17-21).
- **Risk class.** HAND-ROLL MED. Conjugate of F2.10 — same migration target.

### F2.15 — `forum-client/src/auth/webauthn.rs` — WebAuthn helpers (HAND-ROLL MED)

- **File:line.** `forum-client/src/auth/webauthn.rs:1-312`.
- **LOC.** 312.
- **Protocol.** PRF-output extraction from CredentialCreation/Assertion,
  hybrid-transport detection.
- **Risk class.** HAND-ROLL MED. Forum-specific PRF wiring is project glue;
  the ceremony helpers (build_creation_options, build_request_options) are
  upstream candidates.

### F2.16 — `forum-client/src/auth/nip07.rs` — NIP-07 extension shim (LIBRARY USE OK)

- **File:line.** `forum-client/src/auth/nip07.rs:1-293`.
- **LOC.** 293.
- **Risk class.** LIBRARY USE OK / MED gotcha. NIP-07 extension calls; one
  hazard at lines 188-207 silently routes NIP-04 through NIP-44 (gotchas
  §6 footnote). The fix is project-side, not library-side.

### F2.17 — `forum-client/src/dm/mod.rs` — DM gift-wrap consumer (HAND-ROLL MED)

- **File:line.** `forum-client/src/dm/mod.rs:1-650`.
- **LOC.** 650.
- **Protocol.** DM state store using `nostr_core::gift_wrap::{gift_wrap,
  unwrap_gift}` (line 13) and `nostr_core::nip44_decrypt` (line 14).
- **Risk class.** HAND-ROLL MED. Application-state store; the crypto
  surface is `nostr-core` calls. After ADR-076 these become
  `nostr::nips::nip59::*` / `nostr::nips::nip44::decrypt`.

### F2.18 — `pod-worker/src/contexts.rs` — bundled JSON-LD contexts (LIBRARY USE OK)

- **File:line.** `pod-worker/src/contexts.rs:1-80`.
- **LOC.** 80.
- **Protocol.** Bundles `did/v1`, `secp256k1-2019`, `acl`, `solid-terms`
  contexts as `include_str!` to avoid runtime fetches.
- **Risk class.** LIBRARY USE OK. Best-practice pinning per PRD-006 §6.

### F2.19 — `relay-worker/src/whitelist.rs`, `cron.rs`, `auth.rs`, `nip11.rs` (PRIMITIVE USE OK)

- **File:line.** `whitelist.rs:1-547`, `cron.rs:1-387`, `auth.rs:1-167`,
  `nip11.rs:1-53`.
- **LOC.** 547 + 387 + 167 + 53 = 1,154.
- **Risk class.** PRIMITIVE USE OK. Relay-worker policy / cron / WS auth /
  NIP-11 doc emission. Not protocol crypto.

### F2.20 — `preview-worker/src/ssrf.rs` — SSRF guard (PRIMITIVE USE OK)

- **File:line.** `preview-worker/src/ssrf.rs:1-390`.
- **LOC.** 390.
- **Protocol.** SSRF (CF Workers `Url`-class URL validation, manual redirect
  handling).
- **Risk class.** PRIMITIVE USE OK. CF Workers has no general-purpose SSRF
  guard library; manual `is_private_url` enumeration is appropriate.
  However: solid-pod-rs has `solid-pod-rs/src/security/ssrf.rs` (784 LOC) —
  forum's preview-worker SSRF could share the IP-block list once the
  WASM-extraction pattern is established.

### Forum subtotal

- **Pre-ADR-076 hand-roll surface to delete.** 7,200 LOC (per ADR-076).
- **Additional hand-roll absorbable beyond ADR-076.** ~5,500 LOC
  (F2.5+F2.6+F2.7+F2.8 = 3,329; F2.10 = 700 LOC; F2.13+F2.14+F2.15+F2.17 =
  1,500 LOC of consumer code that follows naturally once core libraries are
  available).
- **Total finding count.** 21. CRITICAL: 2 (F2.1, F2.10); HIGH: 5 (F2.2,
  F2.4, F2.5, F2.7, F2.8); MED: 8; PRIMITIVE OK: 4; LIBRARY OK: 2.

---

## 3. Per-finding inventory — Agentbox (`agentbox/`)

### F3.1 — `scripts/sovereign-bootstrap.py:13-68` — bech32 hand-roll (HAND-ROLL CRITICAL)

- **File:line.** `agentbox/scripts/sovereign-bootstrap.py:13-68`.
- **LOC.** 56 (lines 13-68 — BIP-173 bech32 polymod, hrp expansion, checksum,
  convertbits, encode, decode).
- **Protocol.** BIP-173 bech32 / NIP-19 bech32m. The implementation is
  textbook BIP-173 code lifted verbatim from the spec.
- **Risk class.** HAND-ROLL CRITICAL. Already documented as **C2** in
  `docs/integration-research/05-crypto-gotchas.md` §1: bech32-encodes the
  **64-byte uncompressed SEC1 public key** as `npub` instead of the **32-byte
  x-only pubkey** required by NIP-19. Resulting npub is non-interoperable
  with any reference Nostr decoder.
- **Replacement.**
  - **`bech32` PyPI package** (https://pypi.org/project/bech32/) — Pieter
    Wuille's reference implementation, 200 lines, BIP-173 compliant.
  - Also fix the 64-byte → 32-byte x-only pubkey computation per gotchas
    §1 — drop y coordinate, force even-y via `lift_x`.
- **LOC reduction.** 56 → 5 (one `import bech32` + the encode/decode calls).
- **Migration effort.** 0.5 engineer-day. Tiny patch but **protocol-critical**.
- **Risk.** Low. Test against a known-good `nip19.decode_npub` pair.

### F3.2 — `scripts/sovereign-bootstrap.py:81-92, 123-135` — ECDSA-vs-Schnorr-key drift (HAND-ROLL CRITICAL)

- **File:line.** `agentbox/scripts/sovereign-bootstrap.py:81-92, 123-135`.
- **LOC.** ~25.
- **Protocol.** Identity-key generation. Uses `ecdsa.SigningKey` on
  `SECP256k1` curve, then bech32-encodes the **64-byte (X||Y) raw uncompressed
  encoding** as `npub`.
- **Risk class.** HAND-ROLL CRITICAL. Already C2. The scalar is the same
  scalar used by Schnorr (the curve is identical), but the public-key
  *serialisation* differs — Schnorr uses 32-byte x-only with even-y
  convention. **The persisted nsec/npub pair is wrong.**
- **Replacement.**
  - Use `coincurve` or `secp256k1` Python bindings instead of `ecdsa`.
  - For Nostr-compatible keypairs at minimum cost, use `pynostr` or
    `monstr` — both already encode/decode npub/nsec correctly.
  - The bigger play: replace the whole sovereign-bootstrap key-derivation
    surface with the JS `nostr-tools` calls used elsewhere in agentbox
    (`mcp/servers/nostr-bridge.js` already imports `nostr-tools`), or
    swap the Python script for a Node script.
- **LOC reduction.** ~25 → ~10.
- **Migration effort.** 1 engineer-day (includes test against forum/VC).
- **Risk.** Low — the scalar stays the same, only serialisation changes.
  Mitigation: derive once, cross-check pubkey against nostr-tools verify.

### F3.3 — `scripts/sovereign-bootstrap.py:183-203` — DID-Doc verificationMethod.type drift (HAND-ROLL CRITICAL)

- **File:line.** `agentbox/scripts/sovereign-bootstrap.py:192`.
- **LOC.** 1 (the literal string, but it's the bug centre).
- **Protocol.** W3C DID Core 1.0 verificationMethod. Forum +
  solid-pod-rs-nostr now both emit
  `SchnorrSecp256k1VerificationKey2019`; this script emits
  `SchnorrSecp256k1VerificationKey2022` — a **non-existent
  cryptosuite** (W3C never registered ...2022).
- **Risk class.** HAND-ROLL CRITICAL. Already C3 in 05-gotchas §2.
- **Replacement.** Hard-code `SchnorrSecp256k1VerificationKey2019`. Better:
  delegate DID-Doc emission to **agentbox's own surface**
  `management-api/middleware/linked-data/surfaces/s04-did.js`. **But
  that file emits a 4th distinct value** — see F3.7.
- **LOC reduction.** -1, +1.
- **Migration effort.** 0.1 engineer-day for the literal fix; 1 day for
  the proper architectural fix (route DID-Doc emission through one
  agentbox surface).

### F3.4 — `mcp/servers/nostr-bridge.js:321-383` — NIP-98 verifier (HAND-ROLL HIGH)

- **File:line.** `agentbox/mcp/servers/nostr-bridge.js:321-383`.
- **LOC.** 63.
- **Protocol.** NIP-98 HTTP auth verifier — kind-27235 unwrap, base64
  decode, tag matching, timestamp window check, then delegates Schnorr
  verification to `nostr-tools.verifyEvent` (line 374).
- **Risk class.** HAND-ROLL HIGH. The signature path is library code; the
  surrounding wire-format unwrap is hand-rolled. Note the **divergent**
  URL match at line 366: `urlTag !== url && !urlTag.endsWith(url)`,
  whereas every other NIP-98 implementation in the ecosystem (forum, VC,
  solid-pod-rs) uses trailing-slash normalisation. **Inconsistent matching
  semantics across substrates.**
- **Replacement.**
  - **`npm @scure/base` + `nostr-tools` already imported.** `nostr-tools`
    does not provide a NIP-98 verifier directly, but the surrounding
    helpers (verifyEvent, parseEvent) cover everything except tag
    matching. Total replacement code: ~30 LOC.
  - Better: align the URL/method matching code shared with VisionClaw
    (which uses `solid-pod-rs::auth::nip98`).
- **LOC reduction.** 63 → 30.
- **Migration effort.** 0.5 engineer-day, **but blocked on URL-match
  spec alignment** across all four substrates first.

### F3.5 — `management-api/middleware/auth.js` — NIP-98 wrapper middleware (HAND-ROLL MED)

- **File:line.** `agentbox/management-api/middleware/auth.js:1-150`.
- **LOC.** 150.
- **Protocol.** NIP-98 + Bearer fallback dispatcher. Calls F3.4 for the
  actual verification. Hybrid/strict-nip98/bearer mode resolution.
- **Risk class.** HAND-ROLL MED. Application-routing logic; not
  protocol-crypto. Keep as-is, but consume the shared NIP-98 verifier
  from F3.4 once that's tightened.

### F3.6 — `management-api/lib/uris.js` — URN minting + content addressing (LIBRARY USE OK)

- **File:line.** `agentbox/management-api/lib/uris.js:1-286`.
- **LOC.** 286.
- **Protocol.** `urn:agentbox:*` URN grammar (RFC 8141) + bech32 hex-pubkey
  normalisation. Uses `crypto.createHash('sha256')` for content-addressing
  (line 262).
- **Risk class.** LIBRARY USE OK / MED hazard. The crypto step uses the
  Node built-in correctly. **Two issues:**
  1. `_stableStringify` (lines 266-272) is a custom JSON canonicaliser, not
     RFC 8785 JCS. The doc-comment at line 256-260 explicitly notes
     "deterministic enough beats exactly RFC 8785" for naming. Compare to
     `linked-data/jcs.js` (F3.7) which is full RFC 8785. **Deliberate
     divergence between naming-canon and signing-canon.**
  2. The `_normalisePubkey` bech32 decode (lines 184-198) tries
     `nostr-tools.nip19.decode` — same recommendation as F3.4: keep, but
     the dynamic require pattern is fragile.
- **Replacement.** Stable-stringify lookalike: `npm json-stable-stringify`
  (10k weekly downloads, 2 LOC swap). Or absorb the existing JCS function
  in F3.7 for both naming and signing.
- **LOC reduction.** ~20.
- **Migration effort.** 0.5 engineer-day.

### F3.7 — `management-api/middleware/linked-data/jcs.js` — RFC 8785 JCS canonicaliser (HAND-ROLL HIGH)

- **File:line.** `agentbox/management-api/middleware/linked-data/jcs.js:1-166`.
- **LOC.** 166.
- **Protocol.** RFC 8785 JSON Canonicalization Scheme. Used by S3
  (Verifiable Credentials) and S8 (agentic-payment mandates/receipts) for
  the "bytes the proof block signs."
- **Risk class.** HAND-ROLL HIGH. The implementation is correct (verified
  by reading) — careful number serialisation per ECMA-262, code-point key
  sort (lines 143-160), proper string escaping (lines 98-125). But:
  - Several mature npm packages exist:
    - `canonicalize` (https://www.npmjs.com/package/canonicalize) —
      80k weekly downloads, exactly RFC 8785, ~50 LOC, MIT.
    - `@web5/json-canonicalization` — Decentralized Identity Foundation,
      RFC 8785 + W3C DID-cited tests.
  - The agentbox impl sorts UTF-16 code-point sequences manually (line
    143-160) — correct but easy to break on edge-case emoji.
- **Replacement.** `npm canonicalize@2.0.0` — drop-in.
- **LOC reduction.** 166 → 5.
- **Migration effort.** 0.5 engineer-day. Test against the existing
  `tests/contract/linked-data/jcs.spec.js` RFC 8785 test vectors —
  if they pass with `canonicalize`, drop the custom impl.
- **Risk.** Low. RFC 8785 is well-specified; conformance is binary.

### F3.8 — `management-api/middleware/linked-data/surfaces/s04-did.js:71` — **fourth distinct verificationMethod.type** (HAND-ROLL CRITICAL — NEW)

- **File:line.** `agentbox/management-api/middleware/linked-data/surfaces/s04-did.js:71`.
- **LOC.** 1 (the literal).
- **Protocol.** W3C DID Core verificationMethod.
- **Risk class.** **HAND-ROLL CRITICAL — NEW finding** not in 05-gotchas.
  The agentbox JSON-LD surface emits `SchnorrSecp256k1VerificationKey2025`
  — a **fourth distinct value** alongside the three in gotchas §2:

  | Emitter | `verificationMethod[0].type` |
  |---|---|
  | Forum pod-worker (`pod-worker/src/did.rs:88, 146`) | `SchnorrSecp256k1VerificationKey2019` ✓ |
  | solid-pod-rs-nostr (`crates/solid-pod-rs-nostr/src/did.rs:98`) | `NostrSchnorrKey2024` |
  | Agentbox sovereign-bootstrap (`scripts/sovereign-bootstrap.py:192`) | `SchnorrSecp256k1VerificationKey2022` |
  | **Agentbox JSON-LD surface (`s04-did.js:71`)** | **`SchnorrSecp256k1VerificationKey2025`** |

  Worse, agentbox now has **two divergent emitters in one repo**: the Python
  bootstrap and the JS S4 surface produce different DID Documents for the
  same agent. Whichever one the operator hits first wins.
- **Replacement.** All four must converge on
  `SchnorrSecp256k1VerificationKey2019` (the only W3C-registered cryptosuite
  for this curve+hash combo). Add a CI assertion in each repo that
  parses its own emitted DID-Doc and asserts `type ==
  SchnorrSecp256k1VerificationKey2019`.
- **LOC reduction.** Net 0 — string swap.
- **Migration effort.** 0.1 engineer-day each, 4 repos. Total: 0.4 day.
- **Risk.** Low. Spec-mandated value; nothing else accepts the other three.

### F3.9 — JSON-LD encoder + 11 surface emitters (LIBRARY USE OK)

- **File:line.** `management-api/middleware/linked-data/encoder.js:1-257`,
  `lion-linter.js:1-306`, `round-trip.js:1-107`,
  `context-resolver.js:1-271`, surfaces s01-s11 (89+101+75+90+91+139+69+125+
  62+67+79 = 1,087 LOC).
- **LOC.** 257 + 306 + 107 + 271 + 1,087 = 2,028 LOC.
- **Protocol.** JSON-LD compaction + expand round-trip + LION lint + 11
  surface encoders (S1-S11) producing JSON-LD documents for various
  vocabularies (Solid pods, Nostr envelopes, VCs, DID Docs, PROV-O,
  WoT capability descriptors, skill metadata, agentic payments, DCAT,
  architecture docs, HTTP meta).
- **Risk class.** LIBRARY USE OK. The actual JSON-LD processing routes
  through `jsonld@^8.3.2` (visible in `round-trip.js:22-30`,
  `package.json` line `"jsonld": "^8.3.2"`). The 11 surface encoders are
  project-specific schema encoders — analogous to dataclass→JSON adapters,
  not crypto.
- **Note.** The S1-S11 surfaces use AS2 (ActivityStreams 2.0) types in
  S2 (`s02-nostr.js:33-50`). The mapping table from agentbox-internal verbs
  to AS2 types could be replaced by `npm activitystrea.ms` (~50 LOC saved)
  but that's marginal.
- **Estimated LOC reduction.** ~50.
- **Migration effort.** 1 engineer-day. Optional.

### F3.10 — `mcp/servers/nostr-bridge.js::loadSigner` — AES-GCM encrypted nsec at rest (LIBRARY USE OK)

- **File:line.** `mcp/servers/nostr-bridge.js:431-475`.
- **LOC.** 45.
- **Protocol.** AES-256-GCM AEAD wrapping of nsec.hex with PBKDF2-SHA256
  (100k iters) key derivation. Uses Node built-in `crypto.createDecipheriv`,
  `crypto.pbkdf2Sync`. Wipes derived key (line 460).
- **Risk class.** LIBRARY USE OK. Reference Node primitives, correct.
  Forum's `auth-worker/crypto.rs` is the same pattern with ChaCha20Poly1305.

### F3.11 — `mcp/nostr-bridge/relay-consumer.js::_verify` (LIBRARY USE OK)

- **File:line.** `mcp/nostr-bridge/relay-consumer.js:320-335`.
- **LOC.** ~16.
- **Protocol.** Schnorr signature verification on inbound relay events.
  Delegates to `nostr-tools.verifyEvent` (line 327).
- **Risk class.** LIBRARY USE OK. Correct delegation. Has a fail-open
  fallback at line 329-333 ("nostr-tools may not be installed in test runs
  — log once and accept") which is a **TEST-ONLY MEDIUM hazard**;
  production should fail closed. Add an env-var guard.

### Agentbox subtotal

- **Hand-roll surface to delete.** 56 (F3.1) + 25 (F3.2) + 1 (F3.3) +
  33 (F3.4 net) + 161 (F3.7) + 1 (F3.8) ≈ **277 LOC**.
- **JSON-LD library use.** Already on `jsonld@^8.3.2` ✓.
- **Total finding count.** 11. CRITICAL: 4 (F3.1, F3.2, F3.3, F3.8);
  HIGH: 2 (F3.4, F3.7); MED: 1; LIBRARY OK: 4.

---

## 4. Per-finding inventory — solid-pod-rs (`solid-pod-rs/`)

### F4.1 — `crates/solid-pod-rs/src/auth/nip98.rs` (LIBRARY USE OK + HAND-ROLL HIGH split)

- **File:line.** `solid-pod-rs/crates/solid-pod-rs/src/auth/nip98.rs:1-484`.
- **LOC.** 484.
- **Protocol.** NIP-98 HTTP auth verifier; behind feature `nip98-schnorr`
  uses `k256::schnorr::VerifyingKey::verify` for the actual Schnorr check
  (line 199); without the feature returns `Unsupported` (line 207).
- **Risk class.** HAND-ROLL HIGH. The Schnorr verify primitive is library
  code (k256). Everything else (canonical event-id recompute via
  serde_json::json! at lines 152-163, timestamp window, URL/method
  matching, payload-hash recompute) is hand-rolled. This is the **fourth
  separate NIP-98 implementation** in the ecosystem.
- **Comparison to other three:**
  - Forum's nip98.rs: 1,075 LOC, replay store, full schnorr.
  - VisionClaw's nip98.rs: 636 LOC, **token creation only**.
  - Agentbox's verifyNip98: 63 LOC, delegates to `nostr-tools.verifyEvent`.
  - solid-pod-rs's nip98.rs: 484 LOC, optional Schnorr.

  The **same protocol, four times** with subtle divergences:

  | Feature | Forum | VisionClaw | Agentbox | solid-pod-rs |
  |---------|-------|------------|----------|--------------|
  | URL trailing-slash normalisation | yes | n/a (gen only) | partial (`urlTag.endsWith(url)`) | yes (line 220-222) |
  | Method case-insensitive | yes | yes (`.to_uppercase()`) | yes | yes |
  | Timestamp tolerance | 60s | n/a | 60s | 60s |
  | Replay protection | yes (KvReplayStore) | n/a | no | no |
  | Schnorr verify | yes | n/a | via nostr-tools | optional feature |
  | Max event size cap | yes | n/a | no | yes (64KiB) |
- **Replacement.** This crate **is** the upstream for VisionClaw — it
  should grow into the canonical wrapper around `nostr::nips::nip98`.
  Path forward:
  1. Forum gets `nostr` 0.44 per ADR-076 — its NIP-98 verifier disappears.
  2. VisionClaw's `src/utils/nip98.rs` (token-creation) routes through
     `nostr::nips::nip98::HttpData::to_authorization` for symmetric
     creation/verification.
  3. solid-pod-rs's nip98.rs becomes a thin shim over
     `nostr::nips::nip98::HttpData::verify` + project-specific
     `SelfSignedVerifier` adapter (already at lines 247-296).
  4. Agentbox's verifyNip98 stays as-is (Node side; delegates to
     `nostr-tools` already).
- **LOC reduction.** 484 → ~120 (verifier inner is one call; keep
  `Nip98Verifier` adapter shape and the test scaffold).
- **Migration effort.** 2 engineer-days.

### F4.2 — `crates/solid-pod-rs-nostr/src/did.rs` — DID-Doc emitter (HAND-ROLL MED)

- **File:line.** `solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs` (per
  gotchas §2 file paths; not opened above).
- **Protocol.** `did:nostr` DID Document Tier-1 + Tier-3.
- **Risk class.** HAND-ROLL MED. Already analysed in 05-gotchas §2.
  Currently emits `NostrSchnorrKey2024` (drift); needs to converge on
  `SchnorrSecp256k1VerificationKey2019` per ADR-027 + add the
  `secp256k1-2019/v1` context to Tier-1.
- **Replacement.** None at the protocol-library level — this is **the**
  reference implementation for DID:Nostr in the Rust ecosystem. Fix the
  drift in place (per gotchas C3 + H4); export it as the upstream for
  pod-worker (F2.6) and agentbox s04 (F3.8).

### F4.3 — `crates/solid-pod-rs/src/wac/{evaluator,parser,...}.rs` — WAC implementation (LIBRARY USE OK / IS the upstream)

- **File:line.** `solid-pod-rs/crates/solid-pod-rs/src/wac/parser.rs:1-559`,
  `evaluator.rs:1-349`, `mod.rs:1-422`, plus client/conditions/origin/etc.
- **LOC.** ~2,500 across the wac module.
- **Risk class.** LIBRARY USE OK / IS UPSTREAM. This is the canonical Rust
  WAC implementation. VisionClaw uses it directly. Forum **should** use it
  per F2.5.

### F4.4 — `crates/solid-pod-rs-activitypub/src/http_sig.rs` — Cavage HTTP Signatures (HAND-ROLL HIGH)

- **File:line.** `solid-pod-rs/crates/solid-pod-rs-activitypub/src/http_sig.rs:1-563`.
- **LOC.** 563.
- **Protocol.** **draft-cavage-http-signatures-12** with `rsa-sha256` for
  ActivityPub federation. Uses `rsa::pkcs1v15::{Signature, SigningKey,
  VerifyingKey}`, `rsa::pkcs8::{DecodePrivateKey, DecodePublicKey}`,
  `sha2::Sha256`. Doc-comment notes RFC 9421 also supported via auto-detect.
- **Risk class.** HAND-ROLL HIGH. Mature established libraries exist:
  - **`http-signature-normalization`** + `http-signature-normalization-actix`
    crates — used by `kitsune` (Mastodon-compatible Rust AP server).
  - **`sigh`** — simpler crate, narrower scope, AP-specific.
  - **`httpsig`** — RFC 9421 + draft-cavage compatibility.

  **The big tradeoff**: the Rust Fediverse ecosystem has **not** standardised
  on one crate. `kitsune` uses `http-signature-normalization`;
  `lemmy` uses `http-signature-normalization-actix`; some smaller projects
  hand-roll. solid-pod-rs's hand-roll is a defensible choice **today** but
  carries forward the testing burden.
- **Replacement.** `http-signature-normalization-actix` — best-aligned
  given solid-pod-rs is already an `actix-web` crate.
- **LOC reduction.** 563 → ~150.
- **Migration effort.** 5-7 engineer-days. **The risk is not the migration
  itself but the cross-fediverse interop testing** — Mastodon, Pleroma,
  Misskey, GoToSocial all have idiosyncratic header-canonicalisation bugs
  that hand-roll has been tuned against (`canonicalise_request` + the
  body-digest base64 vs hex split). Migration must run a federation
  test suite against all four flagship implementations.
- **Risk.** HIGH. AP federation is the most fragile interop surface in
  the entire ecosystem; established library does not eliminate the test
  burden, only shifts it.

### F4.5 — `crates/solid-pod-rs/src/oidc/jwks.rs` — JWKS / OIDC discovery (LIBRARY USE OK)

- **File:line.** `solid-pod-rs/crates/solid-pod-rs/src/oidc/jwks.rs:1-459`.
- **LOC.** 459.
- **Protocol.** OIDC discovery + JWKS fetch. Uses `jsonwebtoken` for the
  JWT side (line 38), wraps SSRF via `crate::security::ssrf::SsrfPolicy`
  (line 44). The SSRF guard is custom (correctly so — this is the
  attacker-facing surface that needs custody of the IP-block list).
- **Risk class.** LIBRARY USE OK. Reference `jsonwebtoken` use; the SSRF
  defence is project-specific application code.

### F4.6 — `crates/solid-pod-rs-idp/src/{schnorr,passkey,jwks,tokens,session}.rs` (LIBRARY USE OK)

- **File:line.** `solid-pod-rs-idp/src/passkey.rs:1-507`, plus schnorr/jwks/
  tokens/session.
- **Protocol.** Solid-OIDC IdP with WebAuthn passkey via
  `webauthn-rs = "0.5"` (Cargo.toml:56) and JWT via `jsonwebtoken = "9"`
  (line 31).
- **Risk class.** LIBRARY USE OK. **Reference example** of how
  authentication flows should consume mature Rust libraries.

### F4.7 — `crates/solid-pod-rs-didkey/src/{verifier,jwt,pubkey,did}.rs` — did:key (LIBRARY USE OK)

- **File:line.** `solid-pod-rs-didkey/src/jwt.rs:1-351`, `verifier.rs:1-104`,
  `pubkey.rs:1-191`, `did.rs:1-121`.
- **Protocol.** did:key (Ed25519 / P-256 / secp256k1) self-signed JWT
  verifier. Uses `ed25519-dalek = "2"`, `p256 = "0.13"`, `k256 = "0.13"`.
- **Risk class.** LIBRARY USE OK. Direct primitives where appropriate;
  no protocol-level hand-roll.

### F4.8 — `crates/solid-pod-rs/src/security/{ssrf,dotfile,paths}.rs` (PRIMITIVE USE OK)

- **File:line.** `ssrf.rs:1-784`, `dotfile.rs:1-452`, etc.
- **LOC.** 784 + 452 = 1,236.
- **Protocol.** SSRF guard (IP-block list, DNS-rebinding pinning, redirect
  re-validation), dotfile filtering, path traversal guards.
- **Risk class.** PRIMITIVE USE OK. Security primitives are
  project-specific; no upstream eats this surface area.

### solid-pod-rs subtotal

- **Hand-roll surface to delete.** 484 (F4.1) - 120 = ~360 LOC + ~400
  LOC of F4.4 if AP migration is taken.
- **Total finding count.** 8. CRITICAL: 0 (F4.2 is medium drift, fixed in
  place); HIGH: 2 (F4.1, F4.4); MED: 1 (F4.2); LIBRARY OK: 5.

---

## 5. Cross-cutting tally

This section answers the user's specific cross-cutting questions.

### 5.1 NIP-98 implementations — quadruple

Per F1.1, F2.2, F3.4, F4.1:

| Implementation | LOC | Style | Schnorr verify | Replay store |
|----------------|-----|-------|-----------------|--------------|
| forum `nostr-core/src/nip98.rs` | 1,075 | full impl + replay store | yes | yes (KV) |
| solid-pod-rs `auth/nip98.rs` | 484 | structural + opt-in Schnorr | feature-gated | no |
| VisionClaw `src/utils/nip98.rs` | 636 | **token creation only** | n/a (sign only) | n/a |
| agentbox `mcp/servers/nostr-bridge.js::verifyNip98` | 63 | structural; nostr-tools verify | yes (delegated) | no |
| agentbox `management-api/middleware/auth.js` | 150 | wrapper over the above | n/a | no |
| **Total** | **2,408 LOC** | | | |

### 5.2 bech32 implementations — double (one critical bug)

| Implementation | LOC | Status |
|----------------|-----|--------|
| forum `nostr-core/src/nip19.rs` (uses `bech32 = 0.11` crate) | 511 | LIBRARY USE OK ✓ (the *crate* is library; the NIP-19 TLV layer is hand-rolled — disappears under ADR-076) |
| agentbox `scripts/sovereign-bootstrap.py:13-68` | 56 | HAND-ROLL CRITICAL — verbatim BIP-173 lift, with the wrong-pubkey-encoding bug C2 |
| solid-pod-rs no direct bech32 (delegates to upstream Schnorr keys) | 0 | OK |

### 5.3 Schnorr signature verification outside `nostr_sdk::Event::verify_signature`

| Site | File:line | Class | Justified? |
|------|-----------|-------|------------|
| `secp256k1::SECP256K1.verify_schnorr` | `src/services/nostr_identity_verifier.rs:58` | direct primitive | YES — XR challenge-response, not Nostr event |
| `k256::schnorr::Verifier::verify` | `solid-pod-rs/auth/nip98.rs:199` | feature-gated | YES — solid-pod-rs is `nostr-sdk` independent |
| `k256::schnorr::VerifyingKey::verify_raw` | `nostr-core/src/keys.rs:120` | hand-roll | NO — disappears under ADR-076 |
| `nostr-tools.verifyEvent` (JS) | agentbox `nostr-bridge.js:374`, `relay-consumer.js:327` | library delegate | YES — JS side correctly defers |
| `Event::verify` (rust-nostr) | VisionClaw `nostr_bridge.rs:192`, `nostr_service.rs`, forum `relay-worker` | library use | YES — canonical |

So Schnorr-verify-outside-library happens in **3 sites**, two justified
(F1.2 XR challenge, F4.1 IS the library), one to be deleted (forum keys.rs).

### 5.4 content-hash-12 (`sha256-12-<12 hex>`) — double

| Implementation | File:line | LOC |
|----------------|-----------|-----|
| VisionClaw `src/uri/parse.rs::content_hash_12` | `parse.rs:267-281` | 14 |
| agentbox `management-api/lib/uris.js::_contentAddress` | `uris.js:255-264` | 9 |

Both correctly use `sha2::Sha256` / `crypto.createHash('sha256')`, take
the first 6 bytes, hex-encode. **Byte-identical convention** per
PRD-006 F10. Total ~23 LOC of project glue; not absorbable upstream.

### 5.5 RFC 8785 JCS canonicalisation — single hand-roll, two near-canon

| Implementation | File:line | LOC | RFC 8785 strict? |
|----------------|-----------|-----|-------------------|
| agentbox `management-api/middleware/linked-data/jcs.js` | F3.7 | 166 | yes |
| agentbox `management-api/lib/uris.js::_stableStringify` | F3.6 | 7 | no — naming-only stable-stringify |
| VisionClaw IS-Envelope spec (per ADR-075 reference) | not yet implemented | n/a | n/a |

Exactly one full implementation — should consume `npm canonicalize` (50 LOC,
RFC 8785 conformant, MIT).

### 5.6 JSON-LD encoder + 11 surface emitters — single agentbox install

Already routes `expand`/`compact` through `jsonld@^8.3.2` (round-trip.js
line 22). The 11 surface encoders (s01-s11) are per-vocabulary
JSON-LD-shape adapters — project glue. **No drift here.**

### 5.7 NIP-44 v2 — single (with C1 bug)

Only forum implements NIP-44 (forum `nostr-core/src/nip44.rs`). VisionClaw,
solid-pod-rs, and agentbox do not; they consume DMs only via the forum
relay path. **The bug is contained to one repo and disappears under
ADR-076.**

### 5.8 NIP-59 gift-wrap — single (inherits C1)

Only forum (`nostr-core/src/gift_wrap.rs`). Same disposition as 5.7.

### 5.9 NIP-26 delegation — single emitter, two missing verifiers

| Site | File:line | Status |
|------|-----------|--------|
| forum `nostr-core/src/nip26.rs` (sign + verify) | full impl, 372 LOC | absorbed by ADR-076 |
| forum `auth-worker/src/delegation.rs` (HTTP verify endpoint) | F2.12, 288 LOC | thin wrapper, kept post-ADR-076 |
| **VisionClaw nostr_bridge** missing verifier | `src/services/nostr_bridge.rs:181-247` | gap — gotchas H5 |
| **Agentbox relay-consumer** missing verifier | `mcp/nostr-bridge/relay-consumer.js:320-335` | gap — gotchas H5 |

After ADR-076 lands, the verifier is one call (`nostr::nips::nip26::
verify_delegation_signature`). Wiring it into the two missing sites is
0.5 day each.

### 5.10 ECDH x-coordinate recovery from x-only pubkey — double

Same `compressed[0] = 0x02; copy x` pattern in two sibling files:
- `nostr-core/src/nip04.rs:59-79`
- `nostr-core/src/nip44.rs:104-119`

Both replaced by `nostr::PublicKey::from_xonly` under ADR-076.

### 5.11 Aux-rand handling

`nostr-core/src/keys.rs:63` hard-codes `aux_rand = [0u8; 32]`. Only
production callers go through `event::sign_event` which sources random aux.
The `keys.rs` site is reachable from `Keypair::sign` test helpers and
disappears under ADR-076. Lint **L19** (gotchas) prevents future drift.

### 5.12 verificationMethod.type — quadruple drift

| Emitter | Value | Source |
|---------|-------|--------|
| Forum pod-worker | `SchnorrSecp256k1VerificationKey2019` ✓ | `pod-worker/src/did.rs:88, 146` |
| solid-pod-rs-nostr | `NostrSchnorrKey2024` | `crates/solid-pod-rs-nostr/src/did.rs:98, 154` |
| agentbox sovereign-bootstrap.py | `SchnorrSecp256k1VerificationKey2022` | `scripts/sovereign-bootstrap.py:192` |
| **agentbox s04-did.js** (NEW) | **`SchnorrSecp256k1VerificationKey2025`** | `surfaces/s04-did.js:71` |

**Four** distinct values across four emitters. The NEW finding (F3.8) is
that agentbox emits a **fifth** distinct value via the JSON-LD surface,
not noted in 05-gotchas C3 because that report only saw the Python
bootstrap. **Two emitters in agentbox alone diverge.**

### 5.13 WAC evaluator — double

| Implementation | LOC |
|----------------|-----|
| forum `pod-worker/src/acl.rs` (WASM, no tokio) | 821 |
| solid-pod-rs `crates/solid-pod-rs/src/wac/*` | ~2,500 (the canon) |

Forum's is a pure-fn WASM-friendly fork. Sharing requires extracting a
`solid-pod-rs-wac-core` no_std crate.

### 5.14 WebID derivation + verification — double

| Implementation | File:line | LOC |
|----------------|-----------|-----|
| forum `pod-worker/src/webid.rs` | 86 | F2.7 |
| solid-pod-rs `interop::did_nostr` | ~140 | F4.2 |

Same structural pattern as 5.13 — the forum copy is a WASM-friendly
shrunk fork.

### 5.15 Solid LDP method dispatch — double

| Implementation | LOC |
|----------------|-----|
| forum pod-worker (`lib.rs` + 7 sibling modules) | 2,942 |
| solid-pod-rs `ldp` + `storage` + `quota` etc | ~3,000 |

Largest single duplication after ADR-076.

### 5.16 ActivityPub HTTP Signatures — single (no upstream consumer)

| Implementation | File:line | LOC |
|----------------|-----------|-----|
| solid-pod-rs-activitypub `http_sig.rs` | F4.4 | 563 |

No other DreamLab substrate implements AP HTTP signatures. Established
libs exist (`http-signature-normalization-actix`, `sigh`, `httpsig`) but
the migration brings federation-interop testing burden.

### 5.17 WebAuthn ceremony — double

| Implementation | LOC | Library |
|----------------|-----|---------|
| forum `auth-worker/src/webauthn.rs` (CF Worker, hand-roll) | 904 | `passkey-types` declared but **unused** |
| solid-pod-rs-idp `passkey.rs` | 507 | `webauthn-rs = "0.5"` ✓ |

Big delta — forum should adopt `webauthn-rs` 0.5 (or `passkey-types`
already in workspace).

---

## 6. Priority matrix — risk × LOC, descending

| # | Finding | LOC at risk | Risk | Effort | Priority |
|---|---------|-------------|------|--------|----------|
| 1 | F2.1 NIP-44 C1 bug | 549 | CRITICAL | 1d (in ADR-076) | **P0** |
| 2 | F3.1 + F3.2 agentbox bech32/npub C2 | 81 | CRITICAL | 1.5d | **P0** |
| 3 | F3.3 + F3.8 + F4.2 verificationMethod.type drift | 4 strings | CRITICAL | 1d | **P0** |
| 4 | F2.10 forum WebAuthn hand-roll | 904 | CRITICAL | 8-10d | **P1** |
| 5 | F2.8 forum pod-worker LDP machinery | 2,942 | HIGH | 15-20d | **P1** (after F4.3 WASM extraction) |
| 6 | F4.4 solid-pod-rs AP HTTP signatures | 563 | HIGH | 5-7d | **P2** |
| 7 | F2.5 forum WAC evaluator | 821 | HIGH | 5d (gated on WASM extract) | **P2** |
| 8 | F4.1 solid-pod-rs nip98 trim | 484→120 | HIGH | 2d | **P2** |
| 9 | F1.1 VisionClaw nip98 generator | 636→80 | HIGH | 0.5d | **P3** quick win |
| 10 | F3.7 agentbox JCS → npm canonicalize | 166→5 | HIGH | 0.5d | **P3** quick win |
| 11 | F2.6 + F2.7 pod-worker DID-Doc/WebID forks | 401 | MED | 3d (gated on WASM extract) | **P3** |
| 12 | F2.12 auth-worker delegation endpoint | 288→120 | MED | 1d (gated on ADR-076) | **P3** |
| 13 | F3.4 agentbox NIP-98 verifier alignment | 63→30 | MED | 0.5d | **P3** |
| 14 | F2.13 forum-client NIP-98 token | 334→80 | MED | 0.5d (gated on ADR-076) | **P3** |
| 15 | F1.7 VisionClaw nostr_bridge delegation gap | +30 | MED | 0.5d | **P3** |

---

## 7. Quick wins — 1-day refactors that close meaningful drift

These are landed in isolation, no upstream-extraction prerequisites.

1. **F3.3 + F3.8 + F4.2 — verificationMethod.type alignment.** Set all four
   emitters to `SchnorrSecp256k1VerificationKey2019`. Add CI assertion in
   each repo. **0.4 day total. Closes C3 and a NEW critical drift in
   one go.**
2. **F3.1 + F3.2 — agentbox sovereign-bootstrap.py uses `bech32` PyPI +
   x-only-pubkey computation.** Closes C2. **1 day.**
3. **F3.7 — agentbox JCS replaced by `npm canonicalize`.** Run existing
   RFC 8785 vector tests against drop-in. **0.5 day.**
4. **F1.1 — VisionClaw NIP-98 token generator uses
   `nostr_sdk::nips::nip98::HttpData`.** Replaces 600 LOC. **0.5 day.**
5. **F1.7 — VisionClaw nostr_bridge wires `Event::verify_delegation_tag`
   before re-signing.** Closes M14. **0.5 day.**
6. **F3.4 — agentbox NIP-98 URL match aligned with trailing-slash
   normalisation.** Brings agentbox into spec parity with forum + VC +
   solid-pod-rs. **0.5 day.**

**Quick-wins total: ~3.4 engineer-days. Closes 3 CRITICAL findings + 3 MED.**

---

## 8. Strategic absorptions — multi-sprint, high payoff

1. **ADR-076 forum `nostr-core` → `nostr` 0.44 absorption.** Already
   specified. **2 sprints.** Deletes ~6,500 LOC, closes C1, eliminates
   the NIP-44 bug class, gives forum interop with the broader Nostr
   ecosystem.
2. **Extract `solid-pod-rs-{wac,ldp,did}-core` no_std crates** for
   forum-pod-worker consumption. **2-3 sprints.** Deletes
   ~3,500 LOC across F2.5, F2.6, F2.7, F2.8. Opens the door to a
   single Rust pod implementation across the ecosystem.
3. **Forum `auth-worker` adopts `webauthn-rs` 0.5 or
   `passkey-types`.** **2 sprints.** Deletes ~700 LOC of
   ceremony code (F2.10, F2.14, F2.15). Brings forum into parity
   with `solid-pod-rs-idp`'s mature passkey implementation. Test
   surface inheritance from FIDO conformance vectors.
4. **solid-pod-rs-activitypub HTTP Sigs → `http-signature-normalization-actix`.**
   **1-2 sprints + interop testing window.** Saves 400 LOC, transfers test
   burden to a fediverse-tested crate. Risk concentration.
5. **Cross-substrate URI minting consolidation (PRD-006 §8).** Establish
   a `dreamlab-uri` crate consumed by both `urn:visionclaw:*` and
   `urn:agentbox:*` minters. Not strictly crypto, but converges the
   bech32/hex-pubkey/content-hash convention currently duplicated across
   `src/uri/` (Rust) and `management-api/lib/uris.js` (Node).

---

## 9. Cross-system convergence opportunities

The four substrates have **three** identity-vocabularies (hex pubkey, bech32
npub, DID:Nostr) and parallel content-addressing conventions. A unified
absorption strategy clusters them into shared crates:

| Cluster | Members | Single upstream | Migration target |
|---------|---------|----------------|--------------------|
| **Nostr core** | forum, VC, solid-pod-rs | `nostr = 0.44` | ADR-076 |
| **Solid pod core** | forum (pod-worker), VC (handler), solid-pod-rs (ref) | `solid-pod-rs-core` (no_std) | needs WASM extraction |
| **WebAuthn ceremony** | forum, solid-pod-rs-idp | `webauthn-rs = 0.5` or `passkey-types` | F2.10 + F4.6 |
| **JSON-LD canonicalisation** | agentbox (one impl) | `npm canonicalize` (RFC 8785) | F3.7 |
| **DID:Nostr emission** | forum, solid-pod-rs-nostr, agentbox (twice!) | `solid-pod-rs-nostr::did` | F2.6, F3.3, F3.8, F4.2 |
| **NIP-98 verify** | forum, VC (gen), solid-pod-rs, agentbox | `nostr::nips::nip98` everywhere; replay-store project glue | F1.1, F2.2, F3.4, F4.1 |
| **bech32** | forum (NIP-19), agentbox (Python) | `bech32` crate / `bech32` PyPI | F3.1 |
| **HTTP Signatures (AP)** | solid-pod-rs-activitypub | `http-signature-normalization-actix` | F4.4 |

The **single biggest convergence win** is `solid-pod-rs-core` (item 2 of
strategic absorptions): one no_std crate exporting WAC + LDP + DID:Nostr +
WebID, consumed by forum's pod-worker (CF Workers WASM) and by
VisionClaw's solid_pod_handler. That single move kills ~3,500 LOC of
hand-roll across two substrates and establishes solid-pod-rs as the
canonical implementation, closing the door on future drift.

---

## 10. Findings registry (cross-reference)

Every numbered finding maps to a file:line citation:

| ID | File | LOC | Class |
|----|------|-----|-------|
| F1.1 | `src/utils/nip98.rs:1-636` | 636 | HIGH |
| F1.2 | `src/services/nostr_identity_verifier.rs:1-92` | 92 | OK |
| F1.3 | `src/utils/opaque_id.rs:1-282` | 282 | OK |
| F1.4 | `src/utils/canonical_iri.rs:1-161` | 161 | OK |
| F1.5 | `src/uri/parse.rs:1-291` + `legacy.rs:1-92` | 383 | OK |
| F1.6 | `src/services/server_identity.rs:1-400` | 400 | OK |
| F1.7 | `src/services/nostr_bridge.rs:1-487` | 487 | MED |
| F1.8 | `src/services/nostr_service.rs:1-668` + `nostr_bead_publisher.rs:1-423` | 1,091 | OK |
| F1.9 | `src/handlers/solid_pod_handler.rs:16` | n/a | OK |
| F2.1 | `nostr-core/src/nip44.rs:122-128` (within 549 LOC module) | 549 | CRITICAL |
| F2.2 | `nostr-core/src/nip98.rs:1-1075` | 1,075 | HIGH |
| F2.3 | `nostr-core/src/nip04.rs:59-79` + `nip44.rs:104-119` | 60 | MED |
| F2.4 | `nostr-core/src/keys.rs:60-70` | 11 | HIGH |
| F2.5 | `pod-worker/src/acl.rs:1-821` | 821 | HIGH |
| F2.6 | `pod-worker/src/did.rs:1-315` | 315 | MED |
| F2.7 | `pod-worker/src/webid.rs:1-86` | 86 | MED |
| F2.8 | `pod-worker/src/{lib,container,patch,content_negotiation,conditional,quota,provision,notifications}.rs` | 2,942 | HIGH |
| F2.9 | `relay-worker/src/relay_do/nip_handlers.rs:1-762` | 762 | OK |
| F2.10 | `auth-worker/src/webauthn.rs:1-904` | 904 | CRITICAL |
| F2.11 | `auth-worker/src/crypto.rs:1-222` | 222 | OK |
| F2.12 | `auth-worker/src/delegation.rs:1-288` | 288 | MED |
| F2.13 | `forum-client/src/auth/nip98.rs:1-334` | 334 | MED |
| F2.14 | `forum-client/src/auth/passkey.rs:1-480` | 480 | MED |
| F2.15 | `forum-client/src/auth/webauthn.rs:1-312` | 312 | MED |
| F2.16 | `forum-client/src/auth/nip07.rs:1-293` | 293 | MED |
| F2.17 | `forum-client/src/dm/mod.rs:1-650` | 650 | MED |
| F2.18 | `pod-worker/src/contexts.rs:1-80` | 80 | OK |
| F2.19 | `relay-worker/{whitelist,cron,auth,nip11}.rs` | 1,154 | OK |
| F2.20 | `preview-worker/src/ssrf.rs:1-390` | 390 | OK |
| F3.1 | `scripts/sovereign-bootstrap.py:13-68` | 56 | CRITICAL |
| F3.2 | `scripts/sovereign-bootstrap.py:81-92, 123-135` | 25 | CRITICAL |
| F3.3 | `scripts/sovereign-bootstrap.py:192` | 1 | CRITICAL |
| F3.4 | `mcp/servers/nostr-bridge.js:321-383` | 63 | HIGH |
| F3.5 | `management-api/middleware/auth.js:1-150` | 150 | MED |
| F3.6 | `management-api/lib/uris.js:1-286` | 286 | OK |
| F3.7 | `management-api/middleware/linked-data/jcs.js:1-166` | 166 | HIGH |
| F3.8 | `management-api/middleware/linked-data/surfaces/s04-did.js:71` | 1 | **CRITICAL — NEW** |
| F3.9 | linked-data encoder + 11 surfaces | 2,028 | OK |
| F3.10 | `mcp/servers/nostr-bridge.js:431-475` | 45 | OK |
| F3.11 | `mcp/nostr-bridge/relay-consumer.js:320-335` | 16 | OK |
| F4.1 | `solid-pod-rs/src/auth/nip98.rs:1-484` | 484 | HIGH |
| F4.2 | `solid-pod-rs-nostr/src/did.rs` | ~315 | MED |
| F4.3 | `solid-pod-rs/src/wac/*` | 2,500 | OK (IS UPSTREAM) |
| F4.4 | `solid-pod-rs-activitypub/src/http_sig.rs:1-563` | 563 | HIGH |
| F4.5 | `solid-pod-rs/src/oidc/jwks.rs:1-459` | 459 | OK |
| F4.6 | `solid-pod-rs-idp/src/passkey.rs:1-507` etc | 507 | OK |
| F4.7 | `solid-pod-rs-didkey/src/{verifier,jwt,pubkey,did}.rs` | ~767 | OK |
| F4.8 | `solid-pod-rs/src/security/{ssrf,dotfile,paths}.rs` | 1,236 | OK |

**49 distinct findings**, **15 absorbable for non-trivial wins**, **4 fresh
CRITICAL items** (one entirely new — F3.8 verificationMethod.type fourth
divergence in agentbox's JSON-LD surface).

---

## 11. Closing observations

1. **VisionClaw is the cleanest of the four** — nine findings, only one
   HAND-ROLL HIGH (F1.1) and one MED gap (F1.7 delegation). All other
   surfaces correctly delegate to `nostr_sdk` or `solid-pod-rs`.
2. **Forum carries the largest absorption debt** — ~13,500 LOC of which
   ~10,500 is removable across ADR-076 + WASM-core extraction.
   ADR-076 already handles ~7,200 of those; this audit identifies a
   further ~3,300 LOC absorbable beyond ADR-076 if the WASM-extraction
   pattern lands.
3. **Agentbox is the smallest substrate but carries the most CRITICAL
   findings (4)** because three of them are the C1/C2/C3 set that
   05-gotchas already raised — and F3.8 reveals that **agentbox itself
   has internal divergence**: the Python bootstrap and the JSON-LD S4
   surface emit different `verificationMethod.type` values for the
   same agent.
4. **solid-pod-rs is the canonical Rust implementation** for WAC/LDP/DID:Nostr
   and **already** is the upstream for VisionClaw. The work for the
   ecosystem is to extract no_std-compatible cores and have forum's
   pod-worker consume them.
5. **The NIP-98 quadruple duplication is the loudest signal in the audit**
   — four implementations of a 27235-event verifier, with five subtle
   divergences (URL-trailing-slash, max-event-size cap, replay store,
   Schnorr-verify-via-different-libraries, payload-hash-required-when-body).
   None of the divergences are spec-mandated; they are accidental.
   Convergence on `nostr::nips::nip98::HttpData` (Rust) +
   `nostr-tools.verifyEvent` (Node) + a project-side replay store trait
   would eliminate ~1,650 LOC of test surface across the ecosystem.

The lens this audit applies — **"where do we hand-roll a protocol
when an established library exists?"** — finds two distinct failure
modes:

- **Type A: hand-rolled protocol code that should be a library call.**
  The forum `nostr-core` situation (ADR-076), the agentbox bech32 +
  JCS situations. Pure absorption.
- **Type B: hand-rolled fork of a project-internal library.** The
  forum pod-worker forking solid-pod-rs to reach a WASM target. Not a
  library-quality issue — a packaging issue. The fix is to extract
  no_std cores from solid-pod-rs, not to find a different upstream.

Both classes are addressable; neither is intrinsically harder than the
other. The order of operations — ADR-076 first (closes C1 + 6,500 LOC),
then the no_std core extraction (closes another 3,500 LOC), then the
WebAuthn unification (closes another 700 LOC), then the AP HTTP-Sig
absorption (closes 400 LOC + interop test debt) — gives ~11,100 LOC
of strategic deletion across **5 sprints**, plus ~3.4 days of P0/P3
quick wins that close 3 CRITICAL findings before any of the sprints
begin.
