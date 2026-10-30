---
id: ADR-2043
title: Reject the full-disclosure flag pair at boot, whether or not a profile is declared
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: b00c28a0d766c8cf46cd00b100dab60ef2dd74a4
verified_paths: []
owner: jjohare
review_trigger: a new security flag whose combination with another flag can widen disclosure, or the removal of PUBKEY_VISIBILITY_FILTER; activates on the first production-artefact boot (CY-C prod-profile bring-up on the Trust host) whose boot receipt shows the assertion ran
repo: visionclaw
domain: SECURITY-profiles
lineage: ADR-2003 (visibility filter default on) named the combination; ADR-2027 named the profiles; ADR-2038 built the boot-time assertion this rule lives in.
---

# ADR-2043 — Reject the full-disclosure flag pair at boot, whether or not a profile is declared

## Context

`RBAC_PUBLIC_READS=1` serves every `/api` read to unauthenticated callers.
`PUBKEY_VISIBILITY_FILTER=0` puts private nodes on the wire unredacted. Either
alone is a supported posture; together they publish every node of every user to
anyone who can reach the port. `docs/SECURITY-profiles.md`'s "Illegal
combinations" table listed the pair as **"Not machine-enforced — operator must
never combine"**, the only honour-system row in a table whose every other row has
real enforcement. Phase 1 diagram VC-09.6 showed why the ADR-2038 assertion did
not already catch it: the pair matches no ratified profile, so a deployment that
*declared* one raised `ProfileDrift` while a deployment that declared nothing was
classified `Unnamed` and bound anyway — the careless deployment was the one that
got through, which is backwards relative to the risk.

## Decision

`evaluate_effective_profile` raises `ProfileFinding::FullDisclosureFlagPair`
whenever anonymous reads are enabled **and** the visibility filter is disabled.
The rule is keyed on the pair itself and is evaluated unconditionally — it does
not depend on `VISIONCLAW_SECURITY_PROFILE` being set, on the observed flags
matching a named profile, or on any other finding. Like every other finding it is
fatal in a production artefact (exit 2, before the listener binds) and reported
without stopping a development build.

Each flag is read with the **same semantics as its live consumer**, so the
assertion cannot disagree with the runtime behaviour it predicts:
`public_reads_enabled_in` mirrors `rbac_gate::public_reads_enabled` (only `1` or
`true` enable; absence means auth-required), and `visibility_filter_enabled_in`
mirrors `position_updates::parse_visibility_flag` (ON by default; only `0`,
`false`, `off` or `no` disable, so an unrecognised value fails safe). Both are
duplicated against the `EnvSnapshot` rather than calling the live functions,
because the gate reads the live process environment while the assertion reads the
boot snapshot — and the snapshot exists precisely so every check sees one
consistent set of values.

## Consequences

- The `Not machine-enforced` row in `docs/SECURITY-profiles.md` becomes
  enforced, and Invariant 5 gains a mechanism rather than an instruction.
- **The shipped compose posture is unaffected.** `demo-open` enables anonymous
  reads but keeps `PUBKEY_VISIBILITY_FILTER=1`, so it does not trip the rule; a
  test asserts this for all three ratified profiles, because ADR-2027's decision
  that the image boots `demo-open` must keep working (PHASE2 policy 2).
- A deployment that genuinely wants unauthenticated reads of unfiltered data now
  cannot start. That is the intent: there is no supported posture for it.
- The duplicated flag readers are a maintenance obligation — a change to
  `public_reads_enabled` or `parse_visibility_flag` must be mirrored here. Two
  tests pin the semantics of each reader against its live counterpart's rules so
  a divergence fails the suite rather than silently weakening the assertion.
- This does **not** close the wider `Unnamed`-is-not-fatal gap: an undeclared
  production deployment landing on some *other* unrecognised combination still
  binds. ADR-2038's text says such a deployment should default to
  `multi-user-locked`; the code implements no implicit profile. That remains open
  and is deliberately not bundled here, being a much larger behavioural change.

## Verification

Implemented in `src/config/security_profile.rs`: the `FullDisclosureFlagPair`
variant on `ProfileFinding` with its `Display` arm naming both observed values and
the two remedies; the rule as step 5b of `evaluate_effective_profile`; and the
`public_reads_enabled_in` / `visibility_filter_enabled_in` readers.

```
cargo test -p visionclaw-server --lib security_profile
    test result: ok. 37 passed; 0 failed; 0 ignored; 1240 filtered out
```

Eight tests are new in this change:
`full_disclosure_pair_is_rejected_without_a_declared_profile` (the case that
previously slipped through entirely), `..._with_a_declared_profile_too`,
`every_ratified_profile_is_free_of_the_pair` (all three of ADR-2027's profiles,
including the shipped `demo-open`), `either_flag_alone_is_not_the_pair`,
`absent_flags_do_not_trip_the_rule`,
`public_reads_reader_matches_the_rbac_gate_semantics`,
`visibility_reader_matches_parse_visibility_flag_semantics`, and
`full_disclosure_is_fatal_in_a_production_artefact` (asserting
`may_bind_listener()` is false for a production artefact and true for a debug
build).

`cargo check -p visionclaw-server --lib` introduces no new errors. The four
pre-existing errors are all in `src/services/owl_extractor_service.rs`
(E0425/E0433, `AnnotatedOntology` and `read_functional` unresolved) and belong to
another lead's in-flight change.

**Verification ran on the uncommitted working tree above
`b00c28a0d766c8cf46cd00b100dab60ef2dd74a4` and must be re-run at the landing
commit, which sets `verified_paths`.** Note in particular that
`src/config/security_profile.rs` — the file this ADR modifies — is itself
**untracked** in the working tree and absent from that SHA, so a clean checkout of
`b00c28a0d` contains neither the ADR-2038 assertion nor this rule.
`activation_status` is therefore `staged`, not `live`: the code is complete and
tested but has never run in a deployed image.
