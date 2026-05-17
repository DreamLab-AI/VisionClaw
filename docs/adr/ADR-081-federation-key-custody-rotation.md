# ADR-081 — Federation Key Custody & Rotation Protocol

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-010 R7 (federation key proliferation), PRD-011 G7, ADR-074 D12 (key rotation), ADR-080 D6 (custody options) |
| Companion ADRs | ADR-073, ADR-074, ADR-076, ADR-077, ADR-078, ADR-079, ADR-080 |
| Companion PRDs | PRD-010, PRD-011 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` |
| Affected repos | `nostr-rust-forum` (kit federation key), `agentbox` (sovereign + federation keys), `solid-pod-rs-idp` (issuer key), `VisionClaw` (operator + bridge keys), `dreamlab-ai-website` (consumer admin key) |

## Context

The federated mesh defined in PRD-010 + ADR-073/074/075 introduces multiple long-lived cryptographic keys per deployment. Q3 §I12 quantified federation key cardinality at **7–8 long-lived keys per multi-substrate deployment** and warned: *"Without `agentbox.sh rotate-keys` tooling, operators will collapse roles back into one shared key."* That outcome would defeat every defence-in-depth boundary the mesh is designed to provide.

Concurrent findings from the QE fleet:
- Q3 CRITICAL-03: `agentbox/scripts/sovereign-bootstrap.py` writes plaintext `private_key_hex` to `/var/lib/agentbox/identities/<id>.json` with default file permissions; `agentbox/mcp/servers/nostr-bridge.js:439-475 loadSigner` reads from a non-existent `nostr.key.enc` AES-256-GCM file. The at-rest encryption layer documented in PRD-001 §378-380 does not exist in code.
- Q3 G6: `agentbox.sh rotate-keys` documented in PRD-001 §379 but does not exist.
- Q3 G7: `solid-pod-rs-idp/src/jwks.rs SigningKey` lacks `Zeroize` derive on `private_pem`/`private_der` — heap-leak risk on drop.
- Q2 S-HIGH-6: agentbox identity file permissions not enforced at chmod 0600.

Per-actor-class key inventory across the mesh (post-PRD-010):

| Actor class | Key role | Holder | Storage |
|-------------|----------|--------|---------|
| Forum user | passkey-PRF-derived nsec | browser PRF + WebAuthn extension | volatile, re-derived on login |
| Forum admin | static admin pubkey | TOML config + relay D1 row | filesystem (kit operator) |
| Agentbox sovereign agent | per-agent nsec | filesystem `/var/lib/agentbox/identities/<id>.json` | filesystem (broken at-rest plan) |
| Agentbox federation | relay-relay AUTH key (ADR-073 D4) | proposed `/var/lib/agentbox/identities/federation.json` | filesystem (proposed) |
| VisionClaw operator | substrate operator key | `SERVER_NOSTR_PRIVKEY` env (post-PRD-010 F1 unification absorbs `VISIONCLAW_NOSTR_PRIVKEY`) | env var |
| VisionClaw bridge | re-attribution key (proposed PRD-010 F9) | `BRIDGE_NSEC` env or KMS | env var or KMS |
| Mesh-side federation | federation-session AUTH (ADR-073 D4) | `MESH_FEDERATION_PRIVKEY` env | env var |
| Forum kit federation | per-deployment relay-relay AUTH | TOML + secret store | filesystem or KMS |
| Forum kit welcome bot | bot pubkey for welcome flows | TOML + secret store | filesystem or KMS |
| solid-pod-rs-idp issuer | OIDC token signing | `solid-pod-rs/idp/jwks.json` or KMS | filesystem or KMS |
| ActivityPub HTTP-Sig actor | Solid-pod-rs-activitypub actor key | `solid-pod-rs/activitypub/actor.key` | filesystem |

That's ≥10 distinct key roles in a fully-deployed federated configuration. Without an explicit custody + rotation protocol, operators face a choice between (a) generating each key once and never rotating, (b) collapsing all roles to one shared key, or (c) inventing ad-hoc rotation per role with no cross-substrate alignment. All three outcomes are bad.

This ADR specifies the canonical custody options (formalising ADR-080 D6), the rotation protocol (filling in ADR-074 D12), revocation semantics, and the operator runbook + tooling commitments.

## Decision

### D1 — Three custody tiers, one taxonomy

Every long-lived mesh key falls into one of three custody tiers. Operators choose per role; the kit/agentbox/VC tooling supports all three.

**Tier-1 — Filesystem custody** (default; lowest barrier)
- Key persisted as JSON or PEM file on the same host running the consuming process.
- File permissions: **`chmod 0600`, owned by the service user**. Enforced by bootstrap script. Verified at startup; fail-closed on permission drift.
- At-rest encryption: optional via OS-level disk encryption (LUKS, FileVault, BitLocker). NOT in-application encryption (the documented `nostr.key.enc` in agentbox is removed from this ADR; instead, the chmod 0600 + disk encryption combo is the supported model).
- Suitable for: single-host development, low-stakes evaluation, CI runners.

**Tier-2 — Cloud secret store custody**
- AWS Secrets Manager, Cloudflare Workers Secrets, Hashicorp Vault, GCP Secret Manager, Azure Key Vault.
- Process reads key on boot via secret-store SDK; key never written to local disk.
- Audit log surface comes from the secret store provider.
- Suitable for: production CF Workers deployments, CI/CD pipelines, multi-region production.

**Tier-3 — Hardware HSM custody**
- YubiHSM 2, AWS CloudHSM, Ledger / Trezor, Nitrokey, Apple Secure Enclave (macOS-only).
- Key never leaves the HSM; signing operations execute on-device.
- Implies higher latency per signature (typically 5–50 ms); acceptable for low-volume signing (federation handshakes, kind-30033 service-list emissions, weekly rotation announcements) but may not be acceptable for high-volume signing (event-per-DM).
- Suitable for: regulated environments, federation roots, identity-of-record use cases.

A deployment may mix tiers: e.g. operator pubkey in Tier-3 HSM (rotated quarterly), federation key in Tier-2 secret store (rotated weekly), bot keys in Tier-1 filesystem (rotated never).

### D2 — Custody declared per role in TOML

Per-deployment TOML config (per PRD-011 §5.2) gains a `[custody]` table:

```toml
[custody]
# Per-role custody tier; defaults to filesystem ("tier-1") if absent.
operator         = "tier-3"   # HSM
federation       = "tier-2"   # cloud secret store (e.g. CF Workers Secret)
bridge           = "tier-2"
welcome_bot      = "tier-1"
sovereign_agent  = "tier-1"   # agentbox container default

[custody.tier-3]
provider         = "yubihsm" | "cloud-hsm" | "ledger"
config           = "..."

[custody.tier-2]
provider         = "cf-workers-secret" | "aws-secrets-manager" | "vault" | "gcp-secret-manager"
secret_name      = "mesh/{role}/v{version}"   # version suffix gates rotation
```

The `secret_name` template is critical for D6 rotation: incrementing `{version}` in the template causes the next boot to fetch the new key without an in-place rewrite.

### D3 — Key generation centralised

Every key role has exactly one canonical generator. No ad-hoc `openssl rand` invocations.

| Role | Generator |
|------|-----------|
| Forum user nsec | `passkey-PRF` derivation in `forum-client/auth/passkey.rs` (already canonical) |
| Forum admin static | `nostr-bbs-admin keygen --role=admin` (NEW — PRD-011 F8) |
| Agentbox sovereign | `agentbox/scripts/sovereign-bootstrap.py` (existing; F5 fix for x-only npub) |
| Agentbox federation | `agentbox sovereign federation-keygen` (NEW — replaces non-existent `rotate-keys`) |
| VisionClaw operator | `vc-cli identity generate-server` (NEW; reuses `nostr-sdk::Keys::generate`) |
| VisionClaw bridge | `vc-cli identity generate-bridge` (NEW) |
| Mesh federation | `vc-cli mesh generate-federation-key` (NEW) |
| Forum kit federation | `nostr-bbs-admin federation-key generate` (NEW — PRD-011 F8) |
| Forum kit welcome bot | `nostr-bbs-admin bot-key generate` (NEW) |
| solid-pod-rs-idp issuer | `solid-pod-rs-server idp keygen` (existing) |
| AP actor | `solid-pod-rs-server activitypub keygen` (existing) |

All generators MUST:
- Use the platform CSPRNG (`getrandom` Rust, `crypto.randomBytes` JS, `secrets.token_bytes` Python). Never `Math.random()` / `js_sys::Math::random()` (Q2 S-HIGH-3 explicitly prohibits in auth contexts).
- Apply BIP-340 `lift_x` parity correction for x-only Schnorr secp256k1 keys (per ADR-074 D1, Q1 F3.2).
- Emit a key-receipt to stdout: pubkey hex, npub, generation timestamp, custody tier, suggested rotation cadence, intended role.
- NEVER print the private key to stdout or log files.

### D4 — Zeroisation on drop

Every in-memory representation of a private key MUST implement `Zeroize` + `ZeroizeOnDrop`:
- Forum `nostr-core::SecretKey` already does (per Q1 F2.4 review).
- VisionClaw `nostr-sdk` types — verify, add wrapper if missing.
- **solid-pod-rs-idp/src/jwks.rs `SigningKey`** — Q3 G7 finding: missing `Zeroize` on `private_pem`/`private_der`. Fix in solid-pod-rs 0.5 (per ADR-078 S5).
- Agentbox's loadSigner JS — JS does not have `Zeroize`; mitigate by reading + signing + nulling the variable in the smallest possible scope.

### D5 — File permissions enforced

For Tier-1 custody, the consuming process verifies permissions at startup:

```rust
// pseudo-Rust
fn verify_key_file_permissions(path: &Path) -> Result<()> {
    let meta = fs::metadata(path)?;
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o600 && mode != 0o400 {
        return Err(KeyError::FilePermsTooOpen { path, mode });
    }
    let owner = meta.uid();
    if owner != current_uid() {
        return Err(KeyError::FileWrongOwner { path });
    }
    Ok(())
}
```

Bootstrap scripts (sovereign-bootstrap.py, vc-cli) write keys with `umask 0o077` and explicitly chmod `0600` post-write. If permissions drift via operator action, the next boot fails with a clear remediation message.

### D6 — Rotation protocol

Each key has a **rotation cadence** advised by the kit but operator-overridable:

| Role | Cadence | Window |
|------|---------|--------|
| Forum user nsec | rotate-on-passkey-replace | n/a (re-derived) |
| Forum admin static | annually | 7-day overlap |
| Agentbox sovereign | rotate-on-container-rebuild | 24h overlap (legacy key honored during) |
| Agentbox federation | weekly | 24h overlap |
| VisionClaw operator | quarterly | 7-day overlap |
| VisionClaw bridge | monthly | 24h overlap |
| Mesh federation | weekly | 6h overlap |
| Forum kit federation | monthly | 24h overlap |
| Forum kit welcome bot | annually | 7-day overlap |
| solid-pod-rs-idp issuer | quarterly with JWKS dual-publishing | 72h overlap |
| AP actor | annually | 7-day overlap |

Rotation procedure (uniform across roles):

1. **Generate new key** via D3 generator. Store in custody tier per D2.
2. **Sign delegation tag** with old key authorising new key under NIP-26 (kind=relevant, created_at<now+window). This binds the rotation to the cryptographic identity chain.
3. **Publish kind-30033 mesh service-list** (per ADR-074 D9) advertising new key under `service` entries. The replaceable event supersedes the previous entry; receivers update their cached DID Documents within the cache TTL.
4. **Publish kind-30050 IS-Envelope `mesh_ping`** (per ADR-075 D3) with `body.kind = "rotation_announcement"`, payload listing old + new pubkeys, delegation tag, effective timestamp. Federated to all peers.
5. **Run dual-key window**: both keys honoured for the configured overlap. Receivers use whichever signature verifies.
6. **Mark old key deactivated** at end of window: publish a kind-30033 with the old key removed from `service` entries; optionally publish kind-5 (NIP-09 deletion) on the old key's events from its perspective.
7. **Decommission old key**: per custody tier — Tier-1 secure-delete, Tier-2 expire secret version, Tier-3 attestation that the HSM slot is wiped.

The transition window MUST be at least as long as the receiver-cache TTL of resolved DID Documents (default 600s for kind-30033 entries). Cross-substrate clients that haven't refreshed within the window may briefly see signature-verification failures from the old key — failures are non-fatal (events drop silently) but add observability load.

### D7 — Revocation (emergency rotation)

When a key is compromised (or suspected), the operator runs the **emergency revocation** flow — same shape as D6 but with zero overlap window and an explicit revocation announcement:

1. Generate new key; bind via NIP-26 delegation as in D6 step 2.
2. Publish kind-30033 with **both** old key removed AND new key advertised.
3. Publish kind-30050 `mesh_ping` with `body.kind = "revocation"` and `body.reason = "<incident reference>"`.
4. Operators of receiving substrates receive the revocation; their replay-store gates begin rejecting events signed by the old key (additive deny rule applied at ingest).
5. The compromised key remains parseable historically (existing kind-30000-39999 replaceable events stay readable) but no new events from that key are accepted across the mesh.
6. Forensic capture: rotation event preserved in the relay's archived events table for incident review.

The kit ships `nostr-bbs-admin emergency-revoke <role>` as the one-shot revocation primitive; agentbox + VC have analogues.

### D8 — Federation key cardinality bound

**Hard rule**: no key role MAY be reused across roles. Specifically:
- Federation key ≠ operator key (per ADR-073 D4 — peers see federation events authored by federation key, not operator).
- Bridge key ≠ federation key (PRD-010 F9 — bridge attribution distinct from peer-relay session).
- Sovereign agent key ≠ container operator key (per agentbox sovereign architecture).
- Welcome bot key ≠ admin key (kit usability — bot keys may need higher rate limits than admin keys).

Anti-collision lint check: `nostr-bbs-admin custody verify` (per D9 tooling) audits the running deployment and reports if any two roles share a pubkey, with severity HIGH.

### D9 — Operator tooling commitments

Each substrate ships canonical CLI commands:

**Forum kit (`nostr-bbs-admin`)**:
- `keygen --role=<role>` — D3 generator
- `federation-key generate` — convenience for federation role
- `bot-key generate --role=welcome|moderator` — bot keys
- `rotate <role>` — D6 protocol
- `emergency-revoke <role>` — D7 protocol
- `custody verify` — D8 anti-collision audit
- `custody status` — operator-facing key inventory + cadence + last-rotation table

**Agentbox (`agentbox sovereign`)**:
- `bootstrap` — generates sovereign agent key (existing; F5 fix)
- `federation-keygen` — NEW; generates federation key
- `rotate <role>` — implements D6
- `emergency-revoke <role>` — implements D7
- `custody verify`

**VisionClaw (`vc-cli identity` + `vc-cli mesh`)**:
- `identity generate-server` — operator key
- `identity generate-bridge` — bridge key
- `mesh generate-federation-key` — federation key
- `identity rotate <role>` — D6
- `identity emergency-revoke <role>` — D7
- `mesh custody verify`

All commands write structured-JSON to stdout (operator's DevOps pipeline can parse), with optional `--quiet` flag for noisy environments.

### D10 — Observability

Each substrate exposes `/health/keys` returning:

```jsonc
{
  "roles": [
    {
      "role": "operator",
      "pubkey": "did:nostr:abcd...",
      "custody_tier": "tier-3",
      "last_rotated_at": "2026-04-01T00:00:00Z",
      "rotation_due_at": "2026-07-01T00:00:00Z",
      "days_until_due": 55,
      "transition_active": false
    },
    {
      "role": "federation",
      "pubkey": "did:nostr:ef12...",
      "custody_tier": "tier-2",
      "last_rotated_at": "2026-05-04T00:00:00Z",
      "rotation_due_at": "2026-05-11T00:00:00Z",
      "days_until_due": 4,
      "transition_active": false
    }
  ],
  "anti_collision": "ok",
  "permission_audit": "ok"
}
```

Rotation due dates feed Prometheus `mesh_key_rotation_due_seconds{role}` gauges; alerts fire at 7-day and 1-day thresholds.

### D11 — Test surface

Per ADR-077 P1+P5:
- Reference vector tests for NIP-26 delegation construction in rotation announcements (paulmillr conditions=`kind=30033`).
- Property tests: rotation flow preserves signature chain; emergency revocation drops new old-key events; concurrent rotations are detected.
- Cross-substrate contract test: forum kit federation key rotation → agentbox + VisionClaw observe the kind-30033 update + accept new federation events.
- Mutation testing target ≥80% on rotation flow code.

### D12 — Migration from current state

Existing DreamLab deployment has at minimum:
- Forum admin static pubkey (D1)
- Pod-worker WebID-bound pubkey (effectively operator)
- VisionClaw `SERVER_NOSTR_PRIVKEY` (operator) + `VISIONCLAW_NOSTR_PRIVKEY` (bridge — to be unified per PRD-010 F1)
- Agentbox sovereign agent key (broken npub; F5 fix)

Migration path:
1. Phase 0 (PRD-010 P0 gating): F1 unification, F5 npub fix, F4 type-string fix. NO new keys generated.
2. Phase 1: deploy `nostr-bbs-admin custody verify` against existing DreamLab forum; classify each pubkey by role; decide custody tier per role.
3. Phase 2: generate new federation + bridge keys (these are NEW roles; no migration concern).
4. Phase 3: rotate operator key on the kit's standard cadence (quarterly); old operator key stays valid for 7-day overlap.
5. Steady state: rotation cadences per D6 timetable.

Existing forum users (passkey-PRF nsecs) are unaffected — rotation applies only to operator/federation/bridge/bot roles.

## Consequences

### Positive

- **Defence-in-depth restored**: 10+ distinct key roles each with explicit tier + rotation cadence; no operator collapse to a single shared key.
- **Cross-substrate alignment**: kit, agentbox, VisionClaw all use the same custody taxonomy + rotation protocol shape; documentation matches reality.
- **Observability-first**: `/health/keys` + Prometheus gauges expose rotation lag operationally; alerts fire before keys go stale.
- **Emergency response codified**: D7 revocation flow gives operators a concrete one-shot command for compromise scenarios.
- **Anti-collision enforced**: D8 lint catches role-key reuse before deployment, addressing the Q3 §I12 collapse-risk warning.
- **At-rest encryption story coherent**: Tier-1 chmod 0600 + OS disk encryption replaces the documented-but-non-existent `nostr.key.enc` AES-256-GCM layer with something operators can actually verify.

### Negative

- **Tooling implementation cost**: 3 substrates × ~5 CLI commands each = ~15 new commands to implement. Phase 1 of PRD-010 + PRD-011.
- **Operator runbook complexity**: rotation cadence + transition windows require operator awareness; documentation burden (per substrate `docs/deployment/key-rotation.md`).
- **HSM cost barrier**: Tier-3 hardware ($50–$650 per device per role) creates a deployment-ladder where small operators stay on Tier-1 and miss out on the strongest custody.
- **Cross-substrate rotation choreography**: weekly federation key rotation requires all peer substrates to refresh DID-Document caches within the 6-hour overlap window; misconfigured cache TTL = federation flap.

### Neutral

- **Existing forum users unaffected**: passkey-PRF nsecs sit outside this ADR's scope; rotation is operator/federation/bridge/bot-only.
- **JWKS handling continues per OAuth standards** for solid-pod-rs-idp (RFC 7519 + standard `kid` header rotation); no Nostr-specific overlay needed there.

## Alternatives Considered

### Alt-A — Single shared operator key across all roles
Use `SERVER_NOSTR_PRIVKEY` for everything; eliminate per-role keys.

*Rejected*: defeats every defence-in-depth boundary. Per ADR-073 D4 explicitly: "federation events are authored by federation key, not operator key" — sharing keys means a compromised federation worker leaks the operator key.

### Alt-B — Mandatory HSM-only custody for federation roles
Force every federation role into Tier-3 HSM.

*Rejected*: pushes deployment cost too high for evaluators and small operators. Tier-2 cloud secret store is a 95th-percentile defensible custody for federation; HSM raises bar to enterprise without proportional security gain in most operator threat models.

### Alt-C — In-application AES-GCM at-rest encryption (the original PRD-001 §378 plan)
Keep keys encrypted on filesystem with key-encryption-key.

*Rejected*: KEK custody becomes the actual problem. If KEK is in env: same-host visibility; if KEK is in HSM: just use HSM directly. The chmod 0600 + OS disk encryption combination matches what most operators expect and verifies.

### Alt-D — Per-event ephemeral keys
Generate fresh keys per event (per the NIP-59 wrap pattern, generalised).

*Rejected*: NIP-59 already does this for wraps; gift-wrap fan-out creates new throwaway keys per message. Federation/operator roles need persistent identity for replay protection + admin authority. Generalising ephemeral pattern breaks identity continuity.

### Alt-E — Threshold signatures (FROST / MuSig)
Distribute key shares; require N-of-M to sign.

*Rejected*: ecosystem support (rust-nostr, nostr-tools) doesn't yet ship Schnorr threshold primitives. Worth revisiting as a future ADR when nostr-rs/secp256k1 ecosystem catches up.

### Alt-F — Skip rotation entirely; rely on revocation when compromised
Generate keys once; only emergency-rotate.

*Rejected*: keys leak slowly via observed event metadata, side-channel attacks, employee turnover, etc. Quarterly cadence for high-stakes roles is cheap insurance. ADR-074 D12 already assumed scheduled rotation; this ADR formalises it.

## Implementation notes

### Per-substrate boot sequence

```
1. Read TOML [custody] section; per role, resolve tier.
2. For each role:
   a. Tier-1: read filesystem, verify perms (D5), zeroise on drop (D4).
   b. Tier-2: SDK call to secret store, parse, hold in-memory only.
   c. Tier-3: open HSM connection, hold session handle (no private bytes ever in process).
3. Run D8 anti-collision check; abort startup if any role-pubkey duplicates.
4. Run D5 permission verifier; abort startup on perms drift.
5. Compute key_age = now - last_rotated_at per role; emit metrics; warn at 75% of cadence elapsed.
```

### Rotation runbook template

For each role's rotation, ship a runbook at `docs/deployment/runbooks/rotate-{role}.md`:

```
# Rotate <role> Key

## Pre-flight
- [ ] Confirm `mesh custody verify` reports OK
- [ ] Confirm `mesh_key_rotation_due_seconds{role="<role>"}` is positive (not overdue)
- [ ] Notify on-call: rotation window starts at <time>, ends at <time + overlap>

## Rotate
- [ ] Run `<substrate>-admin keygen --role=<role>`
- [ ] Verify key receipt: pubkey, npub, timestamp, custody tier
- [ ] Run `<substrate>-admin rotate <role> --new-pubkey=<hex>`
- [ ] Verify kind-30033 published (peer relays log receipt)
- [ ] Verify kind-30050 mesh_ping published

## Verify
- [ ] All peers report `transition_active: true` for <role>
- [ ] No mesh_peer_unreachable_total{role="<role>"} increment in last 5min
- [ ] Test event signed with new key — accepted by all peers

## Decommission
- [ ] After overlap window: run `<substrate>-admin rotate <role> --decommission-old`
- [ ] Verify kind-30033 reflects single advertised key per service
- [ ] Tier-1: shred old key file; Tier-2: expire secret version; Tier-3: HSM attestation

## Sign-off
- [ ] Update `docs/operations/key-rotation-log.md` with completion timestamp
- [ ] Notify on-call: rotation complete
```

### Failure recovery

- **Failed mid-rotation**: old key stays valid (transition_active tracking persists). Operator reverts new-key publish via kind-30033 supersession or runs explicit rollback CLI command.
- **Lost new key during transition**: emergency revocation D7 against the new key (which has not signed any production traffic yet); old key stays canonical.
- **Lost old key**: emergency revocation against old; transition window collapses to zero. Acceptable but sub-optimal.

### Incident-response logging

All rotation events emit structured logs to standard observability stack (Loki, Datadog, etc.):
```
{event: "key.rotation.started", role: "...", actor: "operator-pubkey", timestamp: "..."}
{event: "key.rotation.kind_30033_published", relay: "...", event_id: "..."}
{event: "key.rotation.kind_30050_published", relay: "...", event_id: "..."}
{event: "key.rotation.transition_complete", role: "...", duration_seconds: 86400}
{event: "key.rotation.decommissioned", role: "...", actor: "operator-pubkey"}
```

## References

- PRD-010 — DID:Nostr Mesh Federation, R7
- PRD-011 — VisionFlow Forum Kit Extraction, G7
- ADR-073 — Mesh topology (D4 federation key per relay; D11 health probes)
- ADR-074 — DID:Nostr canonicalisation (D12 key rotation announcement; D9 kind-30033 mesh service-list)
- ADR-076 — `nostr-core` absorption
- ADR-077 — Ecosystem QE Policy (P1 reference vectors; P5 property/fuzz)
- ADR-078 — Cross-substrate library convergence (S5 jwks Zeroize fix)
- ADR-079 — Forum-Setup Skill Provider Abstraction
- ADR-080 — Forum Kit Deployment Topology Patterns (D6 custody options)
- `docs/integration-research/qe-fleet/Q1-crypto-protocol-audit.md` F2.4, F3.2, F3.10
- `docs/integration-research/qe-fleet/Q2-security-primitive-audit.md` S-HIGH-3, S-HIGH-6
- `docs/integration-research/qe-fleet/Q3-identity-custody-audit.md` §I12, CRITICAL-03, G6, G7
- BIP-340 — Schnorr signatures for secp256k1
- NIP-26 — Delegated event signing
- W3C DID Core — https://www.w3.org/TR/did-core/
- GitHub repos:
  - https://github.com/DreamLab-AI/nostr-rust-forum (kit `nostr-bbs-admin` CLI)
  - https://github.com/DreamLab-AI/agentbox (`agentbox sovereign` CLI)
  - https://github.com/DreamLab-AI/VisionClaw (`vc-cli` CLI)
  - https://github.com/DreamLab-AI/solid-pod-rs (Zeroize fix landing in 0.5)
  - https://github.com/DreamLab-AI/dreamlab-ai-website (downstream consumer of D12 migration)
