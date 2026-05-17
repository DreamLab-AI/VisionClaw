# ADR-078 — Cross-Substrate Library Convergence

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-010 G10 + F25–F30 (absorption batches); PRD-011 G10 |
| Companion ADRs | ADR-073, ADR-074, ADR-075, ADR-076, ADR-077 |
| Supersedes | — |
| Affected repos | All four — VisionClaw, dreamlab forum, agentbox, solid-pod-rs |

## Context

ADR-076 absorbs forum's `nostr-core` (~7,900 LOC) into upstream `nostr` crate. The QE fleet audit (`Q1-crypto-protocol-audit.md`, 49 findings) extended this analysis across the whole ecosystem and identified **six additional substantial absorption opportunities** plus a structural convergence pattern.

Total absorption potential per Q1 §0 executive summary: **~11,100 LOC strategic deletion across 5 sprints** if all opportunities pursued. The largest individual ones:

| Surface | LOC hand-rolled | Established library | Locations |
|---------|-----------------|---------------------|-----------|
| Forum nostr-core (ADR-076) | 6,500 | `nostr` crate | dreamlab-ai-website |
| Forum pod-worker LDP/WAC/DID | 3,329 | extracted `solid-pod-rs-{wac,ldp,did,nostr}-core` | dreamlab-ai-website |
| Forum WebAuthn auth-worker + forum-client | 904 | `webauthn-rs = "0.5"` + `passkey-types = "0.3"` | dreamlab-ai-website (already in workspace Cargo.toml, never imported) |
| NIP-98 quadruple duplication | 2,408 (1,075+484+636+213) | `nostr::nips::nip98` (Rust) / `nostr-tools` (JS) | all four substrates |
| Agentbox JCS hand-roll | 161 | `canonicalize` (npm) | agentbox |
| Agentbox bech32 hand-roll | ~60 | `bech32` PyPI | agentbox |
| ActivityPub HTTP Signatures | TBD | `http-signature-rs` | solid-pod-rs |

The pattern: each substrate optimised for its own runtime constraints (CF Workers WASM, Nix Python, actix-web tokio) and forked instead of upstreaming the WASM-/runtime-compatibility fixes. Result: drift, duplicated maintenance, repeatable bugs.

ADR-076 sets the precedent: **absorb upstream, validate compatibility via gating spike, ship as thin shim**. This ADR generalises that pattern across every absorbable surface in the ecosystem and codifies the policy of "library first, fork only with ADR".

## Decision

### D1 — Library convergence registry

Maintain a registry at `docs/ecosystem-libraries.md` listing every protocol primitive used in the ecosystem and its canonical upstream library. The registry is the source of truth for ADR-077 P3 (library convergence policy).

Initial registry (extracted from QE Q1):

```
nostr_protocol_rust:    nostr = "0.44"           # rust-nostr.org
nostr_protocol_js:      nostr-tools = "^2.10.4"  # paulmillr
nostr_protocol_python:  pynostr = "^0.7"         # opt-in for Python paths
bech32_rust:            bech32 = "0.11"          # RustCrypto
bech32_python:          bech32 = "1.2"           # PyPI
webauthn_rust:          webauthn-rs = "0.5"      # plus passkey-types = "0.3"
webauthn_js:            @simplewebauthn/browser  # client side
http_sig_rust:          http-signature-rs        # for ActivityPub
jcs_rust:               serde_jcs = "0.1"        # RFC 8785
jcs_js:                 canonicalize = "^2.0"    # npm
solid_pod_workspace:    solid-pod-rs (DreamLab-owned, federated)
schnorr_secp256k1:      secp256k1 = "0.29"   /  k256 = "0.13"
chacha20_poly1305:      chacha20poly1305 = "0.10"
hkdf_hmac:              hkdf = "0.12" + hmac = "0.12" (RustCrypto)
sha2:                   sha2 = "0.10"
multibase:              multibase = "0.9"
```

The registry is updated whenever a new primitive is identified or an upstream version pin moves. Updates require ADR amendment.

### D2 — Forum absorption set

Per ADR-076 D6 ordering for nostr-core, plus **four additional absorption batches** that this ADR scopes:

**Batch B1 — `nostr-core` → `nostr` crate** (per ADR-076; deletes 6,500 LOC)

**Batch B2 — `pod-worker` → extracted `solid-pod-rs-*-core` no_std crates** (3,329 LOC deletion; depends on B5 below)

The forum's `pod-worker/src/{acl,did,webid,provision,quota,notifications,patch,container,content_negotiation,conditional}.rs` reimplements ~2,300 LOC of `solid-pod-rs` because the upstream `Storage` trait is Tokio-coupled (per `04-solid-pod-rs-surfaces.md` §2 + Q1 F2.5–F2.8). Batch B2 depends on B5 (solid-pod-rs 0.5 with `KvBackend` + `MaybeSend` futures).

Until B5 ships, B2 cannot proceed — pod-worker continues its Cloudflare-Workers-flavoured fork.

**Batch B3 — Forum WebAuthn → `webauthn-rs` 0.5 + `passkey-types` 0.3** (904 LOC deletion)

`auth-worker/src/webauthn.rs`, `forum-client/src/auth/{passkey,webauthn}.rs`. The dependencies are **already declared in the forum workspace `Cargo.toml:35`** but never imported (Q1 F2.10). Solid-pod-rs-idp already uses `webauthn-rs` correctly per Q1 F2.10. Migration delegates the WebAuthn ceremony machinery to the published crate, retaining DreamLab-specific PRF salt management + `nostr::Keys` derivation.

**Batch B4 — Forum NIP-98 → `nostr::nips::nip98`** (per ADR-076; deletes 1,075 LOC; retains `Nip98ReplayStore` trait + `KvReplayStore` impl)

**Batch B5 — solid-pod-rs 0.5 with `KvBackend` + `MaybeSend` futures** (foundation for B2; ~3-4 sprints owner-side)

Per `04-solid-pod-rs-surfaces.md` §15 capability gaps, solid-pod-rs's `Storage` trait must grow:
- A `KvBackend` companion trait below `Storage` (CF KV / R2 implementable)
- `MaybeSend` futures (cfg-gated `Send + ?Send`) for WASM Workers compatibility
- A `WireSink + WireStream` runtime-agnostic transport pair behind the current Tokio `serve_relay_ws` pump

This is a solid-pod-rs 0.5 effort, not a downstream-substrate effort; tracked in solid-pod-rs's own ADR set. PRD-010 P5 awaits this; B2 unblocks at that point.

### D3 — VisionClaw absorption set

**Batch V1 — Delete `src/utils/nip98.rs`** (636 LOC; addresses Q1 §F1.1 NIP-98 quadruple duplication identified in `docs/integration-research/qe-fleet/Q1-crypto-protocol-audit.md`; aligned with PRD-010 F25 absorption pattern)

Replace with `nostr_sdk::nips::nip98` for token generation + `solid_pod_rs::auth::nip98::verify` for verification (already used in `solid_pod_handler.rs:16`). The duplicate hand-roll has no replay store — Q2 S-HIGH-4 — fixed by deletion.

**Batch V2 — Add `nostr` workspace dep** (PRD-010 F29; type-flow alignment with forum + agentbox)

`Cargo.toml`: `nostr = { workspace = true, version = "0.44" }`. Already transitively present via `nostr_sdk`; explicit dep enables direct `nostr::Event` types across the BC20 boundary without translation.

**Batch V3 — Identity emitter** (PRD-010 F2 + F4 + F15; new code at canonical type from inception)

New `src/handlers/identity_did_handler.rs` emits `SchnorrSecp256k1VerificationKey2019` from inception; mounted at `/api/v1/identity/{hex}/did.json` per resolver contract.

### D4 — Agentbox absorption set

**Batch A1 — `scripts/sovereign-bootstrap.py` bech32 hand-roll → `bech32` PyPI** (~60 LOC deletion; PRD-010 F5 + Q1 F3.1)

Replace `_bech32_polymod`, `_bech32_hrp_expand`, `_bech32_create_checksum`, `_convertbits`, `bech32_encode`, `bech32_decode` with `bech32` package. Same PR fixes C2 (use BIP-340 x-only 32-byte form) since the bug is at the caller, not the encoder.

**Batch A2 — `mcp/servers/nostr-bridge.js::verifyNip98` (91 LOC) → `nostr-tools.verifyAuth`** (Q1 F3.4)

`nostr-tools` 2.10+ exposes a NIP-98 verifier; if it doesn't, file a PR upstream and fall back to a thin (~30 LOC) wrapper. Either way: delete the 91 LOC of pre-Schnorr validation reimplementation.

**Batch A3 — `management-api/middleware/linked-data/jcs.js` (~161 LOC) → `canonicalize` npm package** (Q1 F3.7)

RFC 8785 is subtle (key sort order, number formatting, escape sequences); a well-tested upstream is much safer.

**Batch A4 — Fix C4 (the fourth `verificationMethod.type` drift)**

`management-api/middleware/linked-data/surfaces/s04-did.js:71`: change `SchnorrSecp256k1VerificationKey2025` to `SchnorrSecp256k1VerificationKey2019`. Plus reconcile with `scripts/sovereign-bootstrap.py:192` (also wrong) so agentbox emits ONE consistent type from both Python and JS sides.

**Batch A5 — At-rest key encryption** (Q1 F3.10 + Q3 CRITICAL-03)

`agentbox/mcp/servers/nostr-bridge.js:439-475 loadSigner` reads `nostr.key.enc` (AES-256-GCM); the file is **never created** by `sovereign-bootstrap.py`. Two fixes (operator chooses):
- (a) Add encryption-at-rest writer to bootstrap; protect with `os.chmod(0o600)` + key-encryption-key from env or HSM.
- (b) Document at-rest plaintext as accepted risk in standalone deployments; require HSM/KMS in federated deployments.

**Batch A6 — `agentbox.sh rotate-keys`** (Q3 G6)

Documented in `agentbox/docs/reference/prd/PRD-001-capabilities-and-adapters.md:379` but not implemented. Implements key rotation per ADR-074 D12 transition window.

### D5 — solid-pod-rs absorption set (upstream maintenance)

**Batch S1 — solid-pod-rs 0.5 with `KvBackend` + `MaybeSend`** (per D2 B5)

**Batch S2 — Fix `verificationMethod.type` drift** (Q1 F4.2 + ADR-074 H4)

`crates/solid-pod-rs-nostr/src/did.rs:98, 154`: change `NostrSchnorrKey2024` to `SchnorrSecp256k1VerificationKey2019`. Add `https://w3id.org/security/suites/secp256k1-2019/v1` to Tier-1 `@context` (line 93).

**Batch S3 — `Nip98ReplayStore` trait** (Q1 F4.1; PRD-010 F20)

Add `solid_pod_rs::auth::Nip98ReplayStore` trait — mirroring forum's `nostr-core::Nip98ReplayStore` (1075 LOC NIP-98 module already provides this). Wire into `solid_pod_rs::auth::nip98::verify` as an optional parameter.

**Batch S4 — `solid-pod-rs-server::lib.rs:174-187` body_hash fix** (Q2 S-HIGH-5)

Pass `body_hash = Some(...)` from request body, not `None`. Restores payload-tag verification.

**Batch S5 — `solid-pod-rs-idp/src/jwks.rs SigningKey Zeroize` derive** (Q3 G7)

Add `#[derive(Zeroize, ZeroizeOnDrop)]` to `SigningKey`. Fixes private_pem/private_der heap-leak risk.

**Batch S6 — DID-via-relay resolver** (PRD-010 F14)

`crates/solid-pod-rs-nostr/src/resolver.rs::NostrWebIdResolver::resolve_via_relay(pubkey, relay_url) -> Result<Option<DidDocument>>`. Implements DID-via-relay path per ADR-074 D5.

**Batch S7 — Cross-runtime traits + no_std core extraction** (foundation for forum B2)

Extract pure-protocol-logic from solid-pod-rs into no_std crates:
- `solid-pod-rs-wac-core` (WAC evaluator)
- `solid-pod-rs-ldp-core` (LDP method dispatch)
- `solid-pod-rs-did-core` (DID Document renderer)
- `solid-pod-rs-nostr-core` (NIP wire format types)

Each crate is `no_std`, async-runtime-agnostic, WASM-compatible. Forum's pod-worker imports the cores; agentbox's solid-pod-rs-server pulls them transitively. Single-source-of-truth for the ecosystem.

### D6 — Phasing into PRD-010

Maps to PRD-010 phases per cost / dependency / risk:

| Batch | Phase | Cost (e-days) | Depends on |
|-------|-------|---------------|------------|
| B1 (nostr-core absorption) | P0 | 10 | F26 spike |
| V1 (VC NIP-98 deletion) | P0 | 1 | nostr_sdk dep |
| V2 (VC nostr workspace dep) | P0 | 0.5 | none |
| A1 (agentbox bech32 → PyPI) | P0 | 0.5 | none |
| A4 (agentbox C4 fix) | P0 | 0.5 | none |
| S2 (solid-pod-rs C3 fix) | P0 | 0.5 | none |
| S5 (jwks Zeroize) | P0 | 0.25 | none |
| **P0 subtotal** | | **13.25 e-days** | |
| V3 (VC identity emitter) | P1 | 2 | V1 |
| A2 (agentbox NIP-98 → nostr-tools) | P2 | 1 | A1 |
| A3 (agentbox JCS → canonicalize) | P2 | 1 | none |
| A5 (at-rest key encryption) | P2 | 2 | A1 |
| A6 (rotate-keys) | P3 | 3 | A5 |
| S3 (Nip98ReplayStore trait) | P3 | 2 | none |
| S4 (body_hash fix) | P0 | 0.5 | none — fold into P0 |
| S6 (DID-via-relay) | P3 | 3 | S2 |
| B3 (forum WebAuthn) | P3 | 4 | dep already declared |
| B4 (forum NIP-98) | P0 | 1 | B1 |
| **P1-P3 subtotal** | | **18.5 e-days** | |
| S1+S7 (solid-pod-rs 0.5 KvBackend + cores) | P5 | 30-40 (upstream) | — |
| B2 (forum pod-worker absorption) | P6 (post-PRD-010) | 15 | S1+S7 |
| **P4-P6 + post subtotal** | | **45-55 e-days** | |

**Total ecosystem absorption: ~76-86 engineer-days.** Roughly 4-6 engineer-sprints. Most of P0+P1-3 (~32 e-days) lands inside PRD-010's existing 5-6 sprint envelope; P4+P5+B2 extends slightly.

### D7 — Cross-substrate type sharing

Once D2 (B1) + D3 (V2) land, the same `nostr::Event`, `nostr::Filter`, `nostr::EventBuilder` types flow across:
- Forum's `nostr-core` shim (re-exports)
- VisionClaw's substrate code (`nostr` workspace dep + `nostr_sdk`)
- BC20 anti-corruption layer (PRD-006 §5.5) — translates URN forms but not event types

Agentbox JS side stays on `nostr-tools` (paulmillr); wire-compatible with `nostr` Rust by spec, validated by Q5 cross-language fixtures.

### D8 — License audit

All the absorbed-into libraries are MIT or Apache-2.0:
- `nostr` crate: MIT
- `nostr-tools`: MIT
- `bech32`: MIT
- `webauthn-rs`: MPL-2.0
- `passkey-types`: MIT/Apache-2.0
- `http-signature-rs`: MIT/Apache-2.0
- `canonicalize` (npm): Apache-2.0
- `serde_jcs`: MIT/Apache-2.0

solid-pod-rs is AGPL-3.0-only (DreamLab-owned, no third-party AGPL exposure to consumers). All compatible with the MIT-licensed forum and visionclaw repos under the AGPL-aware library-vs-aggregate analysis at `solid-pod-rs/docs/developer/licensing.md`.

CI license audit (ADR-077 P8.3) prevents regression.

## Consequences

### Positive

- **~11,100 LOC of ecosystem-wide deletion** when fully completed. Each absorption deletes self-rolled drift surface.
- **C1-class bugs (HKDF mis-mapping) cannot recur** in absorbed modules — they delegate to community-maintained, vector-validated implementations.
- **Cross-substrate type symmetry**: forum + VisionClaw share `nostr::Event` directly; agentbox via wire compat with nostr-tools. BC20 ACL becomes simpler.
- **Operator confidence**: every absorbed surface inherits its upstream's test ecosystem and CVE response cadence.
- **Reduced maintenance cost**: every NIP fix or NIP evolution arrives via `cargo update` / `npm update` instead of hand-port. Sprint v9-v11's 2-sprint NIP-04/NIP-44/NIP-26 hand-port effort would have been zero days post-absorption.

### Negative

- **6 months of multi-substrate migration work** (4-6 sprints). Real cost; phased to bound risk.
- **Upstream dependency exposure**: ecosystem now depends on 8 community-maintained libraries not under DreamLab control. Mitigation: pinned versions; explicit cargo update review process; security advisories monitored via dep-audit (P8.2).
- **Three substrates' WASM compat must be re-validated**: forum CF Workers, agentbox solid-pod-server (Nix), VisionClaw substrate (actix-web tokio). Validation spikes per ADR-076 D5 generalised to each absorption.
- **B2 (forum pod-worker absorption) is gated on solid-pod-rs 0.5 (B5/S1)**; that's a separate workstream with its own timeline. PRD-010 P0-P5 succeeds without B2; B2 is a follow-up sprint.

### Neutral

- **Project-specific shims remain**: each substrate keeps a small (~500-1000 LOC) shim layer for its specific concerns (forum: moderation kinds + Signer trait + WASM bridge; agentbox: federation worker + LDN bridge; VisionClaw: BC20 ACL + URN minting). Right balance of "use library" vs "encode project specifics".
- **Lockfile churn**: `Cargo.lock` and `package-lock.json` expand transitively. Acceptable.

## Alternatives Considered

### Alt-A — Absorb only nostr-core (ADR-076 alone)

Stop after ADR-076; leave V1, A1, A2, A3, A4, A5, B3, B4, S2, S3, S4, S5 hand-rolls in place.

*Rejected*: closes only 6,500 of 11,100 LOC. The same drift class persists in the un-absorbed surfaces — C4 (agentbox fourth DID type) is exactly this class of bug, in a non-nostr-core surface.

### Alt-B — Fork all libraries into a DreamLab monorepo

Pull `nostr`, `nostr-tools`, `bech32`, `webauthn-rs`, etc. into `solid-pod-rs/` workspace as forks; control versions explicitly.

*Rejected*: forks bitrot; we'd reintroduce maintenance burden. The right model is upstream-first with thin shims for substrate specifics.

### Alt-C — Wait for solid-pod-rs 0.5 to absorb everything via S7 cores

Skip individual absorption batches; concentrate effort on solid-pod-rs 0.5 with no_std cores; have all substrates consume those.

*Rejected*: solid-pod-rs 0.5 is a 30-40 e-day effort that doesn't address Nostr NIPs, WebAuthn, or JCS. Need both: solid-pod-rs cores for solid-flavoured surfaces; established libraries for everything else.

### Alt-D — Per-substrate piecemeal migration without cross-system coordination

Each substrate decides its own absorption schedule and library choices.

*Rejected*: defeats the convergence purpose. Cross-system contracts (ADR-077 P2) require shared library choices; piecemeal would re-introduce drift.

## Implementation notes

### Absorption PR template

Every absorption batch PR ships with:
1. **Validation spike outcome** (per ADR-076 D5 pattern) — confirms upstream library works on substrate's runtime target.
2. **Behaviour-preservation tests** — existing test suite must pass; adds reference vectors per ADR-077 P1.
3. **LOC delta report** — shows lines deleted vs added.
4. **Bundle size delta** — for CF Workers / WASM substrates.
5. **License audit pass** — `cargo deny check` / `npm audit` clean.
6. **Coverage report** — meets ADR-077 P6 thresholds.

### Migration order within each batch

Per ADR-076 D6 ordering pattern: small modules first, large modules last; behaviour-preserving at each step; CI gate per PR.

### Anti-drift CI on absorbed modules

Once a surface is absorbed, the corresponding hand-roll path is deleted; an anti-drift lint (ADR-077 P3) prevents reintroduction:
- `nip(\d+|9[0-9])\.rs` not allowed under `crates/nostr-core/src/` (post-B1)
- `webauthn.rs` not allowed under `auth-worker/src/` (post-B3)
- `verifyNip98` function not allowed in `mcp/` outside the thin shim (post-A2)
- `bech32_*` Python functions not allowed in `scripts/` (post-A1)

Each lint fires on CI; offending PR cannot merge.

### Cross-substrate lockfile coherence

When nostr crate version moves (e.g. 0.44 → 0.45), all four substrates upgrade together. Coordinated PRs across:
- `dreamlab-ai-website/community-forum-rs/Cargo.toml`
- `/home/devuser/workspace/project/Cargo.toml` (VisionClaw)
- `agentbox/management-api/package.json` (`nostr-tools` JS counterpart move)
- `solid-pod-rs/Cargo.toml`

Coordination via VisionClaw's monorepo (which holds the master spec); upgrade PRs land first in VisionClaw, mirrored to other repos within 1 sprint.

### Staged rollout

Operators of existing installations:
- Phase 0 (gating): in-place crypto patches + bech32/JCS swaps. Deployment rolls out within 1-2 sprints; existing forum DMs remain decryptable via legacy fallback (per ADR-076 R2).
- Phase 1-3: identity unification + AUTH wiring + bridge wiring. Operators flip mesh-mode flags; forum DMs interoperate with reference clients post-Phase 0 NIP-44 fix.
- Phase 4-5: envelope contract + federation smoke + mutation testing baseline.
- Post-PRD-010: solid-pod-rs 0.5 ships → forum pod-worker absorption (B2) → final cleanup.

### Documentation alignment

Each substrate's CLAUDE.md gains an **Ecosystem & Federation** section (PRD-010 F44; per-project docs task) cross-referencing this ADR + ADR-076 + ADR-077.

## References

- PRD-010 — DID:Nostr Mesh Federation, G10 + F25-F30 (absorption requirements; this ADR generalises the absorption pattern across ecosystem)
- ADR-073 — Mesh topology
- ADR-074 — DID:Nostr canonicalisation
- ADR-075 — IS-Envelope
- ADR-076 — Forum nostr-core absorption (precedent)
- ADR-077 — Ecosystem QE policy (the framework)
- `docs/integration-research/qe-fleet/Q1-crypto-protocol-audit.md` — full inventory
- `docs/integration-research/qe-fleet/Q2-security-primitive-audit.md` — security findings
- `docs/integration-research/qe-fleet/Q3-identity-custody-audit.md` — identity findings
- `docs/integration-research/04-solid-pod-rs-surfaces.md` §15 — capability gaps for B5/S1
- `docs/ecosystem-libraries.md` (NEW; D1 registry)
- ADR-053 — solid-pod-rs crate extraction (precedent for solid-pod-rs strategy)
