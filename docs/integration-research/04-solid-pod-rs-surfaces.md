# solid-pod-rs Crate Surfaces

Research date: 2026-05-07. All file/line citations refer to
`/home/devuser/workspace/project/solid-pod-rs/` (gitignored symlink, repo
`github.com/dreamlab-ai/solid-pod-rs`, branch `main`, working-tree state).

Workspace `version = "0.4.0-alpha.2"`
(`solid-pod-rs/Cargo.toml:14`); RELEASE_NOTES.md:1 reflects an *intended*
`v0.5.0-alpha.2` Sprint 12 close (2026-05-06) but the workspace
`Cargo.toml` has not yet been bumped — the source tree carries the
**unpublished** delta on top of the last published `0.4.0-alpha.1`.

---

## 1. Workspace layout (every crate, role, public version)

`solid-pod-rs/Cargo.toml:1-10` — workspace members:

| Crate (path) | Role | Cargo `version` | Lib name |
|---|---|---|---|
| `crates/solid-pod-rs` | Core protocol library: LDP, WAC 1.x/2.0, WebID, NIP-98, Solid Notifications 0.2, Solid-OIDC 0.1, provisioning, multitenant resolver, security primitives, quota | `0.4.0-alpha.2` (workspace) | `solid_pod_rs` |
| `crates/solid-pod-rs-server` | Drop-in actix-web server binary + library wrapping the core (route table, middleware stack, CLI) | `0.4.0-alpha.2` | `solid_pod_rs_server` (+ bin) |
| `crates/solid-pod-rs-activitypub` | ActivityPub federation: actor profile, inbox/outbox, HTTP Signatures, delivery worker, SQLite-backed store | `0.4.0-alpha.2` | `solid_pod_rs_activitypub` |
| `crates/solid-pod-rs-git` | Git smart-HTTP backend wrapping `git-http-backend(1)` CGI; `Basic nostr:<token>` ↔ NIP-98 bridge | `0.4.0-alpha.2` | `solid_pod_rs_git` |
| `crates/solid-pod-rs-idp` | Solid-OIDC IdP: `/auth`, `/token`, `/me`, JWKS, DPoP, dynamic client reg, passkey + Schnorr SSO | `0.4.0-alpha.2` | `solid_pod_rs_idp` |
| `crates/solid-pod-rs-nostr` | did:nostr Tier-1/3 DID Docs, bidirectional WebID resolver, embedded NIP-01/11/16 relay + WebSocket wire | `0.4.0-alpha.2` | `solid_pod_rs_nostr` |
| `crates/solid-pod-rs-didkey` | did:key (Ed25519/P-256/secp256k1) self-signed JWT verifier; plugs into `CidVerifier` | `0.4.0-alpha.2` | `solid_pod_rs_didkey` |

License: AGPL-3.0-only (`solid-pod-rs/Cargo.toml:16`). MSRV 1.75.

The workspace pins version uniformly via `version.workspace = true` in
each crate manifest (e.g. `crates/solid-pod-rs-server/Cargo.toml:3`).

There is **no `solid-pod-rs-wac` standalone crate, no `solid-pod-rs-webid`
standalone crate, no `solid-pod-rs-storage` standalone crate**: WAC,
WebID, storage, provision, notifications, security all live as `pub mod`
inside the core crate.

---

## 2. solid-pod-rs core (`Storage` trait, KvBackend, types)

Top-level surface re-exports (`crates/solid-pod-rs/src/lib.rs:84-152`):

- Modules: `auth`, `config`, `error`, `interop`, `ldp`, `metrics`,
  `multitenant`, `notifications`, `provision`, `quota`, `security`,
  `storage`, `wac`, `webid`. `oidc` is feature-gated (line 99-100);
  `handlers` only when `legacy-notifications` is on (line 106-107).
- Re-exports include: `auth::nip98::Nip98Verifier`,
  `auth::self_signed::{CidVerifier, ProofEnvelope, SelfSignedError,
  SelfSignedVerifier, VerifiedSubject}`, `error::PodError`,
  `metrics::SecurityMetrics`, `security::{is_path_allowed, is_safe_url,
  resolve_and_check, DotfileAllowlist, …, SsrfPolicy}` (lines 110-119),
  `storage::{ResourceMeta, Storage, StorageEvent}` (line 120),
  `wac::{check_origin, evaluate_access, evaluate_access_with_groups,
  parse_turtle_acl, serialize_turtle_acl, wac_allow_header,
  AccessMode, AclDocument, GroupMembership, Origin, OriginDecision,
  OriginPattern, StaticGroupMembership}` (lines 121-126), and the LDP /
  interop / multitenant / provision / quota / webid surface.

### `Storage` trait — file `crates/solid-pod-rs/src/storage/mod.rs`

`pub trait Storage: Send + Sync + 'static` (line 73). Method set:

- `async fn get(&self, path: &str) -> Result<(Bytes, ResourceMeta), PodError>` — line 75
- `async fn put(&self, path, body: Bytes, content_type: &str) -> Result<ResourceMeta, PodError>` — line 80-85
- `async fn delete(&self, path) -> Result<(), PodError>` — line 88
- `async fn list(&self, container) -> Result<Vec<String>, PodError>` — line 94
- `async fn head(&self, path) -> Result<ResourceMeta, PodError>` — line 97
- `async fn exists(&self, path) -> Result<bool, PodError>` — line 100
- `async fn create_container(&self, path) -> Result<ResourceMeta, PodError>` (default impl) — line 108
- `async fn watch(&self, path) -> Result<tokio::sync::mpsc::Receiver<StorageEvent>, PodError>` — line 128-131

**Critical contract observation**: `watch()` returns
`tokio::sync::mpsc::Receiver` (line 131). The trait is therefore
**Tokio-runtime-coupled** — implementations cannot be back-ended by
Cloudflare Workers' single-threaded JS event loop without paying a
Tokio compatibility shim. `Send + Sync + 'static` (line 73) further
bars `?Send` futures, ruling out `wasm32-unknown-unknown` Workers
where futures are intentionally `!Send`.

`ResourceMeta` (lines 26-41) carries `etag`, `modified`,
`size`, `content_type`, `links: Vec<String>`. `StorageEvent` (lines 57-64)
is `Created | Updated | Deleted` keyed by path.

### Built-in backends

`crates/solid-pod-rs/src/storage/`:

- `fs.rs` — 400 LOC (`storage/fs.rs:1-400`); uses `tokio::fs`
  (line 13) and `tokio::sync::mpsc` (line 14) plus `notify` 6.x for
  filesystem watch events. Not portable to WASM.
- `memory.rs` — 263 LOC; in-memory `HashMap`, gated by
  `feature = "memory-backend"` (`storage/mod.rs:22-23`).
- No `s3` source file is present in the working tree even though
  Cargo declares `s3-backend = ["dep:aws-sdk-s3"]`
  (`Cargo.toml:81`); the implementation appears to be deferred /
  unmerged in this branch.

There is **no `KvBackend`, no Cloudflare R2/KV adapter**, and **no
generic key-value trait** above the file-shaped `Storage` API. The
Solid LDP semantics (containers, RDF, ETag on HTTP body)
*are* the abstraction, not a primitive get/put on bytes.

### Other shared types

- `error::PodError` — `crates/solid-pod-rs/src/error.rs:1-101` (101
  LOC). Variants used by every consumer: `NotFound`, `BadRequest`,
  `Unsupported`, `Forbidden`, `Unauthenticated`,
  `PreconditionFailed`, `PayloadTooLarge`, `Nip98(String)`,
  `IoError`. `PodError` is the cross-module error currency; both
  `solid-pod-rs-nostr` and `solid-pod-rs-server` translate to / from
  it (`solid-pod-rs-server/src/lib.rs:155-167` for actix mapping).
- `metrics::SecurityMetrics` — `metrics.rs:1-80`; small Counter/Histogram
  bag wired into the `SsrfPolicy` and `DotfileAllowlist`.

---

## 3. solid-pod-rs-nostr (DID Tier-1/3, relay, ws transport, resolver)

`crates/solid-pod-rs-nostr/Cargo.toml:23` re-uses the core crate with
features `nip98-schnorr`, `did-nostr`, `security-primitives` —
`default-features = false`. Wire deps: `tokio` (sync/rt/macros/time/net/io-util,
line 25), `tokio-tungstenite = 0.24` (line 26), `futures-util = 0.3`,
`reqwest = 0.12` (rustls-tls), `k256 = 0.13` schnorr feature, `url`,
`hex`, `sha2`, `thiserror`, `tracing`. dev-deps: `wiremock`, `tempfile`.

Module map (`src/lib.rs:58-74`): `did`, `error`, `relay`, `resolver`,
`ws`. Re-exports the full Tier-1/Tier-3 surface plus `Relay`,
`InMemoryEventStore`, `RelayInfo`, `Filter`, `Event`,
`NostrWebIdResolver`, `DefaultSsrfCheck`, `SsrfCheck`,
`dispatch_message`, `serve_relay_ws`, `serve_relay_ws_stream`, all
three error enums.

### DID document layer (`src/did.rs`, 324 LOC)

- `pub struct NostrPubkey([u8; 32])` (line 26) — 32-byte x-only
  Schnorr secp256k1 pubkey. Constructors `from_hex(&str)` (line 30)
  rejects non-64-char-hex; `to_hex` (line 44).
- `pub fn did_nostr_uri(&NostrPubkey) -> String` — line 50; produces
  `did:nostr:<hex>`.
- `pub fn well_known_path(&NostrPubkey) -> String` — line 56;
  `/.well-known/did/nostr/<pubkey>.json`.
- `pub struct ServiceEntry { id, service_type, service_endpoint, extra }`
  — line 66-78.
- `pub fn render_did_document_tier1(&NostrPubkey) -> serde_json::Value`
  — line 90; emits `@context` (`https://www.w3.org/ns/did/v1`),
  `id`, empty `alsoKnownAs`, `verificationMethod` of type
  `NostrSchnorrKey2024` with `publicKeyHex` and a multibase
  (`multicodec 0xe7 || 0x01 || pk`, base58btc) — lines 92-103.
- `pub fn render_did_document_tier3(&NostrPubkey, webid: Option<&str>,
  services: &[ServiceEntry]) -> serde_json::Value` — line 113-163.
  Adds `@context` `secp256k1-2019/v1`, `alsoKnownAs: [webid]`,
  `authentication`, `assertionMethod`, and merged `service[]`
  entries; extras cannot override the canonical
  `id/type/serviceEndpoint` (defence-in-depth, lines 130-141).
- The `interop::did_nostr` module inside the **core** library
  (`crates/solid-pod-rs/src/interop.rs:295-321`,
  `did_nostr_well_known_url`, `did_nostr_document`) ships the
  Tier-1 renderer too — duplication is intentional: `did-nostr`
  feature-gated server-side path on the core, vs. richer Tier-3 +
  resolver on the sibling crate (per `solid-pod-rs-nostr/README.md:5-10`
  and `lib.rs:9-19`).

### Embedded relay (`src/relay.rs`, 734 LOC)

Public surface (line numbers from `relay.rs`):
- `pub struct Event { id, pubkey, created_at, kind, tags, content, sig }`
  — line 35.
- `Event::canonical_id(&self) -> String` — line 48; computes the
  NIP-01 canonical hash `sha256(json([0, pubkey, created_at, kind,
  tags, content]))`.
- `Event::verify(&self) -> Result<(), RelayError>` — line 63; checks
  pubkey/sig length, recomputes id, BIP-340 schnorr verification via
  `k256::schnorr::VerifyingKey`.
- `pub struct Filter { ids, authors, kinds, since, until, limit,
  tags: HashMap<String, Value> }` — line 108; `Filter::from_value`
  decodes `#X` tag filters (line 129); `Filter::matches(&Event) -> bool`
  (line 147).
- Replaceable-event classifiers: `is_replaceable(kind)` (line 203,
  kinds 0,3, 10000-19999), `is_ephemeral` (20000-29999, line 207),
  `is_parameterised_replaceable` (30000-39999, line 211).
- `pub trait EventStore: Send + Sync` — line 222. Methods:
  `put`, `remove`, `snapshot`, `replace_where(predicate, event)`,
  `len`, `is_empty`. **Sync trait** — implementations cannot await.
- `pub struct InMemoryEventStore` — line 246; `Mutex<Vec<Event>>`
  with `max_events` ring-buffer cap. Default cap 1000 (line 262).
- `pub struct RelayInfo { name, description, pubkey, contact,
  supported_nips, software, version }` — line 313 (NIP-11). Default
  `RelayInfo::jss_compatible()` (line 324) advertises `supported_nips:
  [1, 11, 16]` only — so cross-system DM relays consuming this would
  *not* see NIP-04/NIP-17/NIP-44 advertised.
- `pub struct Relay { store: Arc<dyn EventStore>, events_tx:
  broadcast::Sender<Event>, info: Arc<RelayInfo> }` — line 345.
  - `Relay::new(store, info, broadcast_capacity)` — line 354
  - `Relay::in_memory()` (capacity 256) — line 369
  - `Relay::info() -> &RelayInfo` — line 378
  - `Relay::subscribe() -> broadcast::Receiver<Event>` — line 383
  - `Relay::ingest(Event) -> Result<(), RelayError>` — line 403; runs
    `event.verify()` then dispatches by kind classifier; ephemerals
    broadcast-only, replaceables `replace_where`, regulars `put`.
  - `Relay::history(&[Filter]) -> Vec<Event>` — line 453; per-filter
    limit + dedup by id.

### WebSocket transport (`src/ws.rs`, 364 LOC)

- `pub async fn serve_relay_ws_stream<S>(relay: Arc<Relay>, ws:
  WebSocketStream<S>)` — line 35; runs the NIP-01 message loop.
  Bound `S: AsyncRead + AsyncWrite + Unpin + Send + 'static`
  (line 37) — Tokio-AsyncRead, **not** WASM-compatible.
- `pub async fn serve_relay_ws<S>(relay, stream: S)` — line 95;
  performs `tokio_tungstenite::accept_async` upgrade.
- `pub fn dispatch_message(&Relay, &mut HashMap<String, Vec<Filter>>,
  text: &str) -> Vec<String>` — line 111. **Pure** message
  parser — accepts a JSON text frame and returns the responses to
  send. This is the only piece of the relay machinery that has any
  chance of being driven from a WASM Worker (no AsyncRead bound, no
  awaiting).
- Server-to-client frames: `EVENT`, `EOSE`, `OK`, `NOTICE`.
  Client-to-server: `EVENT`, `REQ`, `CLOSE` (line 129-134).

### Bidirectional resolver (`src/resolver.rs`, 431 LOC)

- `pub trait SsrfCheck: Send + Sync` (line 35) — single async method
  `verify_host(&self, host: &str) -> Result<(), String>`.
- `pub struct DefaultSsrfCheck` (line 46) delegates to the core
  crate's `solid_pod_rs::security::ssrf::resolve_and_check`
  (line 51). RFC 1918 / loopback / link-local / multicast /
  cloud-metadata refused by default.
- `pub struct NostrWebIdResolver { http: reqwest::Client, ssrf:
  Arc<dyn SsrfCheck> }` — line 63.
  - `NostrWebIdResolver::new()` (default SSRF, 10 s timeout) — line 70.
  - `with_ssrf(Arc<dyn SsrfCheck>)` — line 75.
  - `with_http(Client, Arc<dyn SsrfCheck>)` — line 84.
  - `resolve_webid_to_nostr(&self, webid: &str) ->
    Result<Option<NostrPubkey>, ResolverError>` — line 93. Fetches
    the WebID profile (Accept JSON-LD/JSON/Turtle/HTML), runs the
    body through `extract_nostr_pubkey_from_profile` (line 207).
    Recognises `alsoKnownAs`, `sameAs`, `owl:sameAs`,
    `schema:sameAs`, both prefixed and full IRI variants
    (line 238-245); recurses into `@graph` arrays (line 256-271);
    Turtle text-substring fallback (line 300-323); HTML JSON-LD
    `<script type="application/ld+json">` data island
    (line 325-337).
  - `resolve_nostr_to_webid(&self, origin: &str, pk: &NostrPubkey)
    -> Result<Option<String>, ResolverError>` — line 136. Fetches
    `<origin>/.well-known/did/nostr/<hex>.json`, requires
    `id == did:nostr:<hex>` else `Malformed`, returns the first
    HTTP(S) `alsoKnownAs` (lines 174-186).

**Critical question (raised in the brief): "Does the DID resolver
resolve a Tier-3 doc given just a hex pubkey + relay URL?"**

**No.** `resolve_nostr_to_webid` requires an *origin URL* (for the
`/.well-known/did/nostr/<hex>.json` HTTPS endpoint), **not** a Nostr
relay URL. The current resolver does **not** fetch a Tier-3 DID
document **from a Nostr relay** by way of a `kind:0` profile event or
parameterised replaceable event. It is purely a `.well-known` HTTPS
fetch. To support pubkey + relay-URL → DID-doc lookup the resolver
would need a `relay::Filter`-driven query path that subscribes for
the user's profile event and parses `alsoKnownAs` from
`content.tags`. That capability does **not** exist today.

### Tests (`tests/`)

- `relay_nip11.rs` (57 LOC) — verifies `Relay::info()` populates
  required NIP-11 fields and serialises cleanly; checks NIPs 1, 11,
  16 advertised.
- `resolver_integration.rs` (135 LOC) — wiremock-driven WebID →
  did:nostr (line 27), did:nostr → WebID (line 54), id-mismatch
  rejection (line 83), private-IP refusal (line 111), GCP metadata
  refusal (line 125). All four resolver paths + SSRF defaults
  exercised.

---

## 4. solid-pod-rs-idp (Schnorr signer, IdP)

`crates/solid-pod-rs-idp/Cargo.toml`. Pulls core with features `oidc`,
`dpop-replay-cache`, `rate-limit`, `security-primitives`,
`nip98-schnorr` (line 26).

`src/lib.rs:65-83` modules: `credentials`, `discovery`, `error`,
`invites`, `jwks`, `provider`, `registration`, `session`, `tokens`,
`user_store`. Optional: `passkey` (feature `passkey` → webauthn-rs +
dashmap), `schnorr` (feature `schnorr-sso` → dashmap + k256), and
`axum_binder` (feature `axum-binder`).

### Provider façade (`src/provider.rs`)

- `pub struct ProviderConfig` (line 48) — issuer-rooted config.
- `pub struct Provider` (line 70) — composition root.
  `Provider::new(ProviderConfig, ClientStore, SessionStore,
  Arc<dyn UserStore>, Jwks)` (line 80).
- `pub async fn authorize(AuthorizeRequest) ->
  Result<AuthorizeResponse, ProviderError>` (line 136).
- `pub async fn token(TokenRequest) -> Result<TokenResponse, …>` (line 210).
- `pub async fn userinfo(…)` (line 309).
- `discovery_document(&self) -> DiscoveryDocument` (line 125).

### `UserStore` (`src/user_store.rs`)

- `pub trait UserStore: Send + Sync + 'static` (line 62) — pluggable.
- `pub struct InMemoryUserStore` (line 102), built-in for tests + dev.
- `MIN_PASSWORD_LENGTH = 8` and `validate_password_length()`
  enforced on registration (Sprint 12 hardening; see
  `RELEASE_NOTES.md:30-42`).

### Schnorr SSO (NIP-07-style sign-in, `src/schnorr.rs`, 322 LOC)

- `pub trait SchnorrSso: Send + Sync + 'static` (line 121). Methods:
  `issue_challenge(user_id) -> SchnorrChallenge`,
  `verify_response(user_id, pubkey_hex, signature_hex) ->
  SchnorrAssertion`. Single-use challenges per
  user_id; one-shot semantics (success or failure consumes the
  challenge — line 245-250).
- `pub struct Nip07SchnorrSso { challenges: DashMap<String,
  (SchnorrChallenge, Instant)>, ttl: Duration }` (line 178). 5-min
  default TTL (line 187).
- Canonical digest: `SHA-256(token ‖ user_id ‖ pubkey)` (line 203-210).
- `verify_response` does BIP-340 verify via `k256::schnorr` —
  delegates `nip98-schnorr` feature plumbing from the core crate
  (line 277-288).
- `SchnorrTodo` (line 144) — `#[doc(hidden)]` always-Unimplemented
  fallback so callers can wire a `Provider` before deciding whether
  to enable `schnorr-sso`.

### IdP test surface

`crates/solid-pod-rs-idp/tests/` exists (Cargo dev-deps include
`wiremock`, `serde_urlencoded`, `k256`, `sha2`,
`Cargo.toml:80-86`). Coverage of authorize-code flow + DPoP-bound
token issuance + ES256 JWKS publication is exercised through the
core's `oidc_*` integration tests (see §10 / §12).

---

## 5. solid-pod-rs-server (LDP/HTTP layer)

`crates/solid-pod-rs-server/Cargo.toml`. Compiles to both a library
(reuse from integration tests) and a binary `solid-pod-rs-server`
(lines 16-22). Pulls core with features `fs-backend`, `memory-backend`,
`config-loader`, `legacy-notifications` (line 26). Pulls
`solid-pod-rs-idp` for the CLI ops (`account delete`, `invite create`,
line 30). Optional `tls` feature → rustls 0.23 (line 79).

### Library API (`src/lib.rs`, 1283 LOC)

- `pub struct AppState { storage: Arc<dyn Storage>, dotfiles:
  Arc<DotfileAllowlist>, body_cap: usize, nodeinfo: NodeInfoMeta,
  mashlib_cdn: Option<String> }` — line 92.
  `AppState::new(Arc<dyn Storage>)` (line 140).
- `pub struct NodeInfoMeta { software_name, software_version,
  open_registrations, total_users, base_url }` — line 102.
- `pub const DEFAULT_BODY_CAP: usize = 50 * 1024 * 1024` — line 124.
- `pub fn body_cap_from_env() -> usize` — line 128 (parses
  `JSS_MAX_REQUEST_BODY` via `solid_pod_rs::config::sources::parse_size`).
- `pub fn build_app(state: AppState) -> App<…>` — line 1201.

### Route table

`src/lib.rs:21-41` (doc-comment) and lines 1264-1281 register:

- `GET/HEAD /{tail:.*}` → `handle_get` (line 282).
- `PUT /{tail:.*}` and `PUT /{tail:.*}/` (with `Link:
  <ldp:BasicContainer>; rel="type"`) → `handle_put` (line 344).
- `POST /{tail:.*}/` → `handle_post` (line 393).
- `PATCH /{tail:.*}` → `handle_patch` (line 434). Dialects: N3,
  SPARQL Update, JSON Patch, dispatched via
  `ldp::patch_dialect_from_mime` (line 452).
- `DELETE /{tail:.*}` → `handle_delete` (line 600).
- `COPY /{tail:.*}` (custom method) → `handle_copy` (line 783).
- `OPTIONS /{tail:.*}` → `handle_options` (line 616).
- Pod management (line 1264-1269): `POST /api/accounts/new` → JSS-
  parity provisioning (line 738), `GET /pods/check/{name}` (line 726),
  `POST /login/password`, `POST /account/password/{reset,change}`.
- Discovery: `/.well-known/{solid,webfinger,nodeinfo,nodeinfo/2.1}`
  (lines 645-697). `did:nostr` document at
  `/.well-known/did/nostr/{pubkey}.json` only mounted when
  `feature = "did-nostr"` (line 1255-1261).

### Middleware stack (in apply order, `src/lib.rs:1217-1230`)

1. `ErrorLoggingMiddleware` — outermost; logs 5xx with full chain
   (line 1016-1095).
2. `NormalizePath::new(MergeOnly)` — collapses `//`.
3. `PathTraversalGuard` — rejects `..` after percent-double-decode
   (line 933-998).
4. `DotfileGuard` — enforces `DotfileAllowlist`; carves out
   `/.well-known/*` (line 1122-1188).
5. `PayloadConfig` body cap.
6. WAC enforcement on writes via `enforce_write` (line 204-251),
   which builds a `wac::conditions::RequestContext`, looks up the
   effective ACL through `find_effective_acl_dyn` (line 557-598), and
   replies 401 vs. 403 based on whether the agent was authenticated.

### Auth helper

`extract_pubkey(&HttpRequest) -> Option<String>` — line 174-187. Pulls
`Authorization`, calls `solid_pod_rs::auth::nip98::verify(header,
url, method, None)`, returns the pubkey hex on success. Body hash is
**always None** here (line 184) — i.e. the actix server never passes
the request body into NIP-98 payload-tag verification, so any
`payload` tag is **structurally validated only against absence of
body**, not against actual upload bytes. (The library-level
`verify_at` does verify the hash if provided —
`crates/solid-pod-rs/src/auth/nip98.rs:118-131` — it's the wrapper at
line 184 that's the gap.)

---

## 6. solid-pod-rs-didkey (verifier)

`crates/solid-pod-rs-didkey/Cargo.toml`. Pulls core with
`security-primitives` only (`Cargo.toml:22`). 858 LOC across 6
modules per `solid-pod-rs/RELEASE_NOTES.md:84-89`.

`src/lib.rs` modules: `did`, `error`, `jwt`, `pubkey`, `verifier`.

### Verifier (`src/verifier.rs`)

- `pub struct DidKeyVerifier { skew: u64 }` (line 19).
- `DEFAULT_SKEW_SECONDS: u64 = 60` (line 15) — `iat` drift tolerance.
- `DidKeyVerifier::new()` / `with_skew(secs)` (lines 25-37).
- Implements `solid_pod_rs::auth::self_signed::SelfSignedVerifier`
  (line 59), so it slots into a `CidVerifier` next to
  `Nip98Verifier`. `name() -> "did:key"` (line 102).
- The `looks_like_compact_jws` heuristic (line 50) lets the fan-out
  return `Ok(None)` (rather than an error) for non-JWT inputs so the
  next verifier can try.

### JWT layer (`src/jwt.rs`)

`verify_self_signed_jwt(proof, uri, method, now_unix, skew)` (called
from `verifier.rs:67`). Supports the three did:key codec families
declared as deps: Ed25519 (`ed25519-dalek = 2`), P-256
(`p256 = 0.13` ecdsa), secp256k1 (`k256 = 0.13` ecdsa+schnorr).
`alg=none` is hard-rejected per `RELEASE_NOTES.md:91-94`.

29 unit + integration tests across all three curves —
`RELEASE_NOTES.md:91-94`.

---

## 7. solid-pod-rs-git (auth, lib)

`crates/solid-pod-rs-git/Cargo.toml`. Pulls core with no extra
features (line 20). Dev-deps: `tempfile`, `tokio`, `sha2`, `hex`,
`serde_json`. Cargo feature `with-git-binary` (line 41) gates
end-to-end tests requiring `git-http-backend(1)` CGI on PATH.

`src/lib.rs` modules: `auth`, `config`, `error`, `guard`, `service`.
Re-exports: `BasicNostrExtractor`, `GitAuth`, `AuthError`,
`find_git_dir`, `GitDir`, `GitError`, `extract_repo_slug`,
`path_safe`, `GitHttpService`, `GitRequest`, `GitResponse`,
`DEFAULT_GIT_HTTP_BACKEND` (lines 56-60).

### Auth bridge (`src/auth.rs`, 217 LOC)

- `pub trait GitAuth: Send + Sync` (line 44) — single
  `authorise(&GitRequest) -> Result<String, AuthError>`.
- `pub struct BasicNostrExtractor { allowed_pubkeys: Option<Arc<Vec<String>>> }`
  (line 61).
  - `BasicNostrExtractor::new()` (line 70).
  - `with_allowed(Vec<String>)` (line 76).
  - `extract_nostr_token(header_value)` (line 86) parses
    `Basic <b64(nostr:<token>)>` and validates the username is the
    literal string `nostr`.
  - The `GitAuth` impl (line 113) accepts both `Basic` and `Nostr`
    schemes; wraps the extracted token back into `Nostr <b64>` and
    delegates to `solid_pod_rs::auth::nip98::verify_at` (line 146).
    **Body hashing is `None`** (line 154) — git push payloads are
    not signed, only the URL/method/timestamp.

### Service (`src/service.rs`)

`pub struct GitHttpService` (line 60 of `lib.rs` re-exports). Binds
`with_auth(impl GitAuth)`. `handle(GitRequest) -> GitResponse`
spawns `git-http-backend(1)` (path overridable via
`GIT_HTTP_BACKEND_PATH`, doc at `lib.rs:38-42`).

### Tests directory

`tests/` exists (per `crates/solid-pod-rs-git/tests/`, see workspace
`ls`). Auth/guard/config unit tests run unconditionally; CGI
integration tests gated on `with-git-binary`.

---

## 8. WAC + NIP-98 + WebID + provision modules

These all live inside the **core** crate.

### WAC (`src/wac/`, 9 source files, 2722 LOC)

`crates/solid-pod-rs/src/wac/mod.rs` lines 78-127 are the public
parser API. `parse_jsonld_acl(body: &[u8])` (line 78) and
`parse_jsonld_acl_with_limits(body, max_bytes, max_depth)` (line 96)
both fail-fast on JSON depth bombs (line 39-71) before
`serde_json` runs. Default caps: `MAX_ACL_BYTES: usize = 1_048_576`
(line 28), `MAX_ACL_JSON_DEPTH: usize = 32` (line 33). Tunable via
`JSS_MAX_ACL_BYTES` / `JSS_MAX_ACL_JSON_DEPTH` (lines 79-86).

Submodules (`mod.rs:112-120`): `client`, `conditions`, `document`,
`evaluator`, `issuer`, `origin`, `parser`, `resolver`, `serializer`.

- `wac::AccessMode` enum (`mod.rs:145`) — `Read | Write | Append |
  Control`.
- `wac::method_to_mode(&str) -> AccessMode` (line 171).
- `wac::wac_allow_header(Option<&AclDocument>, agent_uri,
  resource_path) -> String` (line 195) — generates
  `WAC-Allow: user="read",public=""` per WAC §"WAC-Allow Header".
- `wac::wac_allow_header_with_dispatcher(...)` (line 219).
- `wac::evaluate_access(...)` (re-exported via `lib.rs:121`).
- `wac::evaluate_access_with_groups(...)`.
- `wac::evaluate_access_ctx_with_registry(...)` —
  `solid-pod-rs-server/src/lib.rs:226-234` shows the binding
  (RequestContext + ConditionRegistry + StaticGroupMembership).

#### WAC 2.0 conditions framework (`wac/conditions.rs`, 330 LOC)

- `pub enum ConditionOutcome` (line 28).
- `pub enum Condition` (line 46) — built-ins: `Client`, `Issuer`.
- `pub struct RequestContext<'a> { web_id, client_id, issuer }` (line 170).
- `pub trait ConditionDispatcher: Send + Sync` (line 184).
- `pub struct ConditionRegistry` (line 202) — registry of
  `ClientConditionEvaluator` + `IssuerConditionEvaluator`.
- `default_with_client_and_issuer()` (line 226) — recommended
  builder.
- `pub struct EmptyDispatcher` (line 272), `UnsupportedCondition` (line 289).
- `pub fn validate_for_write(...)` (line 299), `validate_acl_document` (line 328).

#### Origin enforcement (`wac/origin.rs`, 374 LOC)

- `pub struct Origin(String)` (line 39) with `parse(&str)`,
  `from_url(&Url)`, `as_str()`.
- `pub enum OriginPattern` (line 103) — exact / wildcard suffix match.
- `extract_origin_patterns(&AclAuthorization)` (line 218).
- `pub enum OriginDecision` (line 239); `pub fn check_origin(...)` (line 268).
- All gated behind feature `acl-origin` per `Cargo.toml:101-102`.

#### Issuer condition (`wac/issuer.rs`, 72 LOC)

- `pub struct IssuerConditionBody` (line 16).
- `pub struct IssuerConditionEvaluator` (line 37) — uses
  `CidVerifier` to dispatch self-signed proofs (`evaluate(...)` line 40).

### NIP-98 (`src/auth/nip98.rs`, 484 LOC)

Surface (line numbers from `auth/nip98.rs`):

- `const HTTP_AUTH_KIND: u64 = 27235` (line 22), `TIMESTAMP_TOLERANCE: u64 = 60`
  (line 23), `MAX_EVENT_SIZE: usize = 64 * 1024` (line 24).
- `pub struct Nip98Event { id, pubkey, created_at, kind, tags,
  content, sig }` (line 28).
- `pub struct Nip98Verified { pubkey, url, method, payload_hash,
  created_at }` (line 39).
- `pub async fn verify(header, url, method, body_hash) ->
  Result<String, PodError>` (line 51) — wall-clock now.
- `pub fn verify_at(header, expected_url, expected_method,
  body, now) -> Result<Nip98Verified, PodError>` (line 65) —
  deterministic tests. Validates: prefix `Nostr `, base64-decoded
  payload size, JSON shape, `kind == 27235`, 64-hex pubkey,
  ±60 s timestamp, URL match (trailing-slash tolerant — line 220),
  method match, payload-hash match if `body` present.
- `compute_event_id(&Nip98Event) -> String` (line 152) — canonical
  NIP-01 hash.
- `verify_schnorr_signature(&Nip98Event)` (line 172) under feature
  `nip98-schnorr`; stub returning `PodError::Unsupported` otherwise
  (line 206).
- `pub fn authorization_header(token_b64) -> String` (line 224).
- `pub struct Nip98Verifier` (line 248) — adapter implementing
  `SelfSignedVerifier` so NIP-98 is one CID-fan-out option.

### WebID (`src/webid.rs`, 391 LOC)

- `pub fn generate_webid_html(pubkey, name, pod_base) -> String` (line 15).
- `pub fn generate_webid_html_with_issuer(pubkey, name, pod_base,
  oidc_issuer)` (line 22).
- `pub fn validate_webid_html(...)` (re-exported via `lib.rs:151`).
- `pub fn extract_oidc_issuer(...)` (re-exported via `lib.rs:150`).
- The HTML emits a JSON-LD island (lines 49-71) carrying
  `solid:oidcIssuer` + LWS 1.0 `service[]` typed
  `lws:OpenIdProvider` + `schema:identifier: did:nostr:<pubkey>`.

### Provision (`src/provision.rs`, 548 LOC)

- `pub struct ProvisionPlan { pubkey, display_name, pod_base,
  containers, root_acl, quota_bytes }` (line 29).
- `pub struct ProvisionOutcome { webid, pod_root,
  containers_created, quota_bytes, public_type_index,
  private_type_index, public_type_index_acl }` (line 51).
- `pub const PUBLIC_TYPE_INDEX_PATH: &str = "/settings/publicTypeIndex.jsonld"` (line 72).
- `pub const PRIVATE_TYPE_INDEX_PATH` (line 75) and
  `PUBLIC_TYPE_INDEX_ACL_PATH` (line 78).
- `pub async fn provision_pod<S: Storage + ?Sized>(
  storage: &S, plan: &ProvisionPlan) -> Result<ProvisionOutcome,
  PodError>` (line 155). Lays out: WebID HTML, pod root, all
  containers in `plan.containers`, both type-index sidecars + their
  public-read carve-out ACL.
- `pub struct QuotaTracker` (line 280) — atomic-counter, `reserve`
  / `release` / `used` / `quota`.
- `pub struct AdminOverride` (line 342), `pub fn check_admin_override(
  ...)` (line 347).

### Multitenant (`src/multitenant.rs`, 317 LOC)

- `pub struct ResolvedPath` (line 28), `pub trait PodResolver:
  Send + Sync` (line 38), `pub struct PathResolver` (line 49),
  `pub struct SubdomainResolver` (line 68). Both built-in resolvers
  return a `ResolvedPath` so handlers can route requests across pods.

### Cross-cutting answer (forum's pod-worker reimplementation)

Forum's `dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/`
files reimplement the following surfaces (LOC totals):

| Pod-worker file | LOC | solid-pod-rs equivalent | Why reimplemented |
|---|---:|---|---|
| `acl.rs` | 821 | `solid_pod_rs::wac::*` (parsers, evaluator, origin, conditions) | WASM target — tokio mpsc/fs in core trait blocks compile |
| `did.rs` | 315 | `solid_pod_rs_nostr::did::{render_did_document_tier1, render_did_document_tier3, format_multibase_schnorr, base58_encode}` | Same; explicit `// Mirrors solid_pod_rs_nostr::did::…` comments at lines 75, 102, 167, 178 |
| `webid.rs` | 86 | `solid_pod_rs::webid::generate_webid_html_with_issuer` | Same |
| `provision.rs` | 425 | `solid_pod_rs::provision::{PUBLIC_TYPE_INDEX_PATH, PRIVATE_TYPE_INDEX_PATH, PUBLIC_TYPE_INDEX_ACL_PATH, provision_pod, render_type_index_body}` | Same; `// Mirrors solid_pod_rs::provision::…` at lines 17, 22, 27, 34 |
| `auth.rs` | 123 | `solid_pod_rs::auth::nip98::verify_at` (rewrapped via `nostr_core::verify_nip98_token_at_with_replay`) | **Replay store needed** — solid-pod-rs exposes none |
| `quota.rs` | 107 | `solid_pod_rs::quota::FsQuotaStore` | FS-backed only |
| `notifications.rs` | 167 | `solid_pod_rs::notifications::{LegacyWebSocketSession, …}` | Tokio-coupled |
| `patch.rs` | 319 | `solid_pod_rs::ldp::{apply_n3_patch, apply_sparql_patch, apply_json_patch}` | Likely portable; not yet migrated |
| `container.rs`, `content_negotiation.rs`, `conditional.rs` | 162+243+110 | `solid_pod_rs::ldp::*` | Same |
| `payments.rs`, `remote_storage.rs`, `contexts.rs` | 307+197+80 | (Forum-specific, no equivalent) | Forum domain |

Total `acl + did + webid + provision + auth + quota +
notifications + patch + container + content_negotiation +
conditional + lib.rs glue` ≈ 4,278 LOC of forum-side work that
mirrors solid-pod-rs surfaces, of which roughly 2,300 LOC
(`acl + did + webid + provision + auth + quota + container +
conditional + content_negotiation`) **are direct re-implementations
of present solid-pod-rs APIs that cannot be linked because of the
WASM-Workers boundary**.

The migration mapping above is the prerequisite work for any
"forum imports solid-pod-rs" plan — the trait/feature redesign in
§11 is the unblocker.

---

## 9. NIP-11 relay capability advertisement

Defined in `crates/solid-pod-rs-nostr/src/relay.rs:313-335`:

- `RelayInfo { name, description, pubkey, contact, supported_nips:
  Vec<u64>, software, version }` (line 313).
- `RelayInfo::jss_compatible()` (line 324) hard-codes
  `supported_nips: vec![1, 11, 16]` (line 330).
- The wire-level handler exposes the document via
  `Relay::info() -> &RelayInfo` (line 378). The HTTP `Accept:
  application/nostr+json` content type is a **consumer
  responsibility**: the crate does not ship a full HTTP server, the
  embedder serves it (see `relay_nip11.rs:32-37` — round-trip JSON
  encode test).

NIPs **not** advertised today: 4 (legacy DM), 17 (modern DM via
gift-wrap), 33 (parameterised replaceable; the relay supports
the *kind classes* but does not advertise the NIP), 44 (encrypted
payloads), 09 (event deletion). The SQLite/postgres durable adapter
the brief asks about does not exist — `EventStore` + the in-memory
adapter is the only shipped pair (line 222-305).

---

## 10. NIP-98 verifier surface (with replay store trait)

**Critical answer**: solid-pod-rs's NIP-98 verifier does **not**
expose a replay-store trait.

`crates/solid-pod-rs/src/auth/nip98.rs`:
- The only freshness defence is the ±60 s timestamp window
  (line 23, line 95-100).
- `verify`, `verify_at`, and the `Nip98Verifier` `SelfSignedVerifier`
  adapter (lines 51, 65, 248) all accept the body / now / URL /
  method but offer **no hook for "have I seen this event id
  before?"**.
- A jti-style replay cache exists *only for DPoP* in the OIDC layer
  (`src/oidc/replay.rs` — `pub struct DpopReplayCache` line 89,
  `pub struct JtiReplayCache` line 314, `pub fn check_and_insert`
  line 370, gated `dpop-replay-cache` feature). It is **not wired
  into NIP-98** and the trait is concrete (`JtiReplayCache`), not
  pluggable.

This is the single biggest reason `community-forum-rs` ships its
own `nostr_core::Nip98ReplayStore` trait (file
`dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip98.rs:45`
declares `pub trait Nip98ReplayStore`; the pod-worker implements it
over a Cloudflare KV namespace at
`dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/auth.rs:11-41`).

To unify, solid-pod-rs core would need to expose:

```rust
#[async_trait]
pub trait Nip98ReplayStore: Send + Sync {
    async fn seen_or_record(&self, event_id: &str) -> Result<bool, String>;
}

pub async fn verify_at_with_replay(
    header: &str, url: &str, method: &str,
    body: Option<&[u8]>, now: u64,
    replay: &dyn Nip98ReplayStore,
) -> Result<Nip98Verified, PodError>;
```

Today the core is one short feature-flagged commit away from this
surface — every input piece is already plumbed (event id is
recomputed on line 175-181 of `nip98.rs`).

---

## 11. WS transport (Tokio vs WASM compatibility)

`crates/solid-pod-rs-nostr/src/ws.rs:35-105`:

- `serve_relay_ws_stream<S>` requires `S: AsyncRead + AsyncWrite +
  Unpin + Send + 'static` (line 37). `AsyncRead` + `AsyncWrite` here
  are the **Tokio** traits (`use tokio::io::{AsyncRead, AsyncWrite};`
  line 23), not `futures-io`. The `Send` bound rules out
  Cloudflare Workers' `!Send` futures, and `tokio_tungstenite`
  is itself unsupported on `wasm32-unknown-unknown`.
- `serve_relay_ws<S>` calls `tokio_tungstenite::accept_async` (line
  102) so the same Tokio runtime constraint applies.
- **The only piece reusable from a WASM Worker** is
  `dispatch_message(&Relay, &mut HashMap<…>, &str) -> Vec<String>`
  (line 111). It is a synchronous, allocator-only function: parse a
  JSON text frame, mutate a subscription map, return a `Vec<String>`
  of frames to send. This matches the shape `worker::WebSocket`
  needs in Cloudflare Workers — **but** `Relay::ingest` (which
  `dispatch_message` calls into via `handle_event`) is also
  synchronous, so a Workers consumer could in principle drive the
  full ingestion path provided they accept the `Mutex<Vec<Event>>`
  in `InMemoryEventStore` is `std::sync::Mutex` (`relay.rs:21`,
  `Mutex` import; line 247-248) — Cloudflare Workers' `wasm32-unknown-unknown`
  target does support `std::sync::Mutex` (single-threaded), so this
  is feasible.

**Bottom line**:
- The relay's *event-store + dispatcher* is portable to WASM in
  principle.
- The *WebSocket pump* (`serve_relay_ws*`) is Tokio-only. WASM
  consumers must implement their own pump that calls
  `dispatch_message` per inbound frame and `Relay::subscribe()` for
  live broadcast (and `tokio::sync::broadcast::Receiver` is
  itself Tokio-only — `relay.rs:347`).

There is **no `futures-io` flavour, no abstract `WireSink/WireStream`
trait, no runtime-agnostic relay loop**. Adding one would unblock
the forum's relay-worker (which today reimplements the wire
loop wholesale).

---

## 12. Test coverage of the cross-runtime surface

### Core crate (`crates/solid-pod-rs/tests/`, 47 files)

Notable for the cross-system surface:

- `did_nostr_resolver.rs` (206 LOC) — server-side
  `interop::did_nostr` resolver coverage.
- `nip98_extended.rs` (191 LOC) — extended NIP-98 fixtures including
  `payload` tag, body hash, malformed inputs.
- `cid_verifier_sprint11.rs` (265 LOC) — fan-out across `did:key` +
  `nip98` + bridged `did:nostr` proofs.
- `dpop_replay_test.rs`, `oidc_dpop_signature.rs`,
  `oidc_thumbprint_rfc7638.rs`, `oidc_jwks_ssrf.rs`,
  `oidc_access_token_alg.rs`, `oidc_integration.rs`,
  `oidc_mod_direct.rs` — full OIDC + DPoP coverage.
- `wac_*.rs` (8 files) — basic, inheritance, parser bounds, origin
  enforcement (Sprint 9), conditions (Sprints 9 + 12), validate-for-write.
- `legacy_notifications_*.rs` (3 files), `notifications_mod_direct.rs`,
  `webhook_retry.rs`, `webhook_signing.rs` — full notifications
  surface.
- `storage_trait.rs` — minimum-viable contract test.
- `tenancy_subdomain.rs` — multitenant subdomain resolver.
- `parity_close.rs`, `parity_sprint12.rs`, `sprint12_security.rs`,
  `security_primitives_test.rs`, `server_security.rs` — parity / hardening.

`RELEASE_NOTES.md:9-12` claims **702 workspace tests pass** as of
Sprint 12 (2026-05-06). `RELEASE_NOTES.md:79-82` claims **835
workspace tests pass** as of Sprint 11 (2026-04-24). The drop suggests
some Sprint-12 reorganisation; no failures are documented.

### Sibling crate test coverage

- `solid-pod-rs-nostr/tests/`: `relay_nip11.rs` (57 LOC) +
  `resolver_integration.rs` (135 LOC). 45 tests claimed in
  `crates/solid-pod-rs-nostr/README.md:3`.
- `solid-pod-rs-idp/tests/` exists; tests count not documented in
  notes (covered through the core's `oidc_*` integration suite via
  `solid-pod-rs-server` dev-dep in the core's Cargo.toml line 151).
- `solid-pod-rs-didkey`: 29 tests (`RELEASE_NOTES.md:91-94`),
  Ed25519 + P-256 + secp256k1 + alg=none + JWT compact-serialisation.
- `solid-pod-rs-git`: tests directory exists; `with-git-binary`
  feature gates the CGI smoke.
- `solid-pod-rs-server`: test plumbing via `actix-web/dev::test`,
  exercised through the core crate's `tests/server_routes_jss.rs` etc.

### Cross-runtime gaps

There are **no tests pinned to a `wasm32-unknown-unknown` target**.
The only "wasm-shaped" code (`dispatch_message` in
`solid-pod-rs-nostr/src/ws.rs:111`) is exercised via Tokio duplex
streams in a `tokio::test` (line 316-363) — proving the wire
protocol works, but not that the parse/dispatch path is
allocator-only and `?Send`-friendly.

---

## 13. What forum / agentbox / visionclaw EACH currently use vs. reimplement

### visionclaw (this repo, the Rust substrate)

`Cargo.toml:24-26`:
```
solid-pod-rs = { version = "0.4.0-alpha.1", features = ["fs-backend",
    "memory-backend", "nip98-schnorr", "security-primitives"] }
```

That's the **published** 0.4.0-alpha.1, **not** the working tree.
`Cargo.lock` (lines 3905, 3948-3964) likewise pins `0.4.0-alpha.1`
fetched from `crates.io`. So visionclaw still consumes the older
published surface, missing every Sprint-9-through-12 enhancement
landed locally in `solid-pod-rs/`.

Used directly (`src/handlers/solid_pod_handler.rs:1-310`):
- `solid_pod_rs::auth::nip98` (line 16)
- `solid_pod_rs::ldp` (line 17)
- `solid_pod_rs::storage::fs::FsBackend` (line 18) +
  `solid_pod_rs::storage::Storage` (line 19)
- `solid_pod_rs::wac::{ ... AccessMode, parse_jsonld_acl,
  parse_turtle_acl, …}` (lines 20-23)
- `solid_pod_rs::PodError` (line 24)
- `solid_pod_rs::ResourceMeta` (line 309)

The handler stamps `wac::AccessMode` based on HTTP method
(line 58), runs NIP-98 verify, looks up ACLs, and wraps everything
under VisionClaw's actor mesh. It **does not use** the
`solid-pod-rs-nostr`, `-idp`, `-server`, `-git`, `-didkey`,
`-activitypub` sibling crates at all today. Solid Pod routes are
mounted natively (`src/handlers/mod.rs:71-73`).

Visionclaw also publishes Prometheus counters
`solid_pod_rs_requests_total` and `solid_pod_rs_wac_denied_total`
(`src/services/metrics.rs:130-131,325-330`) — the only externally
visible signal of solid-pod-rs activity.

### agentbox

`agentbox/lib/solid-pod-rs.nix:35-50` ships a Nix derivation for the
`solid-pod-rs-server` binary, **pinned to git rev `7f8bc89`**
(line 47), labelled `version = "0.4.0-alpha.1+sprint-9"` (line 41).
`agentbox/lib/solid-pod-rs.cargo-lock` carries this Sprint-9 lockfile
unchanged.

The derivation builds with cargo features `fs-backend`,
`nip98-schnorr`, `security-primitives` (per the comment block at
`agentbox/lib/solid-pod-rs.nix:21-25`). Optional features deferred:
`oidc`, `dpop-replay-cache`, `s3-backend`, `legacy-notifications`
(lines 27-30).

Agentbox runs solid-pod-rs as a **standalone supervisord program,
never linked as a library** (line 31 of the same file). On the port
table in this repo's `CLAUDE.md` it surfaces as port `8484`.

There is **no Rust code in agentbox** that consumes the solid-pod-rs
crate directly — agentbox's management API is in JavaScript
(`agentbox/management-api/lib/uris.js`), and the only mention of
solid-pod-rs in JS land is in the URI minter's docstring
(`management-api/lib/uris.js:66`, comparing the URN minting pattern).

### forum (community-forum-rs)

`dreamlab-ai-website/community-forum-rs/Cargo.toml:81-82`:
```
solid-pod-rs = { version = "0.4.0-alpha.2", default-features = false,
    features = ["nip98-schnorr"] }
solid-pod-rs-nostr = { version = "0.4.0-alpha.2", default-features = false }
```

Forum pulls **only** the published `nip98-schnorr` feature off the
core (no FS, no memory, no notifications, no WAC modules linked at
runtime). And it pulls solid-pod-rs-nostr at default-feature-stripped
level. Then it **re-implements 4,200+ LOC of equivalent surface** in
`pod-worker` — see §8's mapping table — because:

1. The `Storage` trait is Tokio-coupled (§2).
2. NIP-98 has no replay-store trait (§10).
3. The WS relay loop is Tokio-only (§11).
4. The `notifications` module uses `tokio::sync::broadcast` (§2 footer).
5. Cloudflare Workers (`wasm32-unknown-unknown`) require `?Send`
   futures, `worker::kv::KvStore` for state, `worker::WebSocket`
   for sockets — none of which solid-pod-rs surfaces accommodate.

Forum's `pod-worker/src/auth.rs:11-46` is the reference implementation
of how the replay-store trait *should* look — Cloudflare KV-backed,
TTL-bounded, atomic insert-or-detect.

---

## 14. Currently-published crate versions vs. source tree

Cross-referencing the workspace `Cargo.toml`, RELEASE_NOTES, README,
and consumer Cargo.lock entries:

| Crate | Cargo.toml in tree | RELEASE_NOTES claims | crates.io published (consumed by) |
|---|---|---|---|
| `solid-pod-rs` | `0.4.0-alpha.2` | `0.5.0-alpha.2` (Sprint 12) | `0.4.0-alpha.1` (visionclaw, agentbox via git rev `7f8bc89`); `0.4.0-alpha.2` (forum) |
| `solid-pod-rs-server` | `0.4.0-alpha.2` | — | `0.4.0-alpha.1` (agentbox) |
| `solid-pod-rs-nostr` | `0.4.0-alpha.2` | — | `0.4.0-alpha.2` (forum) |
| `solid-pod-rs-idp` | `0.4.0-alpha.2` | — | not consumed externally; agentbox builds its peer crate |
| `solid-pod-rs-didkey` | `0.4.0-alpha.2` | NEW Sprint 11 | unpublished |
| `solid-pod-rs-activitypub` | `0.4.0-alpha.2` | Sprint 12 outbox + accept-negotiation | unpublished |
| `solid-pod-rs-git` | `0.4.0-alpha.2` | — | unpublished |

Discrepancies:

- The workspace `Cargo.toml` ships `0.4.0-alpha.2` while the
  release notes describe an *intended* `v0.5.0-alpha.2`. Either the
  bump is pending or the notes are aspirational; the Cargo.lock
  consumed by `community-forum-rs` (which has `0.4.0-alpha.2`)
  matches the tree, so a `0.4.0-alpha.2` was published at some point.
- Visionclaw and agentbox are **two minor versions behind**
  (`0.4.0-alpha.1` vs. workspace `0.4.0-alpha.2`, plus all the
  Sprint-9-through-12 unreleased work).
- Sprint-9-through-12 features (`acl-origin`, `dpop-replay-cache`,
  `webhook-signing`, `did-nostr`, `rate-limit`, `quota`,
  `config-loader`, the `CidVerifier`/`SelfSignedVerifier` trait,
  `solid-pod-rs-didkey`, the WAC 2.0 conditions framework) are **not
  yet observable to visionclaw or agentbox** through their pinned
  versions until a republish lands.

---

## 15. Capability gaps for cross-system DM / federation messaging

The brief asks specifically about LDN-bridge to nostr DMs and
cross-system DM federation. Findings:

### What exists

- `solid-pod-rs::notifications` (mod.rs:1-908, signing.rs:1-428,
  legacy.rs:1-1029) ships **Solid Notifications 0.2** —
  WebSocketChannel2023 (`mod.rs:177`), WebhookChannel2023
  (`mod.rs:317`), RFC 9421 webhook signing
  (`mod.rs:375`, `signing.rs`), JSS `solid-0.1` legacy adapter
  (`legacy.rs:728`).
- `solid-pod-rs-activitypub` ships **ActivityPub federation** (Sprint
  10/12): inbox, outbox, delivery worker, HTTP Signatures, actor
  cache (per `RELEASE_NOTES.md:43-58`). Outbox auto-wraps Notes in
  `Create` activities; delivery fans out to follower inboxes.
- `solid-pod-rs-nostr::relay` ships an **embedded NIP-01 relay**
  (`relay.rs`) — but only for *replicating arbitrary Nostr events*,
  not as a DM transport.

### What is missing (gap inventory)

1. **No NIP-04 / NIP-44 / NIP-17 module.** `grep` across
   `crates/solid-pod-rs*/src/` finds zero references to `nip04`,
   `nip44`, `nip17`, `gift_wrap`, `kind4`/`kind:4`, `kind14`/`kind:14`,
   "DM", "direct message". Forum's `nostr-core` ships these at
   `community-forum-rs/crates/nostr-core/src/{nip04.rs,nip44.rs,gift_wrap.rs}`,
   solid-pod-rs does not.

2. **NIP-11 advertises only NIPs 1, 11, 16** (`relay.rs:330`). A
   relay built on this crate cannot truthfully advertise DM
   support, gift-wrap support, or NIP-09 deletion semantics (even
   though `EventStore::remove` exists at `relay.rs:226`, the kind-5
   delete event is not dispatched on by `Relay::ingest`).

3. **No durable EventStore beyond `InMemoryEventStore`.** Cargo
   features for SQLite or postgres adapters are not declared
   (`solid-pod-rs-nostr/Cargo.toml`); the `EventStore` trait is the
   sole extension point, and the trait is `Send + Sync` non-async,
   so async-DB adapters need to bridge through `tokio::task::block_in_place`
   or similar. This was an explicit non-goal at the Sprint-9
   landing per `crates/solid-pod-rs-nostr/README.md:30-32`.

4. **No notifications↔nostr bridge / LDN bridge.** Searching for
   "LDN", "Linked Data Notifications", "ChangeNotification → nostr"
   in the crates returns nothing. The current path between `Storage`
   events and Nostr events is two disjoint pipelines:
   - `Storage::watch` → `WebSocketChannelManager::pump_from_storage`
     → AS2 JSON-LD over WebSocket / Webhook (Solid Notifications).
   - `Relay::ingest` → broadcast::Sender<Event> → connected
     subscribers (NIP-01).
   Nothing wires `StorageEvent` into a `kind:1`/`kind:30023` Nostr
   event for federation, and nothing bridges an inbound
   `kind:14`/`kind:1059` DM into the LDP `inbox/` container.

5. **No DID resolver hop via relay query.** Per §3, the resolver
   only fetches `.well-known/did/nostr/<hex>.json` over HTTPS; it
   cannot query a relay for the user's `kind:0` profile event to
   read `alsoKnownAs` from `content.tags`. For a hex-pubkey →
   Tier-3 DID-doc round-trip with no DNS, the resolver would have
   to grow a `relay_url: Option<&str>` parameter, build a `Filter
   { authors: vec![hex], kinds: vec![0], limit: 1 }` query, and
   parse the resulting event. That code does not exist.

6. **No NIP-65 outbox/inbox relay-list.** Without
   `kind:10002` parsing, federated systems (forum, visionclaw,
   agentbox) cannot discover *which* relay holds a given user's
   profile/messages — they must be configured manually or fall back
   to HTTPS DID-doc lookup.

7. **No NIP-26 delegation, no NIP-90 DVM, no calendar (NIP-52).**
   Forum's `nostr-core` ships all three. solid-pod-rs sees Nostr as
   "auth + relay-replication" only.

8. **NIP-98 has no replay-store trait** (§10) — so even if a DM
   bridge layered NIP-98 on top of inbox POSTs, the receiver
   couldn't reject a re-played authorisation header.

### Concrete federation-ready surface required

To support cross-system DMs (forum ↔ visionclaw ↔ agentbox over
nostr or LDN):

a. `solid-pod-rs::auth::nip98::Nip98ReplayStore` trait + a
   `verify_at_with_replay()` overload (§10).
b. `solid-pod-rs-nostr::nip04` / `nip17` / `nip44` / `gift_wrap`
   modules (or a new sibling crate `solid-pod-rs-nostr-msg`).
c. `solid-pod-rs-nostr::resolver::resolve_via_relay(pubkey,
   relay_url)` (Tier-3 DID doc from `kind:0`).
d. A `RelayInfo` builder that lets consumers append NIPs to the
   advertised set without reaching into the struct.
e. A `WireSink + WireStream` runtime-agnostic pair behind the
   current Tokio `serve_relay_ws` pump, so WASM consumers can drive
   the relay without re-implementing the loop.
f. A `notifications::nostr_bridge` module that maps
   `ChangeNotification` ↔ `Event { kind: 30023 }` (for changefeed
   federation) and `Event { kind: 14 }` ↔ inbox POST (for DMs into
   LDP `/inbox/`).
g. A versioned `Storage` trait (or a runtime-agnostic
   `KvBackend` trait below it) that drops the
   `tokio::sync::mpsc::Receiver` in `watch()` for a `futures-core::Stream`
   and replaces `Send + Sync + 'static` with a feature-gated
   `MaybeSend` so `wasm32-unknown-unknown` consumers can implement
   `Storage` against Cloudflare R2/KV directly. This is the
   **single highest-leverage change**: it would let forum's
   pod-worker collapse 2,300+ LOC of reimplementation back to
   thin wrappers.

The surface is otherwise mature, well-tested, and protocol-correct —
the gap is in *runtime ergonomics for consumers outside the
Tokio/actix-web shape that solid-pod-rs was originally cut for*.

---

## Reference index of file paths

Source tree (`/home/devuser/workspace/project/solid-pod-rs/`):

- Workspace manifest: `Cargo.toml`
- Workspace lockfile: `Cargo.lock` (148 KB, 1.17M tokens of
  resolved deps)
- Release notes: `RELEASE_NOTES.md`, top-of-file = Sprint 12 close
- Workspace README: `README.md`
- Crate-local CLAUDE.md: `CLAUDE.md` (Agentic QE policies)

Per-crate `src/` roots:

- `crates/solid-pod-rs/src/` — lib (152 LOC), auth/ (3 files),
  config/ (3 files), handlers/ (2 files, gated), notifications/ (3
  files), oidc/ (3 files, gated), security/ (4 files), storage/ (3
  files), wac/ (9 files), and top-level error.rs (101), interop.rs
  (541), ldp.rs (2433), metrics.rs (80), multitenant.rs (317),
  provision.rs (548), webid.rs (391), quota.rs.
- `crates/solid-pod-rs-server/src/` — lib.rs (1283), main.rs, cli/.
- `crates/solid-pod-rs-nostr/src/` — lib.rs (74), did.rs (324),
  error.rs (59), relay.rs (734), resolver.rs (431), ws.rs (364);
  total 2178 LOC, 47 tests in source.
- `crates/solid-pod-rs-idp/src/` — lib.rs (105), provider.rs,
  schnorr.rs (322), credentials.rs, discovery.rs, error.rs,
  invites.rs, jwks.rs, registration.rs, session.rs, tokens.rs,
  user_store.rs, axum_binder.rs (gated), passkey.rs (gated).
- `crates/solid-pod-rs-didkey/src/` — lib.rs, did.rs, error.rs,
  jwt.rs, pubkey.rs, verifier.rs (105 LOC for `verifier.rs`).
- `crates/solid-pod-rs-git/src/` — lib.rs (61), auth.rs (217),
  config.rs, error.rs, guard.rs, service.rs.
- `crates/solid-pod-rs-activitypub/src/` — lib.rs, actor.rs,
  delivery.rs, discovery.rs, error.rs, http_sig.rs, inbox.rs,
  outbox.rs, store.rs.

Consumer paths cited:

- `/home/devuser/workspace/project/Cargo.toml:24-26` — visionclaw
  pin.
- `/home/devuser/workspace/project/src/handlers/solid_pod_handler.rs:1-310`
  — visionclaw consumer code.
- `/home/devuser/workspace/project/src/services/metrics.rs:129-131`
  — visionclaw Prometheus counters.
- `/home/devuser/workspace/project/agentbox/lib/solid-pod-rs.nix:35-65`
  — agentbox Nix derivation pin.
- `/home/devuser/workspace/project/agentbox/lib/solid-pod-rs.cargo-lock`
  — agentbox vendored lockfile (`solid-pod-rs = 0.4.0-alpha.1`).
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/Cargo.toml:81-82`
  — forum pins.
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/{auth,acl,did,webid,provision,quota,notifications,patch,container,content_negotiation,conditional}.rs`
  — forum re-implementations.
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip98.rs:45`
  — forum's `Nip98ReplayStore` trait (the missing piece in
  solid-pod-rs core).
