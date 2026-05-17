# Cross-Substrate Fixture Coverage Matrix

Per ADR-082 D3. Tracks which substrate consumes which fixture.

| Fixture | nostr-rust-forum (kit) | agentbox | VisionClaw | solid-pod-rs | dreamlab-ai-website (consumer) |
|---------|:----------------------:|:--------:|:----------:|:-------------:|:-------------------------------:|
| nip01-events.json | ✓ | ✓ | ✓ | ✓ | inherits via kit |
| nip04-dm.json | ✓ | ✓ | ✓ | n/a | inherits via kit |
| nip19-bech32.json | ✓ | ✓ | ✓ | ✓ | inherits via kit |
| nip26-delegation.json | ✓ | ✓ | ✓ | n/a | inherits via kit |
| nip44-v2.json | ✓ | ✓ | ✓ | n/a | inherits via kit |
| nip59-gift-wrap.json | ✓ | ✓ | ✓ | n/a | inherits via kit |
| nip98-tokens.json | ✓ | ✓ | ✓ | ✓ | inherits via kit |
| rfc8785-jcs.json | ✓ | ✓ | ✓ | ✓ | inherits via kit |
| bip340-schnorr.json | ✓ | ✓ | ✓ | ✓ | inherits via kit |
| multibase.json | ✓ | ✓ | ✓ | ✓ | inherits via kit |
| did-doc-conformance.json | ✓ | ✓ | ✓ | ✓ | inherits via kit |
| is-envelope-v1.json | ✓ | ✓ | ✓ | ✓ | inherits via kit |
| mesh-federation.json | ✓ | ✓ | ✓ | n/a | inherits via kit |

**Total**: 13 fixtures × 4 substrates (excluding consumer which inherits) = 47 substrate × fixture pairs.

## Sync method per substrate

Per ADR-082 D5:
- Each substrate runs `scripts/sync-fixtures.sh` to copy from VisionClaw monorepo's `docs/specs/fixtures/`.
- CI verifies SHA-256 match via `CHECKSUM.txt`.
- `dreamlab-ai-website` consumes via Cargo dep on kit; inherits kit's fixture set.

## Update propagation

When a fixture is updated:
1. PR in VisionClaw monorepo updates fixture + CHECKSUMS.txt
2. Synchronised sub-PRs in each consuming substrate update their tests/fixtures/CHECKSUM.txt
3. CI gates (per ADR-082 D4 Option β) catch any drift

## Per-substrate test runner reference

| Substrate | Runner | Path |
|-----------|--------|------|
| nostr-rust-forum (Rust) | `cargo test --test upstream_vectors -p nostr-bbs-core` | tests/upstream_vectors/ |
| agentbox (Node.js, jest) | `npx jest tests/contract/upstream_vectors` | tests/contract/upstream_vectors/ |
| VisionClaw (Rust) | `cargo test --test upstream_vectors` | tests/upstream_vectors/ |
| solid-pod-rs (Rust) | `cargo test --workspace --test upstream_vectors` | crates/*/tests/upstream_vectors/ |
