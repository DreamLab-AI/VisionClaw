# Public Rust API reference

Canonical reference for the `solid_pod_rs` crate. Every public type,
trait, and function exported from `lib.rs` is listed. Private items
are not documented here.

- Crate root: [`src/lib.rs`](../../src/lib.rs)
- Crate version: `0.2.0-alpha.1`

## Modules

| Module | Purpose |
|---|---|
| [`auth`](#auth)                | HTTP authentication primitives (NIP-98). |
| [`error`](#error)              | Crate-wide error type. |
| [`ldp`](#ldp)                  | LDP container/resource semantics, PATCH, content negotiation. |
| [`notifications`](#notifications) | Solid Notifications 0.2 channels (WebSocket + Webhook). |
| [`oidc`](#oidc-types) (feature-gated) | Solid-OIDC server-side primitives. |
| [`storage`](#storage-trait)    | `Storage` trait and built-in backends. |
| [`wac`](#wac)                  | Web Access Control evaluator. |
| [`webid`](#webid)              | WebID profile generation and validation. |

Re-exported top-level items (for ergonomic callers):

```rust
pub use error::PodError;
pub use storage::{ResourceMeta, Storage, StorageEvent};
pub use wac::{
    evaluate_access, evaluate_access_with_groups, method_to_mode,
    mode_name, wac_allow_header, AccessMode, AclDocument,
    GroupMembership, StaticGroupMembership,
};
pub use ldp::{
    apply_n3_patch, apply_sparql_patch, link_headers, negotiate_format,
    patch_dialect_from_mime, server_managed_triples,
    ContainerRepresentation, Graph, PatchDialect, PatchOutcome,
    PreferHeader, RdfFormat, Term, Triple, ACCEPT_POST,
};
```

---

## `error`

### `PodError`

Crate-wide `thiserror`-derived enum. Every public API returns
`Result<T, PodError>`.

| Variant | Message | Typical HTTP mapping |
|---|---|---|
| `NotFound(String)`        | `"resource not found: {}"`    | 404 |
| `AlreadyExists(String)`   | `"resource already exists: {}"`| 409 |
| `Forbidden`               | `"access forbidden"`           | 403 |
| `Unauthenticated`         | `"authentication required"`    | 401 |
| `Io(std::io::Error)`      | `"I/O error: {}"`              | 500 |
| `InvalidPath(String)`     | `"invalid path: {}"`           | 400 |
| `InvalidContentType(String)` | `"invalid content type: {}"` | 415 |
| `Json(serde_json::Error)` | `"JSON error: {}"`             | 400 |
| `UrlParse(url::ParseError)` | `"URL parse error: {}"`      | 400 |
| `Base64(base64::DecodeError)` | `"base64 decode error: {}"` | 400 |
| `Hex(hex::FromHexError)`  | `"hex decode error: {}"`       | 400 |
| `AclParse(String)`        | `"ACL parse error: {}"`        | 500 |
| `Nip98(String)`           | `"NIP-98: {}"`                 | 401 |
| `Watch(String)`           | `"watch subsystem error: {}"`  | 500 |
| `Backend(String)`         | `"backend error: {}"`          | 500 |
| `PreconditionFailed(String)` | `"precondition failed: {}"` | 412 |
| `Unsupported(String)`     | `"unsupported: {}"`            | 400 or 415 |

See [reference/error-codes.md](error-codes.md) for the full HTTP-code
mapping.

---

## `storage`

### `Storage` trait

```rust
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    async fn get   (&self, path: &str)                             -> Result<(Bytes, ResourceMeta), PodError>;
    async fn put   (&self, path: &str, body: Bytes, ct: &str)      -> Result<ResourceMeta, PodError>;
    async fn delete(&self, path: &str)                             -> Result<(), PodError>;
    async fn list  (&self, container: &str)                        -> Result<Vec<String>, PodError>;
    async fn head  (&self, path: &str)                             -> Result<ResourceMeta, PodError>;
    async fn exists(&self, path: &str)                             -> Result<bool, PodError>;
    async fn watch (&self, path: &str)
        -> Result<tokio::sync::mpsc::Receiver<StorageEvent>, PodError>;
}
```

#### `Storage::get`

Returns body + metadata. On miss returns `PodError::NotFound(path)`.

#### `Storage::put`

Create-or-replace. Returns the new metadata (including computed
ETag). Emits `StorageEvent::Created` on a new path and
`StorageEvent::Updated` on an existing path.

#### `Storage::delete`

Removes the resource and its metadata. Emits
`StorageEvent::Deleted`. 404 on missing paths.

#### `Storage::list`

Returns direct children of `container`. Sub-containers carry a
trailing `/`; resources do not. Paths are relative to the container.

#### `Storage::head` / `exists`

Cheap metadata-only variants.

#### `Storage::watch`

Returns an `mpsc::Receiver<StorageEvent>` that emits changes at or
under `path`. Dropping the receiver detaches the watcher.

### `ResourceMeta`

```rust
pub struct ResourceMeta {
    pub etag: String,
    pub modified: chrono::DateTime<chrono::Utc>,
    pub size: u64,
    pub content_type: String,
    pub links: Vec<String>,   // pre-composed Link header values
}
```

Constructor:

```rust
pub fn new(etag: impl Into<String>, size: u64, content_type: impl Into<String>) -> Self;
```

### `StorageEvent`

```rust
pub enum StorageEvent {
    Created(String),
    Updated(String),
    Deleted(String),
}
```

### Built-in backends

| Type | Feature flag | Path |
|---|---|---|
| `storage::memory::MemoryBackend` | `memory-backend` (default) | `src/storage/memory.rs` |
| `storage::fs::FsBackend`         | `fs-backend` (default)     | `src/storage/fs.rs` |
| `storage::s3::S3Backend`         | `s3-backend` (opt-in)      | planned P2 |

Both shipped backends pass `tests/storage_trait.rs`.

#### `MemoryBackend::new`

No-arg constructor. Uses `Arc<RwLock<HashMap<String, Entry>>>` +
`broadcast::channel(256)`.

#### `FsBackend::new`

```rust
pub async fn new(root: impl Into<PathBuf>) -> Result<Self, PodError>;
pub fn root(&self) -> &Path;
```

Creates the root directory if missing. Writes a `.meta.json` sidecar
alongside each body file. Uses `notify` for filesystem change events.

---

## `wac`

### Types

```rust
pub enum AccessMode { Read, Write, Append, Control }
pub const ALL_MODES: &[AccessMode]; // [Read, Write, Append, Control]

pub struct AclDocument {
    pub context: Option<serde_json::Value>,
    pub graph:   Option<Vec<AclAuthorization>>,
}

pub struct AclAuthorization {
    pub agent:       Option<IdOrIds>,
    pub agent_class: Option<IdOrIds>,
    pub agent_group: Option<IdOrIds>,
    pub origin:      Option<IdOrIds>,
    pub access_to:   Option<IdOrIds>,
    pub default:     Option<IdOrIds>,
    pub mode:        Option<IdOrIds>,
    // plus id, type (opaque)
}

pub enum IdOrIds { Single(IdRef), Multiple(Vec<IdRef>) }
pub struct IdRef { pub id: String }
```

### Evaluation

```rust
pub fn evaluate_access(
    acl_doc:       Option<&AclDocument>,
    agent_uri:     Option<&str>,
    resource_path: &str,
    required_mode: AccessMode,
) -> bool;

pub fn evaluate_access_with_groups(
    acl_doc:       Option<&AclDocument>,
    agent_uri:     Option<&str>,
    resource_path: &str,
    required_mode: AccessMode,
    groups:        &dyn GroupMembership,
) -> bool;
```

Returns `true` iff at least one authorization in the document grants
the requested mode to the requesting agent on the given path.

### Helpers

```rust
pub fn method_to_mode(method: &str) -> AccessMode;
// GET/HEAD → Read; PUT/DELETE/PATCH → Write; POST → Append; other → Read

pub fn mode_name(mode: AccessMode) -> &'static str;
// Read → "read", Write → "write", Append → "append", Control → "control"

pub fn wac_allow_header(
    acl_doc:       Option<&AclDocument>,
    agent_uri:     Option<&str>,
    resource_path: &str,
) -> String;
// Returns `user="..." public="..."` per WAC spec.
```

### Group membership

```rust
pub trait GroupMembership {
    fn is_member(&self, group_iri: &str, agent_uri: &str) -> bool;
}

pub struct StaticGroupMembership {
    pub groups: HashMap<String, Vec<String>>,
}
impl StaticGroupMembership {
    pub fn new() -> Self;
    pub fn add(&mut self, group_iri: impl Into<String>, members: Vec<String>);
}
```

Consumers who resolve groups over HTTP implement `GroupMembership`
themselves.

### `AclResolver` trait

```rust
#[async_trait]
pub trait AclResolver: Send + Sync {
    async fn find_effective_acl(
        &self,
        resource_path: &str,
    ) -> Result<Option<AclDocument>, PodError>;
}
```

### `StorageAclResolver`

```rust
pub struct StorageAclResolver<S: Storage> { /* ... */ }
impl<S: Storage> StorageAclResolver<S> {
    pub fn new(storage: Arc<S>) -> Self;
}
```

Walks up from `resource_path` looking for `.acl` sidecars. First hit
wins. Returns `Ok(None)` if the walk completes without finding one.

---

## `ldp`

### Container & ACL helpers

```rust
pub fn is_container(path: &str) -> bool;
pub fn is_acl_path(path: &str) -> bool;
pub fn is_meta_path(path: &str) -> bool;
pub fn meta_sidecar_for(path: &str) -> String;
pub fn link_headers(path: &str) -> Vec<String>;
pub fn resolve_slug(container: &str, slug: Option<&str>) -> String;
pub const ACCEPT_POST: &str; // "text/turtle, application/ld+json, application/n-triples"
```

See [reference/link-headers.md](link-headers.md) for `link_headers`
output semantics.

### Prefer header

```rust
pub enum ContainerRepresentation { Full, MinimalContainer, ContainedIRIsOnly }
pub struct PreferHeader {
    pub representation:         ContainerRepresentation,
    pub include_minimal:        bool,
    pub include_contained_iris: bool,
    pub omit_membership:        bool,
}
impl PreferHeader {
    pub fn parse(value: &str) -> Self;
}
```

See [reference/prefer-headers.md](prefer-headers.md).

### RDF format / content negotiation

```rust
pub enum RdfFormat { Turtle, JsonLd, NTriples, RdfXml }
impl RdfFormat {
    pub fn mime(&self) -> &'static str;
    pub fn from_mime(mime: &str) -> Option<Self>;
}
pub fn negotiate_format(accept: Option<&str>) -> RdfFormat;
```

See [reference/content-types.md](content-types.md).

### RDF primitives

```rust
pub enum Term {
    Iri(String),
    BlankNode(String),
    Literal { value: String, datatype: Option<String>, language: Option<String> },
}
impl Term {
    pub fn iri(i: impl Into<String>) -> Self;
    pub fn blank(b: impl Into<String>) -> Self;
    pub fn literal(v: impl Into<String>) -> Self;
    pub fn typed_literal(v: impl Into<String>, dt: impl Into<String>) -> Self;
}

pub struct Triple { pub subject: Term, pub predicate: Term, pub object: Term }
impl Triple {
    pub fn new(subject: Term, predicate: Term, object: Term) -> Self;
}

pub struct Graph { /* BTreeSet<Triple> */ }
impl Graph {
    pub fn new() -> Self;
    pub fn from_triples(it: impl IntoIterator<Item = Triple>) -> Self;
    pub fn insert(&mut self, triple: Triple);
    pub fn remove(&mut self, triple: &Triple) -> bool;
    pub fn contains(&self, triple: &Triple) -> bool;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn triples(&self) -> impl Iterator<Item = &Triple>;
    pub fn extend(&mut self, other: &Graph);
    pub fn subtract(&mut self, other: &Graph);
    pub fn to_ntriples(&self) -> String;
    pub fn parse_ntriples(input: &str) -> Result<Self, PodError>;
}
```

### Server-managed triples

```rust
pub fn server_managed_triples(
    resource_iri:   &str,
    modified:       chrono::DateTime<chrono::Utc>,
    size:           u64,
    is_container:   bool,
    contained:      &[String],
) -> Graph;

pub const SERVER_MANAGED_PREDICATES: &[&str];  // dc:modified, stat:size, stat:mtime, ldp:contains
pub fn find_illegal_server_managed(graph: &Graph) -> Vec<Triple>;
```

### Container rendering

```rust
pub struct ContainerMember { pub id: String, pub types: Vec<&'static str> }

pub fn render_container_jsonld(
    container_path: &str,
    members:        &[String],
    prefer:         PreferHeader,
) -> serde_json::Value;

pub fn render_container(container_path: &str, members: &[String]) -> serde_json::Value;

pub fn render_container_turtle(
    container_path: &str,
    members:        &[String],
    prefer:         PreferHeader,
) -> String;
```

### PATCH

```rust
pub enum PatchDialect { N3, SparqlUpdate }
pub fn patch_dialect_from_mime(mime: &str) -> Option<PatchDialect>;

pub struct PatchOutcome {
    pub graph:    Graph,
    pub inserted: usize,
    pub deleted:  usize,
}

pub fn apply_n3_patch    (target: Graph, patch:  &str) -> Result<PatchOutcome, PodError>;
pub fn apply_sparql_patch(target: Graph, update: &str) -> Result<PatchOutcome, PodError>;
```

See [reference/patch-semantics.md](patch-semantics.md).

### `LdpContainerOps` trait

```rust
#[async_trait]
pub trait LdpContainerOps: Storage {
    async fn container_representation(&self, path: &str) -> Result<serde_json::Value, PodError>;
}
impl<T: Storage + ?Sized> LdpContainerOps for T {}
```

Blanket implementation: any `Storage` implementor can call
`.container_representation(path)` to render a JSON-LD container view
using default `PreferHeader`.

### IRIs

```rust
pub mod iri {
    pub const LDP_RESOURCE:        &str;
    pub const LDP_CONTAINER:       &str;
    pub const LDP_BASIC_CONTAINER: &str;
    pub const LDP_CONTAINS:        &str;
    pub const LDP_PREFER_MINIMAL_CONTAINER: &str;
    pub const LDP_PREFER_CONTAINED_IRIS:    &str;
    pub const LDP_PREFER_MEMBERSHIP:        &str;
    pub const DCTERMS_MODIFIED:    &str;
    pub const STAT_SIZE:           &str;
    pub const STAT_MTIME:          &str;
    pub const XSD_DATETIME:        &str;
    pub const XSD_INTEGER:         &str;
    pub const PIM_STORAGE:         &str;
    pub const PIM_STORAGE_REL:     &str;
    pub const ACL_NS:              &str;
    // plus namespaces
}
```

---

## `auth`

### `auth::nip98`

```rust
pub const NOSTR_PREFIX:       &str = "Nostr ";
pub const HTTP_AUTH_KIND:     u64  = 27235;
pub const TIMESTAMP_TOLERANCE: u64  = 60;
pub const MAX_EVENT_SIZE:      usize = 64 * 1024;

pub struct Nip98Event {
    pub id:         String,
    pub pubkey:     String,
    pub created_at: u64,
    pub kind:       u64,
    pub tags:       Vec<Vec<String>>,
    pub content:    String,
    pub sig:        String,
}

pub struct Nip98Verified {
    pub pubkey:       String,
    pub url:          String,
    pub method:       String,
    pub payload_hash: Option<String>,
    pub created_at:   u64,
}

pub async fn verify(
    header:    &str,
    url:       &str,
    method:    &str,
    body_hash: Option<&[u8]>,
) -> Result<String, PodError>;

pub fn verify_at(
    header:         &str,
    expected_url:   &str,
    expected_method: &str,
    body:           Option<&[u8]>,
    now:            u64,
) -> Result<Nip98Verified, PodError>;

pub fn authorization_header(token_b64: &str) -> String;
```

`verify` returns the signer pubkey; `verify_at` returns the structured
`Nip98Verified`.

---

## `webid`

```rust
pub fn generate_webid_html(pubkey: &str, name: Option<&str>, pod_base: &str) -> String;
pub fn validate_webid_html(data: &[u8]) -> Result<(), String>;
```

`generate_webid_html` emits HTML+JSON-LD with `foaf:Person`,
`solid:account`, `solid:privateTypeIndex`, `solid:publicTypeIndex`,
and `schema:identifier` (= `did:nostr:{pubkey}`).

---

## `notifications`

```rust
pub mod as_ns {
    pub const CONTEXT: &str = "https://www.w3.org/ns/activitystreams";
    pub const CREATE:  &str = "Create";
    pub const UPDATE:  &str = "Update";
    pub const DELETE:  &str = "Delete";
}

pub enum ChannelType { WebSocketChannel2023, WebhookChannel2023 }

pub struct Subscription {
    pub id:           String,
    pub topic:        String,
    pub channel_type: ChannelType,
    pub receive_from: String,
}

pub struct ChangeNotification {
    pub context:   String,  // "@context"
    pub id:        String,
    pub kind:      String,  // "type"
    pub object:    String,
    pub published: String,
}
impl ChangeNotification {
    pub fn from_storage_event(event: &StorageEvent, pod_base: &str) -> Self;
}

#[async_trait]
pub trait Notifications: Send + Sync {
    async fn subscribe  (&self, subscription: Subscription) -> Result<(), PodError>;
    async fn unsubscribe(&self, id: &str)                    -> Result<(), PodError>;
    async fn publish    (&self, topic: &str, note: ChangeNotification) -> Result<(), PodError>;
}

pub struct InMemoryNotifications { /* ... */ }
impl InMemoryNotifications { pub fn new() -> Self; }

pub enum WebhookDelivery {
    Delivered      { status: u16 },
    FatalDrop      { status: u16 },
    TransientRetry { reason: String },
}

pub struct WebhookChannelManager {
    pub retry_base:  Duration,    // 500ms
    pub max_retries: u32,         // 3
    /* ... */
}
impl WebhookChannelManager {
    pub fn new() -> Self;
    pub fn with_client(client: reqwest::Client) -> Self;
    pub async fn subscribe(&self, topic: &str, target_url: &str) -> Subscription;
    pub async fn unsubscribe(&self, id: &str);
    pub async fn active_subscriptions(&self) -> usize;
    pub async fn deliver_one(&self, url: &str, note: &ChangeNotification) -> WebhookDelivery;
    pub async fn deliver_all(
        &self,
        note: &ChangeNotification,
        topic_matches: impl Fn(&str) -> bool,
    ) -> Vec<(String, WebhookDelivery)>;
    pub async fn pump_from_storage(
        self,
        rx: mpsc::Receiver<StorageEvent>,
        pod_base: String,
    );
}

pub struct WebSocketChannelManager { /* ... */ }
impl WebSocketChannelManager {
    pub fn new() -> Self;
    pub fn with_heartbeat(self, interval: Duration) -> Self;
    pub fn heartbeat_interval(&self) -> Duration;
    pub async fn subscribe(&self, topic: &str, base_url: &str) -> Subscription;
    pub async fn unsubscribe(&self, id: &str);
    pub fn stream(&self) -> broadcast::Receiver<ChangeNotification>;
    pub async fn active_subscriptions(&self) -> usize;
    pub async fn pump_from_storage(
        self,
        rx: mpsc::Receiver<StorageEvent>,
        pod_base: String,
    );
}

pub fn discovery_document(pod_base: &str) -> serde_json::Value;
```

---

## `oidc` (feature: `oidc`)

### Client registration

```rust
pub struct ClientRegistrationRequest { /* RFC 7591 fields */ }
pub struct ClientRegistrationResponse { /* RFC 7591 fields */ }
pub fn register_client(req: &ClientRegistrationRequest, now: u64) -> ClientRegistrationResponse;
```

### Discovery

```rust
pub struct DiscoveryDocument { /* all standard endpoints */ }
pub fn discovery_for(issuer: &str) -> DiscoveryDocument;
```

### JWK + thumbprint

```rust
pub struct Jwk {
    pub kty: String,
    pub alg: Option<String>,
    pub kid: Option<String>,
    pub use_: Option<String>,
    pub crv: Option<String>, pub x: Option<String>, pub y: Option<String>,
    pub n:   Option<String>, pub e: Option<String>,
    pub k:   Option<String>,
}
impl Jwk {
    pub fn thumbprint(&self) -> Result<String, PodError>;  // RFC 7638 SHA-256
}
```

### DPoP

```rust
pub struct DpopHeader { pub typ: String, pub alg: String, pub jwk: Jwk }
pub struct DpopClaims { pub htu: String, pub htm: String, pub iat: u64, pub jti: String, pub ath: Option<String> }
pub struct DpopVerified { pub jkt: String, pub htm: String, pub htu: String, pub iat: u64, pub jti: String }

pub fn verify_dpop_proof(
    proof:         &str,
    expected_htu:  &str,
    expected_htm:  &str,
    now:           u64,
    skew:          u64,
) -> Result<DpopVerified, PodError>;
```

### Access-token verification

```rust
pub struct SolidOidcClaims {
    pub iss:       String,
    pub sub:       String,
    pub aud:       serde_json::Value,
    pub exp:       u64,
    pub iat:       u64,
    pub webid:     Option<String>,
    pub client_id: Option<String>,
    pub cnf:       Option<CnfClaim>,
    pub scope:     Option<String>,
}
pub struct CnfClaim { pub jkt: String }

pub struct AccessTokenVerified {
    pub webid:     String,
    pub client_id: Option<String>,
    pub iss:       String,
    pub jkt:       String,
    pub scope:     Option<String>,
    pub exp:       u64,
}

pub fn verify_access_token(
    token:           &str,
    secret:          &[u8],
    expected_issuer: &str,
    dpop_jkt:        &str,
    now:             u64,
) -> Result<AccessTokenVerified, PodError>;

pub fn extract_webid(claims: &SolidOidcClaims) -> Result<String, PodError>;
```

### Introspection

```rust
pub struct IntrospectionResponse {
    pub active:    bool,
    pub webid:     Option<String>,
    pub client_id: Option<String>,
    pub exp:       Option<u64>,
    pub iss:       Option<String>,
    pub scope:     Option<String>,
    pub cnf:       Option<CnfClaim>,
}
impl IntrospectionResponse {
    pub fn from_verified(v: &AccessTokenVerified) -> Self;
    pub fn inactive() -> Self;
}
```

---

## Feature flags

| Feature | Default | Pulls in | Exposes |
|---|---|---|---|
| `memory-backend` | yes | — | `storage::memory::MemoryBackend` |
| `fs-backend`     | yes | `notify` | `storage::fs::FsBackend` |
| `s3-backend`     | no  | `aws-sdk-s3` | `storage::s3::S3Backend` (P2 impl) |
| `oidc`           | no  | `openidconnect`, `jsonwebtoken` | `oidc::*` |

## Crate-level attributes

- `#![deny(unsafe_code)]` — no `unsafe` anywhere in the crate.
- `#![warn(rust_2018_idioms)]` — strict idiom linting.
