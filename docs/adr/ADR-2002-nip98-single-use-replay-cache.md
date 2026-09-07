---
id: ADR-2002
title: NIP-98 replay protection is a two-layer scheme with a single-use cache
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 1ad881cab5ed786fc112f6e50db03fd587e23ec0
verified_paths: [src/utils/nip98.rs, docs/SECURITY-profiles.md]
owner: jjohare
review_trigger: horizontal scaling of the backend (replicas/load balancer), or any change to TOKEN_MAX_AGE_SECONDS
repo: visionclaw
---

# ADR-2002 — NIP-98 replay protection is a two-layer scheme with a single-use cache

## Context

NIP-98 validation enforced only a ±60 s freshness window plus method/URL binding.
A captured signed mutation could be replayed freely for its whole validity window;
the XR client's "single-use NIP-98" comment was aspirational. Both 2026-08-31
external reviews flagged this; the sense-check swarm confirmed no event-id cache
existed anywhere in `src/`.

## Decision

Replay resistance is the freshness window **plus** a process-wide single-use
event-id cache (`src/utils/nip98.rs`). The id is claimed atomically under one
lock only **after** full validation, so failed signatures/URL/method/freshness
checks cannot burn a legitimate id. Cache bookkeeping uses monotonic `Instant`
(TTL 2× the window); a hard cap of 100 000 live entries **fails closed** via
`ReplayCacheFull` — we never evict a live entry, because eviction would hand a
flooder a replay primitive. The cache is **process-local by design**: replay
protection does not span replicas.

## Consequences

- Every NIP-98 call site inherits replay protection automatically because all
  validation funnels through `validate_nip98_token()`.
- Under a sustained valid-signature flood the server rejects new auth
  (availability cost) rather than growing memory or permitting replay.
- Horizontal scaling requires shared state (e.g. Redis) or sticky routing
  before the single-process invariant can be relaxed — see the review trigger.
- `docs/SECURITY-profiles.md` invariant 4 records the scheme; weakening either
  layer re-opens replay-within-60s.

## Verification

`cargo test --lib nip98` → 26 passed at `e78e958fa`, including barrier-race
atomicity (16 threads, exactly one winner), cap-boundary fail-closed on an
isolated map, expired-entry reclaim, and non-burning of ids on failed
signature/URL/method/freshness. Two codex adversarial rounds (round 1 BLOCK →
fixed; round 2 concerns closed by the cap-boundary test + prune-clamp fix).

## Closeout extension — 2026-09-04

CP-04/05/08. Owner remains jjohare with authentication/runtime maintainers. Current helper source preserves atomic claim after validation, bounded capacity and process-local state. Six isolated claim assertions pass, including no live eviction and exact-TTL reuse. Historical complete/live declarations remain scoped to the cache decision; no deployed or full-signature re-certification is implied.

**Acceptance condition:** Test combined wall-clock freshness and monotonic expiry at exact boundaries and clock steps; retain explicit restart/replica policy. Define route-specific body binding because the validator compares payload only when both supplied body and tag exist. Trace token consumption through user resolution, authorisation and mutation failure; specify fresh-token retries and application idempotency after response loss. Verify capacity failure handling without opening an authentication fallback. Reopen on window/TTL, scaling, route body handling, retry or cache lifecycle changes.

See the [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/runtime-ingress.md#visionclaw-replay-and-operation-boundaries), [reproducer](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/replay-cache-probe.py) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/replay-cache-probe.json). The extracted helper test does not exercise signatures, mutex concurrency, HTTP mutation or a deployed service; historical tests remain separately identified.

## Acceptance progress — 2026-09-05

**Implemented.** `src/utils/nip98.rs`. (1) Route-specific body binding is now a
declared policy: `BodyBinding::{NoBody, Required, RequiredWhenTagged}` plus
`validate_nip98_token_bound(...)`. The reproduced gap — the validator compared
the payload hash *only when both a supplied body and a `payload` tag existed*,
so a token minted without the tag authenticated any body — is closed under
`Required`, which rejects a body with no tag (`PayloadHashMissing`) and a tag
with no body (`PayloadHashMismatch`); `NoBody` rejects a stray tag
(`UnexpectedPayloadHash`). `validate_nip98_token` keeps the legacy
`RequiredWhenTagged` semantics so the three existing callers are unchanged;
`BodyBinding::default()` is `Required`, so a newly written route is strict.
(2) Exact-boundary freshness/expiry: the TTL is half-open (replay at
`TTL - 1ns`, claimable at exactly `TTL`), a re-claim restarts the TTL, a
backward monotonic step cannot expire an entry, and `REPLAY_CACHE_TTL` is
asserted equal to `2 x TOKEN_MAX_AGE_SECONDS`. (3) Capacity: the last slot is
usable, the next fails closed with `ReplayCacheFull`, a full cache still detects
a replay, no live entry is evicted, and expiry frees capacity. A binding
rejection happens before the claim, so it does not burn the event id.

**Tests.** `cargo test --lib --no-default-features nip98` — 15 new cases, all
pass (whole-crate run: 1254 passed, 0 failed).

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2002-2009-nip98.txt`.

**Remains open.** Restart/replica policy is unchanged (process-local cache;
horizontal scaling still needs shared storage or sticky routing). Token
consumption through user resolution, authorisation and mutation failure, and
application idempotency after response loss, are not exercised — those need a
running server. No route has yet been migrated to `BodyBinding::Required`; that
is a per-route decision requiring the deployed client to mint payload tags.

## Re-verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8


**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; **no production logic changed**. Verified,
not assumed: comparing every changed file with all whitespace stripped leaves
only rustfmt artefacts — struct-literal reflow, import/module reordering and
added trailing commas. The largest single case,
`src/models/simulation_params.rs` (+303/-70 raw), is the `SIMPARAMS_MANIFEST`
literal reflowed one-field-per-line: its field names and byte offsets hash
identically on both sides. Citations below are
therefore re-derived line numbers over unchanged code, not new findings.

**Governed changes since `f326a3b11`:** `src/utils/nip98.rs` (+383/-3, the
`BodyBinding` work recorded above) and `docs/SECURITY-profiles.md` (+95, the
2026-09-05 remediation section). Both are additive; neither weakens a layer.

**Both layers still hold at HEAD.** The freshness window is
`TOKEN_MAX_AGE_SECONDS = 60` (`nip98.rs:169`) checked symmetrically at
`:423`/`:428`. The single-use claim is still the *last* step of validation:
signature `verify()` at `:511-512`, then `claim_event_id(&nip98_event.id,
Instant::now())` at `:520` — so a failed signature, URL, method, freshness or
body-binding check cannot burn a legitimate event id. `claim_in`
(`:245-277`) still runs check→prune→cap→insert under one lock; the cap is
`REPLAY_CACHE_MAX_ENTRIES = 100_000` (`:198`) and returns
`Nip98ValidationError::ReplayCacheFull` at `:272` rather than evicting a live
entry. `REPLAY_CACHE_TTL` is still `2 × TOKEN_MAX_AGE_SECONDS` (`:178`). The
cache remains a process-global `HashMap` behind one mutex — process-local by
design, so the horizontal-scaling review trigger is unchanged.

`docs/SECURITY-profiles.md` invariant 4 (`:193-200`) still records both layers,
the process-local scope and the fail-closed ceiling. No citation in this record
was found wrong.

**Commands run:** `git diff --stat f326a3b11..HEAD -- src/utils/nip98.rs
docs/SECURITY-profiles.md`; `grep -n` over `nip98.rs` for `BodyBinding`,
`claim_event_id`, `REPLAY_CACHE_*`, `TOKEN_MAX_AGE_SECONDS`, `verify()`;
`sed -n '230,285p' src/utils/nip98.rs`; `sed -n '187,215p'
docs/SECURITY-profiles.md`.

**Test receipt at this commit:** `cargo test --lib --no-default-features nip98` →
**39 passed, 0 failed** (1228 filtered out), including
`test_invalid_signature_does_not_burn_id`, `test_claim_event_id_atomic_under_concurrency`,
`ttl_boundary_is_exact_and_half_open`, `ttl_covers_the_entire_freshness_window` and
`the_default_policy_is_strict_and_legacy_callers_are_unchanged`.

## Landing re-verification — 2026-09-06 (2cf222406)

Governed paths changed in the Wave 3 landing commit: docs/SECURITY-profiles.md: invariant 6 citation re-pinned to `.bind()` at main.rs:1177 by the ledger lead; src/utils/nip98.rs is token-identical across the range. Decision unaffected; `verified_commit` moved to the landing commit. Gates at that commit: cargo check --workspace --all-targets exit 0, 827 crate + 1600 root + 309 xr-client tests, vitest 809, fmt and lint clean.

## Landing re-verification — 2026-09-06 (c9734a524)

Governed paths changed in the doc-sync commit: docs/SECURITY-profiles.md — frontmatter `version`/`verified_commit` bump and changelog entry for the Remediation — 2026-09-05 section, plus narrative corrections; no code or citation this record depends on changed. `verified_commit` moved to the doc-sync commit.


## Source closeout verification — 2026-09-07

`admit_signed_in` now applies the per-pubkey quota after cryptographic verification and before the global claim under one mutex. Existing single-use TTL/capacity invariants remain; the signed WebSocket carrier enters the same validator. Fairness/expiry, replay and URL-binding tests pass. This remains process-local, not cross-replica replay protection.

Verified implementation: `1ad881cab5ed786fc112f6e50db03fd587e23ec0`. Evidence: [VisionClaw execution report](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/closeout/2026-09-07-execution-visionclaw.md). The embedded-pod library suite passed 1,364 tests (six ignored); a subsequent focused three-test handshake suite also passes. Source verification does not assert deployment activation. Earlier dated observations remain historical.
