# Agentbox Communication Surfaces

> Snapshot date: 2026-05-07. Submodule rev under `agentbox/`. Source: `github.com/DreamLab-AI/agentbox`.
>
> Status legend: **ACTUAL** = code exists, exercised by image. **PLANNED** = ADR/PRD/DDD specifies it but no shipping code yet (or written and not wired into the boot path).

All paths in this document are absolute under `/home/devuser/workspace/project/agentbox/` unless explicitly noted as a container-internal path (e.g. `/etc/agentbox.toml`, `/var/lib/solid`).

---

## 1. Sovereign identity bootstrap (did:nostr key generation)

**ACTUAL.** Generation, persistence, and DID-document materialisation all happen in one Python script invoked exactly once per container boot.

### Where keys are generated

`agentbox/scripts/sovereign-bootstrap.py` is the canonical mint point. It is gated by `[sovereign_mesh].enabled = true`:

- `sovereign-bootstrap.py:226-240` — `main()` short-circuits when `sovereign_mesh.enabled = false`; otherwise computes `agent_id` (default `"agentbox-core"`), `identity_root = /var/lib/agentbox/identities`, `pod_root = ${SOLID_POD_ROOT|/var/lib/solid}/pods`, `run_root = /run/agentbox`, then calls `ensure_identity → ensure_acl → write_runtime_env`.
- `sovereign-bootstrap.py:95-137` — `ensure_identity(agent_id, identity_root)`. Three precedence rules:
  1. `AGENTBOX_PRIVKEY_HEX` env (64-char hex) — if set, decoded directly via `_keypair_from_privkey_hex` (line 81).
  2. `AGENTBOX_NSEC` env (bech32 `nsec1...`) — `bech32_decode` on lines 105-107, validated as 32 bytes.
  3. Persisted file at `/var/lib/agentbox/identities/<agent_id>.json` — re-read on subsequent boots (line 117).
  4. Fresh `SigningKey.generate(curve=SECP256k1)` if none of the above (line 123).
- The keypair tuple written is `{ agent_id, created_at, private_key_hex, public_key_hex, x_only_pubkey_hex, nsec, npub }` (lines 127-135). The bech32 encoder is hand-rolled in the same file (`bech32_encode`, lines 50-53) — no `nostr-tools` dep on the Python side.

### Persistence

- File: `/var/lib/agentbox/identities/<agent_id>.json` — owner-readable JSON, includes private key hex. Mounted as named volume `sovereign-identities` (`docker-compose.yml:89`, mapping volume `agentbox-sovereign-identities`). Survives container restarts; nuked only by `docker volume rm`.
- Stage A bootstrap sequence (`config/entrypoint-unified.sh:201-202`):
  ```
  echo "[2/8] Bootstrapping sovereign mesh identity..."
  python3 /opt/agentbox/scripts/sovereign-bootstrap.py
  ```
  Runs as root (PID 1 supervisord parent) before any service starts.

### DID document publication

Two parallel artefacts are written by `ensure_acl()` (`sovereign-bootstrap.py:140-203`):

1. **Per-pod DID document** at `pods/<npub>/did-nostr.json` (lines 183-203), shape:
   ```json
   {
     "@context": ["https://www.w3.org/ns/did/v1",
                  "https://w3id.org/security/suites/secp256k1-2019/v1"],
     "id": "did:nostr:<x_only_pubkey_hex>",
     "verificationMethod": [{ "type": "SchnorrSecp256k1VerificationKey2022", ... }],
     "alsoKnownAs": ["http://localhost:8484/pods/<npub>/profile.json"]
   }
   ```
   This is consumed by `solid-pod-rs`'s `did-nostr` Cargo feature (`enable_did_nostr = true` in `agentbox.toml:201`) which serves it at `GET /did:nostr:<hex>` on port 8484.

2. **Pod profile** at `pods/<npub>/profile.json` (lines 171-179), and a default WAC ACL at `pods/<npub>/.acl.json` (lines 157-170) granting `[Read, Write, Append, Control]` to the owner DID `did:nostr:<hex_pubkey>`.

### Public DID document at /.well-known/did.json

`management-api/server.js:206-210` declares the path public (auth-gated everywhere else):

```js
if (request.url === '/.well-known/did.json') {
  return;  // skip auth
}
```

But there is **no Fastify route handler** in `management-api/routes/` that serves this path — `grep -rn "fastify\.get.*\.well-known"` returns zero handlers. Resolution flows through solid-pod-rs at port 8484 instead:

- `routes/uri-resolver.js:67-69` redirects `did:nostr:*` URNs with `307 → ${podBase}/.well-known/did.json`.
- `lib/uris.js:216-219` — `resolveCanonical` does the same lookup.

So the **operative DID endpoint is the pod-server's** `/.well-known/did.json` on port 8484, not the management-api on 9090. The management-api auth bypass on line 208 is a placeholder for a future first-party route.

### Runtime env propagation

`sovereign-bootstrap.py:206-223` writes `/run/agentbox/identity.env`:

```
export AGENTBOX_AGENT_ID=...
export AGENTBOX_NPUB=...
export AGENTBOX_NSEC=...
export AGENTBOX_PUBKEY_HEX=...
export AGENTBOX_X_ONLY_PUBKEY_HEX=...
export AGENTBOX_DID=did:nostr:<hex>
export AGENTBOX_URN=urn:agentbox:agent:<agent_id>
```

These are sourced by `flake.nix:1736,1743,1754,1765-1766` (one each for bash/sh/fish startup).

`sovereignKeyset` aggregate-of-record per `DDD-003 §AgentIdentity` (`docs/reference/ddd/DDD-003-sovereign-messaging-domain.md:62-79`).

---

## 2. nostr-rs-relay deployment (process, port, config, federation modes)

**ACTUAL package + config + supervisord block.** **NOT exposed externally** in the shipped compose file.

### Package selection

`flake.nix:699-726` resolves the relay implementation:

- `relayEnabled = relayCfg.enabled or false` — gate on `[sovereign_mesh.relay].enabled`.
- `relayImpl = relayCfg.implementation or "nostr-rs-relay"`.
- `relayLocal = relayEnabled && (relayImpl == "nostr-rs-relay" || relayImpl == "rnostr")`.
- Hard fail if `rnostr` requested but not in nixpkgs (lines 710-719).
- `relayPkg = pkgs.nostr-rs-relay` (line 722) — i.e. nixpkgs-tracked, currently v0.9.0 per `ADR-009:36-37`.

### Generated relay config

`flake.nix:728-781` renders `/etc/agentbox/nostr-relay.toml` from manifest fields:

```toml
[info]
name        = "agentbox-relay"
description = "${relayCfg.info_description}"
relay_url   = "ws://${relayCfg.bind|127.0.0.1}:${relayCfg.port|7777}/"

[network]
address        = "${relayCfg.bind|127.0.0.1}"
port           = ${relayCfg.port|7777}

[database]
engine         = "sqlite"
data_directory = "${relayCfg.data_dir|/var/lib/nostr-relay}"

[limits]
messages_per_sec      = ${relayCfg.messages_per_sec|5}
max_event_bytes       = ${relayCfg.max_event_bytes|131072}

[authorization]
nip42_auth = ${"true" if ingress_policy != "open" else "false"}
nip42_dms  = ${"true" if allow_nip04 else "false"}

[retention]
default_days = ${relayCfg.retention_days|30}
```

Materialised onto disk at `flake.nix:1699`:
```nix
cp ${pkgs.writeText "nostr-relay.toml" relayConfigText} $out/etc/agentbox/nostr-relay.toml
```

### Supervisord block

`flake.nix:1129-1141` (gated on `relayLocal`):

```ini
[program:nostr-relay]
command=${relayPkg}/bin/nostr-rs-relay --config /etc/agentbox/nostr-relay.toml
directory=${relayCfg.data_dir or "/var/lib/nostr-relay"}
user=devuser
environment=HOME="/home/devuser",RUST_LOG="info",AGENTBOX_REQUIRED_FOR_READINESS="false"
autostart=true
autorestart=true
priority=35
```

Priority 35 = starts after solid-pod (priority 30) and management-api (priority 20).

### Effective config from agentbox.toml

`agentbox.toml:87-104`:

```toml
[sovereign_mesh.relay]
enabled          = true
implementation   = "nostr-rs-relay"
port             = 7777
bind             = "127.0.0.1"
expose           = false
data_dir         = "/var/lib/nostr-relay"
ingress_policy   = "allowlist"
allowed_pubkeys  = []                  # operator pubkey auto-added at boot
allowed_kinds    = [1, 1059, 30078, 27235, 38000, 38100]
pod_bridge       = true
external_fanout  = "off"
max_event_bytes  = 131072
messages_per_sec = 5
retention_days   = 30
allow_nip04      = false
```

### Port exposure (the gap)

`docker-compose.yml:19-26` publishes only:
```yaml
ports:
  - "9090:9090"   # management-api
  - "9700:9700"   # ruvector
  - "9091:9091"   # metrics
  - "8484:8484"   # solid-pod-rs
  - "8888:8888"   # jupyter
  - "5901:5901"   # vnc
  - "8080:8080"   # code-server
```

**Port 7777 is not exposed.** `flake.nix:1968-1970` only publishes 8484:
```nix
sovereignPorts = lib.optionalAttrs (sovereignCfg.enabled or false) {
  "8484/tcp" = {};
};
```

So the embedded relay is currently reachable **only from inside the container** via `ws://127.0.0.1:7777`. The bridge consumes it locally; no external Nostr client can dial it without (a) flipping `sovereign_mesh.relay.expose = true` and `bind = "0.0.0.0"`, AND (b) adding `7777:7777` to the compose ports list AND (c) opening `commonPorts`/`sovereignPorts` in `flake.nix`.

### NIP support matrix (claimed, ACTUAL on relay binary)

Per `ADR-009:138-148`:

| NIP | Supported by `nostr-rs-relay` 0.9.0 |
|-----|--------------------------------------|
| NIP-01 | yes |
| NIP-11 | yes (HTTP `GET /` returns relay info doc) |
| NIP-42 | yes (mandatory when `ingress_policy != "open"`) |
| NIP-04 | configurable; off by default (`allow_nip04 = false`) |
| NIP-17 | yes (read; bridge decrypts) |
| NIP-40 | yes (expiration tag honoured for TTL) |
| NIP-50 | no |

### wss URL pattern (PLANNED, not active)

There is no TLS terminator in front of the relay. `info.relay_url = "ws://..."` (not `wss://`) is hardcoded in `flake.nix:745`. For external reachability operators would need to pair the relay with the `https-bridge` (priority-32 supervisord block at `flake.nix:1086-1098`) which currently bridges only port 9090 (management-api), not 7777.

---

## 3. pod-inbox bridge (ADR-009)

**ACTUAL implementation file. NOT YET wired into the management-api boot path.**

### File and contract

`agentbox/mcp/nostr-bridge/relay-consumer.js` (473 lines) implements the full bidirectional bridge per `ADR-009:84-124`:

- `relay-consumer.js:55-58` — top-level imports of `NostrBridge`, `loadSigner`, `kinds` from `mcp/servers/nostr-bridge.js`.
- `relay-consumer.js:60-64` — agent-intent kind ranges:
  ```js
  const AGENT_INTENT_MIN  = 38000;
  const AGENT_INTENT_MAX  = 38099;
  const AGENT_RESPONSE_MIN = 38100;
  const AGENT_RESPONSE_MAX = 38199;
  ```
- `relay-consumer.js:66-68` — `DEFAULT_POD_ROOT = process.env.SOLID_POD_ROOT || '/var/lib/solid'`.

### Inbound path — kinds watched

`relay-consumer.js:130-137` defaults the subscription filter:
```js
return [
  1,                                  // general notes
  1059,                               // NIP-17 gift wrap DMs
  30078,                              // agent state (NIP-33)
  AGENT_INTENT_MIN, AGENT_RESPONSE_MIN,
];
```

This matches `agentbox.toml:96`'s `allowed_kinds = [1, 1059, 30078, 27235, 38000, 38100]`.

### Inbound checks (DDD-003 invariants enforced)

`relay-consumer.js:183-291`:

- **I01** (signature-before-write): `_verifySig` (line 322) calls `nostr-tools.verifyEvent`; structural-only acceptance falls back when nostr-tools missing.
- **I07** (allowlist): `_passesIngressPolicy` (line 339) — three modes (`open` / `signed-only` / `allowlist`).
- **I10** (recipient match): `_findRecipientNpub` (line 353) walks `event.tags` for `["p", <hex|npub>]` and matches the local set.
- **I08** (content-addressed dedup): inbox path is `pods/<recipient>/events/inbox/<event.id>.json`; existing files short-circuit (line 207).
- Atomic-rename write: `fs.writeFileSync(tmpPath, ...)` then `fs.renameSync(tmpPath, finalPath)` (lines 215-222).

### Inbox file shape

`relay-consumer.js:216-221`:
```json
{
  "event": <signed Nostr event>,
  "recipient_npub": "<npub or hex>",
  "received_at": "<ISO-8601>",
  "relay_url": "<ws://...>"
}
```

Important: this is **NOT** LDN ([Linked Data Notifications](https://www.w3.org/TR/ldn/)) format. The event lands as raw Nostr JSON wrapped with metadata, not as a Notification with `@context`/`@type`. The Solid Notifications 0.2 path mentioned in `ADR-010:198` is described as a side-channel (`solid-pod-rs`'s WebSocket channel) — agentbox's bridge writes directly to the filesystem under WAC bypass. No LDN inbox semantic.

### Agent-intent path

`relay-consumer.js:247-290`: when kind ∈ `[38000, 38099]`:
1. `_writeIntentMarker` (line 300) writes a separate durable marker at `pods/<npub>/events/intent-queue/<id>.json` (lines 304-319).
2. If the operator supplied an `intentSpec` callback AND the orchestrator adapter exposes `spawnAgent`, the bridge invokes `orchestrator.spawnAgent(spec)` with the Nostr event injected as env vars `NOSTR_EVENT_ID`, `NOSTR_EVENT_KIND`, `NOSTR_EVENT_PUBKEY`, `NOSTR_RECIPIENT_NPUB`, `NOSTR_EVENT_JSON` (lines 271-280).

### Outbox path

`relay-consumer.js:391-464`:
- 500-ms poll of `pods/<npub>/events/outbox/*.json` (line 67: `DEFAULT_OUTBOX_POLL_MS`).
- Status states: `pending` → `published` (success) or `failed` (after exhausting `[1s, 5s, 30s, 300s]` retry backoff at line 67).
- Sign via `loadSigner(stack)` (line 423) — lazy-loaded so test runs don't force key decryption.
- `_bridge.publish(event, signer)` (line 437) hits both loopback `:7777` and (when `external_fanout != "off"`) the `NOSTR_RELAYS` list.
- File renamed `<pending-id>.json` → `<final-event-id>.json` on ack (line 446).

### Wiring gap

`grep -rn "RelayConsumer" management-api/` returns **zero hits**. The class exists at `mcp/nostr-bridge/relay-consumer.js:70`; the management-api boot path (`server.js:1-880`) never imports or instantiates it.

What's wired now:
- `management-api/middleware/auth.js:15-19` soft-imports `NostrBridge` for NIP-98 verification.
- `mcp/servers/nostr-bridge.js` ships as an in-process library (per its own header at lines 1-26: "ARCHITECTURE DECISION: library-only, consumed in-process by management-api").
- The bridge's `connect()`, `subscribe()`, `publish()` (lines 220, 254, 294) are called by no boot path. The relay socket is never opened from JS.

So in practice today: **relay process runs, sovereign-bootstrap writes pod scaffolding, but no JS process subscribes to the relay or watches the outbox**. PRD-004 §12 step 3 explicitly lists `mcp/nostr-bridge/pod-writer.js` as "to build" in `DDD-003:289`.

`flake.nix:1082-1085` (the supervisord block annotation) confirms this is by design — the bridge is library-only and the gate `[sovereign_mesh].nostr_bridge = true` (`agentbox.toml:21`) signals to management-api that it *should* call `NostrBridge.connect()` at boot. Today it doesn't.

---

## 4. solid-pod adapter (ADR-010) — pod URLs, WAC, WebID

**ACTUAL.** Pod server runs, adapter speaks Solid 0.11, WAC enforced, did:nostr resolver active.

### Server process

`flake.nix:1075` (supervisord block, gated on `pods = "local-solid-rs"`):

```
[program:solid-pod]
command=solid-pod-rs-server --config /etc/agentbox/solid-pod-rs.toml
environment=HOME="/home/devuser",JSS_HOST="127.0.0.1",JSS_PORT="8484",
  JSS_BASE_URL="http://127.0.0.1:8484",JSS_STORAGE_ROOT="/var/lib/solid",
  JSS_LOG_LEVEL="info",RUST_LOG="info",
  JSS_ENABLE_DID_NOSTR="true",JSS_ENABLE_RATE_LIMIT="true",
  JSS_RATE_LIMIT_PER_SEC="20",JSS_ENABLE_QUOTA="true",
  JSS_QUOTA_DEFAULT_BYTES="10737418240",JSS_ENABLE_WEBHOOK_SIGNING="true",
  JSS_V04_COMPAT="true",AGENTBOX_REQUIRED_FOR_READINESS="true"
```

Source: `lib/solid-pod-rs.nix` builds from `github:DreamLab-AI/solid-pod-rs`, pinned `main@7f8bc89` (Sprint 9) per `ADR-010:128-129`.

### Pod URL pattern

`agentbox.toml:182-207`:
- `port = 8484` (exposed in `docker-compose.yml:23`).
- `bind = "0.0.0.0"` (note: differs from relay's loopback default — pod is intended to be reachable).
- `base_url = "http://localhost:8484"`.
- Storage backend `fs` rooted at `/var/lib/solid`.

Per-agent pod URL: `http://localhost:8484/pods/<npub>/...`. Notable static endpoints inside each pod:
- `pods/<npub>/profile.json` — Solid WebID
- `pods/<npub>/.acl.json` — root WAC document
- `pods/<npub>/did-nostr.json` — DID doc (Tier 1 + Tier 3)
- `pods/<npub>/events/inbox/`, `pods/<npub>/events/outbox/`, `pods/<npub>/events/intent-queue/`
- `pods/<npub>/memory/<namespace>/<key>.json` — fallback memory storage when `adapters.memory = "off"` (`management-api/routes/memory.js:84-95`).

### Adapter implementation

`management-api/adapters/pods/local-solid-rs.js` (157 lines) extends `_solid-http-base.js`:

- `local-solid-rs.js:25` — `DEFAULT_BASE = 'http://127.0.0.1:8484'`.
- `local-solid-rs.js:51-64` — `OPTIONS /` capability probe.
- `local-solid-rs.js:72-104` — LDP container listing with `Link: rel="next"` pagination.
- `local-solid-rs.js:126-143` — N3 patch preferred when supported, JSON-patch fallback.
- `local-solid-rs.js:146-153` — `_assertFromRes` maps 404 → `NotFound`, 401/403 → `PermissionDenied`.

### WAC

ACL grammar from `sovereign-bootstrap.py:157-170`:

```json
{
  "@context": "http://www.w3.org/ns/auth/acl#",
  "owner": "did:nostr:<x_only_pubkey_hex>",
  "rules": [{
    "@type": "Authorization",
    "agent": "did:nostr:<x_only_pubkey_hex>",
    "mode": ["Read", "Write", "Append", "Control"],
    "accessTo": "./",
    "default": "./"
  }]
}
```

**Important:** WAC subject is the **DID** (`did:nostr:<hex>`), not raw npub or hex. This was an explicit ADR-010 Sprint 6 absorption (`sovereign-bootstrap.py:152-156` comment). The `solid-pod-rs` `did-nostr` Cargo feature accepts both pubkey hex and bech32 npub at the resolver, which lets WAC policies be written DID-first.

### WebID

`profile.json` (`sovereign-bootstrap.py:173-179`):

```json
{
  "@context": "https://www.w3.org/ns/solid/terms#",
  "id": "did:nostr:<hex>",
  "webId": "http://localhost:8484/pods/<npub>/profile.json",
  "alsoKnownAs": ["did:nostr:<hex>"]
}
```

The WebID URL is hard-baked to `http://localhost:8484` at bootstrap time, irrespective of where the operator might serve the pod externally — this is currently a deployment limitation.

### NIP-98 HTTP auth

`agentbox.toml:193`: `enable_schnorr_verify = true` (Cargo feature `nip98-schnorr` on by default per `ADR-010:67`). Same bytes verified by JS-side `mcp/servers/nostr-bridge.js:321-383` (`NostrBridge.verifyNip98`) — NIP-98 events are kind 27235 (per `nostr-bridge.js:55`, `kinds.AUTH = 27235`).

### Pod health probe

`management-api/server.js:80-144` — `probePodHealth()`:
- `GET ${baseUrl}/health` → checks `solid_pod_rs_health`.
- `GET ${baseUrl}/did:nostr:${didIdentifier}` → checks `did_nostr_resolves` (line 113).
- `fs.accessSync(SOLID_POD_ROOT, W_OK)` → `writable_storage`.

Surfaced at public endpoint `GET /health/pods` (`server.js:451-469`).

---

## 5. management-api server.js public surface

**ACTUAL.** Fastify on port 9090. All routes documented from grep + manual sweep.

### Boot wiring

`management-api/server.js:1-32`:
- Port: `process.env.MANAGEMENT_API_PORT || 9090` (line 26).
- Host: `0.0.0.0` (line 27).
- Hard requires `MANAGEMENT_API_KEY` env var (line 28-32) — exits 1 if missing.

### Authentication

`server.js:181-213` — global `onRequest` hook delegates to `createAuthMiddleware` (`middleware/auth.js`):
- Modes (`auth.js:80-110`): `hybrid` | `nip98` | `bearer` | `strict-nip98`.
- Auto-elevation: when `AGENTBOX_SOVEREIGN_MESH_ENABLED=true` and mode unset, becomes `strict-nip98` (`auth.js:104-107`).
- Bypassed paths: `/livez`, `/health`, `/ready`, `/metrics`, `/v1/meta`, `/lo/*`, `/.well-known/did.json` (`server.js:188-210`).

### Public routes (no auth)

| Path | Method | Handler | Purpose |
|------|--------|---------|---------|
| `/livez` | GET | `server.js:300-316` | Always 200; event-loop liveness |
| `/ready` | GET | `server.js:319-413` | 200 only when bootstrap sentinel + adapters healthy + paths writable + relays declared |
| `/health` | GET | `server.js:418-446` | Aggregate adapter health |
| `/health/pods` | GET | `server.js:451-469` | Pod + did:nostr probe |
| `/v1/meta` | GET | `server.js:472-515` | image/manifest hash, federation_mode, contract versions, observability endpoints |
| `/metrics` | GET | `server.js:518-532` | Prometheus text |
| `/.well-known/did.json` | GET | (no handler — auth-bypassed but unbound) | Falls through to 404; clients should hit pod's `8484` endpoint |
| `/lo/*` | GET | `routes/linked-objects.js:64-209` | Linked-Object viewer bundle (S12 viewer) |

### Authed routes

| Path | Methods | File | Notes |
|------|---------|------|-------|
| `/v1/tasks`, `/v1/tasks/:id`, `/v1/tasks/:id/logs/stream` | POST/GET/DELETE/WS | `routes/tasks.js:13-219` | Task spawning + log streaming |
| `/v1/comfyui/workflow`, `/v1/comfyui/models`, `/v1/comfyui/outputs`, `/v1/comfyui/stream` | POST/GET/DELETE/WS | `routes/comfyui.js:12-279` | ComfyUI proxy |
| `/v1/agent-events`, `/v1/agent-events/stream`, `/v1/agent-events/emit`, `/v1/agent-events/batch`, `/v1/agent-events/types`, `/v1/agent-events/hook`, `/v1/agent-events/registry`, `/v1/agent-events/status` | GET/POST/WS | `routes/agent-events.js:52-433` | Hook event ingest + WebSocket fanout |
| `/v1/memory`, `/v1/memory/:key`, `/v1/memory/search` | POST/GET | `routes/memory.js:54-185` | RuVector or pod-fallback memory |
| `/v1/status` | GET | `routes/status.js` | System monitor |
| `/v1/uri/:urn`, `/v1/uri` | GET | `routes/uri-resolver.js:42-172` | Canonical URI dereferencer (see §7) |

### Where Nostr is *not* surfaced

There is **no Nostr-aware HTTP route**. No `/v1/nostr/*`, no `/v1/relay/*`, no `/v1/messages/*`. The relay is reachable only on its own WebSocket port (7777) and only locally.

The closest thing is auth-side NIP-98 verification (`middleware/auth.js:33-63`) which validates inbound `Authorization: Nostr <base64>` headers but does not expose any Nostr functionality.

---

## 6. URN minting (lib/uris.js, 18 kinds)

**ACTUAL.** Single mint library at `management-api/lib/uris.js` (286 lines).

### Grammar

`lib/uris.js:69-90`:

```
URI            ::= identity-uri | name-uri
identity-uri   ::= "did:nostr:" pubkey-hex                    ; 64 lc hex (BIP-340 x-only)
name-uri       ::= "urn:agentbox:" kind ":" [scope ":"] local
```

Kind catalogue (line 71 `KINDS = Object.freeze({...})`) — 17 kinds (the project CLAUDE.md says 18; the discrepancy is `meta` having been added):

| Kind | ownerScope | contentAddressed | resolvableSurface |
|------|-----------|------------------|-------------------|
| `pod` | true | true | `pods` |
| `envelope` | true | true | `pods` |
| `credential` | true | true | `pods` |
| `mandate` | true | true | `pods` |
| `receipt` | true | true | `pods` |
| `activity` | true | true | `agent-events` |
| `event` | true | true | `agent-events` |
| `mcp` | false | false | `things` |
| `memory` | false | false | `memory` |
| `skill` | false | false | `skills` |
| `adr` | false | false | `docs` |
| `prd` | false | false | `docs` |
| `ddd` | false | false | `docs` |
| `thing` | false | false | `things` |
| `dataset` | true | false | `memory` |
| `bead` | true | false | `beads` |
| `agent` | false | false | `agents` |
| `meta` | false | false | `meta` |

(That's 18.)

### Minting rules

- **R1 — content-addressed** (`lib/uris.js:139-144`): `local = sha256-12-<first 12 hex of SHA-256(stableStringify(payload))>`.
- **R2 — scope-bearing** (`lib/uris.js:151-160`): scope is BIP-340 x-only pubkey hex; bech32 npub and `did:nostr:*` accepted at the boundary and normalised by `_normalisePubkey` (line 177).
- **R3 — stable on identity** (line 145): `local` is a slug of `localId`, max 96 chars.

### Regex enforcement

```js
URN_RE          = /^urn:agentbox:([a-z]+):([^:]+(?::[^:]+)?)$/  // line 92
PUBKEY_HEX_RE   = /^[0-9a-f]{64}$/                              // line 96
DID_NOSTR_RE    = /^did:nostr:([0-9a-f]{64})$/                  // line 97
NPUB_PREFIX_RE  = /^npub1[a-z0-9]+$/                            // line 101
```

### Public API

`lib/uris.js:278-286`:
```js
module.exports = { KINDS, mint, resolveCanonical, parse, isCanonical, UnknownUriKind, MalformedUri };
```

### Cross-namespace relationship

Per `agentbox/CLAUDE.md` (loaded with this turn) and project `CLAUDE.md`:
- agentbox URN scope = 64-char hex pubkey (canonical).
- VisionClaw uses parallel `urn:visionclaw:*` (6 kinds) minted by Rust at `src/uri/`.
- Both share `did:nostr:<hex>` identity and `sha256-12-<12 hex>` content addressing.
- BC20 anti-corruption layer (planned, `docs/ddd-agentbox-integration-context.md`) maps between them at the federation boundary.

---

## 7. URI resolver `/v1/uri/<urn>` route

**ACTUAL.** Mounted unconditionally regardless of `[linked_data].enabled`.

### Mount point

`management-api/server.js:716-729`:
```js
await app.register(require('./routes/uri-resolver'), { logger, manifest });
```
Comment notes: "Always available — the resolver does not depend on `[linked_data].enabled` because URI uniqueness is unconditional."

### Resolver semantics

`routes/uri-resolver.js:42-151`. Input validation:
- 400 `malformed-uri` if not `uris.isCanonical`.

Dispatch by kind:

| Input shape | Output |
|-------------|--------|
| `did:nostr:<hex>` (with `did_documents` enabled) | `307 → ${podBase}/.well-known/did.json` |
| `did:nostr:<hex>` (with `did_documents = "off"`) | `404 not-resolvable` (line 60-67) |
| `urn:agentbox:pod\|envelope\|credential\|mandate\|receipt:<hex>:<local>` | `307 → ${podBase}/agents/<hex>/<kind>/<local>` (line 89-93) |
| `urn:agentbox:activity\|event:...` | `307 → /v1/agent-events?id=<encoded urn>` (line 99-102) |
| `urn:agentbox:mcp\|thing:...` | `307 → /v1/things/<local>` (line 104-107) |
| `urn:agentbox:memory\|dataset:<ns>.<key>` | `307 → /v1/memory/<key>?namespace=<ns>` (line 109-122) |
| `urn:agentbox:skill:...` | `307 → /v1/skills/<local>` (line 124-126) |
| `urn:agentbox:adr\|prd\|ddd:<id>` | `307 → /docs/reference/<kind>/<id>.md` (line 128-132) |
| `urn:agentbox:meta:...` | `307 → /v1/meta` (line 134-136) |
| `urn:agentbox:bead:...` | `307 → /v1/beads/<local>` (line 138-140) |
| anything else canonical-form | 404 `not-resolvable` (line 145-149) |

### Self-describing endpoint

`routes/uri-resolver.js:154-172` — `GET /v1/uri` (no urn) returns the grammar + kind table + contract claim ("uniqueness always; resolvability best-effort").

---

## 8. Adapter slot mechanism (5 slots: beads, pods, memory, events, orchestrator)

**ACTUAL contract surface.** Per ADR-005.

### Slot list

`management-api/adapters/index.js:14`:
```js
const SLOTS = ['beads', 'pods', 'memory', 'events', 'orchestrator'];
```

### Per-slot implementation classes

| Slot | local-* | external | off | placeholder |
|------|---------|----------|-----|-------------|
| beads | `local-sqlite.js` | `external.js` | `off.js` | `placeholder.js` |
| pods | `local-solid-rs.js` | `external.js` | `off.js` | `placeholder.js` |
| memory | `embedded-ruvector.js` | `external-pg.js` | `off.js` | `placeholder.js` |
| events | `local-jsonl.js` | `external.js` | `off.js` | `placeholder.js` |
| orchestrator | `local-process-manager.js` | `stdio-bridge.js` | `off.js` | `placeholder.js` |

(See `ls management-api/adapters/<slot>/`.)

### Resolution

`management-api/adapters/index.js:113-138`:
1. For each slot, read `manifest.adapters[slot]`, default `'off'`.
2. `requireImpl(slot, impl)` (line 86) loads `<slot>/<impl>.js`; falls back to `placeholder.js` for `off`.
3. Construct via `new AdapterClass(slotConfig(slot, impl, manifest))`.
4. Decorate with `_implName`, `_slot` for health/meta endpoints.

### Lifecycle

`server.js:686-862`:
- Construct (line 686): `resolveAdapters(manifest)`.
- Decorate Fastify (line 687): `app.decorate('adapters', resolvedAdapters)`.
- Connect with 10-second total timeout (line 819-862): each adapter's `connect()` raced.
- Orchestrator failure is fatal (line 830-833): `process.exit(1)`.
- Other slot failures degrade to `off` impl (line 834-846).
- Disconnect on shutdown with 5-s timeout (line 619-639).

### Is "events" the slot that wires nostr?

**No, not currently.** The events slot has three implementations (`local-jsonl`, `external` HTTP-POST, `off`). None of them is a nostr-aware adapter. `relay-consumer.js:230-242` calls `events.dispatch(...)` to forward inbound Nostr events into the events slot for downstream consumers — i.e., Nostr is upstream of events, not implemented by events.

ADR-005 §events lists "Nostr (parameterised-replaceable kind)" as a possible **external** shape (line 45 of ADR-005), but no `events/local-nostr.js` adapter file ships today. ADR-009 §Related files (line 276) lists `management-api/adapters/events/local-nostr.js` as **planned** ("new adapter impl for the events slot").

So the contract is: agentbox ingests Nostr at `RelayConsumer` (still being wired) and dispatches into `events.dispatch()`; nothing mints Nostr events from `events.dispatch()` calls today. PRD-004 §12 step 7 frames the bridge from outbox → relay as the to-build path.

### Effective config from agentbox.toml

`agentbox.toml:11-16`:
```toml
[adapters]
beads        = "off"
pods         = "local-solid-rs"
memory       = "external-pg"
events       = "local-jsonl"
orchestrator = "local-process-manager"
```

So beads is currently `off`; everything else has a real backing. RuVector PostgreSQL conninfo lives at `agentbox.toml:174-175`, the runtime template includes a placeholder `@@RUVECTOR_PG_PASSWORD@@` substituted by the entrypoint.

### Contract versions

`management-api/adapters/contract-versions.js:1-12` — every slot at `1.0.0`. Surfaced at `GET /v1/meta`'s `adapter_contract_versions` field (`server.js:495-509`).

---

## 9. Federation modes (standalone vs client) — what differs

**ACTUAL gating logic exists in validators and adapter resolver. `[federation]` block is absent from the shipped `agentbox.toml`** — so the running container is implicitly in standalone-with-fallbacks shape.

### Where federation mode is read

- `scripts/agentbox-config-validate.js:154` — `federation.mode` controls validator rules E001 (any `external` adapter requires `federation.mode = "client"` and `federation.external_url`), E004 (all-`off` requires standalone), W012 (client mode with local adapters), E027 (relay external impl requires client).
- `management-api/adapters/index.js:32-77` — `slotConfig` reads `manifest.federation.external_url` for every `external` slot.
- `flake.nix:1844` — `AGENTBOX_FEDERATION_MODE` env never set in the imageEnv (grep returns no source).
- `server.js:507` — `/v1/meta` reads `process.env.AGENTBOX_FEDERATION_MODE || null`.
- `scripts/start-agentbox.sh:289-308` — wizard flow that writes `[federation] mode = ...` and `[federation] external_url = ...` to a state.json that `tui-write-manifest.py` materialises into the manifest. Defaults `mode = "standalone"`.

### What changes between standalone and client

| Aspect | standalone | client |
|--------|-----------|--------|
| `[federation].mode` | `"standalone"` | `"client"` |
| `[federation].external_url` | `""` | host-mesh base URL (e.g. `http://host-orchestrator:7070`) |
| Validator rules | E004 enforced (must be standalone if all adapters are `off`) | E001 / E027 require `federation.external_url` non-empty for any `external` adapter |
| Adapter resolution | `local-*` impls (`local-solid-rs`, `external-pg`, `local-jsonl`, `local-process-manager`) | `external` / `external-pg` / `stdio-bridge` impls invoked with the host URL |
| Embedded relay | runs (default) | usually `off` or `external` |
| Embedded pod | runs | usually `external` |
| Auth posture | hybrid Bearer + NIP-98 | host orchestrator owns identity end of the link |

### Who is "the federation host"?

This is left **abstract** in agentbox docs deliberately. `agentbox/CLAUDE.md` rule "No host-project specifics in this repo" plus `ADR-005` framing. From the integrating side (this VisionFlow repo), the planned BC20 anti-corruption layer in `src/actors/` is the host orchestrator. The connection contract is in `docs/PRD-004-agentbox-visionclaw-integration.md` (referenced from project `CLAUDE.md`) and `docs/ddd-agentbox-integration-context.md`.

In agentbox's external-adapter shape the connection is generic HTTP / stdio / MCP per slot:
- beads → HTTP REST or MCP at `external_url`.
- pods → Solid-protocol-compatible HTTP at `external_url`.
- memory → PostgreSQL DSN at `integrations.ruvector_external.conninfo` (already populated by default — `agentbox.toml:175`).
- events → HTTP POST at `external_url`.
- orchestrator → stdio bridge over `docker exec -i` at `external_url`.

### The orchestrator slot federation mechanism

`adapters/orchestrator/stdio-bridge.js` is special: per `ADR-005:12,71`, federation is the transport (stdio), not a remote URL. `slotConfig('orchestrator', 'stdio-bridge', ...)` in `adapters/index.js:62-69` returns `{ externalUrl, protocol: 'stdio' }`.

---

## 10. Linked-Data surfaces (S1-S12 from PRD-006/ADR-012/DDD-004)

**Mostly ACTUAL** as encoder modules; **opt-in per surface**, default-off master gate; viewer is **PLANNED-on** in current `agentbox.toml` but the bundle materialisation gate is build-time.

### Master gate

`agentbox.toml:380-406`:
```toml
[linked_data]
enabled                = true
pods                   = "on"
events                 = "on"
credentials            = "emit"
did_documents          = "emit"
provenance             = "emit"
capability_descriptors = "emit"
skill_metadata         = "emit"
payments               = "on"
memory_catalogue       = "emit"
architecture_docs      = "emit"
http_meta              = "emit"
```

(Per-surface values: `on` = bidirectional read+emit; `emit` = output only; `off` = surface inert.)

### Encoder modules

`management-api/middleware/linked-data/`:
- `index.js` — boot entry `createEncoder(...)`.
- `encoder.js` — pipeline.
- `context-resolver.js` — pinned-context catalogue lookup.
- `jcs.js` — RFC 8785 canonicalisation.
- `lion-linter.js` — LION subset enforcement.
- `round-trip.js` — Compact-of-Expand round-trip canary.
- `surfaces/s01-pods.js` ... `s11-http-meta.js`.
- `viewer/manifest.js` — pane manifest (S12).

### S4 (DID Documents) — concrete shape

`management-api/middleware/linked-data/surfaces/s04-did.js:31-89`:

```js
encode(payload, { manifest, agentDid }) {
  const did = payload?.did || agentDid;
  // ... (only did:nostr: methods accepted; line 34-36)
  const services = [];
  if (enabled.includes('pod')) {
    services.push({ id: `${did}#pod`, type: 'SolidPod',
                    serviceEndpoint: sp.base_url || `http://${bind}:${port}` });
  }
  if (enabled.includes('relay')) {
    services.push({ id: `${did}#relay`, type: 'NostrRelay',
                    serviceEndpoint: `ws://${bind}:${port}` });
  }
  // verificationMethod: SchnorrSecp256k1VerificationKey2025
  return { document: { '@context': [DID_CONTEXT, AGBX_CONTEXT], id: did,
                       verificationMethod, service, authentication, assertionMethod },
           contextIri: DID_CONTEXT, pubkey };
}
```

`agentbox.toml:413-416`:
```toml
[linked_data.did]
method                = "nostr"
service_endpoints     = ["pod", "relay"]
publish_to_well_known = true
```

So the DID Document advertises both the pod (`http://...:8484`) and the relay (`ws://...:7777`) as service endpoints on the agent DID. This is the linkage that lets a third party derive both addresses from a single `did:nostr:<hex>`.

### Viewer slot S12

`agentbox.toml:421-435`:
```toml
[linked_data.viewer]
mode                 = "local-linkedobjects"     # off | local-linkedobjects | external
mount_path           = "/lo"
bundle_path          = "/opt/agentbox/browser"
```

Wired at `server.js:735-752`:
```js
const { resolveViewerImpl } = require('./middleware/linked-data/viewer');
const viewer = resolveViewerImpl({ manifest, logger });
await app.register(require('./routes/linked-objects'), { logger, viewer });
```

`routes/linked-objects.js:64-209` mounts `/lo/*`, `/lo/manifest.json`, `/lo/panes/:file`, `/lo/proxy`. AGPL-3.0 bundle per `ADR-013` viewer aggregation rule.

### Encoder ordering (dispatch pipeline)

Per `ADR-012` and `agentbox.toml:418-419`:
```toml
[linked_data.privacy_handoff]
order = "after"   # documentation only — the order is fixed in code (E048)
```

Ordering at `DDD-004:101`: `["observability", "privacy_filter", "linked_data", "adapter"]` — fixed in source, not configurable.

---

## 11. Existing relay endpoints, ports, auth posture

### Single relay endpoint

| Listener | Bind | Port | Protocol | Reachable from | Auth |
|----------|------|------|----------|----------------|------|
| nostr-rs-relay | `127.0.0.1` (default) | 7777 | WebSocket (Nostr wire); HTTP NIP-11 GET / | container localhost only | NIP-42 challenge unless `ingress_policy = "open"` |
| nostr-rs-relay (HTTP info) | same | 7777 | `GET / Accept: application/nostr+json` | per above | none (NIP-11 is public) |

`docker-compose.yml` does **not** publish 7777 (`yml:19-26`). Sibling containers on the same Docker network cannot reach 7777 either, because `bind = 127.0.0.1` means the relay only listens on the loopback interface inside the container's namespace.

To make external Nostr clients reach the relay today an operator must:
1. Set `[sovereign_mesh.relay].bind = "0.0.0.0"` and `expose = true` (validator E029).
2. Add `"7777:7777"` to `docker-compose.yml`.
3. Add `"7777/tcp" = {}` to `commonPorts` or `sovereignPorts` in `flake.nix`.
4. Optionally front it with TLS via the `https-bridge` (currently routes 9090 only).

### NIP-42 challenge

`flake.nix:737-738`:
```nix
relayNip42Auth =
  if (relayCfg.ingress_policy or "allowlist") == "open" then "false" else "true";
```

So with `ingress_policy = "allowlist"` (default), every connecting client must complete a 32-byte NIP-42 challenge from `crypto.randomBytes`, valid for 60 seconds (`ADR-009:135-136`).

### NIP-98 HTTP auth (different layer)

For HTTP requests to management-api (9090) and pod (8484), NIP-98 (kind 27235) is verified by:
- `mcp/servers/nostr-bridge.js:321-383` — `NostrBridge.verifyNip98` (Schnorr via `nostr-tools.verifyEvent`).
- `solid-pod-rs` Cargo feature `nip98-schnorr` (active per `agentbox.toml:193`).

### NOSTR_RELAYS env

`flake.nix:1863`:
```
"NOSTR_RELAYS=wss://relay.damus.io,wss://relay.primal.net"
```

Default fan-out targets — only consulted when `external_fanout != "off"` AND the bridge is wired to publish out. With shipping config (`external_fanout = "off"`, `agentbox.toml:98`), the embedded relay is the sole destination for outbox messages.

### Auth-posture summary

| Boundary | Auth | Source-of-truth file |
|----------|------|----------------------|
| `ws://127.0.0.1:7777` (relay ingress) | NIP-42 challenge + ingress_policy allowlist | `flake.nix:771`, `agentbox.toml:94-95` |
| `http://0.0.0.0:8484` (pod) | NIP-98 (Schnorr) for write paths; WAC for resource-level | `agentbox.toml:193`, pod ACL files |
| `http://0.0.0.0:9090` (management-api) | hybrid Bearer + NIP-98; auto-strict-NIP98 when sovereign_mesh on | `middleware/auth.js:80-110` |
| `/.well-known/did.json` | none (public) | `server.js:206-210` (auth bypass; serves through pod) |

---

## 12. Gaps relative to "agentbox can talk to forum and visionclaw via private relay"

This section addresses each critical question the task posed.

### Gap A — DM discoverability: can a forum user today DM an agentbox agent?

**No, not without operator intervention.** The chain that has to work is:

1. **Forum user discovers DID.** Requires `did:nostr:<hex>` to be resolvable. Today:
   - The pod hosts `pods/<npub>/did-nostr.json` (ACTUAL).
   - `solid-pod-rs`'s `did-nostr` feature serves `GET /did:nostr:<hex>` (ACTUAL).
   - But port 8484 is exposed only to the docker host (`docker-compose.yml:23`), not internet-routable.
   - There is no `/.well-known/did.json` route on the management-api itself — `server.js:208` only auth-bypasses; no handler.
2. **Forum user finds the agent's relay endpoint.** The DID Document service array does include a `NostrRelay` service endpoint (`s04-did.js:55-64`) → `ws://127.0.0.1:7777`. This URL is correct from inside the container but **useless externally** because (a) it points at loopback, (b) port 7777 is not published.
3. **Forum user connects to the relay.** Cannot, today, because of (b) above.
4. **Forum user passes NIP-42 AUTH.** Their pubkey would need to be in `[sovereign_mesh.relay].allowed_pubkeys` (currently empty list — `agentbox.toml:95`). Otherwise the relay drops their EVENT. This is the deliberate `ingress_policy = "allowlist"` default.
5. **Forum user signs an EVENT with `p` tag = agent's hex pubkey, kind 1059 (NIP-17 sealed DM)** — relay accepts (if AUTHed and allowlisted).
6. **`RelayConsumer` picks up the event** and writes to `pods/<npub>/events/inbox/<id>.json`. **This step does not happen today** because `RelayConsumer` is not wired into management-api boot (§3 above).
7. **Internal agent observes the inbox file and replies** by writing to `pods/<npub>/events/outbox/<pending-id>.json`. Watcher is the same `RelayConsumer._flushOutbox` (line 391) — also not wired.

### Gap B — Wiring up `RelayConsumer` into management-api boot

The class is fully implemented (`mcp/nostr-bridge/relay-consumer.js:70-471`). Wiring requires:
- Reading `process.env.AGENTBOX_NPUB` (already exported by sovereign-bootstrap).
- Constructing `new RelayConsumer({ npubs: [AGENTBOX_NPUB], adapters: resolvedAdapters, ... })`.
- Calling `await consumer.start()` after `connectOps` settle in `server.js:862`.
- Adding `await consumer.stop()` to `closeGracefully`.

Probably 10-15 lines plus an `events`-slot `local-nostr` adapter for symmetry (planned per `ADR-009:276`).

### Gap C — Federation host endpoint

agentbox does not name a host. It references "host project / integrator / external orchestrator". For VisionClaw integration the host is this repo's planned `src/actors/` BC20 anti-corruption layer. Agentbox can already federate via the four `external_url`-bearing slots (beads, pods, events, orchestrator) and the `external-pg` memory slot, but **none of these slots is a Nostr peer connection**. There is no concept in agentbox today of a "private relay federation" via Nostr fan-out beyond the `NOSTR_RELAYS` outbound list.

What's missing for a private-relay mesh:
- An `events/external-nostr-relay.js` adapter that dials a host's relay over `wss://`.
- Or, equivalently: setting `[sovereign_mesh.relay].external_fanout = "bidirectional"` and pointing `NOSTR_RELAYS` at the host's relay URL — this is supported in principle (`flake.nix:1863`, `relay-consumer.js:142-148`), but again only once `RelayConsumer` is wired into boot.
- A way for the host to enumerate a federation roster of agent DIDs — not present in the agentbox surface.

### Gap D — Pod Inbox semantics vs LDN

The pod-inbox bridge writes raw Nostr-event JSON wrapped in metadata, **not** [Linked Data Notifications](https://www.w3.org/TR/ldn/). LDN spec wants:
- `Content-Type: application/ld+json` with `@context: ldp:`
- Server advertising `Link: <inbox>; rel="ldp:inbox"` on resources.
- Notifications themselves being JSON-LD documents.

Today's bridge writes:
```json
{ "event": <signed nostr event>, "recipient_npub": "...", "received_at": "...", "relay_url": "..." }
```

Plain JSON. The pod's `solid-pod-rs` content-negotiates on read (it can return `application/ld+json`), but the inbox payloads themselves are not Linked Data. The S2 (events) linked-data surface (`middleware/linked-data/surfaces/s02-nostr.js`, agentbox.toml `[linked_data].events = "on"`) is intended to encode the event payload's `content` field as JSON-LD ActivityStreams 2.0 — but that fires on the **encoder pipeline** for outbound `events.dispatch()` calls, not on inbound bridge writes. PLANNED bridge-side conversion is part of the pod-inbox bridge work.

### Gap E — Adapter "events" slot is not wired to Nostr

The `events` slot ships three impls (`local-jsonl`, `external` HTTP, `off`). None of them speaks Nostr. ADR-005 §events lists Nostr as a possible external shape; ADR-009 lists `events/local-nostr.js` as planned. For the integration story, this means the adapter contract test harness does **not** exercise Nostr at the adapter dispatch layer — only at the relay-consumer side.

### Gap F — The DID Document does not yet round-trip to the JSON-LD viewer

S4 encoder builds a DID Document Object (s04-did.js:76-83); the viewer manifest (`viewer/manifest.js:260`) advertises `did-document → ${podBase}/.well-known/did.json` as a known link; the URI resolver redirects `did:nostr:*` to the same path (`uri-resolver.js:67-69`). That all hangs together **once port 8484 is reachable**. Externally, the linkage breaks at deployment.

### Gap G — Operator pubkey allowlist provisioning

`agentbox.toml:72-76` declares an operator identity:
```toml
[sovereign_mesh.operator]
pubkey_hex   = "11ed64225dd5e2c5e18f61ad43d5ad9272d08739d3a20dd25886197b0738663c"
npub         = ""
display_name = ""
```

`agentbox.toml:64-68` claims this pubkey is "Added to the relay allowlist (can publish events to the embedded relay) ... operator-level access to management API via NIP-98 ... delegator on NIP-26 agent delegations ... WebID owner in Solid pod ACLs". 

But: `flake.nix:732-736` only emits `pubkey_whitelist` from `relayCfg.allowed_pubkeys` (which is empty), not from `operator.pubkey_hex`. There is no source-side code path that auto-injects the operator pubkey into the relay allowlist. The comment is **PLANNED behaviour, not yet implemented** in flake or entrypoint.

### Gap H — Multiple internal agents per container

`sovereign-bootstrap.py:233` hardcodes `agent_id = os.getenv("AGENTBOX_AGENT_ID", "agentbox-core")` — one keypair per container. PRD-004 §11 (per `ADR-009:262-263`) explicitly defers multi-npub-per-container to follow-up work. So a forum DM addressed to a specific internal agent (`p` tag) maps to the single container DID, not to a per-agent DID. To split across agents, operators today have to run multiple containers, each with its own `AGENTBOX_AGENT_ID`.

### Summary table — "can a forum user discover and DM an agentbox agent today?"

| Step | Status | Blocker |
|------|--------|---------|
| Discover DID via `/.well-known/did.json` | yellow | requires port 8484 internet-reachable; currently host-bound only |
| Resolve service endpoints (pod URL, relay URL) from DID Doc | green | S4 encoder produces correct DID Doc |
| Reach the relay over the network | red | port 7777 not exposed; bind = 127.0.0.1 by default |
| Pass NIP-42 AUTH | yellow | works in principle if operator's pubkey is allowlisted |
| Send NIP-17 sealed DM (kind 1059) | green | relay accepts it once authed |
| Have it persisted to pod inbox | red | RelayConsumer not wired into management-api boot |
| Internal agent reply via outbox | red | same — outbox watcher is the same uninstantiated class |
| Discover agent's DID without prior knowledge | red | no public registry, no `/.well-known/agentbox-agents`, no DID enumeration endpoint |

The mechanism exists across files; the boot-time wiring is the dominant remaining gap.

---

## File index (all paths absolute under `/home/devuser/workspace/project/agentbox/`)

Identity / bootstrap:
- `scripts/sovereign-bootstrap.py:81-203` — keypair gen, ACL writer, DID doc writer
- `config/entrypoint-unified.sh:201-202` — Stage A invocation
- `config/entrypoint-unified.sh:556-595` — runtime-env publication

Manifest:
- `agentbox.toml:11-16` — adapters
- `agentbox.toml:18-104` — sovereign_mesh + relay
- `agentbox.toml:182-207` — solid_pod_rs integration
- `agentbox.toml:380-435` — linked_data + viewer
- `agentbox.toml:702-704` — relay security exception

Flake / image:
- `flake.nix:699-781` — relay package + config
- `flake.nix:1075-1080` — solid-pod supervisor block
- `flake.nix:1129-1141` — nostr-relay supervisor block
- `flake.nix:1840-1956` — image env (AGENTBOX_RELAY_*, NOSTR_RELAYS, etc.)
- `flake.nix:1968-1970` — sovereignPorts (8484 only)

Management-API:
- `management-api/server.js:80-144` — `probePodHealth`
- `management-api/server.js:181-213` — auth + bypass list
- `management-api/server.js:300-516` — public routes (livez, ready, health, metadata, metrics)
- `management-api/server.js:686-862` — adapter resolution + connect

Adapters:
- `management-api/adapters/index.js:14-138` — slot resolver
- `management-api/adapters/pods/local-solid-rs.js` — Solid 0.11 client
- `management-api/adapters/events/local-jsonl.js` — JSONL events sink
- `management-api/adapters/orchestrator/local-process-manager.js` — child_process spawner

URI:
- `management-api/lib/uris.js:71-90` — KINDS catalogue
- `management-api/lib/uris.js:135-164` — mint
- `management-api/routes/uri-resolver.js:42-172` — /v1/uri/:urn

Linked-data:
- `management-api/middleware/linked-data/surfaces/s04-did.js:31-89` — DID Doc encoder
- `management-api/routes/linked-objects.js:64-209` — /lo/* viewer mount

Nostr:
- `mcp/servers/nostr-bridge.js:175-411` — NostrBridge library (subscribe, publish, verifyNip98)
- `mcp/servers/nostr-bridge.js:431-481` — loadSigner (key decryption)
- `mcp/nostr-bridge/relay-consumer.js:70-471` — RelayConsumer (pod-inbox bridge — not yet wired)
- `management-api/middleware/auth.js:33-63,112-148` — NIP-98 hybrid auth middleware

Reference docs:
- `docs/reference/adr/ADR-005-pluggable-adapter-architecture.md`
- `docs/reference/adr/ADR-008-privacy-filter-routing.md`
- `docs/reference/adr/ADR-009-embedded-nostr-relay.md`
- `docs/reference/adr/ADR-010-rust-solid-pod-adoption.md`
- `docs/reference/adr/ADR-012-jsonld-federation-grammar.md`
- `docs/reference/adr/ADR-013-canonical-uri-grammar.md`
- `docs/reference/prd/PRD-001-capabilities-and-adapters.md`
- `docs/reference/prd/PRD-004-external-agent-messaging.md`
- `docs/reference/prd/PRD-006-linked-data-interfaces.md`
- `docs/reference/ddd/DDD-003-sovereign-messaging-domain.md`
- `docs/reference/ddd/DDD-004-linked-data-interchange-domain.md`
- `docs/user/nostr-relay.md`, `docs/user/solid-pod.md`, `docs/user/uris.md`
