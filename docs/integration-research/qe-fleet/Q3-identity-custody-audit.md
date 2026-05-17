# Q3 — Identity, Key Custody & DID Document Audit

> Scope: VisionClaw substrate (`/home/devuser/workspace/project`), Dreamlab forum
> (`./dreamlab-ai-website/community-forum-rs/`), Agentbox container
> (`./agentbox/`), shared `solid-pod-rs/` workspace.
> All file:line citations are absolute.
> Cross-references: `docs/integration-research/05-crypto-gotchas.md` §1, §2, §3,
> §11, §13, §14; PRD-010; ADR-073; ADR-074; ADR-075.

This audit catalogues **every key-creation, key-derivation, key-persistence,
DID-emission and DID-resolution surface** across the four substrates and
identifies drift, custody-boundary leaks, missing rotation paths, attribution
loss vectors, and federation-key-proliferation risk.

---

## I1 — Key generation paths

The four substrates create secp256k1 / Ed25519 / RSA / EC P-256 keys at the
following sites. "Observable" means whether the secret bytes are
plaintext-readable from outside the immediate scope (env var, log line, return
value, on-disk JSON).

| # | Site | Curve / scheme | RNG | BIP-340 lift_x | Storage destination | Observable |
|---|---|---|---|---|---|---|
| G1 | `dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/keys.rs:200-215 generate_keypair` | secp256k1 Schnorr (BIP-340 x-only) | `getrandom::getrandom` (line 202) | implicit via `k256::schnorr::SigningKey::from_bytes` | in-process `SecretKey { bytes: [u8;32] }`, `Zeroize`-on-drop (line 34-38) | no — never serialised |
| G2 | `keys.rs:185-197 derive_from_prf` | secp256k1 Schnorr | n/a — IKM is WebAuthn PRF output | implicit | in-process `Keypair`, `Zeroize`-on-drop | no — only `okm` intermediate, zeroised line 194 |
| G3 | `forum-client/src/auth/passkey.rs:140-152` (register) | secp256k1 via PRF | WebAuthn `hmac-secret` | implicit | `PasskeyRegistrationResult::privkey_bytes [u8;32]`, `Drop` zeroise (line 42-46) | yes within rust closure; never crosses JS boundary; never persisted |
| G4 | `forum-client/src/auth/passkey.rs:259-265` (authenticate) | secp256k1 via PRF | re-derived from PRF on each login | implicit | `PasskeyAuthResult::privkey_bytes`, zeroise on drop (line 56-59) | as G3 |
| G5 | `nostr-core/src/gift_wrap.rs:228 wrap_seal → generate_keypair` | secp256k1 Schnorr (throwaway) | `getrandom` | implicit | local stack only; double-zeroise (line 261-263 + Zeroize-drop) | no |
| G6 | `relay-worker/src/relay_do/session.rs:96 generate_challenge` | not a keypair (NIP-42 challenge string) | derived from `session_id` | n/a | DO transactional storage `ws_auth:{sid}` (line 188-191) | challenge only — public |
| G7 | `auth-worker/src/webauthn.rs:59-65 deterministic_salt_for` | not a key — domain-separated SHA-256 | hash-only | n/a | response body | yes — public-derivable on purpose (audit B2 fix) |
| G8 | `agentbox/scripts/sovereign-bootstrap.py:123-126 SigningKey.generate(SECP256k1)` | **ECDSA** secp256k1 (`from ecdsa import SECP256k1, SigningKey`) | `python-ecdsa` default RNG (relies on `os.urandom`) | **NOT applied — emits 64-byte SEC1 X‖Y, then `[:32]` truncation** (line 132) | filesystem JSON `/var/lib/agentbox/identities/<agent_id>.json` 0644 (`write_json` line 76, no chmod) | **YES** — `private_key_hex` written verbatim to disk (line 130, 87) |
| G9 | `agentbox/scripts/sovereign-bootstrap.py:81-92 _keypair_from_privkey_hex` (env-supplied) | as G8 | n/a — caller-supplied key | none — no x-only conversion | as G8, plus runtime env `AGENTBOX_NSEC` / `AGENTBOX_PRIVKEY_HEX` (line 101-107, 213-216) | as G8; also exported to fish profile via `/run/agentbox/identity.env` (line 208-222) |
| G10 | `agentbox/management-api/middleware/linked-data/surfaces/s04-did.js:67-74` | not a generator — re-publishes pubkey only | n/a | n/a | DID Doc body returned to caller | no |
| G11 | `src/services/server_identity.rs:94 Keys::generate()` (DEV auto-generate path) | secp256k1 Schnorr via `nostr-sdk` | `nostr-sdk`'s internal `OsRng` | implicit | in-process `Keys`; pubkey logged at info level (line 109-115); private never logged | no — but if `SERVER_NOSTR_AUTO_GENERATE=true` is set in production by mistake the warning at line 95-99 reaches stderr only |
| G12 | `src/services/server_identity.rs:78-81 parse_secret_key(SERVER_NOSTR_PRIVKEY)` (PROD path) | as G11 | n/a — env-supplied | implicit | in-process `Keys`, in-process only | env var observable to anything that can `cat /proc/<pid>/environ` |
| G13 | `src/services/nostr_bridge.rs:83-86 from_env(VISIONCLAW_NOSTR_PRIVKEY)` | secp256k1 Schnorr | n/a — env | implicit | in-process `Keys` | env-observable |
| G14 | `src/services/nostr_bead_publisher.rs:60-66` | as G13 | n/a — env | implicit | in-process `Keys` | env-observable (same env var as G13 — duplicated load, not shared) |
| G15 | `src/services/nostr_identity_verifier.rs:41` | secp256k1 verify-only (`secp256k1::XOnlyPublicKey`) | n/a — verifier | implicit; `XOnlyPublicKey::from_slice` validates 32 bytes | none — pubkey only | no |
| G16 | `solid-pod-rs/crates/solid-pod-rs-idp/src/jwks.rs:89-135 generate_es256` | **EC P-256 ECDSA** (`p256::ecdsa::SigningKey::random`) | `rand::rngs::OsRng` (line 90) | n/a (different curve) | in-process `SigningKey { private_pem, private_der, ... }`, `Arc<RwLock<JwksInner>>` (line 195) | `private_pem` is a struct field; callers can serialise it (line 56-58 doc) — there is no `Zeroize` derive on `SigningKey` |
| G17 | `solid-pod-rs/crates/solid-pod-rs-activitypub/src/actor.rs:74-87 generate_actor_keypair` | **RSA-2048** (`rsa::RsaPrivateKey::new`) | `rand::thread_rng()` (line 75) — note: `thread_rng` is `OsRng`-seeded but cryptographically less rigorous than direct `OsRng` | n/a | returned to caller as `(priv_pem, pub_pem)` strings | yes — caller decides where to store |
| G18 | `solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs` | does not generate keys; only encodes pubkeys | n/a | `NostrPubkey::from_hex` validates 64 hex chars but **does NOT enforce x-only-curve membership** (no `secp256k1::XOnlyPublicKey::from_slice` round-trip) | n/a | no |

### Critical observations

- **G8/G9 (CRITICAL — audit C2):** `sovereign-bootstrap.py` uses `ecdsa.SigningKey`,
  not `k256::schnorr::SigningKey` or `secp256k1::Keypair`. The library returns
  `to_string()` as a 64-byte `(X || Y)` SEC1 encoding (line 85, 126). Bech32
  encoding the 64-byte buffer as `npub` (line 91, 134) is a CRITICAL break of
  NIP-19, which mandates 32-byte x-only. Any standards-compliant decoder will
  refuse to interop. The `[:32]` truncation at line 89, 132 grabs only the
  X coordinate without applying BIP-340 lift_x — there is no even-y check, so
  if the underlying point has odd y the resulting 32-byte buffer is a
  silently-corrupted x-only pubkey.
- **G8 secret-on-disk (HIGH):** `private_key_hex` is written to
  `/var/lib/agentbox/identities/<agent_id>.json` in plaintext via `write_json`
  (line 76-78, no umask, no chmod). The same key is also written to
  `/run/agentbox/identity.env` as `AGENTBOX_NSEC=<bech32>` (line 213-214) — a
  second copy on the tmpfs. Documentation
  (`agentbox/docs/reference/prd/PRD-001-capabilities-and-adapters.md:378`,
  `mcp/servers/nostr-bridge.js:418, 439-457`) **claims** the key is stored
  AES-256-GCM encrypted at `/workspace/profiles/<stack>/nostr.key.enc` with a
  PBKDF2(MANAGEMENT_API_KEY, salt, 100000) passphrase. **No code path in
  `sovereign-bootstrap.py` actually creates the encrypted file.** The
  `loadSigner` function in `nostr-bridge.js:439-475` reads from a path that
  the bootstrap never writes. This is a documented-but-not-implemented
  encryption layer — the at-rest custody story collapses to "JSON file readable
  by anyone with access to the named volume".
- **G16/G17 (LOW):** the IdP's ES256 key and the ActivityPub actor's RSA-2048
  key are in-process and never serialised by default. `SigningKey::private_pem`
  is held by value (line 56) — there is no `Zeroize` derive, so on `Drop` the
  PEM string lingers in heap until reallocated. Severity LOW because the
  key never leaves the process; HIGH if a heap-dump exporter is added.
- **G11 fail-fast (LOW, aligned):** `server_identity.rs:82-92` correctly aborts
  startup in production if `SERVER_NOSTR_PRIVKEY` is unset and
  `SERVER_NOSTR_AUTO_GENERATE` is not true. The dev auto-generate path logs
  pubkey only.
- **G13/G14 duplicated env-var load (MEDIUM):** `VISIONCLAW_NOSTR_PRIVKEY`
  is decoded twice — once in `nostr_bridge.rs:83` and once in
  `nostr_bead_publisher.rs:60`. Two separate `Keys` instances exist holding
  the same secret. Refactor target: a single shared identity singleton.

---

## I2 — Key derivation (HKDF / PBKDF / Argon2 etc.)

| # | Site | Algo | IKM | Salt | Info | Output use | Doc-vs-impl drift |
|---|---|---|---|---|---|---|---|
| D1 | `nostr-core/src/keys.rs:185-197 derive_from_prf` | HKDF-SHA-256 | WebAuthn PRF (32B) | `Some(&[])` (empty) | `b"nostr-secp256k1-v1"` (line 9) | 32B → `SecretKey::from_bytes` | doc-comment line 178-184 says JS uses `new TextEncoder().encode('nostr-secp256k1-v1')` — **must match byte-identical**; not verified by CI |
| D2 | `auth-worker/src/webauthn.rs:59-65 deterministic_salt_for` | SHA-256 | pubkey hex | tag `b"dreamlab-prf-salt-fallback-v1\0"` (line 61) | n/a | base64url, returned to client as `prfSalt` for unregistered pubkeys (B2 fix) | aligned — purely public-derivable, intentionally fakeable |
| D3 | `nostr-core/src/nip44.rs:122-128` (NIP-44 v2 conversation key) | **HKDF-SHA-256 Expand instead of Extract** (audit C1) | ECDH x-coord | `HKDF_SALT = b"nip44-v2"` | empty `&[]` to `expand` | 32B PRK; **wrong**: spec mandates HMAC-SHA256(salt, IKM) i.e. just the Extract step | **CRITICAL bug** — forum DMs are not interoperable with reference NIP-44. See `05-crypto-gotchas.md` §6. |
| D4 | `nostr-core/src/nip04.rs:43-79` (NIP-04 shared secret) | ECDH (k256) → SHA-256 of x-coord → AES-256-CBC | shared point x | n/a | n/a | `aes_key`, IV stored in ciphertext suffix `?iv=…` | aligned with NIP-04 spec |
| D5 | `src/services/opaque_id.rs` (HMAC-SHA-256 with rotating salt) | HMAC-SHA-256 | user identifier | rotating salt (per epoch) | n/a | opaque ID for redaction | needs verification — file not opened here; rotation cadence undocumented |
| D6 | `agentbox/mcp/servers/nostr-bridge.js:439-460 loadSigner` | **PBKDF2-SHA-256** | `MANAGEMENT_API_KEY` | per-profile salt at `${profilesRoot}/${stack}/nostr.salt` | n/a | 32B → AES-256-GCM key for `nostr.key.enc` | iterations=100,000 hard-coded (line 448). **DOCUMENTED but no writer exists** — sovereign-bootstrap.py never produces `nostr.key.enc` or `nostr.salt`. `loadSigner` will throw on first invocation. |
| D7 | `solid-pod-rs/crates/solid-pod-rs-idp/src/jwks.rs:89-122` | n/a (P-256 keypair gen, not derivation) | OsRng | n/a | n/a | ES256 SigningKey | aligned |
| D8 | `solid-pod-rs/crates/solid-pod-rs-idp/src/passkey.rs` (PRF salt logic) | _file not opened — likely mirrors forum_ | n/a | n/a | n/a | n/a | unverified |

### Critical observations

- **D3 (CRITICAL — audit C1):** NIP-44 v2 conversation-key derivation is the
  WRONG HKDF call. `Hkdf::new(salt, ikm).expand(info, okm)` runs Extract
  internally then Expand on top — but the spec requires `conversation_key =
  HKDF-Extract(salt="nip44-v2", IKM=ECDH-x)` only. The forum's output is
  `HMAC-SHA256(PRK, 0x01)`, not `PRK`. Forum kind-1059 gift-wraps and kind-13
  seals are **not interoperable with any reference NIP-44 implementation**
  (nostr-tools, NDK, nostr-rs-relay). This affects every DM and every gift-wrap
  the forum has ever produced.
- **D1 (HIGH — H11):** the JS-side HKDF info string is documented in a
  comment but is not asserted at build time. If the JS `passkey.ts`
  encoder ever drifts (e.g. to `'nostr-secp256k1-v2'`), every passkey user
  re-registering with the new info string gets a different pubkey and is
  effectively a new identity. There must be a CI cross-language test that
  asserts both implementations produce the same pubkey for a fixed PRF input.
- **D6 (CRITICAL — undocumented gap):** the agentbox key-encryption pipeline
  (`pbkdf2(MANAGEMENT_API_KEY, salt, 100k) → aes-256-gcm`) is fully specified
  in the **reader** (`nostr-bridge.js:439-475`) but has **zero writer**
  implementation. `sovereign-bootstrap.py` writes plaintext JSON only. Calls
  to `loadSigner(stack)` will throw `ENOENT`. Either:
  (a) sovereign-bootstrap.py must be extended to perform AES-256-GCM
      encryption on the freshly generated `private_key_hex` and emit
      `nostr.key.enc` + `nostr.salt`, OR
  (b) `nostr-bridge.js:loadSigner` must be amended to read from the plaintext
      `/var/lib/agentbox/identities/<id>.json`.
  Until reconciled, no agentbox event signing actually works through the
  documented `loadSigner` path.

---

## I3 — Key persistence & custody boundary

For each actor class, where the private key lives at rest, who can read it,
restart behaviour, and zeroisation on unload.

### Forum end-user (passkey-derived)

- **At rest:** **nothing.** Pubkey only is persisted in `localStorage` under
  key `nostr_bbs_session` as `StoredSession` (`forum-client/src/auth/session.rs:151-156`).
  Privkey re-derives from PRF on every login.
- **Reader:** the browser tab only. WebAuthn restricts the PRF output to the
  same RP-ID + same authenticator + same credential.
- **Restart:** privkey lost; user re-authenticates → same key re-derives.
- **Zeroisation:** `pagehide` listener (`session.rs:319-352`) zeroes the
  in-memory `StoredValue<[u8;32]>` and clears any deprecated sessionStorage
  copy (line 339-342). `pageshow` after bfcache forces re-auth if key
  vanished (line 354-374).
- **Custody verdict:** AAA. Best-in-class. The PRF + HKDF-SHA-256 chain is
  reproducible only on the user's authenticator-bound device.

### Forum end-user (NIP-07 extension)

- **At rest:** nothing client-side; private key lives inside the extension
  (Alby, nos2x, etc.).
- **Reader:** the browser extension's process (typically isolated from page
  context).
- **Restart:** key persists in extension; user does not re-authenticate
  cryptographically.
- **Zeroisation:** outside our control.
- **Custody verdict:** delegated; out of scope.

### Forum end-user (nsec-paste / "local-key")

- **At rest:** **DEPRECATED PATH** (`forum-client/src/auth/session.rs:22 SESSION_PRIVKEY_KEY = "nostr_bbs_sk"`).
  Privkey hex stored in `sessionStorage` (line 96-109, `save_privkey_session`).
- **Reader:** any script running in the same origin / same tab. `sessionStorage`
  is per-tab; survives SPA navigation and refresh; cleared on tab close.
- **Restart:** within tab — key restored from sessionStorage (`restore_session`
  line 247-285).
- **Zeroisation:** `clear_privkey_session` (line 121-128) on `pagehide` (line
  342); B8 hardening overwrites the hex string in place before drop (line
  258-262).
- **Custody verdict:** weak by design. Deprecation warning logged at write
  (line 97-102). **Sprint v9 hardening status:** the audit note in the brief
  about "Local-key (imported nsec) callers persist the privkey to sessionStorage"
  is current — it remains the backing path for the imported-nsec flow. The
  recommendation in `05-crypto-gotchas.md` L18 (panic in release builds) has
  NOT been applied; only `console.warn` is emitted (line 97).

### Agentbox sovereign agent

- **At rest:** plaintext JSON `/var/lib/agentbox/identities/<agent_id>.json`
  (`sovereign-bootstrap.py:96, 137`). Fields: `private_key_hex`, `nsec`,
  `npub`, `x_only_pubkey_hex`, `agent_id`, `created_at`. Permission: whatever
  the python `pathlib.Path.write_text` default umask gives — **no explicit
  `chmod 0600`**.
- **Reader:** any process inside the agentbox container with read access to
  `/var/lib/agentbox/identities/`. Container-internal threats include all
  MCP server processes (gemini-flow, openai-flow, deepseek, z.ai),
  user-installed skills, plugins.
- **Restart:** the script reads the existing file (line 116-121) before
  generating a fresh key. Identity persists across restart.
- **Zeroisation:** none. The file persists for the container lifetime.
- **Documentation drift:** `PRD-001 §378-380` and `DDD-003 §44, 65, 311-313`
  describe encrypted at-rest storage at `/workspace/profiles/<stack>/nostr.key.enc`
  with rotation verb `agentbox.sh rotate-keys`. **No such file is created and
  no such rotate-keys subcommand exists in `agentbox.sh` or `scripts/`.**
- **Custody verdict:** **F**. Plaintext private key on filesystem,
  default-permission JSON, no rotation, no zeroisation. Documentation
  promises a security control that is not implemented.

### Agentbox federation key (proposed)

- ADR-073 D4 mandates a **per-relay federation key**, distinct from the
  agent identity. PRD-010 F9 + ADR-074 D10 patterns α/β/γ further
  distinguish substrate-bridge keys from agent keys.
- **Status:** proposed. No code writes or reads
  `/var/lib/agentbox/identities/federation.json` today.
- **Open questions when implemented:** custody must NOT collapse to the
  agent key. The federation-key threat model is "can sign relay-to-relay
  fan-out events" — this is operator-trust scope; the agent-key threat model
  is "can act as the agent's identity" — user-trust scope. They must remain
  separate keys with separate ACLs. Recommendation: separate file,
  separate env var, separate Loader.

### VisionClaw `SERVER_NOSTR_PRIVKEY`

- **At rest:** environment variable. Loaded via `std::env::var` at
  `src/services/server_identity.rs:65`. The key may be supplied as `nsec1…`
  or 64-char hex (`parse_secret_key` line 267-274).
- **Reader:** anyone with `cat /proc/<pid>/environ`, anyone in the
  container with `/proc` access, anyone with the `Dockerfile` /
  `compose` / orchestration-secret-store unwrap.
- **Restart:** key persists in env; identity stable.
- **Zeroisation:** none. `Keys` from `nostr-sdk` does not zeroise on drop.
- **Documentation:** privkey is "**never** logged, never serialised, and never
  returned from any HTTP handler" (`server_identity.rs:14-17, 108-115`).
  Verified — `info!` logs only `pubkey_hex` and `pubkey_npub`.
- **Custody verdict:** B. Standard env-var custody. No leak path observed.

### VisionClaw `VISIONCLAW_NOSTR_PRIVKEY` (transitional duplicate)

- **At rest:** second env var, hex-only (no nsec accepted —
  `nostr_bridge.rs:83 SecretKey::from_hex`). Loaded twice
  (`nostr_bridge.rs:64-86`, `nostr_bead_publisher.rs:48-66`) — two separate
  `Keys` instances hold the same secret.
- **Reader:** as `SERVER_NOSTR_PRIVKEY`.
- **Restart:** stable.
- **Zeroisation:** none.
- **Custody verdict:** C. Two env vars carry related identity, both can be
  the same byte sequence in practice. **HIGH: PRD-010 F1 unification mandate**
  — collapse to a single `SERVER_NOSTR_PRIVKEY`, delete `VISIONCLAW_NOSTR_PRIVKEY`.
  Current code references at `main.rs:574, 578`, `bin/vc_cli.rs:21, 141, 144,
  469-474`, `services/pod_client.rs:11, 34-130`, `nostr_bridge.rs:64-86`,
  `nostr_bead_publisher.rs:21-66, 269-322`.

### VisionClaw `MESH_FEDERATION_PRIVKEY` (proposed)

- Per ADR-073 D4 + ADR-074 D9, a separate per-relay federation key for
  publishing kind-30033 mesh-service-list events. NOT YET IMPLEMENTED — no
  references in `src/`. The four candidate handlers
  (URI resolver redirect targets) listed at `src/handlers/uri_resolver_handler.rs:151-163`
  do not exist either.
- **Recommendation:** introduce a third env var (or a YAML federation-keyset
  file) when implemented. Do not reuse `SERVER_NOSTR_PRIVKEY` — see I12.

### solid-pod-rs-idp issuer key (ES256)

- **At rest:** `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-idp/src/jwks.rs:194-275`
  — held in `Arc<RwLock<JwksInner>>` in process. Active key + retired keys
  for verifier-rollover window (default 7 days, line 220).
- **Persistence:** none built-in. Doc at line 1-7 says "JSS persists keys to
  disk on first boot; this crate generates in-process keys with in-memory
  rotation. Consumers who need disk persistence can serialise the
  PKCS8-PEM `SigningKey::private_pem` field to their own store."
- **Reader:** the IdP process. Any consumer that serialises the PEM is
  responsible for at-rest custody.
- **Restart:** key regenerated; **all issued ID tokens fail verification**
  unless caller persisted PEM. This is a HIGH availability risk for any
  Solid-OIDC-enabled deployment.
- **Zeroisation:** **NO** `Zeroize` on `SigningKey { private_pem, private_der }`.
- **Custody verdict:** D. In-process by default; persistence pushed onto
  caller without a recommended template.

### solid-pod-rs-activitypub actor key (RSA-2048)

- **At rest:** `solid-pod-rs/crates/solid-pod-rs-activitypub/src/actor.rs:74-87
  generate_actor_keypair` returns `(priv_pem, pub_pem)` strings. Caller
  decides where to store. RSA-2048 chosen for Mastodon interop (line 70-73).
- **Reader:** the AP server process; whoever can read the configured store.
- **Restart:** if not persisted, AP federation breaks (Mastodon caches the
  pubkey by `keyId`, then signature verification fails).
- **Zeroisation:** none.
- **Custody verdict:** D. As G16; persistence is caller-responsibility.

---

## I4 — Key rotation & revocation

**STATUS: missing across all four substrates.** This is the single largest
identity gap.

| Substrate | Rotation? | Revocation? | Recovery on lost key |
|---|---|---|---|
| Forum (PRF passkey) | n/a — pubkey IS the PRF output, can't rotate without losing identity | none | re-register a new passkey = new identity; no kind-5 tombstone, no DID `deactivated` flag |
| Forum (nsec import) | manual: import a different nsec, lose old account | none | restore from backup (user's nsec they exported) |
| Forum (NIP-07) | extension's responsibility | extension's responsibility | extension's responsibility |
| Agentbox sovereign | env-var rewrite (`AGENTBOX_PRIVKEY_HEX`) wipes-and-replaces (`sovereign-bootstrap.py:101-113`) — BUT downstream caches still trust old pubkey, no event signals "old key revoked" | none | restore the JSON file from a backup |
| VisionClaw `SERVER_NOSTR_PRIVKEY` | restart with new key = new identity; nothing in the substrate notifies relays | none | restore env var |
| solid-pod-rs IdP (ES256) | `Jwks::rotate()` (jwks.rs:238-244) ✓ — generates new active, retains old in `retired` for `retention` window (7 days default) | `prune_expired` (line 255-261) drops past-retention | only mechanism that handles rotation correctly across the four substrates |
| solid-pod-rs-activitypub | none — RSA actor keys are static per actor | none | regenerate, federation breaks |

### Documentation gap

- `agentbox/docs/reference/prd/PRD-001-capabilities-and-adapters.md:379` documents:
  `agentbox.sh rotate-keys` regenerates `MANAGEMENT_API_KEY`, re-encrypts
  `nostr.key.enc` under the new key, and writes the new key to the profile
  dir. Old key kept for 24 hours for rollback.
- `grep -n "rotate-keys" agentbox.sh` returns zero matches (verified). The
  rotate-keys verb does not exist. PRD documentation is fictional.

### Recovery scenarios

- **Lost passkey-derived key:** user must re-register and is treated as a new
  identity. Forum kind-3 follow lists, kind-30910 moderation actions, ACL
  entries are all bound to the old pubkey. There is **no migration path**.
- **Compromised passkey-derived key:** authenticator compromise is unrecoverable
  in a way that loses identity. The user must re-register; everyone who
  follows the old pubkey continues to trust it because there is no NIP-26
  delegation or kind-5 tombstone signalling rotation.
- **Lost agentbox sovereign key:** plaintext file lost → identity lost. If
  backups disabled (`agentbox.sh backup --no-include-secrets` is the default
  per docs), the agent has no recovery path.
- **Compromised VisionClaw server key:** no way to signal compromise; restart
  with new key produces a new substrate-identity that downstream consumers
  may or may not trust.

### ADR-074 D12 proposed solution

- Old key signs a kind-30033 (replaceable) event whose `service` array
  includes `{"type":"key_rotation","successor": did:nostr:<new_hex>}`.
- New key signs a NIP-26 delegation back-pointer naming old key (within a
  short transition window e.g. 7 days).
- Verifiers reading old-key signed events SHOULD also fetch the latest
  kind-30033 from the same author and follow the rotation pointer.
- DID Doc emitter sets `deactivated: true` on the old DID once rotation
  completes.
- **Status: design only.** Zero LOC implemented across the four substrates.

---

## I5 — DID Document emission

For every emitter, the verification-method type, contexts, services, encoding,
and HTTP route.

### E1 — Forum pod-worker (`pod-worker/src/did.rs`)

- **Tier-1** (line 76-96):
  - `verificationMethod[0].type`: `SchnorrSecp256k1VerificationKey2019` ✓ (audit C3 fix applied Sprint v9)
  - `@context`: `[did/v1, secp256k1-2019/v1]` ✓
  - Both `publicKeyHex` (JSS parity) AND `publicKeyMultibase` (multibase z + multicodec 0xe7 secp256k1-pub) (line 91, 168-174)
  - `alsoKnownAs`: empty array
- **Tier-3** (line 103-161):
  - same VM type ✓
  - `@context`: `[did/v1, secp256k1-2019/v1]` ✓
  - `service`: `SolidStorage`, `SolidWebID` (when webid supplied), `NostrRelay` (when relay_url supplied)
  - `alsoKnownAs`: `[webid]`
  - optional `profile.name`
- **HTTP route:** `/.well-known/did/nostr/<hex>.json` (line 41-43, `well_known_path`).
- **Wired into worker boot:** confirmed via Sprint v8 W2 memory.
- **Verdict:** ALIGNED.

### E2 — solid-pod-rs-nostr (`solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs`)

- **Tier-1** (line 90-104):
  - `verificationMethod[0].type`: `NostrSchnorrKey2024` ✗ (drift; audit C3)
  - `@context`: `[did/v1]` only — **MISSING secp256k1-2019/v1** ✗ (audit H4)
  - has `publicKeyMultibase` ✓
- **Tier-3** (line 113-163):
  - `verificationMethod[0].type`: `NostrSchnorrKey2024` ✗
  - `@context`: `[did/v1, secp256k1-2019/v1]` ✓
  - flexible `services` array via `ServiceEntry`
  - `alsoKnownAs`: `[webid]` if supplied
- **HTTP route:** `well_known_path()` returns same shape as forum (line 56-58).
- **Verdict:** **DRIFTED CRITICAL** on type (C3); **DRIFTED HIGH** on Tier-1 context (H4).

### E3 — Agentbox sovereign-bootstrap (`agentbox/scripts/sovereign-bootstrap.py:183-203`)

- Single tier (no Tier-1 / Tier-3 distinction):
  - `verificationMethod[0].type`: `SchnorrSecp256k1VerificationKey2022` ✗ (the `…2022` cryptosuite does NOT exist in the W3C registry; only `…2019` is registered)
  - `@context`: `[did/v1, secp256k1-2019/v1]` ✓
  - `publicKeyHex`: `identity["public_key_hex"]` — this is the **64-byte SEC1 X‖Y**, NOT the 32-byte x-only (line 194). Inconsistent with the DID URI which uses `x_only_pubkey_hex` (line 156).
  - **No `publicKeyMultibase`.**
  - `alsoKnownAs`: `[webid_url]` (line 199-201) — but `webid_url` is `http://localhost:{SOLID_POD_PORT}/pods/{npub}/profile.json` (line 200, 176), where `npub` is the broken 64-byte bech32 from G8. Won't survive any external resolution.
- **HTTP route:** written to filesystem at `pods/<npub>/did-nostr.json` (line 203). Not served via HTTPS by sovereign-bootstrap; service depends on solid-pod-rs serving `/pods/<npub>/.well-known/did.json`.
- **Verdict:** **DRIFTED CRITICAL** (non-existent VM type; 64-byte publicKeyHex; localhost WebID).

### E4 — Agentbox management-api S04 surface (`agentbox/management-api/middleware/linked-data/surfaces/s04-did.js:31-89`)

- This is a SEPARATE path from E3. Activated by `manifest.linked_data.did`.
  - `verificationMethod[0].type`: **`SchnorrSecp256k1VerificationKey2025`** (line 70) — yet ANOTHER non-existent suite version. **Four different type strings now exist across emitters.**
  - `@context`: `[did/v1, agentbox.dreamlab-ai.systems/ns/v1#]` — **MISSING** `secp256k1-2019/v1`. The `agbx:` context is custom and not registered with W3C.
  - `service`: `SolidPod` (with serviceEndpoint = `solid_pod_rs.base_url`), `NostrRelay` (with `ws://` URL using `bind` and `port`).
  - `publicKeyHex` only; no `publicKeyMultibase`.
  - `alsoKnownAs` only if caller supplies `payload.alsoKnownAs`.
- **HTTP route:** mounted at `/.well-known/did.json` per `management-api/server.js:208-210`. **PUBLIC** (auth bypass intentional, line 205-210).
- **Verdict:** **DRIFTED CRITICAL** (yet another VM type; missing security context).

### E5 — VisionClaw substrate (currently)

- **Does NOT emit DID Documents today.**
- `src/handlers/uri_resolver_handler.rs:159-163` emits a 307 redirect to
  `/api/v1/identity/<hex>/did.json` but no handler is mounted under that path
  — the redirect target 404s. Confirmed: no `pub fn did_handler` /
  `well-known/did/nostr/` route in `src/handlers/`.
- PRD-010 F2 + F15 mandate adding a DID Document handler. Status: paper only.

### Type-string drift summary (CRITICAL)

| Emitter | `verificationMethod.type` |
|---|---|
| Forum pod-worker | `SchnorrSecp256k1VerificationKey2019` ✓ (registered) |
| solid-pod-rs-nostr | `NostrSchnorrKey2024` ✗ |
| Agentbox sovereign-bootstrap | `SchnorrSecp256k1VerificationKey2022` ✗ |
| Agentbox S04 (management-api) | `SchnorrSecp256k1VerificationKey2025` ✗ |
| VisionClaw | (none — handler missing) |

**Four** different strings are emitted (when the missing handler is counted as
a fifth class). A consumer who follows the W3C registry will reject the
non-2019 forms.

### `publicKeyHex` byte-length drift

- Forum pod-worker: 32-byte x-only (correct per BIP-340).
- solid-pod-rs-nostr: 32-byte x-only (correct).
- Agentbox sovereign-bootstrap: **64-byte SEC1 X‖Y** (incorrect — line 194 uses `identity["public_key_hex"]` which is `public_bytes.hex()` = 128 hex chars).
- Agentbox S04: 32-byte hex via `payload.pubkeyHex || did.slice(...)` (correct because the DID URI is x-only hex).

---

## I6 — DID resolution paths

| Resolver | Path | Integrity check on resolved doc | Cache TTL | Fall-through |
|---|---|---|---|---|
| Forum public | `https://pods.dreamlab-ai.com/.well-known/did/nostr/<hex>.json` | none — JSON returned as-is. There is no signature on the DID Doc itself. Trust derives from TLS to `pods.dreamlab-ai.com`. | none documented; CF cache rules unverified | 404 if pod not provisioned |
| Agentbox sibling pod | `${podBase}/.well-known/did.json` | none. Auth is bypassed at `management-api/server.js:208-210` so the document is unauthenticated public. | none documented | 404 |
| Agentbox URI resolver | `/v1/uri/<urn>` returns 307 → DID Doc location | none on response | none | 404/410 |
| VisionClaw URI resolver | 307 to `/api/v1/identity/<hex>/did.json` (`uri_resolver_handler.rs:159-163`) | n/a — handler MISSING | n/a | always 404 (broken) |
| solid-pod-rs `NostrWebIdResolver::resolve_nostr_to_webid` | HTTPS-only; expects `https://<host>/.well-known/did/nostr/<hex>.json`; checks `alsoKnownAs[0]` for the WebID URL | TLS only | per-call (no cache layer) | error propagated |
| ADR-074 D5 — DID-via-relay | `Filter { authors: [hex], kinds: [0, 30033], limit: 2 }` over any peer relay | event signature ✓ — Schnorr verified by relay-side or client-side `Event.verify()` | none | continues to HTTPS fallback |

### Critical observations

- **Forum DID Doc is unsigned.** Anyone who can intercept the TLS connection
  (or who controls the CDN) can swap the DID Doc. This is true of every
  HTTPS-based DID method, not specifically a forum bug, but it MATTERS because
  the forum's DID Doc is used to bootstrap WebID linkage — see I7.
- **Agentbox /.well-known/did.json is route-bypassed (`server.js:205-210`).**
  Comment at line 205-207: "DID documents must be publicly resolvable per the
  DID-Core spec. The document contains only the public key and service
  endpoints — no private data. Gate removal is intentional, not an oversight."
  Reasoning is correct; bypass is intentional. NOTE: this means the document
  is not authenticated by NIP-98 either. That is fine for the public DID Doc
  but the `service` array exposes internal `bind`+`port` (line 56-62) — when
  `bind` is `127.0.0.1` the resolved relay URL leaks loopback as the
  authoritative endpoint. Operator must override `bind` to a public host.
- **VisionClaw 307 → 404.** `uri_resolver_handler.rs:159-163` mints a 307 but
  the redirect target is unimplemented. Any external consumer that follows a
  `urn:visionclaw:did:<hex>` URN gets a broken-link experience.
- **DID-via-relay (ADR-074 D5)** is the only resolver path with cryptographic
  integrity (event signature). Implementing it is the strongest answer to
  the "unsigned DID Doc" gap. Status: design only.

---

## I7 — WebID / DID interplay

| Surface | DID URI | WebID URL |
|---|---|---|
| Forum pod-worker DID Doc | `did:nostr:<hex>` | `alsoKnownAs[0] = https://pods.dreamlab-ai.com/<hex>/profile/card#me` |
| Forum WebID profile card | `schema:identifier "did:nostr:<hex>"` (`webid.rs:34`) | `https://pods.dreamlab-ai.com/<hex>/profile/card#me` |
| Forum `verify_webid_tag` (`pod-worker/src/did.rs:50-60`) | accepts `did:nostr:<hex>` | accepts only `https://pods.dreamlab-ai.com/<hex>/...` (HARDCODED, line 55) |
| Agentbox sovereign-bootstrap profile.json | `id = did:nostr:<x_only_hex>` (line 175) | `webId = http://localhost:{SOLID_POD_PORT}/pods/{npub}/profile.json` (line 176) |
| Agentbox sovereign-bootstrap profile alsoKnownAs | `[did:nostr:<x_only_hex>]` (line 177) | n/a |
| Agentbox sovereign-bootstrap did-nostr.json alsoKnownAs | `[http://localhost:.../pods/<npub>/profile.json]` (line 199-201) | as above |
| VisionClaw `derive_webid` (`solid_pod_handler.rs:401-411`) | n/a (no DID Doc emission) | `{base}/{pubkey_hex}/profile/card#me` (matches forum shape) |

### Critical observations

- **`pods.dreamlab-ai.com` is hardcoded** in `pod-worker/src/did.rs:55` —
  `verify_webid_tag` rejects any other host. A federated WebID (e.g. a user
  hosting their pod at `https://my-pod.example/<hex>/profile/card#me`) is
  silently rejected as "not controlled by this pubkey", **even when the
  remote DID Doc's `alsoKnownAs` linkage is valid**. This blocks federation
  and is `05-crypto-gotchas.md` H9.
- **Agentbox WebID is `http://localhost:8484` (`sovereign-bootstrap.py:176, 200`).**
  This is fine for a single-host deployment but **breaks the moment the pod is
  served externally**. The bootstrap script does not parameterise `pod_base`
  to read from `agentbox.toml` integrations.solid_pod_rs.base_url. The S04
  surface (`s04-did.js:48-53`) DOES read from `manifest.integrations.solid_pod_rs.base_url`
  — these two surfaces will disagree on WebID host as soon as the operator
  configures a non-localhost pod.
- **Cross-system question — do forum and agentbox profiles for the SAME
  identity agree on `did:nostr:<hex>`?**
  - Forum: `webid.rs:34` writes `schema:identifier "did:nostr:<hex>"` where
    `<hex>` is the 32-byte x-only forum pubkey.
  - Agentbox: `sovereign-bootstrap.py:175` writes `id: did:nostr:<x_only_hex>`
    where `<x_only_hex>` is `public_bytes[:32].hex()` from the **ECDSA**
    keypair (G8). If the underlying point has odd y, this `x_only_hex` is a
    **silently corrupted x-only encoding** that does NOT match what a
    standards-compliant Schnorr signer would produce.
  - For the SAME logical owner to host both surfaces, a single canonical
    32-byte BIP-340 x-only pubkey must be derived. Currently impossible
    because of audit C2 (`sovereign-bootstrap.py` doesn't apply lift_x).
- **VisionClaw's WebID derivation (`solid_pod_handler.rs:401-411`) matches the
  forum's `pods.dreamlab-ai.com` shape** (path = `<base>/<pubkey>/profile/card#me`).
  But VisionClaw never emits a DID Doc, so the bidirectional linkage is
  one-way only.

---

## I8 — alsoKnownAs linkage

The bidirectional cross-references between DID Doc, WebID profile, and kind-0
metadata.

| Direction | Forum | Agentbox sovereign | Agentbox S04 | VisionClaw |
|---|---|---|---|---|
| DID Doc → WebID | `alsoKnownAs[0] = pods.dreamlab-ai.com/<hex>/profile/card#me` ✓ | `alsoKnownAs = [http://localhost:<port>/pods/<npub>/profile.json]` ✗ (localhost) | only if caller supplies `payload.alsoKnownAs` (line 85) ✗ | none — no DID Doc emitted |
| WebID → DID | `schema:identifier "did:nostr:<hex>"` (forum/webid.rs:34) ✓ | `alsoKnownAs: [did:nostr:<hex>]` in profile.json (line 177) ✓ | n/a | none |
| kind-0 metadata → DID | not implemented; kind-0 has no `alsoKnownAs` field today | not emitted by sovereign-bootstrap | n/a | none |
| kind-0 metadata → WebID | not implemented | not emitted | n/a | none |

### Federation breaks

- **Agentbox WebID hardcoded to `http://localhost:8484`** (sovereign-bootstrap.py:176,
  200). The same value is used in the pod profile.json and in the DID Doc
  alsoKnownAs. Any external relay that resolves `did:nostr:<hex>` and
  consults `alsoKnownAs` finds a localhost URL — it cannot reach it. This is
  a single-deployment-only configuration; it does not federate.
- **Forum hardcoded to `pods.dreamlab-ai.com`** (pod-worker/src/did.rs:55, 14;
  webid.rs:10). A user who self-hosts cannot interoperate.
- **No kind-0 alsoKnownAs convention.** ADR-074 D5 + PRD-010 propose using
  kind-0 `content.alsoKnownAs` as an additional carrier; not implemented.

---

## I9 — Multi-DID per actor

For each actor class, what DID Documents need to exist and where they live.

### Forum end-user

- **One DID per user.**
- Emitted via pod-worker at `/.well-known/did/nostr/<hex>.json` per user.
- The user has exactly one pubkey (PRF-derived); one DID Doc.

### Agentbox container

- **One DID per container today** (`sovereign-bootstrap.py:233 agent_id =
  os.getenv("AGENTBOX_AGENT_ID", "agentbox-core")`). The `agent_id` is a
  single string per process. The identity file is at
  `/var/lib/agentbox/identities/<agent_id>.json`.
- **PRD-010 P5 / multi-agent follow-up** describes a many-DIDs-per-container
  case where the agentbox runs multiple distinct agent identities. Today this
  is impossible because:
  - `sovereign-bootstrap.py` reads ONE `AGENTBOX_AGENT_ID` env var.
  - The runtime env file (`/run/agentbox/identity.env`, line 208-222) exports
    a single `AGENTBOX_NPUB`, `AGENTBOX_NSEC`, etc.
  - `loadSigner(stack)` in `nostr-bridge.js:431-475` looks up keys by stack
    name (profiles directory layout), so the JS side could in principle handle
    multiple identities; but the python side only mints one.
- **Recommendation:** when P5 lands, sovereign-bootstrap.py must accept a
  list of `agent_id`s (TOML array) and emit one DID Doc per identity, each
  in its own pod path.

### VisionClaw substrate

- **Today: ZERO DIDs emitted.** Despite holding `SERVER_NOSTR_PRIVKEY` and
  `VISIONCLAW_NOSTR_PRIVKEY` secrets, the substrate publishes neither a
  DID Doc nor a WebID profile. It only exposes
  `GET /api/server/identity` (`server_identity_handler.rs:28-38`) which
  returns pubkey/npub/supported_kinds/relay_urls — a NON-DID-Core JSON shape.
- **Post-PRD-010 F1:** one DID per substrate. The unified
  `SERVER_NOSTR_PRIVKEY` becomes the canonical operator identity.
- **Post-PRD-010 F9 + ADR-074 D9:** plus one **bridge** identity per substrate
  for re-signed events (today, `nostr_bridge.rs:219-222` re-signs under the
  bridge key but the bridge key IS `VISIONCLAW_NOSTR_PRIVKEY`, conflating
  operator and bridge).

### Federation key (relay-relay)

- Per ADR-073 D4: one **federation key per private relay**. Cardinality:
  per-deployment. If forum, agentbox, and VisionClaw each run their own
  private relay, that's three federation keys.
- Today: not implemented.

### Bridge identity

- Per ADR-074 D10:
  - **Pattern α (User → Substrate Bridge):** the substrate signs on behalf of
    the user via NIP-26 delegation. User's hex pubkey appears in the
    `delegation` tag; bridge pubkey in `event.pubkey`.
  - **Pattern β (User → Agent):** the agent's sovereign key signs, but a
    user-issued NIP-26 delegation is attached. Agent pubkey in
    `event.pubkey`; user pubkey in delegation tag.
  - **Pattern γ (Server → Server):** federation-key signs relay-to-relay
    fan-out events. No user identity attached.
- Today: only pattern γ has scaffolding (kind-9 re-signs in `nostr_bridge.rs:219-222`)
  and that scaffolding does NOT carry a delegation tag — original-author
  attribution is **lost** (M14 in `05-crypto-gotchas.md`).

---

## I10 — Identity attribution under delegation

### Display semantics (intended)

- Under NIP-26 delegation:
  - `event.pubkey` = the **delegatee** (the actual signer).
  - `tags[].delegation = [delegator_pk, conditions, sig]` = the **delegator**.
  - UI MUST display **delegator** as author, with a "via <delegatee>" affix
    or similar. (Forum UI does not yet do this — `forum-client/src/auth/nip07.rs`
    has no delegation UI per `05-crypto-gotchas.md` M13.)

### Wire semantics (intended)

- `event.pubkey` carries the delegatee hex (the signer).
- `event.id` recomputed from canonical JSON over delegatee fields.
- Delegation tag's `sig` is over
  `SHA-256("nostr:delegation:" || delegatee_pk || ":" || conditions_str)`
  (`nostr-core/src/nip26.rs:282`).

### WAC interaction (PRD-010 ADR-074 D11)

- Pod ACL accepts `{event.pubkey, delegator_pk}` as the effective agent set
  for authorisation.
- Today, `pod-worker/src/acl.rs:162-188` matches `acl:agent` strings only
  — there is no delegation-aware predicate. To grant access via delegation,
  the agent's pubkey must be in the ACL explicitly.
- ACL drift risk: `lib.rs:447 format!("did:nostr:{pk}")` does not lowercase
  `pk` (audit H7). Delegated agents could miss an ACL match purely from case
  drift.

### Cross-substrate verifier wiring

- Forum (`nostr-core/src/nip26.rs`): full impl, tested.
- Agentbox: NOT WIRED into `mcp/servers/nostr-bridge.js` `verifyNip98` —
  the only verification is the structural NIP-98 check + `verifyEvent` from
  `nostr-tools` which checks **only the outer signature**. Delegation tag
  is ignored.
- VisionClaw `nostr_bridge.rs:192 verified.verify()` — same: only outer
  signature, delegation tag ignored.
- solid-pod-rs: no delegation impl observed.

### What this means

- A user holding a forum passkey-derived key today CANNOT sign an event that
  agentbox accepts as "the user's identity" via delegation. Agentbox
  recognises only `event.pubkey` and would reject a delegation tag as
  unsigned-by-known-key.
- The chosen direction (`05-crypto-gotchas.md` H5, ADR-074 D10) is to wire
  `nip26::verify_delegation_tag` into agentbox's `verifyNip98` function and
  into VisionClaw's `nostr_bridge.rs::forward_to_forum`. Until done, the
  three substrates share a vocabulary (`did:nostr:<hex>`) but cannot share
  authority.

---

## I11 — Cross-system identity smuggling risk

### Threat 1: Forum user impersonates agentbox agent

- **Vector:** forum user signs an event with their own key but tags it
  `["h", "agentbox-core"]` or sets `event.pubkey` to the agentbox hex.
- **Gating step 1 (signature):** `event.verify()` would fail — the signature
  would not match a forged `event.pubkey`. So `event.pubkey` cannot be
  replaced.
- **Gating step 2 (tag-based attribution):** if downstream consumers trust an
  `["agent_id", "agentbox-core"]` tag for routing or display, the forum user
  can spoof that tag freely. **No tag verification currently exists.**
- **Verdict:** signature-protected fields are safe; tag-based attribution is
  forgeable. Document explicitly: tags do not authenticate.

### Threat 2: Agentbox impersonates forum admin (kind 30910)

- **Vector:** agentbox signs a kind-30910 (Ban) event under its sovereign key
  and pushes it to the forum relay.
- **Gating step 1 (signature):** `event.pubkey == agentbox_hex` → signature
  verifies (the agentbox key signs honestly).
- **Gating step 2 (admin-set membership):** moderation enforcement at
  `relay-worker/src/relay_do/nip_handlers.rs:401-405` checks `auth::is_admin`
  via D1 whitelist (`auth-worker/src/admins.rs`). The agentbox key is
  presumably NOT in the admin set.
- **Result:** the event is signature-valid but mod-action-unauthorised, so
  the relay drops it.
- **Risk subscale:** if agentbox pubkey ever gets added to the admin set
  (legitimately for one purpose), it can mint moderation events (different
  purpose) — there is no kind-level scoping in `is_admin`. **Recommendation:**
  scope admin authority by event kind in the D1 schema.

### Threat 3: Compromised relay forges events

- **Vector:** an attacker who controls the JSS relay (between agentbox and
  the forum) injects events authored under any pubkey.
- **Gating step (forward_to_forum):** `nostr_bridge.rs:192 verified.verify()`
  catches any signature mismatch. Confirmed.
- **Gating gap (re-signing):** when the bridge re-signs under its own key
  (line 219-222), the original signature is **discarded**. The forum sees a
  legitimately bridge-signed kind-9 event with `source_event` tag. If the
  bridge's pubkey is in the admin set, an attacker who compromises the JSS
  relay AND controls a forged "source event" can effectively mint
  bridge-attributed events that pass through to the forum. The forum has no
  way to re-verify the source event because the bridge verified it before
  re-signing. **This is the critical bridge-attribution-loss vector
  (M14).**
- **Mitigation:** attach the original signed event as a tag (e.g. wrap it
  inside kind-1059) so the forum can independently verify it. Or: stop
  re-signing and federate the original event verbatim through ADR-073 D2
  fan-out.

### Threat 4: Re-issued NIP-98 token across workers

- **Vector:** an attacker captures a NIP-98 token for `pod-worker` and
  replays it against `auth-worker`.
- **Gating:** `KvReplayStore` is per-worker (Sprint v9 STREAM-B). NIP-98 binds
  the URL into the signed event, but URL rewriting (e.g. CDN rewrites, internal
  proxies) could enable cross-worker reuse.
- **Mitigation:** `05-crypto-gotchas.md` H6 — single shared `NIP98_REPLAY` KV
  binding across all four workers. NOT YET DONE.

---

## I12 — Federation key proliferation risk

### Cardinality count for a fully-deployed mesh

For a deployment running forum + agentbox + VisionClaw + solid-pod-rs:

| Key | Count | Owner | Custody | Rotation |
|---|---|---|---|---|
| Forum end-user signing key (PRF-derived) | 1 per user | end-user | browser memory only | none |
| Forum local-key (nsec import) | 1 per user (alternate) | end-user | sessionStorage (deprecated) | none |
| Forum NIP-07 extension key | 1 per user (alternate) | extension | extension storage | extension's responsibility |
| Forum gift-wrap throwaway | 1 per gift-wrap call (ephemeral) | n/a | local stack, zeroised | n/a |
| Agentbox sovereign agent | 1 per `AGENTBOX_AGENT_ID` (today: 1 per container) | container | plaintext JSON | none |
| Agentbox federation key (proposed ADR-073 D4) | 1 per relay | container | TBD | none yet |
| VisionClaw `SERVER_NOSTR_PRIVKEY` | 1 per substrate | operator | env var | manual restart |
| VisionClaw `VISIONCLAW_NOSTR_PRIVKEY` (transitional) | 1 per substrate (often = `SERVER_NOSTR_PRIVKEY`) | operator | env var | manual restart |
| VisionClaw `MESH_FEDERATION_PRIVKEY` (proposed ADR-073 D4) | 1 per substrate | operator | TBD | none yet |
| VisionClaw bridge key (PRD-010 F9) | 1 per bridge (today: = `VISIONCLAW_NOSTR_PRIVKEY`) | operator | env var | manual |
| VisionClaw kind-30033 publisher (ADR-074 D9) | 1 (could re-use server or federation key) | operator | TBD | none |
| solid-pod-rs IdP ES256 active | 1 per IdP | operator | in-memory (default) | `Jwks::rotate()` ✓ |
| solid-pod-rs IdP ES256 retired | 0..N (7-day window) | operator | in-memory | auto-prune |
| solid-pod-rs ActivityPub actor RSA-2048 | 1 per actor | operator | caller-managed PEM | none |

### Total per-deployment minimum (CURRENT)

- Forum: 1 server-side key (none — forum has no server-side signing key today).
- Agentbox: 1 sovereign agent.
- VisionClaw: up to 2 (`SERVER_NOSTR_PRIVKEY` + `VISIONCLAW_NOSTR_PRIVKEY`).
- solid-pod-rs IdP: 1 ES256.
- solid-pod-rs ActivityPub: 1 RSA-2048.

≈ **5 long-lived keys** per multi-substrate deployment.

### Total per-deployment minimum (POST-PRD-010, ADR-073, ADR-074)

- Forum: 0 server-side (no change).
- Agentbox: 1 sovereign + 1 federation key = 2.
- VisionClaw: 1 unified `SERVER_NOSTR_PRIVKEY` + 1 federation + 1 bridge = 3.
- solid-pod-rs IdP: 1 ES256 active + retired set.
- solid-pod-rs ActivityPub: 1 RSA-2048 per AP actor.

≈ **7-8 long-lived keys** per deployment, plus an ES256 retired set, plus
N AP actor keys. Multi-region or multi-tenant deployments multiply.

### Rotation story

- ES256 IdP key: `Jwks::rotate()` ✓ — only mature path.
- Everything else: **manual restart with new env var or new file**.
- ADR-074 D12 proposes a kind-30033 + NIP-26-anchored rotation announcement
  — design only.

### Operator key-management story

- Today: ad-hoc. Two env vars in VisionClaw, one JSON file in agentbox, one
  in-process key in IdP, one caller-supplied PEM in AP. No single rotation
  command, no single audit log of rotation events, no key-versioning across
  substrates.
- Recommendation: introduce a `keymeta.toml` or RuVector-backed per-substrate
  table tracking `(substrate, role, kid, created_at, retired_at, rotation_event_id)`.
  Wire `agentbox.sh rotate-keys`, `vc rotate-keys` and `forum-admin rotate`
  CLI verbs that update the table and emit a kind-30033 announcement.

### Cardinality risk

- The proposed federation-key + bridge-key + kind-30033-publisher distinction
  is **architecturally correct** (different threat scopes need different
  custody) but **operationally onerous** without tooling. A solo operator
  managing 3 substrates × 3-4 keys each = 9-12 keys is more than they will
  rotate manually.
- **Recommendation:** keep the role separation conceptually, but provide
  a single `agentbox.sh rotate-keys` (and analogues) that rotates ALL roles
  atomically and signs the announcement events. Without tooling support,
  operators will collapse roles back into one shared key under pressure.

---

## I13 — Recommendations summary (file:line, severity, fix, cross-substrate impact, regression test)

### Q3-CRITICAL-01 — sovereign-bootstrap.py emits broken npub

- **File:line:** `/home/devuser/workspace/project/agentbox/scripts/sovereign-bootstrap.py:81-92, 123-135, 141, 176, 200`
- **Severity:** CRITICAL (blocks all federation)
- **Fix:** replace `from ecdsa import SECP256k1, SigningKey` with `secp256k1` (Python binding to the libsecp256k1 C library) or `coincurve`. After keypair generation, apply BIP-340 lift_x: take 32-byte X, parse as `XOnlyPublicKey`, ensure even-y. Bech32-encode the 32-byte x-only buffer as `npub`. Use the same x-only hex everywhere (DID URI, pod path, profile, did-nostr.json).
- **Cross-substrate impact:** all four substrates assume a 32-byte BIP-340 x-only pubkey is canonical. Forum, VisionClaw, solid-pod-rs all reject the current 64-byte agentbox npub. Without this fix, agentbox events are unverifiable on any other substrate.
- **Regression test:** add `tests/sovereign_bootstrap_npub_test.py` that runs the bootstrap with a fixed seed, decodes the resulting `npub` via `nostr-tools::nip19.decode`, asserts `data.length == 32`, asserts the hex matches `x_only_pubkey_hex`, and asserts `secp256k1::XOnlyPublicKey::from_slice(...).is_ok()`.

### Q3-CRITICAL-02 — verificationMethod.type drift

- **File:line:** `solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs:98, 154`; `agentbox/scripts/sovereign-bootstrap.py:192`; `agentbox/management-api/middleware/linked-data/surfaces/s04-did.js:70`
- **Severity:** CRITICAL
- **Fix:** standardise on `SchnorrSecp256k1VerificationKey2019` everywhere. Forum already aligned at `pod-worker/src/did.rs:88, 146`. Patch the three drift sites; add CI test in each repo asserting the exact string.
- **Cross-substrate impact:** consumers using a strict W3C VC verifier reject `NostrSchnorrKey2024` / `…2022` / `…2025`. With four substrates emitting four type strings, no single verifier accepts them all.
- **Regression test:** for each emitter, parse the rendered DID Doc and assert `vm[0].type == "SchnorrSecp256k1VerificationKey2019"` and `@context contains "https://w3id.org/security/suites/secp256k1-2019/v1"`.

### Q3-CRITICAL-03 — agentbox key custody collapse

- **File:line:** `agentbox/scripts/sovereign-bootstrap.py:76, 87, 130-137`; `agentbox/mcp/servers/nostr-bridge.js:439-475`
- **Severity:** CRITICAL (custody documentation/implementation drift)
- **Fix:** EITHER (a) extend `sovereign-bootstrap.py` to perform AES-256-GCM(PBKDF2(MANAGEMENT_API_KEY, salt, 100000)) on `private_key_hex`, write `nostr.key.enc` + `nostr.salt`, AND DELETE the plaintext `private_key_hex` from the JSON; OR (b) amend `nostr-bridge.js:loadSigner` to read directly from `/var/lib/agentbox/identities/<id>.json`'s plaintext field and update PRD-001 + DDD-003 to reflect the plaintext at-rest model.
- **Cross-substrate impact:** today no agentbox event signing actually works through the documented `loadSigner` path because the encrypted file does not exist. Tests that exercise the bridge throw on first call. Also: the documentation that pretends key encryption exists creates a false sense of security.
- **Regression test:** integration test that boots agentbox, calls `loadSigner('default')`, signs an event, verifies it, asserts no plaintext private key on disk.

### Q3-HIGH-04 — Tier-1 secp256k1 context missing in solid-pod-rs-nostr

- **File:line:** `solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs:93`
- **Severity:** HIGH
- **Fix:** change `@context` to `["https://www.w3.org/ns/did/v1", "https://w3id.org/security/suites/secp256k1-2019/v1"]`.
- **Cross-substrate impact:** strict JSON-LD processors silently drop the `NostrSchnorrKey2024` term when its defining context is missing → verificationMethod becomes effectively untyped in expanded form.
- **Regression test:** golden-doc test asserting Tier-1 contains both contexts.

### Q3-HIGH-05 — VisionClaw DID handler missing

- **File:line:** `src/handlers/uri_resolver_handler.rs:159-163` (redirect target unimplemented)
- **Severity:** HIGH (PRD-010 F2 + F15)
- **Fix:** add `src/handlers/identity_handler.rs` exposing
  `GET /api/v1/identity/{hex}/did.json` and `GET /.well-known/did/nostr/{hex}.json`.
  Render Tier-3 DID Doc using `solid_pod_rs_nostr::did::render_did_document_tier3`
  AFTER applying the C2/C3 fixes. Source the pubkey from `ServerIdentity`.
- **Cross-substrate impact:** without this, VisionClaw is a non-citizen of
  the did:nostr mesh — agentbox and forum cannot resolve VisionClaw's
  identity via DID Doc.
- **Regression test:** integration test fetching `/.well-known/did/nostr/<hex>.json`, asserting Tier-3 shape, contexts, type-2019, and matching pubkey.

### Q3-HIGH-06 — Forum verify_webid_tag hardcoded

- **File:line:** `pod-worker/src/did.rs:55`
- **Severity:** HIGH (federation block)
- **Fix:** in addition to the hardcoded `pods.dreamlab-ai.com` check, accept ANY URL whose corresponding DID Doc (resolved via `/.well-known/did/nostr/<hex>.json` from that URL's host) lists the WebID under `alsoKnownAs[]` AND whose verificationMethod contains the `event_pubkey`.
- **Cross-substrate impact:** federated WebID hosting is impossible until this is fixed. Solid-pod-rs deployments at non-`pods.dreamlab-ai.com` hosts cannot interoperate.
- **Regression test:** verify with both same-host and federated-host WebIDs; assert acceptance of valid federated, rejection of invalid (alsoKnownAs missing).

### Q3-HIGH-07 — Agentbox WebID localhost hardcoded

- **File:line:** `agentbox/scripts/sovereign-bootstrap.py:176, 200`
- **Severity:** HIGH
- **Fix:** parameterise `pod_base` from `manifest.integrations.solid_pod_rs.base_url` (already read in `s04-did.js:48-53`). Both surfaces must agree.
- **Cross-substrate impact:** any external resolution of `did:nostr:<hex>` over the agentbox WebID currently 502s.
- **Regression test:** boot agentbox with `solid_pod_rs.base_url=https://example.test`, assert `webId` in profile.json starts with `https://example.test/`.

### Q3-HIGH-08 — Bridge re-signing loses attribution

- **File:line:** `src/services/nostr_bridge.rs:213-222`
- **Severity:** HIGH (M14 in `05-crypto-gotchas.md`)
- **Fix:** option A — propagate the original signed event verbatim (kind 1059 wrap with bridge as recipient or transparent fan-out). Option B — preserve the original `id` AND `sig` AND `pubkey` in dedicated tags `["original_pubkey", ...]`, `["original_sig", ...]`, `["original_id", ...]` so forum readers can independently re-verify.
- **Cross-substrate impact:** if the bridge key is admin-set, attribution loss enables third-party impersonation through bridge.
- **Regression test:** assert that for every bridge-emitted event, original signature is independently verifiable.

### Q3-HIGH-09 — NIP-26 verifier wiring on agentbox + VisionClaw

- **File:line:** `agentbox/mcp/servers/nostr-bridge.js:321-383 verifyNip98` (no delegation check); `src/services/nostr_bridge.rs:192` (no delegation check)
- **Severity:** HIGH (audit H5)
- **Fix:** call `nip26::verify_delegation_tag` (Rust side: import from `nostr-core` crate; JS side: port to `nostr-bridge.js` or use `nostr-tools` 2.x delegation helper) before treating the event as authentic. The user pubkey from a verified delegation tag becomes the effective identity for ACL / attribution purposes.
- **Cross-substrate impact:** without this, a forum user's passkey-derived key cannot grant the agentbox the right to sign on their behalf without the agentbox importing the user's hex pubkey out-of-band. NIP-26 is the standard mechanism.
- **Regression test:** end-to-end: forum user mints delegation tag, agentbox signs an event under its own key with that tag attached, forum relay accepts (event passes both signature and delegation verification), forum reader sees user as effective author.

### Q3-HIGH-10 — Key rotation announcement protocol absent

- **File:line:** none — design only (`05-crypto-gotchas.md` §11; ADR-074 D12)
- **Severity:** HIGH (long-term key hygiene)
- **Fix:** implement kind-30033 (`["d", "rotation"]`, `service[]` includes `{"type":"key_rotation","successor":"did:nostr:<new>"}`) signed by old key. New key signs a NIP-26 delegation back-pointer naming old key with a 7-day `created_at>X & created_at<Y` window. DID Doc emitter writes `deactivated: true` on the old DID. Verifiers fetch latest kind-30033 from each authority before trusting.
- **Cross-substrate impact:** without rotation, any compromise is identity loss.
- **Regression test:** simulate rotation; assert new key inherits old key's privileges within window; assert old key signatures rejected by verifiers after `deactivated: true` flag visible.

### Q3-HIGH-11 — JS↔Rust HKDF info-string drift detection

- **File:line:** `nostr-core/src/keys.rs:9` (`HKDF_INFO = b"nostr-secp256k1-v1"`); JS side at `dreamlab-ai-website/community-forum-rs/crates/forum-client/src/auth/passkey.rs:150` (delegates to `nostr-core::derive_from_prf` from WASM, so they share the same constant — but the auth-worker JS server-side must compute or accept the same).
- **Severity:** HIGH (audit H11)
- **Fix:** add a build-time integration test that imports both implementations and asserts they produce the same pubkey for a fixed PRF input (e.g. all-`0x42` 32 bytes).
- **Cross-substrate impact:** a single byte of drift between JS and Rust `info` produces a different pubkey for every passkey user.
- **Regression test:** the test itself is the regression detector. Wire into CI.

### Q3-MEDIUM-12 — VisionClaw env var unification

- **File:line:** `src/services/nostr_bridge.rs:64-86`; `src/services/nostr_bead_publisher.rs:48-66`; doc references at `main.rs:574, 578`; `bin/vc_cli.rs:21, 141, 144, 469-474`; `src/services/pod_client.rs:11, 34-130`
- **Severity:** MEDIUM (PRD-010 F1)
- **Fix:** delete `VISIONCLAW_NOSTR_PRIVKEY` env var. Use `SERVER_NOSTR_PRIVKEY` everywhere. If a separate bridge identity is needed, introduce `BRIDGE_NOSTR_PRIVKEY` explicitly, do not hide it inside a transitional alias.
- **Cross-substrate impact:** operators currently must set two env vars; one variant is sometimes the same, sometimes different. Clarify by collapsing.
- **Regression test:** boot test asserts substrate runs with only `SERVER_NOSTR_PRIVKEY` set; emits a deprecation warning when `VISIONCLAW_NOSTR_PRIVKEY` is set.

### Q3-MEDIUM-13 — Shared NIP-98 replay store

- **File:line:** `auth-worker/wrangler.toml`, `pod-worker/wrangler.toml`, `relay-worker/wrangler.toml`, `search-worker/wrangler.toml` — each has its own `NIP98_REPLAY` namespace.
- **Severity:** MEDIUM (audit H6)
- **Fix:** introduce a single shared `NIP98_REPLAY` KV binding. Bind from all four `wrangler.toml`. The cost is one extra namespace; the benefit is replay correctness under URL rewriting.
- **Cross-substrate impact:** local issue; doesn't yet affect agentbox/VisionClaw because they don't share a replay store with the forum. Recommend a shared store IF cross-substrate NIP-98 traffic ever flows.
- **Regression test:** capture a NIP-98 token, replay against all four workers, assert exactly one accepts.

### Q3-MEDIUM-14 — ACL agent-IRI normalisation

- **File:line:** `pod-worker/src/lib.rs:447 format!("did:nostr:{pk}")`; `pod-worker/src/acl.rs:162-188`
- **Severity:** MEDIUM (audit H7, H8)
- **Fix:** lowercase `pk` before constructing IRI. Normalise on read AND on write in `agent_matches`. Add regex check at NIP-98 ingress (`nip98.rs:258`) rejecting non-lowercase pubkey.
- **Cross-substrate impact:** agentbox + VisionClaw both write ACLs with `did:nostr:<hex>`; if any of them ever emits uppercase, forum's WAC silently fails-closed on those identities.
- **Regression test:** mixed-case ACL, lowercase agent → expect access. Both case-normalised → expect access. Mismatched → expect 403.

### Q3-MEDIUM-15 — Document case where bridge key conflates with substrate key

- **File:line:** `src/services/nostr_bridge.rs:64` (`VISIONCLAW_NOSTR_PRIVKEY`); `src/services/nostr_bead_publisher.rs:48`
- **Severity:** MEDIUM
- **Fix:** explicitly distinguish bridge key from substrate-operator key in code, even if the operator chooses to set them to the same value. Add a startup log line announcing the distinction.
- **Cross-substrate impact:** today an attacker who compromises the bridge key gets full substrate-operator authority for free.
- **Regression test:** unit test asserting two distinct `Keys` are loaded; warn if equal.

### Q3-MEDIUM-16 — Multi-DID per agentbox container

- **File:line:** `agentbox/scripts/sovereign-bootstrap.py:233`
- **Severity:** MEDIUM (PRD-010 P5)
- **Fix:** read `agent_ids: list[str]` from `agentbox.toml`. Loop the bootstrap, write one identity file per agent_id; update `s04-did.js` to map per-stack identities.
- **Cross-substrate impact:** unblocks multi-agent deployments per container.
- **Regression test:** boot with two agent_ids, assert two identity files, two DID Docs, two pod profiles.

### Q3-LOW-17 — Hardened forum sessionStorage privkey

- **File:line:** `forum-client/src/auth/session.rs:96-109`
- **Severity:** LOW (audit L18)
- **Fix:** in `cfg!(release)` builds, change `console::warn` to a panic / disable the function. Local-key import path then routes through a different storage mechanism (e.g. encrypted-IndexedDB) or the user explicitly accepts a downgraded UX.
- **Cross-substrate impact:** none — same-substrate hardening.
- **Regression test:** WASM release build assert `save_privkey_session` errors.

### Q3-LOW-18 — solid-pod-rs IdP `SigningKey` zeroisation

- **File:line:** `solid-pod-rs/crates/solid-pod-rs-idp/src/jwks.rs:48-65 SigningKey`
- **Severity:** LOW (heap-residue)
- **Fix:** add `zeroize::Zeroize` derive on `SigningKey` (or wrap `private_pem` and `private_der` in `Zeroizing<...>`). Drop now zeroises.
- **Cross-substrate impact:** local hygiene.
- **Regression test:** add memory-residue test using `zeroize` test helper.

### Q3-LOW-19 — Document agentbox S04 third (different) VM type

- **File:line:** `agentbox/management-api/middleware/linked-data/surfaces/s04-did.js:70`
- **Severity:** LOW (subset of Q3-CRITICAL-02)
- **Fix:** part of the C3 standardisation. Note: there are two separate emitters in agentbox (sovereign-bootstrap and S04) — ensure both are patched.
- **Cross-substrate impact:** same as C3.
- **Regression test:** golden-doc test asserting both emitters produce identical type strings.

### Q3-LOW-20 — Add ZeroizeOnDrop to VisionClaw `Keys`

- **File:line:** `src/services/server_identity.rs:44-51`; `src/services/nostr_bridge.rs:54`; `src/services/nostr_bead_publisher.rs:38-43`
- **Severity:** LOW
- **Fix:** wrap the `Keys` field of `ServerIdentity`, `NostrBridge`, `NostrBeadPublisher` in a newtype that zeroises the secret-key bytes on drop. The `nostr-sdk` `Keys` struct does not zeroise by default.
- **Cross-substrate impact:** local hygiene.
- **Regression test:** memory-residue test.

### Q3-LOW-21 — Register kind-30033 publisher key separately

- **File:line:** ADR-074 D9 (proposed); not yet implemented anywhere.
- **Severity:** LOW (forward-looking)
- **Fix:** when implementing D9, ensure the kind-30033 publisher uses a key distinct from `SERVER_NOSTR_PRIVKEY` and from federation key. Emit kind-30033 events that link the publisher's pubkey to the operator's pubkey via NIP-26 delegation, so the operator can rotate the publisher without invalidating the substrate identity.
- **Cross-substrate impact:** prevents the I12 cardinality from becoming a single-key choke point.
- **Regression test:** verifies kind-30033 events carry delegation tags from operator to publisher.

---

## Cross-cutting summary

The four substrates share **one identity vocabulary** (`did:nostr:<64-hex>`)
and **three custody regimes** (browser memory / filesystem JSON / env var).
They do NOT share authority — there is no NIP-26 verifier wired into agentbox
or VisionClaw, so a forum user cannot delegate to an agentbox or VisionClaw
key today. They DO share drift: four different `verificationMethod.type`
strings, a CRITICAL bech32 npub bug on agentbox, a CRITICAL HKDF-Expand-vs-Extract
bug in NIP-44, no rotation protocol anywhere, and a documented but
not-implemented at-rest key-encryption layer in agentbox.

The single highest-impact fix is **C2 + C3 in tandem**: x-only pubkey on
agentbox and unified `SchnorrSecp256k1VerificationKey2019` everywhere. With
those two, the four substrates stop talking past each other on the wire.
After that, NIP-26 verifier wiring (H5) and the rotation protocol (H10)
turn the federation from a "shared vocabulary" into a "shared authority"
mesh.

---

## Files referenced (absolute, for follow-up)

- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/keys.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip04.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip26.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip44.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip98.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/gift_wrap.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/signer.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/did.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/webid.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/acl.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/lib.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/provision.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/forum-client/src/auth/passkey.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/forum-client/src/auth/session.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/forum-client/src/auth/nip07.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/forum-client/src/auth/nip98.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/auth-worker/src/webauthn.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/relay-worker/src/relay_do/nip_handlers.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/relay-worker/src/relay_do/session.rs`
- `/home/devuser/workspace/project/agentbox/scripts/sovereign-bootstrap.py`
- `/home/devuser/workspace/project/agentbox/management-api/server.js`
- `/home/devuser/workspace/project/agentbox/management-api/middleware/auth.js`
- `/home/devuser/workspace/project/agentbox/management-api/middleware/linked-data/surfaces/s04-did.js`
- `/home/devuser/workspace/project/agentbox/management-api/lib/uris.js`
- `/home/devuser/workspace/project/agentbox/mcp/servers/nostr-bridge.js`
- `/home/devuser/workspace/project/agentbox/config/entrypoint-unified.sh`
- `/home/devuser/workspace/project/src/services/server_identity.rs`
- `/home/devuser/workspace/project/src/services/nostr_bridge.rs`
- `/home/devuser/workspace/project/src/services/nostr_bead_publisher.rs`
- `/home/devuser/workspace/project/src/services/nostr_identity_verifier.rs`
- `/home/devuser/workspace/project/src/services/nostr_service.rs`
- `/home/devuser/workspace/project/src/services/pod_client.rs`
- `/home/devuser/workspace/project/src/handlers/server_identity_handler.rs`
- `/home/devuser/workspace/project/src/handlers/uri_resolver_handler.rs`
- `/home/devuser/workspace/project/src/handlers/solid_pod_handler.rs`
- `/home/devuser/workspace/project/src/handlers/nostr_handler.rs`
- `/home/devuser/workspace/project/src/bin/vc_cli.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-idp/src/jwks.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-activitypub/src/actor.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-activitypub/src/http_sig.rs`
- `/home/devuser/workspace/project/docs/integration-research/05-crypto-gotchas.md`
- `/home/devuser/workspace/project/docs/PRD-010-did-nostr-mesh-federation.md`
- `/home/devuser/workspace/project/docs/adr/ADR-073-private-nostr-relay-mesh-topology.md`
- `/home/devuser/workspace/project/docs/adr/ADR-074-cross-system-did-nostr-canonicalisation.md`
- `/home/devuser/workspace/project/docs/adr/ADR-075-is-envelope-message-contract.md`
