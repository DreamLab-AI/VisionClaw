# Dreamlab Forum Communication Surfaces

Working tree symlink: `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/`.
Status legend per claim: **(live)** = production CF Worker behaviour today; **(code)** = present
in tree, deploy-pending; **(planned)** = referenced in CLAUDE.md / comments only.

The forum is a Leptos 0.7 CSR client (`forum-client`) talking to five Cloudflare
Workers (`relay-worker`, `auth-worker`, `pod-worker`, `search-worker`,
`preview-worker`) over a single CF-Workers-hosted Nostr relay
(`wss://dreamlab-nostr-relay.solitary-paper-764d.workers.dev`). All
non-relay HTTP surfaces are NIP-98 gated. There is one shared protocol library
(`nostr-core`) covering NIP-01/04/19/26/44/56/59/65/90/98 plus the custom
moderation kind family.

---

## 1. Identity stack (passkey/PRF + NIP-07 + nsec; did:nostr derivation)

The forum has **three concurrent identity backends** all yielding the same
identity primitive — a 32-byte secp256k1 x-only Schnorr keypair whose hex pubkey
becomes the user's `did:nostr:<hex>` URI.

### 1.1 Passkey + WebAuthn-PRF (primary path) (live)

- Entry point: `dreamlab-ai-website/community-forum-rs/crates/forum-client/src/auth/passkey.rs:105` (`register_passkey`) and `:216` (`authenticate_passkey`).
- Flow:
  1. `POST /auth/register/options` → `passkey.rs:115` returns `prfSalt`.
  2. `navigator.credentials.create()` with PRF extension at `passkey.rs:333` (`create_credential`); software-authenticator/extension interception is rejected up front (`passkey.rs:306` `check_credentials_intercepted`) and hybrid (QR) transport is blocked (`passkey.rs:249` `check_hybrid_transport`).
  3. PRF output (32 B) → `nostr_core::derive_from_prf` (HKDF-SHA-256 with `info="nostr-secp256k1-v1"`) at `passkey.rs:150` → secret key.
  4. `POST /auth/register/verify` → `passkey.rs:171`; server returns `{ didNostr, webId, podUrl }` (`passkey.rs:79-86` `RegisterVerifyResponse`).
- Key material lives in a `StoredValue<Option<Vec<u8>>>` non-reactive holder (`auth/mod.rs:108`), zeroized in `Drop` and on `pagehide` (`auth/mod.rs:826` `register_pagehide_listener`). Drop impls in `passkey.rs:42` and `passkey.rs:56` zeroize the result structs.
- The session record persists ONLY public metadata (`auth/mod.rs:343` `StoredSession` schema; `version: 2`). Private key is not persisted; it is re-derived on each login from the same passkey + server-stored PRF salt.
- Login path enforces explicit pubkey supply (`passkey.rs:216-229`) — the pre-audit-C2 "discover by passkey" enumeration oracle has been removed (`passkey.rs:202-215`).

### 1.2 NIP-07 browser extension (Alby / nos2x) (live)

- `forum-client/src/auth/nip07.rs:50` `nip07_get_pubkey` and `:85` `nip07_sign_event`.
- Wrapped in `Nip07Signer` (`nip07.rs:130`) implementing `nostr_core::signer::Signer` (`nip07.rs:157`) — methods `sign_event`, `nip44_encrypt`, `nip44_decrypt`, `nip04_encrypt` (which falls back to NIP-44 — `nip07.rs:188-208`), `nip04_decrypt`.
- `AuthStore::login_with_nip07` at `auth/mod.rs:444` builds a Nip07Signer, stores it in `StoredValue<Option<SignerHandle>>` (no key bytes ever stored client-side).
- `sign_event_async` at `auth/mod.rs:511` dispatches by `is_nip07` — extension users go through `window.nostr.signEvent`; PRF/local-key users use the in-memory privkey.

### 1.3 Local nsec / hex (paste-key) (live)

- `auth/mod.rs:331` `register_with_generated_key` (creates fresh `nostr_core::generate_keypair` and shows the privkey to user once for backup — `mod.rs:339`).
- `auth/mod.rs:381` `login_with_local_key` accepts both `nsec1…` and 64-char hex; bech32 decode at `mod.rs:654` `decode_nsec`.

### 1.4 did:nostr derivation

- `pod-worker/src/did.rs:35` `did_nostr_uri` → `format!("did:nostr:{hex}")`.
- DID document tier-3 emitted at `did.rs:103` `render_did_document_tier3`.
- WebID derivation: `https://pods.dreamlab-ai.com/{pubkey}/profile/card#me` (`pod-worker/src/lib.rs:496`, `did.rs:55`).
- `auth-worker` register-verify also returns `didNostr` (passkey result handling in `forum-client/src/auth/passkey.rs:79-86`, applied at `auth/mod.rs:547` `apply_passkey_result`).

### 1.5 Signer trait (uniformity layer)

- `nostr-core/src/signer.rs` exposes `Signer` (re-exported `lib.rs:72`) with `PrfSigner` and `Nip07Signer` impls (`forum-client/src/auth/nip07.rs:157`).
- `forum-client/src/auth/nip98.rs:189` `create_nip98_token_with_signer` lets any backend mint NIP-98 headers — used for hardware bunkers, NIP-46, etc.

---

## 2. nostr-core protocol coverage

Module set declared at `nostr-core/src/lib.rs:12-26`:

| NIP | Module | Function |
|-----|--------|----------|
| NIP-01 | `event.rs` | `compute_event_id`, `sign_event`, `verify_event_strict`, `verify_events_batch` (re-exported at `lib.rs:33-36`) |
| NIP-04 | `nip04.rs` | AES-256-CBC DM, ECDH-x → SHA-256 (`nip04.rs:43` `nip04_shared_secret`); `nip04_encrypt`, `nip04_decrypt` re-exported `lib.rs:59` |
| NIP-19 | `nip19.rs` | `npub`/`nsec`/`note`/`naddr`/`nevent`/`nprofile` bech32 codecs (`lib.rs:60-64`) |
| NIP-26 | `nip26.rs` | Delegated event signing — `Conditions`, `DelegationTag`, `validate_delegation_tag` (`lib.rs:65`) |
| NIP-44 | `nip44.rs` | ChaCha20-Poly1305 v2 DM; `lib.rs:39` `nip44_decrypt`/`nip44_encrypt` |
| NIP-56 | `moderation_events.rs` | Standard report kind 1984 (constant `KIND_REPORT_NIP56` at `moderation_events.rs:53`) |
| NIP-59 | `gift_wrap.rs` | Triple-layer Rumor (14) → Seal (13) → Wrap (1059); `gift_wrap`, `unwrap_gift` (`lib.rs:37`); jitter ±48h `gift_wrap.rs:23` |
| NIP-65 | (none — see §11) | Outbox model NOT implemented in nostr-core; relay supports kind-10002 storage only (`relay-worker/src/relay_do/broadcast.rs:295` test reference). The forum hard-codes one relay URL. |
| NIP-90 | `nip90.rs` | DVM job request/result/feedback types: `KIND_JOB_REQUEST_MIN=5000`/`MAX=5999`, `KIND_JOB_RESULT_MIN=6000`/`MAX=6999`, `KIND_JOB_FEEDBACK=7000`, `KIND_HANDLER_INFO=31990` (`nip90.rs:21-31`) |
| NIP-98 | `nip98.rs` | HTTP auth: `create_token`, `verify_token_at_with_replay`, replay store interface (`lib.rs:40-47`) |
| Custom (mod) | `moderation_events.rs` | Kinds 30910–30916 builder + validator (see §6) |
| Calendar | `calendar.rs` | Kind-31922/31923 calendar events + RSVPs (`lib.rs:50`) |
| Groups | `groups.rs` | NIP-29 helpers (no public re-export in lib.rs but module is published) |
| Deletion | `deletion.rs` | NIP-09 helpers |

Test coverage:
- File-level integration tests in `nostr-core/tests/`: `nip04_proptests.rs`, `nip19_tests.rs`, `nip19_proptests.rs`, `nip26_tests.rs`. ~210 inline `#[test]` items across `src/` (counted via grep).
- Benches: `bench_nip44.rs`, `bench_events.rs`, `bench_keys.rs`.
- README claim "129 tests for nostr-core" (`dreamlab-ai-website/CLAUDE.md:46`).

---

## 3. forum-client relay client (relay.rs, relay_url.rs, AUTH flow, reconnect)

### 3.1 Connection management

- Single-relay WebSocket manager: `forum-client/src/relay.rs:106` `RelayConnection`.
- Default URL hardcoded at `relay.rs:23`:
  `wss://dreamlab-nostr-relay.solitary-paper-764d.workers.dev`.
- Resolution priority (`relay.rs:528` `get_relay_url` and `utils/relay_url.rs:8`):
  1. `window.__ENV__.VITE_RELAY_URL` (runtime injection by `index.html`).
  2. `option_env!("VITE_RELAY_URL")` (compile-time Trunk env).
  3. Hard-coded default above.
- Single thread invariant: `SendWrapper<Rc<RefCell<RelayInner>>>` (`relay.rs:101`) with manual `unsafe impl Send/Sync` gated on `target_arch="wasm32"` (`relay.rs:114-117`).
- Connection state: `Disconnected | Connecting | Connected | Reconnecting | Error` exposed as `RwSignal<ConnectionState>` (`relay.rs:32-39`, `:143`).

### 3.2 NIP-01 wire protocol — outbound

`relay.rs:284` `subscribe`: builds `["REQ", sub_id, filter…]`; supports filter
fields `ids/authors/kinds/#e/#p/since/until/limit` (`relay.rs:42-60` `Filter`
struct). Subscriptions are stored in `RelayInner.subscriptions` and **replayed
on reconnect** (`relay.rs:206-219` inside `onopen`).

`relay.rs:320` `unsubscribe` sends `["CLOSE", sub_id]`.

`relay.rs:334` `publish` sends `["EVENT", event]`. `:344` `publish_with_ack`
registers an `OK` callback in `pending_publishes` keyed by event ID — fired
when relay returns `["OK", event_id, accepted, message]` (`relay.rs:496-517`).

`send_raw` (`relay.rs:363`) buffers messages while WS is not OPEN and flushes
in `onopen` (`relay.rs:200-204`).

### 3.3 Inbound message routing

`handle_relay_message` (`relay.rs:416`) dispatches:
- `EVENT` → subscription callback (`relay.rs:440`).
- `EOSE` → on_eose callback (`relay.rs:469`).
- `NOTICE` → console.warn (`relay.rs:489`); **no AUTH challenge handling**.
- `OK` → pending publish callback (`relay.rs:496`).
- Anything else → console.log "Unhandled" (`relay.rs:519`).

**Critical gap:** the client does NOT handle `["AUTH", challenge]` from the
relay (no `kind 22242` AUTH-RESP code path) and does NOT recognise
`auth-required:` NOTICE prefixes. The relay sends a NIP-42 challenge on
connect (see §4.2) — currently the client ignores it. AUTH today is therefore
exclusively a **server-side gate against kind-1059 SUBs** (§4.3); since the
DM module doesn't ever AUTH, the client never receives kind-1059 events from
the relay (this is one of the most consequential gaps — see §10).

### 3.4 Reconnect logic

`schedule_reconnect` (`relay.rs:380`): exponential backoff
`min(1000 * 2^attempts, 30000)` ms (`relay.rs:26-29`). Uses
`set_timeout_once` so closures drop after firing (memory-safe on spotty mobile).
Subscription state persists through `disconnect()` for replay (`relay.rs:264-280`
explicit comment "subscriptions are preserved").

### 3.5 URL resolution helpers

- `utils/relay_url.rs:8` `relay_url()` — WebSocket URL.
- `utils/relay_url.rs:20` `relay_api_base()` — converts wss→https for the
  same host (whitelist/setup-status/profile endpoints live on the relay
  worker too, see §4).
- `utils/relay_url.rs:36` `auth_api_base()` — defaults to
  `https://api.dreamlab-ai.com` (note: distinct from the relay host).
- `ws_to_http` (`utils/relay_url.rs:78`) — strips trailing slash, switches
  scheme. Tested at `:96-156`.

---

## 4. CF Workers — public surface, NIP-98 endpoints, KV/D1/DO bindings

### 4.1 relay-worker (`dreamlab-nostr-relay`)

Deployed at `wss://dreamlab-nostr-relay.solitary-paper-764d.workers.dev` (live).
`wrangler.toml` config at `crates/relay-worker/wrangler.toml`:
- D1 binding `DB` → `dreamlab-relay` (id `97c77d23-…`).
- KV binding `NIP98_REPLAY` (id placeholder — production must provision).
- Durable Object `RELAY` → class `NostrRelayDO` (`wrangler.toml:26-29`).
- Cron `*/5 * * * *` (D1 keep-warm).

HTTP routes (`relay-worker/src/lib.rs:227` `route`):

| Path | Method | Auth | Handler |
|------|--------|------|---------|
| `/` | GET | public | NIP-11 doc if Accept: `application/nostr+json` (`lib.rs:181`); otherwise health JSON (`lib.rs:231`) |
| `/health` | GET | public | health JSON |
| `/api/setup-status` | GET | public | `whitelist::handle_setup_status` (lib.rs:247) |
| `/api/check-whitelist?pubkey=` | GET | public | `whitelist.rs:82` |
| `/api/whitelist/list` | GET | public | `whitelist::handle_whitelist_list` |
| `/api/whitelist/add` | POST | NIP-98 admin | `whitelist::handle_whitelist_add` |
| `/api/whitelist/update-cohorts` | POST | NIP-98 admin | `:267` |
| `/api/whitelist/set-admin` | POST | NIP-98 admin | `:272` |
| `/api/admin/reset-db` | POST | NIP-98 admin | `:277` |
| `/api/reports` | GET | NIP-98 admin | `moderation::handle_list_reports` (`lib.rs:283`) |
| `/api/reports/resolve` | POST | NIP-98 admin | `:289` |
| `/api/admin/audit-log` | GET | NIP-98 admin | `:295` |
| `/api/profiles/batch` | POST | public | `:300` (Sprint v10 kind-0 projection) |
| `/api/profiles/search` | GET | public | `:306` |
| `/api/admin/profiles/backfill` | POST | NIP-98 admin | `:315` (replays kind-0 → projection) |
| upgrade WebSocket | — | — | DO `RELAY` get_by_name("main") fetched (`lib.rs:172-175`) |

D1 schema (idempotent CREATE in `lib.rs:378` `ensure_schema`):
- `whitelist` (cohorts, is_admin, trust_level, days_active, posts_*, suspended_until, silenced).
- `events` (id, pubkey, created_at, kind, tags JSON, content, sig, d_tag).
- `channel_zones`, `admin_log`, `settings`, `reports`, `hidden_events`,
  `moderation_actions`, `profiles`.
- Indexes incl. `idx_events_kind` (`lib.rs:494`) for kind-1059 efficiency.

Auth helper: `relay-worker/src/auth.rs:53` `verify_nip98_replay` (KV-backed
replay store, fail-open if `NIP98_REPLAY` binding missing — `auth.rs:77-89`,
warning logged). `:139` `require_nip98_admin` combines verify + `is_admin`
D1 lookup.

### 4.2 Durable Object — `NostrRelayDO`

`crates/relay-worker/src/relay_do/mod.rs:54`. Single-isolate, `RefCell`-wrapped
state. Capacity: `MAX_CONNECTIONS_PER_IP = 20` (`mod.rs:47`),
`MAX_SUBSCRIPTIONS = 20` per session (`nip_handlers.rs:32`).

Lifecycle:
- `fetch` (`mod.rs:78`): rejects non-WS upgrades with HTTP 426; per-IP
  connection cap; allocates `session_id`; tags WebSocket `["sid:N", "ip:X"]`
  for hibernation recovery (`mod.rs:108-112`); generates NIP-42 challenge
  (`mod.rs:115` via `session::generate_challenge`); **immediately sends
  `["AUTH", challenge]`** to client (`mod.rs:140`). _The client currently
  ignores this._
- `websocket_message` (`mod.rs:147`): JSON decode + dispatch by leading
  `EVENT|REQ|CLOSE|AUTH|COUNT`.
- `websocket_close` / `websocket_error`: drop session.
- `alarm` (`mod.rs:276`): if no sessions, clears in-memory state to allow
  DO eviction (60 s idle timeout — `session.rs:37`).
- Hibernation: subscriptions persisted via `session::save_subscriptions`
  (`session.rs:195`); auth state via `save_auth` (`session.rs:216`); replayed
  on next message via `recover_session` (`session.rs:67`).

### 4.3 NIP-42 AUTH gate

- AUTH challenge auto-sent on connect (`mod.rs:140`).
- `handle_auth` at `nip_handlers.rs:432`: requires kind 22242, valid signature,
  `challenge` tag matching session challenge, ≤600 s old.
- The ONE current gating point: kind-1059 subscriptions
  (`nip_handlers.rs:348-387`). If any filter requests kind 1059 the relay
  rewrites all such filters to inject a forced `#p = <authed_pubkey>`
  constraint, or sends `NOTICE: auth-required: must authenticate to receive
  kind-1059 DMs`. **No other kind requires AUTH at the read path.**
- `broadcast_event` (`broadcast.rs:41-67`): for kind-1059 it iterates only
  sessions whose `authed_pubkey` matches the event's `p` tag, so the relay
  cannot leak DMs to unauthenticated subscribers even if a filter slipped
  through.

### 4.4 Event ingress gate (relay-worker)

`handle_event` at `relay_do/nip_handlers.rs:42`:
1. Per-IP rate limit (`broadcast.rs:75`: `MAX_EVENTS_PER_SECOND = 10`).
2. Structural validation (`validate_event` at `nip_handlers.rs:261`): sizes,
   tag count, ±7-day timestamp drift.
3. NIP-40 expiration tag check (`nip_handlers.rs:55-63`).
4. Whitelist gate (`:65-74`): bypass kinds 0, 9021, 9024 for first-time users;
   any other kind from a non-whitelisted pubkey is rejected with `OK false
   "blocked: pubkey not whitelisted"`.
5. Auto-whitelist on first bypass-kind publish (`:76-81`).
6. Suspension/silence check (`:84-97`).
7. Mute/ban `mod_cache` check for kind 1 and 42 from non-admins
   (`:99-109`) — admins bypass.
8. Trust-level gating (TL0–TL3, `nip_handlers.rs:112-178`):
   - kind 40 (channel create): TL2+ Regular.
   - kind 41 (channel meta): TL2+ for own, TL3+ for others' channels.
   - kind 1984 (NIP-56 report): TL1+ Member.
   - kind 5 (deletion) targeting other authors: TL3+ Trusted.
9. NIP-29 admin-only kinds (`NIP29_ADMIN_KINDS = [9000, 9001, 9005, 39000]` —
   `nip_handlers.rs:35`).
10. Zone enforcement for kind 42 (`:187-197`).
11. Schnorr signature verification via `nostr_core::verify_event_strict` (`:200`).
12. NIP-16 treatment classification (`broadcast.rs:24` `event_treatment`):
    - 20000–29999 → Ephemeral (broadcast, never stored).
    - 10000–19999, 0, 3 → Replaceable.
    - 30000–39999 → ParameterizedReplaceable.
    - All else → Regular.
13. D1 save (`save_event` at `storage.rs:60`) + broadcast to subscribers.
14. Side effects: increment activity counters; trigger trust promotion;
    NIP-09 deletion processing; NIP-56 report → `reports` table; admin-signed
    30910/30911 mirrored into `moderation_actions` (`:249-255`,
    `mirror_moderation_action` at `:710`).

**Kind whitelist in practice:** the relay is **NOT a forum-only relay** in the
"reject any kind not on a list" sense. It accepts arbitrary kinds, modulated
only by validation, whitelist membership, trust level, and zone access. A
whitelisted member with TL≥1 can publish kind 1, 7, 1059, 10002, custom
30000–39999 etc. without further restriction (modulo size/timestamp limits).
NIP-11 advertises `restricted_writes: true` (`nip11.rs:41`) — meaning *who*
can write is gated, not *what*.

### 4.5 auth-worker (`dreamlab-auth-api`)

`wrangler.toml` (`crates/auth-worker/wrangler.toml`):
- D1 `DB` → `dreamlab-auth` (id `e3981999-…`).
- KV: `SESSIONS`, `POD_META` (legacy), `ADMIN_KV`, `NIP98_REPLAY`, alias `KV` →
  SESSIONS.
- R2 `PODS` → `dreamlab-pods`.
- Vars: `RP_ID = "dreamlab-ai.com"`, `EXPECTED_ORIGIN = "https://dreamlab-ai.com"`.

Public hostname (forum default): `https://api.dreamlab-ai.com`
(`forum-client/src/utils/relay_url.rs:57`).

Routes (`auth-worker/src/lib.rs:177` `route` and `:303` `route_sprint_api`):

WebAuthn (no NIP-98):
- `POST /auth/register/options` (`lib.rs:198`)
- `POST /auth/register/verify` (`lib.rs:203`) — also takes `CF-IPCountry` for
  geo gating.
- `POST /auth/login/options` (`lib.rs:209`)
- `POST /auth/login/verify` (`lib.rs:214`) — NIP-98 inside the body via
  `nip98.rs::fetch_with_nip98_post` (`forum-client/src/auth/passkey.rs:285`).
- `POST /auth/lookup` (`lib.rs:219`).

NIP-98 gated `/api/*`:
- `GET /api/profile` — current user profile (`lib.rs:281`).
- Moderation (admin): `POST /api/mod/{ban,mute,warn}`, `POST /api/mod/report`
  (any authed), `GET /api/mod/{actions,reports}`, `POST
  /api/mod/reports/:id/action` (`auth-worker/src/moderation.rs`, dispatch
  `lib.rs:322-349`).
- Web-of-Trust: `/api/wot/{status,set-referente,refresh,override/add,override/remove}`
  (`lib.rs:352-369`).
- Invites: `/api/invites/{create,mine,:id/revoke,:code/redeem,:code (preview)}`
  (`lib.rs:372-405`).
- Welcome bot: `/api/welcome/{config,configure,set-bot-key,test}` (`lib.rs:408-423`).
- Admin management: `GET /api/admins`, `POST /api/admins/{add,remove}` (`lib.rs:426-437`).
- NIP-1984 admin queue: `GET /api/moderation/reports` (`lib.rs:440`).
- NIP-26 delegation verify: `POST /api/delegation/verify` (`lib.rs:446`).
- Username reservations: `GET /api/username/check`, `POST /api/username/{claim,release}` (`lib.rs:451-462`).

Replay store: KV `NIP98_REPLAY`, TTL = 2 × `nostr_core::REPLAY_CACHE_TTL_SECS`.
Rate limit: 20 req / 60 s per IP at `lib.rs:124-131`.

### 4.6 pod-worker (`dreamlab-pod-api`)

`wrangler.toml` (`crates/pod-worker/wrangler.toml`):
- R2 `PODS` (same bucket as auth-worker).
- KV `POD_META`, `ADMIN_KV_RO` (read-only mirror per audit H6),
  `NIP98_REPLAY`.
- `EXPECTED_ORIGIN = "https://dreamlab-ai.com"`.

Public hostname (forum default): `https://dreamlab-pod-api.solitary-paper-764d.workers.dev`
(`forum-client/src/utils/pod_client.rs:13-14`); also reachable via
`https://pods.dreamlab-ai.com` per `did.rs:55`.

Routes (`pod-worker/src/lib.rs:234` `fetch`):

| Path | Auth | Purpose |
|------|------|---------|
| `/health` | public | service+features list (`lib.rs:246-272`) |
| `/.well-known/webfinger?resource=acct:…` | public | remoteStorage / Solid / ActivityPub discovery (`lib.rs:279`) |
| `/.well-known/solid` | public | Solid discovery JSON (`lib.rs:302`) |
| `/.well-known/nostr.json?name=…` | public, rate-limited 60/min | NIP-05 verification (`lib.rs:313`) |
| `/.well-known/did/nostr/{hex}.json` | public | did:nostr Tier-3 doc (`lib.rs:362-380`); MIME `application/did+ld+json` |
| `/pods/{pubkey}/.provision` | NIP-98 (owner or admin) | Solid pod bootstrap (`lib.rs:452-507`) |
| `/pods/{pubkey}/...` | NIP-98 + WAC (`evaluate_access`) | LDP CRUD + ACL (`lib.rs:560-1009`) |
| `*.acl` | NIP-98 + acl:Control on parent | ACL CRUD (`lib.rs:1019-1186`) |

NIP-98 verification at `pod-worker/src/lib.rs:421-440` (note webid-tag identity
check at `:429-433` — rejects tokens whose `["webid", uri]` references a
different identity than the signing pubkey).

### 4.7 search-worker (`dreamlab-search-api`)

`wrangler.toml`:
- R2 `VECTORS` → `dreamlab-vectors`.
- KV `SEARCH_CONFIG`, `NIP98_REPLAY`.
- `RVF_STORE_KEY = "dreamlab.rvf"`, `ADMIN_PUBKEYS` env contains a single hex.

Public hostname: `https://search.dreamlab-ai.com`
(`forum-client/src/utils/search_client.rs:9-12`).

Routes (`search-worker/src/lib.rs:485` `route`):
- `GET /` `GET /health` `GET /status` → `handle_status` (`lib.rs:413`).
- `POST /search` (public): cosine k-NN search over 384-dim embeddings
  (`lib.rs:238`).
- `POST /embed` (public): hash-based fallback embeddings — explicitly *not*
  semantic (`lib.rs:336` "Replace with ONNX WASM model"). Comment in lib.rs
  says model is "all-MiniLM-L6-v2" (`:424`) but runtime is hash-fallback only.
- `POST /ingest` (NIP-98 admin): vector batch insert (`:343`); persists RVF
  bytes to R2 + id↔label mapping to KV.

Rate limit: 100 req / 60 s per IP at `lib.rs:447-454`.

### 4.8 preview-worker (`dreamlab-link-preview`)

`wrangler.toml`:
- KV `RATE_LIMIT`.
- `ALLOWED_ORIGIN = "https://dreamlab-ai.com"`.

Public hostname (forum default): `https://dreamlab-link-preview.solitary-paper-764d.workers.dev`
(`forum-client/src/components/link_preview.rs:10`).

Routes (`preview-worker/src/lib.rs:341`):
- `GET /preview?url=…` (public): OG metadata or Twitter oEmbed; SSRF blocked
  (`ssrf.rs::is_private_url`, `lib.rs:216`); CF Cache API caching
  (`lib.rs:147-180`); 10-day TTL for OG, 1-day for Twitter.
- `GET /health`, `GET /stats` (public).
- Rate limit: 30 req / 60 s per IP (`lib.rs:328`).
- No auth, no D1, no DO. Pure proxy.

---

## 5. DM / gift-wrap stack (NIP-17/44/59)

### 5.1 Outbound (live, NIP-17/44/59)

- `forum-client/src/dm/mod.rs:294` `send_message` is the only DM publish path.
- It calls `nostr_core::gift_wrap::gift_wrap` (`dm/mod.rs:318`) which builds:
  1. **Rumor** — unsigned kind 14 with plaintext (`gift_wrap.rs:26`).
  2. **Seal** — kind 13, sender-signed, NIP-44-encrypted rumor
     (`gift_wrap.rs:29`).
  3. **Wrap** — kind 1059, signed by a fresh ephemeral keypair, NIP-44-encrypted
     seal (`gift_wrap.rs:32-33`).
- Timestamp jitter ±48h on outer wrap (`gift_wrap.rs:23,108`).
- Outer p-tag = recipient (for relay routing); inner sender_pubkey hidden in
  seal.
- NIP-44 v2 ChaCha20-Poly1305 (`nostr-core/src/nip44.rs`).
- Optimistic UI: outer wrap event ID is used for dedup (`dm/mod.rs:337`); the
  rumor's plaintext + `created_at` are surfaced in the message list.
- Auto-publishes **kind-10050** "preferred DM relay" once per pubkey on first
  send (`dm/mod.rs:573` `ensure_dm_relay_published`); localStorage flag
  `nostr_bbs_dm_relay_published_<pk[..8]>` prevents republish. The 10050
  event has a single `r` tag = current relay URL.

### 5.2 Inbound (live, kind 1059 + legacy kind 4)

- `fetch_conversations` (`dm/mod.rs:131`) subscribes to BOTH kinds 4 and 1059
  with two filters (sent: `authors=[me]`, recv: `#p=[me]`).
- `subscribe_incoming` (`:181`) — same kinds, `since=now`.
- `process_dm_event` (`:373`) routes by kind:
  - 1059 → `process_gift_wrap_event` (`:392`): `unwrap_gift` peels three layers,
    returns `UnwrappedGift { sender_pubkey, rumor, seal }` (`gift_wrap.rs:78`).
  - 4 → `process_kind4_event` (`:455`): NIP-44 symmetric decrypt with
    counterparty's pubkey via `nip44_decrypt`.
  - Other kinds dropped silently (`:384`).
- Note the inbound code path uses `nip44_decrypt` for kind 4 — not
  `nip04_decrypt`. NIP-04 wire format (`<ct_b64>?iv=<iv_b64>` AES-256-CBC) is
  **available in `nostr-core` (`nip04.rs:43`) but unused on the kind-4 read
  path.** Consequently legacy NIP-04 messages from external clients won't
  decrypt; only kind-4 ciphertexts created with NIP-44 v2 frame work. A page
  caption at `pages/note_view.rs:59` labels kind 4 as "Encrypted DM (NIP-04)"
  but the decoder is NIP-44 — code/UX drift.
- Privkey access path: `dm/mod.rs:294` requires `privkey_bytes: &[u8;32]` —
  NIP-07 users **cannot send DMs** through this code today because
  `AuthStore::get_privkey_bytes` returns `None` for extension sessions
  (`auth/mod.rs:206`). The Nip07Signer's `nip44_*` capability
  (`auth/nip07.rs:168-186`) is wired but the DM module does not call it.

### 5.3 AUTH dependency for inbound delivery

The relay enforces NIP-42 AUTH for kind-1059 SUB (§4.3,
`relay_do/nip_handlers.rs:354-387`). Forum-client's relay manager does not
implement the AUTH-RESP code path (§3.3). Net effect: even after
`subscribe_incoming` runs, the relay rejects the kind-1059 portion with
`NOTICE: auth-required: …`, and the client console-warns the NOTICE without
any reactive state update (`relay.rs:489-494`). Users will see kind-4 messages
but not kind-1059 unless the relay's gate is bypassed (e.g. WebSocket reconnect
race) or AUTH is implemented client-side. **This is the highest-impact gap.**

---

## 6. Moderation event kinds (30910–30916, 1984)

### 6.1 Definitions (`nostr-core/src/moderation_events.rs`)

| Kind | Const | d-tag | Signer | Replaceable |
|------|-------|-------|--------|-------------|
| 30910 | `KIND_BAN` | banned pubkey hex | admin | param-replaceable (lasts indefinitely) |
| 30911 | `KIND_MUTE` | muted pubkey hex | admin (`expires` tag = unix s) | param-replaceable |
| 30912 | `KIND_WARNING` | `<pubkey>:<created_at>` | admin | each unique (audit trail) |
| 30913 | `KIND_REPORT` | reported event id | any authed | param-replaceable |
| 30914 | `KIND_MODERATION_ACTION` | action UUID | admin | param-replaceable (audit log) |
| 30915 | `KIND_UNBAN` | `<admin>:<target>` | admin (revokes 30910) | param-replaceable |
| 30916 | `KIND_UNMUTE` | `<admin>:<target>` | admin (revokes 30911) | param-replaceable |
| 1984 | `KIND_REPORT_NIP56` | (n/a — Regular) | any authed | NIP-56 standard report |

- Builders: `build_ban`, `build_mute`, `build_warning`, `build_report`,
  `build_unban`, `build_unmute`, `build_moderation_action` (`moderation_events.rs:269-381`).
- Validator: `validate_moderation_event` (re-exported `nostr-core/src/lib.rs:53-57`).
- `ADMIN_ONLY_MOD_KINDS = [30910,30911,30912,30914,30915,30916]` (`moderation_events.rs:67`).

### 6.2 Emit paths

- **`admin-cli/forum-admin`** (`crates/admin-cli/src/commands/mod_ops.rs` —
  not fully read but referenced at `auth-worker/src/moderation.rs:18`):
  CLI builds + signs the event client-side using `nostr-core::build_*`,
  then POSTs to auth-worker `/api/mod/{ban|mute|warn}` with the structured
  body (`moderation.rs:47-59` `ActionBody`).
- **`forum-client` report button** (`components/report_button.rs:90-94`):
  emits standard NIP-56 `kind 1984` events directly via the relay (no
  custom-kind 30913 emit on this path).

### 6.3 Consume paths

- **auth-worker** (`auth-worker/src/moderation.rs:200-203`): validates the
  inbound signed event matches `expected_kind` for each route, derives
  `expires_at` from event for mutes (`:252`), inserts into D1
  `moderation_actions` / `mod_reports` tables.
- **relay-worker** (`relay-worker/src/relay_do/nip_handlers.rs:249-255`):
  any kind 30910/30911 saved through normal ingest is **mirrored** to
  D1 `moderation_actions` via `mirror_moderation_action`
  (`nip_handlers.rs:710`) — only when the signer is an admin
  (`auth::is_admin` at `:104`). On mirror, the `mod_cache` entry for the
  target pubkey is invalidated (`nip_handlers.rs:253`).
- **relay-worker mod_cache** (`relay_do/mod_cache.rs`, struct in
  `mod.rs:62`): 60s-TTL in-memory cache keyed by target pubkey; consulted
  for kind 1 + kind 42 ingress (`nip_handlers.rs:103-109`); admin signers
  bypass.
- **NIP-56 (kind 1984)**: ingested as a Regular event into the D1 `events`
  table (NIP-16 classification — `broadcast.rs:24`); after-save hook
  (`nip_handlers.rs:241`) calls `process_report` (`:655`) which extracts
  `e`/`p`/`report` tags and writes to D1 `reports`. auth-worker
  `GET /api/moderation/reports` (`auth-worker/src/moderation.rs:632`)
  exposes the queue to admins.

---

## 7. NIP-90 DVM marketplace surface

- **Protocol library complete** (code, not yet wired to relay):
  `nostr-core/src/nip90.rs` defines `DvmJobRequest`, `DvmJobResult`,
  `DvmJobFeedback`, `DvmCapabilityAd` (`:142`/`:259`/`:344`/`:408`); helpers
  `is_job_request`, `is_job_result`, `parse_job_inputs`. Re-exports at
  `lib.rs:66-71`. NIP-11 advertises 90 (`nip11.rs:28`).
- **Forum UI placeholder only** (code, planned wiring): `pages/marketplace.rs`
  is a Wave-3 skeleton with a **hard-coded `Vec<DvmListing>`** at
  `:75-93`. Comment at `:75`: "Placeholder DVM listings until relay
  subscription is wired up." No live kind-31990 SUB, no submit button is
  functional.
- **Server-side**: relay accepts kind 5000–5999 (job request), 6000–6999
  (job result), 7000 (feedback), 31990 (handler info) as ordinary events
  modulo whitelist+TL gates. No DVM-specific routing or payment hooks.
  No auth-worker endpoints for DVM.
- **Net status**: protocol primitives **live**, marketplace UI **planned**.
  A whitelisted member could publish a 31990 capability ad today and any
  client that subscribes to kind 31990 could see it.

---

## 8. pod-worker did:nostr DID document endpoint

- Path: `GET /.well-known/did/nostr/{pubkey}.json` — handler at
  `pod-worker/src/lib.rs:362-380`.
- Validation: requires exactly 64 lowercase hex chars (`:365`).
- MIME: `application/did+ld+json` (`:374`).
- Tier: **Tier-3** (`pod-worker/src/lib.rs:205` `build_did_nostr_document`
  → `did::render_did_document_tier3`).
- Service entries advertised (`did.rs:115-135`):
  1. `id: did:nostr:<pk>#solid-pod`, `type: "SolidStorage"`,
     `serviceEndpoint: <pod_base>/pods/<pk>/`.
  2. If WebID known: `#webid` `SolidWebID` →
     `<pod_base>/pods/<pk>/profile/card#me`.
  3. If `relay_url` known: `#nostr-relay` `NostrRelay` (NOT advertised by the
     `build_did_nostr_document` path because `relay_url=None` is hard-coded
     at `pod-worker/src/lib.rs:213` "relay URL: not included at Tier 3
     without lookup").
- Verification method: `SchnorrSecp256k1VerificationKey2019` with both
  `publicKeyHex` and `publicKeyMultibase` (multicodec 0xe7 secp256k1-pub —
  `did.rs:168` `format_multibase_schnorr`).
- `alsoKnownAs`: WebID URI when present (`did.rs:113`).

So the DID doc as deployed advertises pod (Solid storage) + WebID for any
known pubkey, but **does NOT advertise the user's preferred Nostr relay**.
Any system that resolves a `did:nostr:<hex>` could discover the pod URL and
WebID, then federate over Solid HTTP. The discoverability surface is
therefore intentionally bounded to the pod and Solid identity; for relay
discovery a consumer would need to fetch kind-10002 (NIP-65) or 10050 from
the relay directly.

---

## 9. Public surface URLs (production hostnames)

| Worker | Default hostname (forum hard-coded fallback) | Wrangler `name` | Notes |
|--------|---------------------------------------------|-----------------|-------|
| relay-worker | `wss://dreamlab-nostr-relay.solitary-paper-764d.workers.dev` (`relay.rs:23`) | `dreamlab-nostr-relay` | Single CF Workers subdomain — no custom DNS. Same host serves WS + HTTP API. |
| auth-worker | `https://api.dreamlab-ai.com` (`utils/relay_url.rs:57`) | `dreamlab-auth-api` | Custom subdomain; allowed origins also include `https://thedreamlab.uk`, `https://dreamlab-ai.github.io` (relay+search) |
| pod-worker | `https://dreamlab-pod-api.solitary-paper-764d.workers.dev` (`pod_client.rs:13`) — also `https://pods.dreamlab-ai.com` (`did.rs:55`) | `dreamlab-pod-api` | Two hostnames in code. The `pods.dreamlab-ai.com` form is the canonical WebID base. |
| search-worker | `https://search.dreamlab-ai.com` (`search_client.rs:11`) | `dreamlab-search-api` | Custom subdomain. `RVF_STORE_KEY = dreamlab.rvf`. |
| preview-worker | `https://dreamlab-link-preview.solitary-paper-764d.workers.dev` (`link_preview.rs:10`) | `dreamlab-link-preview` | CF Workers subdomain. |

CORS allowed origins:
- relay-worker / search-worker: `https://dreamlab-ai.com,https://thedreamlab.uk,https://dreamlab-ai.github.io` (`wrangler.toml:12`).
- auth-worker / pod-worker / preview-worker: `https://dreamlab-ai.com` only
  (`wrangler.toml` `EXPECTED_ORIGIN`/`ALLOWED_ORIGIN`).

Runtime overrides honoured by forum-client (all read at startup from
`window.__ENV__`):
- `VITE_RELAY_URL` (`relay.rs:532`, `utils/relay_url.rs:9`).
- `RELAY_API_URL` (`utils/relay_url.rs:22`).
- `AUTH_API_URL`, `VITE_AUTH_API_URL` (`utils/relay_url.rs:38-43`).
- `__AUTH_API_URL__` (legacy, `utils/relay_url.rs:46-53`).

---

## 10. Gaps and dead code

1. **No client-side NIP-42 AUTH** (live). Relay sends `["AUTH", challenge]`
   immediately on connect (`relay_do/mod.rs:140`), but
   `forum-client/src/relay.rs:439-524 handle_relay_message` has no `AUTH`
   case. The `NOTICE: auth-required: …` warning produced when subscribing to
   kind 1059 (`relay_do/nip_handlers.rs:362`) is logged but never triggers a
   challenge response.
2. **NIP-07 users cannot send or receive DMs** (live). `dm/mod.rs:294`
   demands raw 32-byte privkey; `auth/mod.rs:206 get_privkey_bytes` returns
   `None` for extension sessions even though `Nip07Signer::nip44_*` is wired
   (`forum-client/src/auth/nip07.rs:168-208`). The DM module never resolves
   through the Signer trait.
3. **Kind-4 decoder uses NIP-44, not NIP-04** (live, code drift).
   `dm/mod.rs:482` calls `nip44_decrypt` for kind 4. Genuine NIP-04
   ciphertext (`<ct>?iv=<iv>`) from external clients will not decrypt. The
   `nostr_core::nip04` module exists and is fully implemented but is not
   called by the forum-client.
4. **Outbox model (NIP-65) absent** (planned). NIP-11 advertises 65
   (`nip11.rs:28`); the relay accepts kind-10002 (it's in the replaceable
   range — `broadcast.rs:295`); but the forum-client never reads kind-10002
   to discover other users' relays, never publishes one for itself, and is
   structurally single-relay. `kind-10050` is published once but only points
   to the same hard-coded relay.
5. **DVM marketplace UI is a stub** (planned). `pages/marketplace.rs:75-93`
   is a hard-coded `Vec<DvmListing>`; no relay subscription, no submit flow.
6. **Search embeddings are hash-based, not semantic** (live). Comment in
   `search-worker/src/lib.rs:336`: "Replace with ONNX WASM model for
   semantic quality." `/status` advertises model `all-MiniLM-L6-v2`
   (`:424`) but the server cannot run that model — false advertising.
7. **NIP-46 bunker signing is stubbed** (planned). `admin-cli/src/auth.rs:33`
   `BunkerUnsupported` error; `AdminSigner::Bunker` enum variant exists at
   `:152` but `sign()` returns `BunkerUnsupported` (`:164`).
8. **`POD_META`/`ADMIN_KV` placeholder IDs in wrangler.toml** (code).
   Several KV namespaces have `id = "REPLACE_WITH_NEW_*_KV_ID"`
   (`relay-worker/wrangler.toml:24`, `auth-worker/wrangler.toml:30`,
   `pod-worker/wrangler.toml:25`, `search-worker/wrangler.toml:20`,
   `auth-worker/wrangler.toml:37`). Production deploy MUST provision; until
   then NIP-98 replay protection fails open with a warning
   (`relay-worker/src/auth.rs:77-88`, same in auth-worker).
9. **DID Tier-3 doc omits `NostrRelay` service** (live). `pod-worker/src/lib.rs:213`
   passes `relay_url=None` "not included at Tier 3 without lookup" — the
   plumbing exists in `did::render_did_document_tier3` (`did.rs:129-135`) but
   the lookup is not implemented.
10. **AUTH-required Cloudflare CORS rule** (live).
    `auth-worker/src/lib.rs:34` allows only `EXPECTED_ORIGIN`
    (`https://dreamlab-ai.com`) — ALLOWED_ORIGINS at relay-worker is broader
    (`https://dreamlab-ai.com,https://thedreamlab.uk,https://dreamlab-ai.github.io`,
    `wrangler.toml:12`) but auth-worker is single-origin only. Requests from
    `dreamlab-ai.github.io` would fail at auth-worker boundary.
11. **No deletion/edit notification path on pod resources** (code).
    `pod-worker/src/lib.rs:770,824,878,964,993` fire `notifications::notify_change`
    after writes but the notifications subscriber path is not wired in
    forum-client (no consumer reads back the webhooks).
12. **NIP-26 verify endpoint has no production caller** (code). The
    `auth-worker/src/delegation.rs:58 handle_verify` endpoint is implemented
    and tested; the forum-client does not invoke `/api/delegation/verify`
    (no occurrences of "delegation" in `forum-client/src/`). The CLI and
    relay also don't enforce delegation tags. Pure code, awaiting use.

---

## 11. Observed coupling to a "private relay" deployment vs a public CF relay

The relay-worker is *administratively* private but *protocol-wise* a permissive
Nostr relay. Nuance:

- **Protocol**: NIP-01/09/11/16/26/29/33/40/42/45/50/59/65/90/98 advertised
  (`nip11.rs:28`); accepts arbitrary `kind` values modulo
  whitelist+trust+zone gating; serves the standard wire (`["EVENT",…]`,
  `["REQ",…]`, `["EOSE",…]`, `["OK",…]`, `["NOTICE",…]`, `["AUTH",…]`,
  `["COUNT",…]`). It is **not** a forum-only relay in the sense of "only
  accepts kinds [1, 7, 40, 42, 1059]".
- **Ingress gate**: every event from a non-whitelisted pubkey is rejected
  except kinds 0/9021/9024 (auto-whitelist self-onboarding —
  `nip_handlers.rs:70-81`). A non-forum agent dropping in over plain
  WebSocket therefore CANNOT publish until they:
  1. Send a kind-0 metadata event (auto-whitelisted into the `lobby` cohort).
  2. Or be added by an existing admin via `/api/whitelist/add` (NIP-98 admin).
- **Read access**: reading is mostly open. A bare WebSocket connection that
  sends `["REQ", "x", { "kinds": [1] }]` will receive every public kind-1
  event ever stored (subject to zone rules for kind-42 only —
  `nip_handlers.rs:392-407`). NIP-11 advertises `auth_required: false`
  (`nip11.rs:39`). The only kind currently AUTH-gated for reading is 1059
  (DMs).
- **Could a non-forum agent connect to the CF relay-worker today and post
  events?** Yes for kinds 0, 9021, 9024 (one-shot self-onboarding which
  self-promotes to lobby cohort, allowing further posting subject to rate
  limits and trust level). For all other kinds, no — the whitelist gate
  blocks them with `OK false "blocked: pubkey not whitelisted"`. So the relay
  is "publicly accessible, privately admitted".
- **What stops them**: (a) IP rate limits — 20 connections/IP, 10 events/s/IP
  (`relay_do/mod.rs:47`, `broadcast.rs:75`); (b) D1 whitelist row check on
  every event (`is_whitelisted` at `relay_do/nip_handlers.rs:71`);
  (c) suspended/silenced flags on the whitelist row; (d) `mod_cache`
  ban/mute lookup; (e) Schnorr signature verification (`verify_event_strict`
  at `nip_handlers.rs:200`). The relay does NOT IP-allowlist; it is reachable
  from anywhere.
- **Coupling to a "private relay" deployment**: The forum hard-codes a single
  relay URL (§3.5) and treats it as trusted (mod-cache mirror, NIP-05 and
  WebID emitted relative to `dreamlab-ai.com`). NIP-11 says
  `restricted_writes: true` and `description: "Private whitelist-only Nostr
  relay for the DreamLab community."` (`nip11.rs:24-25,41`). Whitelist
  semantics make this a **walled garden over Nostr** rather than a
  genuinely private relay; the CF Workers HTTP endpoint is publicly
  accessible.
- **Migration risk to a non-CF relay**: The forum-client
  (`relay.rs:528 get_relay_url`) accepts runtime URL override via
  `window.__ENV__.VITE_RELAY_URL`, so swapping the WebSocket endpoint to
  `wss://relay.damus.io` etc. is a one-line config change. However the relay
  worker's HTTP API (whitelist, profiles batch, audit log) lives at the same
  host and there is no NIP-65 outbox model — the client cannot fan out to
  multiple relays today. Moving to a generic public relay would break
  whitelist enforcement, moderation event mirroring, and the
  `/api/profiles/batch` projection (which is a relay-worker-specific
  endpoint, not a Nostr feature).

In short: the deployment is a **single-relay walled-garden architecture**
with administrative gating, not a relay-agnostic federated client. The CF
relay-worker performs both NIP-01 relay duty AND community admin API duty
on the same workers.dev hostname.

