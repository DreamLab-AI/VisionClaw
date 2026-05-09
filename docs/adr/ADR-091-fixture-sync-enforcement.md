# ADR-091 — Cross-Substrate Fixture Sync Enforcement

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-09) |
| Drives | PRD-015 §5 (cross-substrate overlaps), QE fixture-sync-report (W5 finding) |
| Companion ADRs | ADR-077 P1/P2 (reference vectors, contract tests), ADR-082 (fixture sharing protocol) |
| Companion PRDs | PRD-014, PRD-015 |
| Affected repos | `VisionClaw` (master), `nostr-rust-forum`, `agentbox`, `solid-pod-rs` |

## Context

ADR-082 established the cross-substrate fixture sharing protocol with VisionClaw as master fixture host. The QE fleet audit (W5, 2026-05-09) discovered:

1. **Fixtures were never synced to consumers.** All 3 consuming substrates had `scripts/sync-fixtures.sh` but the target `tests/fixtures/` directories did not exist. Consumer-side L1 tests used `try_load_fixture()` which silently returned `None`.

2. **Cargo did not discover test targets.** `tests/upstream_vectors/` directories lacked `main.rs` entry points in all 4 Rust substrates.

3. **3 JSON Schemas were missing** from the master fixture host.

4. **CI validated only 3 of 13 fixtures.**

The W5 agent fixed all of these, but the root cause — no enforcement mechanism — remains. Without CI gates, sync will drift again.

## Decision

### D1 — CI sync-check in every consumer

Each consumer's CI workflow gains a `fixture-sync` job that runs before test jobs:

```yaml
fixture-sync:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: Sync fixtures from VisionClaw
      run: ./scripts/sync-fixtures.sh
    - name: Verify no drift
      run: |
        if ! git diff --quiet tests/fixtures/; then
          echo "::error::Fixtures are stale — run scripts/sync-fixtures.sh and commit"
          exit 1
        fi
```

Test jobs depend on `fixture-sync` passing.

### D2 — Master validity gate in VisionClaw CI

VisionClaw's `rust-ci.yml` runs `tests/fixture-master-validity.sh` which validates:
- All 13 fixture files parse as valid JSON
- All fixture files pass their JSON Schema
- SHA-256 checksums match `docs/specs/fixtures/CHECKSUMS.txt`
- No fixture file exceeds 1MB (guard against accidental bloat)

### D3 — Sync script contract

Every `scripts/sync-fixtures.sh` must:
1. Accept `--verify` flag (exit non-zero if fixtures would change)
2. Accept `--source <path>` flag (override master fixture location; defaults to `../VisionClaw/docs/specs/fixtures/` or falls back to `cp` from relative path)
3. Copy only fixtures listed in that substrate's fixture manifest (`tests/fixtures/MANIFEST.txt`)
4. Not require network access (fixtures come from local clone or CI artifact)

### D4 — Fixture update propagation protocol

When a fixture is added or updated in VisionClaw:
1. Update `docs/specs/fixtures/CHECKSUMS.txt`
2. Each consumer's `sync-fixtures.sh` picks it up on next run
3. Consumer CI fails if sync script wasn't run → forces commit of updated fixtures
4. Cross-substrate PR template includes "Fixture sync: [ ] N/A [ ] synced" checkbox

### D5 — Consumer fixture manifest

Each consumer lists which fixtures it uses in `tests/fixtures/MANIFEST.txt`:

```
# Fixtures consumed by nostr-rust-forum
nip01-events.json
nip04-dm.json
nip19-bech32.json
nip26-delegation.json
nip44-v2.json
nip98-tokens.json
bip340-schnorr.json
rfc8785-jcs.json
```

This prevents consumers from pulling fixtures they don't test against, and makes the dependency explicit.

## Consequences

**Positive:**
- Fixture drift is impossible — CI catches stale fixtures before merge
- Consumer manifests make fixture dependencies explicit and auditable
- No network dependency in CI — fixtures are file copies

**Negative:**
- Adding a new fixture requires updating CHECKSUMS.txt + consumer manifests + re-running sync
- Consumers must clone VisionClaw (or receive fixtures via CI artifact) for local development

**Risks:**
- Circular dependency: consumer CI needs VisionClaw fixtures, VisionClaw CI needs consumer tests to pass. Mitigated by D1's `sync-fixtures.sh` using local clone path, not git fetch from remote.
