# Server Nostr Identity — Operator Guide

> ADR-050 §"Server-as-identity" and UQ5 ratification.
>
> VisionClaw owns a dedicated Nostr keypair used to sign server-issued
> events: migration approvals (kind 30023), bridge promotions (30100),
> bead provenance stamps (30200), and audit records (30300). Third
> parties verify authenticity by resolving the server's public key at
> `GET /api/server/identity`.

## Contents

1. [Key lifecycle](#key-lifecycle)
2. [Environment variables](#environment-variables)
3. [Generating a production key](#generating-a-production-key)
4. [Storing the secret (Docker Swarm / compose)](#storing-the-secret)
5. [Rotating the key](#rotating-the-key)
6. [Verifying third-party events](#verifying-third-party-events)
7. [Event kinds and payload schemas](#event-kinds-and-payload-schemas)
8. [Security model](#security-model)

---

## Key lifecycle

| Phase | Who | What |
|-------|-----|------|
| Generation | operator | 32-byte secp256k1 secret via nostr-tools / nak / nostril |
| Provisioning | operator | Store secret as Docker secret / k8s secret / file mode 0400 |
| Loading | VisionClaw | `ServerIdentity::from_env()` at startup; fatal if missing in prod |
| Usage | VisionClaw | `ServerNostrActor` signs migration / bridge / bead / audit events |
| Publication | VisionClaw | Pubkey exposed at `GET /api/server/identity` |
| Rotation | operator | Swap `SERVER_NOSTR_PRIVKEY`, restart server, re-announce pubkey |

---

## Environment variables

| Variable | Required | Default | Purpose |
|----------|----------|---------|---------|
| `SERVER_NOSTR_PRIVKEY` | yes in prod | — | 64-char hex secret key |
| `SERVER_NOSTR_AUTO_GENERATE` | no | `false` | dev-only: generate a fresh key per boot if `SERVER_NOSTR_PRIVKEY` is absent |
| `NOSTR_RELAY_URLS` | recommended | — | comma-separated `wss://` / `ws://` relays for broadcast |

Rules:

- `SERVER_NOSTR_PRIVKEY` must be **64 hex characters**. Bech32 `nsec1…` is **not** accepted here — use the hex form. (For `nsec`, see the `POWER_USER_NSEC` path in `vc_cli`.)
- `SERVER_NOSTR_AUTO_GENERATE=true` is for local development only. The generated secret lives only in memory, is never persisted, and every restart yields a brand-new identity — third parties who cached the old pubkey will no longer trust signed events.
- Invalid relay URLs (missing scheme) are logged and skipped at startup; they do not fail the boot.
- `NOSTR_RELAY_URLS` empty means "sign but do not broadcast." Signed events are still returned to callers, but no relay sees them.

### Example (development)

```bash
# .env
SERVER_NOSTR_AUTO_GENERATE=true
NOSTR_RELAY_URLS=ws://jss:3030/relay
```

### Example (production)

```bash
# .env (loaded from Docker secret; see below)
SERVER_NOSTR_PRIVKEY=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
NOSTR_RELAY_URLS=wss://relay.damus.io,wss://nos.lol
```

---

## Generating a production key

Three options — pick whichever you trust.

### Option 1 — `nak` (Go, self-contained)

```bash
go install github.com/fiatjaf/nak@latest
nak key generate
# prints: nsec1…  and  npub1…
# Convert nsec → hex:
nak encoding nsec-to-hex nsec1…
```

### Option 2 — `nostril` (C, small)

```bash
nostril --genkey
# prints hex secret and hex pubkey
```

### Option 3 — JS one-liner (nostr-tools)

```bash
npx -y nostr-tools-cli gen
```

All three produce **32 bytes of entropy** encoded as 64 hex characters. Store the hex form as `SERVER_NOSTR_PRIVKEY`.

After generation, **record the public key in three places**:

1. Your runbook / internal docs.
2. The operator's password manager (1Password / Bitwarden) alongside the secret.
3. Announce it publicly (static `/.well-known/nostr/server.json`, social post, whatever matches your deployment story).

---

## Storing the secret

**Docker Compose (recommended for single-host)**

```yaml
secrets:
  server_nostr_privkey:
    file: ./secrets/server_nostr_privkey.hex

services:
  visionclaw:
    secrets:
      - server_nostr_privkey
    environment:
      # point the env var at the secret file; the container's init script
      # should `export SERVER_NOSTR_PRIVKEY=$(cat /run/secrets/server_nostr_privkey)`
      SERVER_NOSTR_PRIVKEY_FILE: /run/secrets/server_nostr_privkey
      NOSTR_RELAY_URLS: "wss://relay.damus.io,wss://nos.lol"
```

```bash
# Create the secret file with strict perms:
install -m 0400 <(echo -n "$(nak key generate-hex)") ./secrets/server_nostr_privkey.hex
```

**Kubernetes**

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: server-nostr-privkey
type: Opaque
stringData:
  SERVER_NOSTR_PRIVKEY: 0123…  # 64 hex chars
---
# In the Deployment:
env:
  - name: SERVER_NOSTR_PRIVKEY
    valueFrom:
      secretKeyRef:
        name: server-nostr-privkey
        key: SERVER_NOSTR_PRIVKEY
```

**Plain file (dev)**

```bash
chmod 0400 .env
# .env contains SERVER_NOSTR_PRIVKEY=…
```

Never commit `.env` or the secret file. `.env.example` documents the schema only.

---

## Rotating the key

Key rotation is a coordinated event because third parties cache the server pubkey.

1. Generate a new keypair (see above).
2. **Pre-announce** the upcoming rotation over any authenticated channel you already have (mailing list, signed Nostr note from the old key, git commit signed by a known maintainer).
3. Update the secret in your secret store (`docker secret rm … && docker secret create …` / `kubectl apply -f new-secret.yaml`).
4. Roll the server. `ServerIdentity::from_env()` logs the new pubkey at startup — grep for `Loaded server Nostr identity` in the journal.
5. Announce the new pubkey at `GET /api/server/identity`. Third parties will see the change on their next fetch; clients that cache long-term should be prompted to refresh.
6. Keep the old keypair around for a grace period — third parties may need it to verify historical events. Historical events remain valid forever; the old pubkey remains the correct verifier for events signed before rotation.

Rotations should be rare (annual or on suspected compromise).

---

## Verifying third-party events

Any Nostr-aware client can verify an event claiming to come from the VisionClaw server:

```bash
# 1. Fetch the authoritative pubkey.
curl -s https://visionclaw.example/api/server/identity | jq .

# 2. For a Nostr event received from a relay, check the pubkey matches and
#    run signature verification with any library:
nak verify <event.json>     # via nak
```

In Rust (inside VisionClaw tests):

```rust
use nostr_sdk::prelude::*;

let ev: Event = serde_json::from_str(raw_json)?;
assert_eq!(ev.pubkey.to_hex(), expected_server_pubkey_hex);
ev.verify()?;
```

The server pubkey is the only piece of information a third party needs. **Do not rely on the relay URL** — relays are untrusted transports.

---

## Event kinds and payload schemas

| Kind | Name | Content JSON | Tags |
|------|------|--------------|------|
| `30023` | Migration approval (NIP-23 addressable) | `{"migration_id","bridge_iri","confidence"}` | `d=migration_id`, `e=bridge_iri` |
| `30100` | Bridge-to promotion | `{"from_kg","to_owl","signals"}` | `from=from_kg`, `to=to_owl` |
| `30200` | Bead provenance stamp | `{"bead_id","payload_hash"}` | `bead=bead_id`, `sha256=payload_hash` |
| `30300` | Audit record | `{"action","details"}` | `action=<name>`, `actor=<pubkey-or-empty>` |

All four kinds are parameterised replaceable (30000–39999 range) with a `d` tag where it exists, meaning re-publishes overwrite the previous entry on the relay rather than creating duplicates.

---

## Security model

- **Confidentiality.** The secret key is loaded once at startup and lives inside an `nostr_sdk::Keys` value. It is never written to stdout, stderr, the structured log, or any HTTP response. `log::info!` calls include only the **public** key.
- **Integrity.** Every signed event is verified by the library (`event.verify()`) before relays accept it. Third parties must independently re-verify.
- **Availability.** Relay failures never fail the caller — `ServerIdentity::sign_and_broadcast` returns the signed event regardless. Callers who need stricter delivery semantics can retry by re-calling the actor.
- **Non-repudiation.** Because the key is controlled by a single process, everything it signs is attributable to the VisionClaw deployment. Audit kind 30300 records include the originating actor's pubkey (where known) so the server signature acts as a notarisation witness on top of the underlying operator's claim.
- **Blast radius on compromise.** An attacker holding the secret can forge all four event kinds. The mitigation is prompt rotation + a canary: the operator should include a periodic signed heartbeat (kind 30300 `action=heartbeat`) so third parties can detect silent takeover via freshness gaps.

---

## Related files

- [`src/services/server_identity.rs`](../../src/services/server_identity.rs)
- [`src/actors/server_nostr_actor.rs`](../../src/actors/server_nostr_actor.rs)
- [`src/handlers/server_identity_handler.rs`](../../src/handlers/server_identity_handler.rs)
- `docs/adr/ADR-050-sovereign-schema.md` §"Server-as-identity"
