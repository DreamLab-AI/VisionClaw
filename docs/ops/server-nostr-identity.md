# Server Nostr Identity — Operations Guide

The VisionClaw server holds its own Nostr keypair and signs a defined set of
operational events with it. Third parties can verify those events without
trusting any individual human user.

| Kind  | Purpose                                  | Issuer         |
|-------|------------------------------------------|----------------|
| 30001 | Bead provenance (JSS → forum relay)      | Bridge / user  |
| **30023** | **Migration approvals**              | **Server**     |
| **30100** | **BRIDGE_TO promotions (KG → OWL)** | **Server**     |
| **30200** | **Bead provenance stamps**           | **Server**     |
| **30300** | **Audit records**                    | **Server**     |

Events in **bold** are server-signed. They carry the server pubkey (advertised
publicly at `GET /api/server/identity`) and each includes an `h` tag of
`visionclaw-server` plus kind-specific tags (see below).

## Generating the server key

### Using `nak` (Go CLI, recommended)

```bash
nak key generate
# Prints both nsec1... and hex secret key + npub1... pubkey.
```

Store **only** the secret (`nsec1…` or the hex). Never commit it.

### Using a tiny Rust snippet

```rust
use nostr_sdk::prelude::*;

fn main() {
    let keys = Keys::generate();
    println!("nsec: {}", keys.secret_key().to_bech32().unwrap());
    println!("npub: {}", keys.public_key().to_bech32().unwrap());
}
```

## Storing the key

### Docker secrets (production)

```yaml
# docker-compose.yml
services:
  visionclaw:
    secrets:
      - server_nostr_privkey
    environment:
      # Do NOT inline the key. Read the secret file at runtime.
      SERVER_NOSTR_PRIVKEY_FILE: /run/secrets/server_nostr_privkey

secrets:
  server_nostr_privkey:
    file: ./secrets/server_nostr_privkey.txt  # nsec1... single line
```

Then in your entrypoint:

```bash
export SERVER_NOSTR_PRIVKEY="$(cat "$SERVER_NOSTR_PRIVKEY_FILE")"
exec /usr/local/bin/visionclaw-server
```

### Development

Set `SERVER_NOSTR_AUTO_GENERATE=true` in a non-production env. A fresh
ephemeral key is generated at startup. The pubkey is logged; the secret is
never logged and is lost on restart (by design — it is ephemeral).

In `APP_ENV=production` auto-generate is **rejected**. Missing
`SERVER_NOSTR_PRIVKEY` aborts startup.

## Key rotation

1. Generate a new key on a secure host.
2. Deploy the new `SERVER_NOSTR_PRIVKEY` to the **standby** instance.
3. Publish a kind-30300 audit event from the **old** key with
   `action=key_rotation_begin, new_pubkey=<new_hex>, deprecate_at=<iso8601>`.
4. Cut traffic to the standby.
5. After the grace period (≥ 7 days), stop honouring signatures from the old
   key in any verifier. The old key is then retired.

Because all server-issued events carry the pubkey in the event itself,
verifiers can unambiguously tell which key signed what. Rotation does not
invalidate historical events.

## Third-party verification

```bash
# 1. Fetch the server's current pubkey
curl https://visionclaw.example.com/api/server/identity
# → { "pubkey_hex": "...", "pubkey_npub": "npub1...",
#     "supported_kinds": [30023, 30100, 30200, 30300],
#     "relay_urls": ["wss://relay.damus.io", ...] }
```

Given any event with `kind ∈ supported_kinds`:

```rust
use nostr_sdk::prelude::*;

fn verify_server_event(event_json: &str, expected_pubkey_hex: &str) -> anyhow::Result<()> {
    let event = Event::from_json(event_json)?;
    event.verify()?;
    if event.pubkey.to_hex() != expected_pubkey_hex {
        anyhow::bail!("event not signed by server pubkey");
    }
    Ok(())
}
```

The server **does not** re-sign user events. Any event with a user pubkey was
signed by that user — the server's identity is strictly additive and
orthogonal to user auth.

## What the server signs vs what users sign

| Scenario                                        | Signer           |
|-------------------------------------------------|------------------|
| Migration candidate approval                    | Server (30023)   |
| BRIDGE_TO KG→OWL promotion                      | Server (30100)   |
| Bead stamp witness                              | Server (30200)   |
| System audit record (cron, reconciliation, etc) | Server (30300)   |
| Bead creation                                   | User / bridge    |
| NIP-98 HTTP auth                                | User             |
| NIP-07 login                                    | User             |

## Environment reference

```bash
# Required in production
SERVER_NOSTR_PRIVKEY=nsec1abcdef...        # or 64-char hex

# Dev-only
SERVER_NOSTR_AUTO_GENERATE=false           # true generates ephemeral key

# Relay publication (optional)
NOSTR_RELAY_URLS=wss://relay.damus.io,wss://nos.lol
```

If `NOSTR_RELAY_URLS` is empty, signing still works — events are returned to
callers but not published. This keeps unit tests and offline development
frictionless.

## Operational guarantees

* The private key is loaded once at startup and never written back to any
  storage medium by this service.
* No log line, no HTTP response, and no error message contains the private
  key. Search the codebase for `SERVER_NOSTR_PRIVKEY` — the variable is read
  exactly once.
* Relay publication failures are logged but never propagated to callers: the
  signed event is the authoritative artefact and is always returned.
* `GET /api/server/identity` is unauthenticated by design — this is public
  identity info equivalent to advertising a PGP fingerprint.
