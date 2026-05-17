# solid-pod-rs — Operational Runbook

| Field | Value |
|-------|-------|
| Substrate | solid-pod-rs |
| Repo | github.com/DreamLab-AI/solid-pod-rs |
| Version | 0.4.0-alpha.6 |
| Type | Rust library crate (not a standalone service) |

## Architecture

Foundation library consumed by all other substrates:
- LDP (Linked Data Platform) container/resource operations
- WAC (Web Access Control) evaluation
- WebID-TLS / DID:Nostr identity resolution
- NIP-98 HTTP Auth verification
- Schnorr signature primitives
- Config loader for consumer integration

Backends: `fs-backend` (filesystem), `memory-backend` (in-memory for tests).

## Deployment Model

solid-pod-rs is a library, not a standalone service. It runs inside:
- **VisionClaw**: `solid_pod_handler.rs` wraps `NativeSolidService`
- **agentbox**: Pod bridge service on port 8484
- **nostr-rust-forum**: `nostr-bbs-pod-worker` WASM worker

## Health Checks (per consumer)

| Consumer | Endpoint | Port |
|----------|----------|------|
| VisionClaw | GET /api/solid/.well-known/solid | 4000 |
| agentbox | GET /.well-known/solid | 8484 |
| Forum | GET /.well-known/solid | 8787 |

## Common Failure Modes

### Feature Flag Mismatch
- **Symptom**: Compile error in consumer
- **Cause**: Consumer enables feature not available in pinned version
- **Fix**: Check `Cargo.toml` features match published version. Current features: `fs-backend`, `memory-backend`, `nip98-schnorr`, `security-primitives`, `did-nostr`, `config-loader`, `acl-origin`, `webhook-signing`

### NIP-98 Verification Failure
- **Symptom**: 401 on authenticated Pod operations
- **Cause**: Clock skew (NIP-98 has 60s window), wrong pubkey format, or missing `Authorization` header
- **Fix**: Verify client sends `Nostr <base64-event>` header; check server clock sync

### WebID Resolution Failure
- **Symptom**: ACL evaluation returns 403 for valid users
- **Cause**: DID document not found or `verificationMethod` type mismatch
- **Fix**: Verify DID document at `did:nostr:<hex-pubkey>` resolves with `SchnorrSecp256k1VerificationKey2019`

### Filesystem Backend Permission Error
- **Symptom**: 500 on resource creation
- **Cause**: Pod data directory not writable
- **Fix**: Check permissions on pod data path (typically `/data/pods/` in containers)

## Testing

```bash
cd /home/devuser/workspace/solid-pod-rs
cargo test                           # Unit + integration tests
cargo test --features memory-backend # Memory backend tests
```

## Versioning

- Follows semver: `0.4.0-alpha.N` during pre-release
- Breaking changes bump minor (0.4 → 0.5)
- Consumers pin exact version in Cargo.toml
- After bump: update all consumer Cargo.toml references

## Backup / Restore

Not applicable as a library. Consumer-side pod data backup:
- **VisionClaw**: Pod data in configured `SOLID_POD_DATA_DIR`
- **agentbox**: `/data/pods/` in container (see agentbox-runbook.md)

## RTO / RPO Targets

| Component | RTO | RPO | Notes |
|-----------|-----|-----|-------|
| Library availability | N/A | N/A | Compiled into consumer binary |
| Pod data (per consumer) | See consumer runbook | See consumer runbook | |
| crates.io publish | < 1h | N/A | `cargo publish` from CI |

## Release Process

1. Bump version in root `Cargo.toml` and all 6 internal crate references
2. Update CHANGELOG
3. `cargo test --all-features`
4. `git tag v0.4.0-alpha.N && git push --tags`
5. `cargo publish` (when ready for crates.io)
6. Update consumer Cargo.toml pins

## Escalation

1. Check consumer error logs for solid-pod-rs stack traces
2. Run library tests: `cargo test --all-features`
3. Check feature flag compatibility between library and consumer
4. File GitHub issue with minimal reproduction
