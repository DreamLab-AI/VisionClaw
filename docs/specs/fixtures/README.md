# Cross-Substrate Test Fixtures

Per ADR-082. Master fixtures consumed by all four mesh substrates (VisionClaw, nostr-rust-forum, agentbox, solid-pod-rs) and the dreamlab-ai-website downstream consumer.

## Files

- **`UPSTREAM_PINS.md`** — commit hashes for each upstream source
- **`COVERAGE_MATRIX.md`** — per-substrate consumption mapping
- **`schemas/`** — JSON Schema 2020-12 validators per fixture
- **`<fixture>.json`** — individual fixture files

## Sprint Phase 0 deliverables

The following fixtures MUST be populated before PRD-010 P0 implementation lands:

| Fixture | Source | Status | Owner |
|---------|--------|--------|-------|
| nip44-v2.json | paulmillr/nip44 | DONE (Phase 0, 2026-05-07) | sprint Phase 0 |
| bip340-schnorr.json | bitcoin/bips | DONE (Phase 0, 2026-05-07) | sprint Phase 0 |
| rfc8785-jcs.json | cyberphone/json-canonicalization | DONE (Phase 0, 2026-05-07) | sprint Phase 0 |
| nip01-events.json | nostr-protocol/nips | DONE (Phase 1, 2026-05-07) | sprint Phase 1 |
| nip04-dm.json | nostr-protocol/nips | DONE (Phase 1, 2026-05-07) | sprint Phase 1 |
| nip19-bech32.json | nostr-protocol/nips | DONE (Phase 1, 2026-05-07) | sprint Phase 1 |
| nip26-delegation.json | nostr-protocol/nips | DONE (Phase 1, 2026-05-07) | sprint Phase 1 |
| nip59-gift-wrap.json | nostr-protocol/nips | DONE (Phase 1, 2026-05-07) | sprint Phase 1 |
| nip98-tokens.json | nostr-protocol/nips | DONE (Phase 1, 2026-05-07) | sprint Phase 1 |
| multibase.json | multiformats/multibase | DONE (Phase 1, 2026-05-07) | sprint Phase 1 |
| did-doc-conformance.json | DreamLab-internal (ADR-074 D2) | DONE (Phase 1, 2026-05-07) | sprint Phase 1 |
| is-envelope-v1.json | DreamLab-internal (ADR-075 D1+D3) | DONE (Phase 1, 2026-05-07) | sprint Phase 1 |
| mesh-federation.json | DreamLab-internal (ADR-073 D2/D6/D9 + ADR-074 D9) | DONE (Phase 1, 2026-05-07) | sprint Phase 1 |

## Consumption

Each substrate runs `scripts/sync-fixtures.sh` to pull fixtures into its own `tests/fixtures/` directory + verify checksums.

## Validation

`tests/fixture-master-validity.sh` (in VisionClaw monorepo root) MUST run on every PR that touches `docs/specs/fixtures/`. Verifies:
- Every fixture file passes its JSON Schema
- Every fixture has ≥3 vectors
- UPSTREAM_PINS.md commit hashes are well-formed (40-char hex)
- COVERAGE_MATRIX.md row count matches fixture count

## ADR references

- ADR-082 — Cross-Substrate Test Fixture Sharing Protocol (the master spec)
- ADR-077 — Ecosystem QE Policy (P1 reference vectors)
- ADR-074 — DID:Nostr canonicalisation (drives did-doc-conformance.json)
- ADR-075 — IS-Envelope contract (drives is-envelope-v1.json)
- ADR-073 — Mesh topology (drives mesh-federation.json)
- ADR-076 — Forum nostr-core absorption (consumes via upstream `nostr` crate validation)
