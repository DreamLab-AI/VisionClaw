# ADR-082 — Cross-Substrate Test Fixture Sharing Protocol

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | ADR-077 P1 (reference vectors), ADR-077 P2 (cross-substrate contracts), PRD-010 G8 cryptographic correctness |
| Companion ADRs | ADR-073, ADR-074, ADR-075, ADR-076, ADR-077, ADR-078, ADR-080, ADR-081 |
| Companion PRDs | PRD-010, PRD-011 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` |
| Affected repos | `VisionClaw` (master fixture host), `nostr-rust-forum`, `agentbox`, `solid-pod-rs`, `dreamlab-ai-website` |

## Context

The C1 NIP-44 v2 conversation-key bug shipped because **no project vendored paulmillr/nip44 reference vectors** (Q4 G2 finding). All four projects had hand-rolled NIP-44 implementations whose round-trip-with-self tests passed because the bug was symmetric between encrypt and decrypt. Q4 G3 found **zero cross-substrate contract tests** (defined as: same input, ≥2 implementations, byte-equal output asserted) across the ecosystem.

ADR-077 P1 mandates "reference vectors are mandatory for every protocol primitive" and ADR-077 P2 establishes three contract test levels (L1 within-substrate, L2 cross-substrate, L3 federation smoke). Q5 (`docs/integration-research/qe-fleet/Q5-test-fixture-design.md`) designed:
- Three fixture classes (T1): reference vectors, contract assertions, negative cases
- Reference vector source plan with upstream pins (T2)
- Fixture file layout at `docs/specs/fixtures/` (T3)
- Per-substrate test runners (T4)

This ADR formalises the **mechanics of cross-substrate fixture sharing**: where the master fixtures live, how each substrate's CI consumes them, how upstream pins are tracked, how updates propagate, and who triages cross-substrate contract test failures (the diamond problem).

## Decision

### D1 — Single source of truth: VisionClaw monorepo

Master test fixtures live at `/home/devuser/workspace/project/docs/specs/fixtures/` (i.e. inside the `DreamLab-AI/VisionClaw` repository). VisionClaw is the integration substrate per ADR-077 P9 and the canonical home of the cross-substrate work; fixtures live with the spec they validate.

```
docs/specs/fixtures/
├── README.md                  # what each file covers; how to consume
├── UPSTREAM_PINS.md           # commit hashes per upstream source
├── COVERAGE_MATRIX.md         # which substrates consume which fixtures
├── nip01-events.json          # event id, sign, verify
├── nip04-dm.json              # ECDH+CBC DM
├── nip19-bech32.json          # bech32 entities
├── nip26-delegation.json      # delegation tags
├── nip44-v2.json              # paulmillr/nip44 — load-bearing
├── nip59-gift-wrap.json       # NIP-59 wrap-seal-rumor
├── nip98-tokens.json          # HTTP auth tokens
├── rfc8785-jcs.json           # JSON canonicalisation
├── bip340-schnorr.json        # Schnorr signature vectors
├── multibase.json             # multibase encoding
├── did-doc-conformance.json   # DreamLab-internal DID Document contract
├── is-envelope-v1.json        # ADR-075 envelope conformance
└── mesh-federation.json       # ADR-073 federation behaviour
```

Each fixture file is JSON Schema 2020-12 validated; schemas at `docs/specs/fixtures/schemas/<name>.schema.json`.

### D2 — UPSTREAM_PINS.md tracks every external source

Reference vectors come from upstream specs, libraries, or test suites. Each pinned to a commit hash. `UPSTREAM_PINS.md` is the lockfile:

```markdown
# Upstream Reference Vector Pins

## NIP-44 v2 (paulmillr/nip44)
- Repository: https://github.com/paulmillr/nip44
- Pinned commit: `7fe2cabb02bdce6f3d5b3e90c2b4a3e1f0c4d8a3` (2026-04-15)
- Path: `javascript/test/vectors.json`
- Last refresh: 2026-05-01
- Contains: encrypt vectors, decrypt vectors, padded length, MAC tags

## NIP-19 bech32 (nostr-protocol/nips)
- Repository: https://github.com/nostr-protocol/nips
- Pinned commit: `8b3c5e9a0d12345...`
- Path: `examples/19/`
- Last refresh: 2026-04-30

## RFC 8785 JCS (cyberphone/json-canonicalization)
- Repository: https://github.com/cyberphone/json-canonicalization
- Pinned commit: `9e8d7c6b...`
- Path: `testdata/`

## BIP-340 Schnorr (bitcoin/bips)
- Repository: https://github.com/bitcoin/bips
- Pinned commit: `4f6e1c2...`
- Path: `bip-0340/test-vectors.csv`

## DID Core (w3c/did-test-suite)
- Repository: https://github.com/w3c/did-test-suite
- Pinned commit: `master @ 2026-04-22`
- Path: `tests/`

## Multibase (multiformats/multibase)
- Repository: https://github.com/multiformats/multibase
- Pinned commit: `a1b2c3...`
- Path: `tests/`

## Refresh policy
- Quarterly review of upstream commits
- On-demand refresh when an upstream NIP / RFC version moves
- All refreshes via PR with explicit "refresh fixtures" title
```

When an upstream commit moves, the workflow is:
1. Open a "refresh fixtures" PR in VisionClaw.
2. Re-run all consuming substrates' tests against new vectors.
3. If any test fails, root-cause: is upstream wrong (rare) or our implementation wrong (common)?
4. Merge refresh + any required implementation fixes together.

### D3 — Coverage matrix

`COVERAGE_MATRIX.md` enumerates which substrates consume which fixtures:

```markdown
# Cross-Substrate Fixture Coverage

| Fixture | Forum (kit) | Agentbox | VisionClaw | solid-pod-rs |
|---------|-------------|----------|------------|--------------|
| nip01-events.json | ✓ | ✓ | ✓ | ✓ |
| nip04-dm.json | ✓ | ✓ | ✓ | n/a |
| nip19-bech32.json | ✓ | ✓ | ✓ | ✓ |
| nip26-delegation.json | ✓ | ✓ | ✓ | n/a |
| nip44-v2.json | ✓ | ✓ | ✓ | n/a |
| nip59-gift-wrap.json | ✓ | ✓ | ✓ | n/a |
| nip98-tokens.json | ✓ | ✓ | ✓ | ✓ |
| rfc8785-jcs.json | ✓ | ✓ | ✓ | ✓ |
| bip340-schnorr.json | ✓ | ✓ | ✓ | ✓ |
| multibase.json | ✓ | ✓ | ✓ | ✓ |
| did-doc-conformance.json | ✓ | ✓ | ✓ | ✓ |
| is-envelope-v1.json | ✓ | ✓ | ✓ | ✓ |
| mesh-federation.json | ✓ | ✓ | ✓ | n/a |

Total: 13 fixtures × 4 substrates = 47 substrate × fixture pairs.
```

Updates to a fixture trigger CI test runs in every substrate marked ✓.

### D4 — Cross-repo distribution: copy-with-CI-check

VisionClaw is the master; other substrates need fixtures locally for offline-capable CI runs (their CI doesn't necessarily have access to VisionClaw repo at test time). Mechanism:

**Option α — Git submodule** (rejected; submodule semantics confuse contributors)

**Option β — Copy at PR time, CI hash-check**: each substrate's repo holds a **copy** of the fixtures it consumes at `tests/fixtures/`. A CI job in each substrate computes SHA-256 of every fixture file and compares against the SHA-256s recorded at `tests/fixtures/CHECKSUM.txt` and at `<VisionClaw>/docs/specs/fixtures/CHECKSUMS.txt`. If they diverge, CI fails with "fixture drift detected — run `scripts/sync-fixtures.sh` to update".

**Option γ — npm/cargo package distribution** (rejected; cross-language so no single package format works)

We adopt **Option β**.

### D4a — Sequencing constraint: D1 master fixtures must exist before any D4 sync

**Hard ordering invariant** (V2 phase-graph hazard remediation):

The D1 master fixtures at `docs/specs/fixtures/` in VisionClaw monorepo MUST exist and pass schema validation BEFORE any consuming substrate's `scripts/sync-fixtures.sh --verify` (D4) executes in CI. Otherwise the consuming substrate's CI silently passes an empty fixture corpus → defeats the entire ADR.

**Enforced via**:
1. PRD-010 P0 explicit prerequisite: `docs/specs/fixtures/{nip04,nip19,nip26,nip44-v2,nip59,nip98,bip340-schnorr,rfc8785-jcs,multibase,did-doc-conformance,is-envelope-v1,mesh-federation,nip01-events}.json` MUST exist with non-empty vector arrays.
2. CI gate in each consuming substrate: `scripts/sync-fixtures.sh --verify` exits non-zero if pulled fixtures are empty or schema-invalid.
3. CI gate in VisionClaw: `tests/fixture-master-validity.sh` runs in PR-required workflow; asserts every fixture file passes its JSON Schema + has ≥3 vectors.

This closes the V2 sequencing hazard ("ADR-082 D1 must precede PRD-010 P0 — comment present but not CI-enforced").

### D5 — Sync mechanism: `scripts/sync-fixtures.sh`

Each substrate ships `scripts/sync-fixtures.sh` that pulls the latest VisionClaw `docs/specs/fixtures/` into the substrate's `tests/fixtures/` directory. Default mechanism: `git clone --depth=1` of VisionClaw, copy the relevant subdirectory, write CHECKSUM.txt. Override-able via env var `VISIONCLAW_FIXTURES_PATH` for offline / development environments.

```bash
#!/bin/bash
# scripts/sync-fixtures.sh — copy fixtures from VisionClaw monorepo
set -eu

VISIONCLAW_PATH="${VISIONCLAW_FIXTURES_PATH:-https://github.com/DreamLab-AI/VisionClaw.git}"
TARGET_DIR="tests/fixtures"
mkdir -p "$TARGET_DIR"

# Pull master fixtures
if [[ "$VISIONCLAW_PATH" =~ ^https://.*\.git$ ]]; then
  TMPDIR=$(mktemp -d)
  git clone --depth=1 --filter=blob:none --sparse "$VISIONCLAW_PATH" "$TMPDIR"
  cd "$TMPDIR" && git sparse-checkout add docs/specs/fixtures && cd -
  rsync -av "$TMPDIR/docs/specs/fixtures/" "$TARGET_DIR/"
  rm -rf "$TMPDIR"
else
  rsync -av "$VISIONCLAW_PATH/" "$TARGET_DIR/"
fi

# Compute checksums + write CHECKSUM.txt
cd "$TARGET_DIR"
sha256sum *.json schemas/*.json > CHECKSUM.txt
echo "Synced $(wc -l < CHECKSUM.txt) fixture files."
```

Each substrate's CI runs `sync-fixtures.sh --verify` (a flag that skips the actual sync but compares local checksums against the master's; flag exits 0 if consistent, 1 if drift detected).

### D6 — Per-substrate test runner harness

Each substrate consumes the fixtures via a substrate-specific test harness:

**Forum (Rust, `nostr-bbs-core`)**:
```rust
// tests/upstream_vectors/nip44.rs
use serde_json;
use nostr::nips::nip44;

#[derive(serde::Deserialize)]
struct Nip44Vec {
    name: String,
    sk: String,
    pk: String,
    payload: String,
    expected_plaintext: String,
    expected_conv_key: String,
}

#[test]
fn nip44_v2_paulmillr_vectors() {
    let vectors: Vec<Nip44Vec> = serde_json::from_str(
        include_str!("../fixtures/nip44-v2.json")
    ).expect("fixture parses");
    
    for v in &vectors {
        let conv_key = nip44::ConversationKey::derive(
            &hex::decode(&v.sk).unwrap(),
            &hex::decode(&v.pk).unwrap(),
        ).expect("derive ok");
        
        assert_eq!(hex::encode(conv_key.as_bytes()), v.expected_conv_key,
            "conv_key mismatch on {}", v.name);
        
        let plaintext = nip44::decrypt(&conv_key, &v.payload).expect("decrypt ok");
        assert_eq!(plaintext, v.expected_plaintext,
            "plaintext mismatch on {}", v.name);
    }
}
```

**Agentbox (Node.js, jest)**:
```js
// tests/contract/upstream_vectors/nip44.test.js
const fs = require('fs');
const { nip44 } = require('nostr-tools');
const vectors = JSON.parse(
  fs.readFileSync('./tests/fixtures/nip44-v2.json', 'utf8')
);

describe.each(vectors)('NIP-44 v2 — $name', (v) => {
  test('conversation key matches', () => {
    const convKey = nip44.v2.utils.getConversationKey(v.sk, v.pk);
    expect(Buffer.from(convKey).toString('hex')).toBe(v.expected_conv_key);
  });
  
  test('decrypt matches expected plaintext', () => {
    const convKey = nip44.v2.utils.getConversationKey(v.sk, v.pk);
    expect(nip44.v2.decrypt(v.payload, convKey)).toBe(v.expected_plaintext);
  });
});
```

**VisionClaw (Rust)**:
Same shape as Forum — uses upstream `nostr` or `nostr_sdk` types per ADR-076 + ADR-078.

**solid-pod-rs**:
Used for NIP-98 + DID Document + Multibase; same Rust shape.

### D7 — Cross-substrate Level 2 contract tests

ADR-077 P2 L2 = "same input through ≥2 implementations, byte-equal output asserted". L2 tests live in **VisionClaw monorepo** at `tests/cross_substrate/`. Each L2 test:
1. Spawns processes / functions from ≥2 substrates (forum nostr-bbs-core, VisionClaw substrate, agentbox via subprocess, solid-pod-rs).
2. Feeds them the same fixture vector.
3. Asserts byte-equal outputs.

Example (`tests/cross_substrate/nip98_token_round_trip.rs`):

```rust
#[tokio::test]
async fn nip98_token_built_by_kit_validates_in_visionclaw() {
    let vector = load_fixture("nip98-tokens.json")[0].clone();
    
    // Build token in forum kit (Rust, via process)
    let token = run_forum_kit_helper(&vector).await;
    
    // Validate in VisionClaw substrate
    let result = visionclaw_substrate::nip98_verify(&token, &vector.url, &vector.method).await;
    assert_eq!(result.unwrap().pubkey, vector.expected_pubkey);
    
    // Validate in agentbox (subprocess)
    let agentbox_result = run_agentbox_helper(&token, &vector).await;
    assert_eq!(agentbox_result, "valid");
    
    // Validate in solid-pod-rs (direct dep)
    let sp_result = solid_pod_rs::auth::nip98::verify(...).await.unwrap();
    assert_eq!(sp_result.pubkey, vector.expected_pubkey);
}
```

L2 tests run **nightly** + **per release**, not per-PR (per ADR-077 P2 — they require multi-substrate process orchestration which is too heavy for per-PR cost).

### D8 — Diamond-problem triage protocol

When an L2 contract test fails, two implementations disagree on the same input. Per Q5 T15.2 + ADR-077 P9:

1. **Re-run upstream conformance** in both substrates against canonical vectors. If one substrate fails its own L1 reference vector, that substrate is wrong → owner fixes.
2. **Compare against third reference impl** (e.g. `paulmillr/nostr-tools` JS for NIP-44) — whichever matches third-party behaviour is correct.
3. **Default to last-touched-by**: `git blame` the protocol-implementing module in each substrate; the substrate whose code changed most recently is the suspect.
4. **Escalate to ADR** if neither substrate is obviously wrong — the disagreement may be a genuine ambiguity in the spec or in ADR-076/078 absorption decisions. Open a new ADR or amend an existing one.

Triage owner: VisionClaw integration maintainer (the substrate that runs L2). May delegate to substrate maintainers depending on root cause.

### D9 — Negative-case fixtures

Per Q5 T1, fixtures include **negative cases** — malformed inputs that MUST be rejected:

```json
[
  {
    "name": "nip44_invalid_padding",
    "input": "<base64 with bad padding>",
    "expected_error_class": "Nip44Error::Padding"
  },
  {
    "name": "nip44_mac_mismatch",
    "input": "<base64 with corrupted MAC>",
    "expected_error_class": "Nip44Error::Mac"
  },
  {
    "name": "is_envelope_missing_kind",
    "input": "{ \"v\": 1, \"to\": \"...\", \"from\": \"...\" }",
    "expected_error_class": "EnvelopeError::MissingField('kind')"
  },
  ...
]
```

Each substrate's parser must produce the documented error class on each negative case. Error class mappings (e.g. Rust `Nip44Error::Padding` ↔ JS `Nip44Error.Padding`) tracked in `docs/specs/fixtures/error-class-mapping.md`.

### D10 — Fixture authorship policy

Adding a new fixture:
1. Author creates JSON file in `docs/specs/fixtures/` with at least 5 vectors covering happy path + error path.
2. Author writes JSON Schema for the fixture format (`schemas/<name>.schema.json`).
3. Author writes per-substrate consumer tests (D6 shape).
4. Author updates `COVERAGE_MATRIX.md` + `UPSTREAM_PINS.md` (if upstream-sourced).
5. PR runs L1 consumer tests in every substrate; failure to consume = PR cannot merge.

Editing an existing fixture (rare; only when upstream moves or DreamLab-internal contract evolves):
1. Author opens "fixture refresh" PR with explicit reason for edit.
2. CI re-runs all consuming substrates — failures are root-caused before merge.
3. CHECKSUM.txt files in all substrates are updated in synchronised follow-up PRs.

Removing a fixture:
1. Verify no CI job references it (grep across all substrates).
2. Update COVERAGE_MATRIX.md.
3. Remove from VisionClaw + each substrate via paired PRs.

### D11 — Fixture freshness alerting

A nightly CI job (`fixture-freshness.yml` in VisionClaw) checks each upstream commit hash in UPSTREAM_PINS.md against the upstream HEAD. If upstream has moved >30 days since pin, posts to `#qe-alerts` Slack channel: "NIP-44 vectors pinned 2026-04-15; upstream HEAD is 2026-06-01. Consider refresh."

Operators take the alert as a hint; refresh decisions stay manual (we don't auto-refresh because upstream changes may correlate with spec changes).

### D12 — Property-based fixture extension

For each NIP / spec, a property test generator extends the fixture corpus dynamically:

```rust
proptest! {
    #[test]
    fn nip44_round_trip(
        sk in any_secret_key(),
        pk in any_public_key(),
        plaintext in any::<Vec<u8>>().prop_filter("non-empty", |b| !b.is_empty()),
    ) {
        let conv_key = derive(&sk, &pk).unwrap();
        let ciphertext = encrypt(&conv_key, &plaintext).unwrap();
        let decrypted = decrypt(&conv_key, &ciphertext).unwrap();
        prop_assert_eq!(plaintext, decrypted);
    }
}
```

When proptest finds a counter-example, it's automatically added as a regression vector at `tests/fixtures/<spec>.regression.json` — feeding back into the fixture corpus for cross-substrate consumption.

### D13 — IS-Envelope conformance fixture (ADR-075)

`is-envelope-v1.json` is the load-bearing DreamLab-internal contract. Per ADR-075 D15:
- 7 envelope kinds (chat / tool_invoke / tool_result / knowledge_link / moderation / mesh_ping / unknown)
- For each: sample envelope JSON, JCS canonical form, kind-1059 wrap, unwrap path
- Negative cases: missing required field, wrong types, oversized body, expired ttl, version > supported, via chain too long
- Round-trip assertion: decode → re-encode → byte-identical

This fixture is verified by ALL FOUR substrates' IS-Envelope encoders/decoders.

### D14 — Mesh-federation conformance fixture (ADR-073)

`mesh-federation.json` covers ADR-073 federation behaviour:
- Federation worker fan-out ordering
- LRU dedup with seen_ids
- Tag-injection (`x-mesh-from`) loop avoidance
- NIP-42 AUTH session shape
- mesh service-list (kind-30033) replaceable semantics

Substrate's federation worker runs against the fixture and asserts behaviour matches.

## Consequences

### Positive

- **C1-class bugs structurally prevented**: every protocol primitive has paulmillr-style upstream vectors as regression guard; any future drift fails CI before merge.
- **Single source of truth**: VisionClaw monorepo holds master; no fork drift across substrates.
- **Cross-substrate symmetry verifiable**: L2 tests guarantee forum nostr-bbs-core ≡ VisionClaw substrate ≡ agentbox impl ≡ solid-pod-rs upstream on the same input.
- **Diamond problem has a triage protocol**: when implementations disagree, triage path is deterministic.
- **Fixture refresh process** scales: quarterly reviews, on-demand refreshes, no special-casing per upstream.

### Negative

- **VisionClaw becomes a critical-path dependency**: any substrate's CI requires VisionClaw fixtures fetchable. If VisionClaw repo unavailable (rare), CI fails. Mitigation: each substrate caches fixtures in its own repo via D4 copy-with-CI-check; VisionClaw outage = no fresh sync but existing tests continue.
- **Cross-substrate test orchestration cost**: L2 tests require docker-compose-shape fixtures per Q5 T7. Test environment is heavier than per-substrate. ~5-10 min L2 test runtime per nightly.
- **CHECKSUM.txt discipline**: every fixture refresh requires synchronised PRs across multiple repos. Easy to forget; mitigation = nightly drift-detection CI alert.
- **JSON Schema authoring overhead**: D10 mandates schemas for all fixtures; ~1-2 hours per fixture. Acceptable for the load-bearing primitives; trivial for negative-case-only fixtures.

### Neutral

- **Fixture file size**: 13 files × ~50 KiB average = ~650 KiB total. Trivial repo overhead.
- **No per-substrate fixture variation**: all substrates consume identical bytes. This is the point.

## Alternatives Considered

### Alt-A — Each substrate maintains its own fixtures
No central repository; each substrate forks fixtures as needed.

*Rejected*: this is how C1 happened. Each substrate's tests passed because the bug was symmetric within the substrate. Cross-substrate validation only works against shared canonical inputs.

### Alt-B — Fixtures published as a npm/cargo package
Distribute via package registries.

*Rejected*: cross-language fixture sharing makes single package format awkward (Rust crate vs npm vs PyPI). Git-based copy is simpler and language-agnostic.

### Alt-C — VisionClaw fetches fixtures at L1 test time via HTTPS
Each substrate's L1 test downloads fixtures live from a hosted URL.

*Rejected*: makes CI dependent on internet + hosting infrastructure. Offline CI breaks. Local copy + checksum verification is more robust.

### Alt-D — Fixtures only at L2 cross-substrate level
Skip L1 reference-vector tests; only run L2 contract tests.

*Rejected*: L1 catches bugs ~90% of the time at the cheapest cost (single substrate, fast feedback). L2 is for cross-substrate disagreement; L1 catches per-substrate regression. Both are needed.

### Alt-E — VisionClaw is the ONLY repo that runs reference vector tests
Other substrates trust VisionClaw's L2 tests.

*Rejected*: per-substrate tests give substrate maintainers fast PR feedback. Centralising to VisionClaw means a forum PR has to wait for VisionClaw nightly to detect a regression.

## Implementation notes

### Phase 0 deliverable (PRD-010 P0 gating)
- [ ] Create `docs/specs/fixtures/` directory in VisionClaw monorepo.
- [ ] Vendor paulmillr/nip44 vectors as `nip44-v2.json`. This single act defends against C1 regression.
- [ ] Vendor BIP-340 Schnorr vectors.
- [ ] Vendor RFC 8785 JCS vectors.
- [ ] Author `did-doc-conformance.json` (DreamLab-internal contract per ADR-074 D2).
- [ ] Author `is-envelope-v1.json` (DreamLab-internal contract per ADR-075 D1).
- [ ] Write `UPSTREAM_PINS.md` with initial pins.
- [ ] Write `COVERAGE_MATRIX.md` with initial mapping.

### Phase 1 deliverable
- [ ] Each substrate gains `tests/fixtures/` directory + `scripts/sync-fixtures.sh`.
- [ ] Each substrate's CI runs `sync-fixtures.sh --verify` as a gate.
- [ ] L1 reference vector tests added per substrate per fixture.
- [ ] CHECKSUM.txt verified across substrates.

### Phase 2 deliverable
- [ ] L2 cross-substrate contract tests at `<VisionClaw>/tests/cross_substrate/`.
- [ ] L2 nightly CI workflow.
- [ ] Diamond-problem triage runbook at `docs/operations/triage-l2-failure.md`.

### Phase 3 deliverable
- [ ] Property-based fixture extension (D12) generators per spec.
- [ ] Fixture freshness alerting (D11) nightly CI.
- [ ] Negative-case fixture coverage ≥80% of error paths in each substrate's parser.

### CI workflow shape

Per-substrate `.github/workflows/qe-fixtures.yml`:
```yaml
name: QE Fixture Tests
on: [push, pull_request]
jobs:
  fixture-checksums:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: ./scripts/sync-fixtures.sh --verify
  reference-vectors:
    runs-on: ubuntu-latest
    needs: fixture-checksums
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --test upstream_vectors  # or npm/jest equivalent
```

VisionClaw monorepo `.github/workflows/qe-cross-substrate.yml` (nightly):
```yaml
name: QE Cross-Substrate Contracts
on:
  schedule:
    - cron: '17 3 * * *'  # nightly at 03:17 UTC
  workflow_dispatch:
jobs:
  L2-contracts:
    runs-on: ubuntu-latest
    services:
      forum-kit: { image: ghcr.io/dreamlab-ai/nostr-rust-forum:latest }
      agentbox: { image: ghcr.io/dreamlab-ai/agentbox:latest }
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --test cross_substrate
      - if: failure()
        uses: actions/upload-artifact@v4
        with:
          name: l2-failure-evidence
          path: ./target/cross-substrate-failures/
```

### Fixture authorship template

```json
{
  "spec": "NIP-XX",
  "version": "vN",
  "source": "paulmillr/nip44 @ <commit-hash>",
  "schema": "../schemas/nip-xx.schema.json",
  "vectors": [
    {
      "name": "<descriptive name>",
      "category": "happy_path | edge_case | negative",
      "input": { ... },
      "expected": { ... },
      "expected_error": null
    }
  ]
}
```

## References

- ADR-073 — Mesh topology (federation contract)
- ADR-074 — DID:Nostr canonicalisation (D2 DID Document shape, kind-30033 mesh service-list)
- ADR-075 — IS-Envelope v1 (D15 conformance suite)
- ADR-076 — Forum `nostr-core` absorption (test surface inheritance)
- ADR-077 — Ecosystem QE Policy (P1 vectors, P2 contract levels)
- ADR-078 — Cross-substrate library convergence
- ADR-080 — Forum kit deployment topology
- ADR-081 — Federation key custody & rotation (provides keys for L3 federation tests)
- PRD-010 — DID:Nostr Mesh Federation, G8
- PRD-011 — VisionFlow Forum Kit Extraction, F6
- `docs/integration-research/qe-fleet/Q4-coverage-gap-audit.md` G2 (reference vectors), G3 (cross-substrate contracts), G4 (cross-system smoke)
- `docs/integration-research/qe-fleet/Q5-test-fixture-design.md` T1-T15 (fixture design)
- paulmillr/nip44: https://github.com/paulmillr/nip44
- nostr-protocol/nips: https://github.com/nostr-protocol/nips
- bitcoin/bips BIP-340: https://github.com/bitcoin/bips/tree/master/bip-0340
- cyberphone/json-canonicalization: https://github.com/cyberphone/json-canonicalization
- w3c/did-test-suite: https://github.com/w3c/did-test-suite
- multiformats/multibase: https://github.com/multiformats/multibase
- GitHub repos:
  - https://github.com/DreamLab-AI/VisionClaw (master fixture host)
  - https://github.com/DreamLab-AI/nostr-rust-forum (consumes via sync-fixtures.sh)
  - https://github.com/DreamLab-AI/agentbox (consumes via sync-fixtures.sh)
  - https://github.com/DreamLab-AI/solid-pod-rs (consumes via sync-fixtures.sh)
  - https://github.com/DreamLab-AI/dreamlab-ai-website (consumes via sync-fixtures.sh, downstream of kit)
