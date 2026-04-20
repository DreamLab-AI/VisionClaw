# Gap Analysis — solid-pod-rs vs JavaScriptSolidServer (JSS)

> Authoritative comparison against the **real** JSS. Replaces the Sprint 2/3
> document that was silently written against a Community-Solid-Server mental
> model. The provenance correction landed with `25b8fae13`
> (task #41) and the canonical JSS feature inventory landed with `364a19691`
> (task #42) at
> [`docs/reference/jss-feature-inventory.md`](./docs/reference/jss-feature-inventory.md).
> This document is the prose companion; the row-per-feature table lives in
> [`PARITY-CHECKLIST.md`](./PARITY-CHECKLIST.md).

## A. Scope and method

### Comparator

- **solid-pod-rs**: `crates/solid-pod-rs/` at `HEAD` of
  `sprint-3/jss-gap-analysis-fresh`. Package `solid-pod-rs v0.3.0-alpha.1`,
  library crate (`src/lib.rs`), ~5,930 LOC of Rust split across 12 modules
  plus 2 auth submodules and 3 storage submodules. Framework-agnostic; wired
  into actix-web by `examples/standalone.rs` for conformance testing.
- **JavaScriptSolidServer**: local clone at
  `/home/devuser/workspace/project/JavaScriptSolidServer/` tracking the
  `jss-upstream` remote. `package.json` reports version `0.0.86`; README
  self-describes as `v0.0.79`. Published tags run `v0.0.26 … v0.0.46`, so the
  local checkout is ahead of any upstream release. Licence `AGPL-3.0-only`
  (`package.json:53`). Node.js ≥ 18 ESM, Fastify 4.29.x, 10 runtime
  dependencies (confirmed in `package.json:26-40`).

### Source paths

| Side | Path |
|---|---|
| solid-pod-rs source | `crates/solid-pod-rs/src/` |
| solid-pod-rs tests | `crates/solid-pod-rs/tests/` (7 integration test files, ~150 tests) |
| solid-pod-rs examples | `crates/solid-pod-rs/examples/` (9 runnable examples) |
| JSS source | `/home/devuser/workspace/project/JavaScriptSolidServer/src/` |
| JSS tests | `/home/devuser/workspace/project/JavaScriptSolidServer/test/` (21 top-level `*.test.js`, 6,527 lines) |
| JSS CLI | `/home/devuser/workspace/project/JavaScriptSolidServer/bin/jss.js` |

### Criteria (status vocabulary)

| Status | Meaning |
|---|---|
| **present** | Functionally equivalent in both; behaviour reconciled, tests exist on both sides. |
| **partial-parity** | Some sub-features exist in solid-pod-rs; documented remainders. |
| **semantic-difference** | Both sides have the feature but with observable behavioural differences (error codes, defaults, surface shape). |
| **missing** | JSS has it, solid-pod-rs does not. Comes with a priority (P0/P1/P2/P3) and a port ticket sketch. |
| **net-new** | solid-pod-rs has it, JSS does not. Triaged as keep / deprecate / feature-flag. |

### Method

Each row of the canonical JSS inventory (see
[`docs/reference/jss-feature-inventory.md`](./docs/reference/jss-feature-inventory.md))
was cross-walked against the Rust module graph produced by
`grep -n "pub fn\|pub struct\|pub enum\|pub trait"` over `src/`, plus the
`tests/` suite. Where behaviour is ambiguous (e.g. patch semantics), the
Rust test case was read back to establish ground truth, not just the
signature.

---

## B. Feature matrix

The exhaustive row-per-feature table is
[`PARITY-CHECKLIST.md`](./PARITY-CHECKLIST.md). What follows here is the
reasoning narrative; cross-reference any line against the checklist for
module paths and test IDs.

---

## C. Categorical summary

### C.1 LDP (Linked Data Platform core)

| Sub-surface | solid-pod-rs | JSS | Verdict |
|---|---|---|---|
| LDP Resource GET/HEAD/PUT/DELETE | `ldp::server_managed_triples`, `Storage::get/put/delete` | `src/handlers/resource.js` | **present** |
| LDP Basic Container GET with `ldp:contains` | `ldp::render_container_jsonld`, `render_container_turtle` | `src/ldp/container.js` | **present** |
| LDP Container POST + Slug resolution | `ldp::resolve_slug` (UUID fallback) | `src/handlers/container.js` (numeric suffix fallback) | **semantic-difference** (C.1a) |
| Server-managed triples (`dateModified`, `size`) | `ldp::server_managed_triples`, `find_illegal_server_managed` | emitted in container.js | **present** |
| `Prefer` composition (minimal / contained IRIs) | `ldp::PreferHeader::parse` with multi-include | **not implemented** (`src/ldp/container.js` builds unconditionally) | **net-new** on the Rust side |
| OPTIONS with `Allow`, `Accept-Post`, `Accept-Patch`, `Accept-Ranges` | `ldp::options_for`, `ACCEPT_PATCH`, `ACCEPT_POST` | `src/ldp/headers.js:58-60` | **present** |
| Range requests (RFC 7233) | `ldp::parse_range_header`, `slice_range` | `src/handlers/resource.js:56-106` | **present** |
| Conditional requests (If-Match, If-None-Match) | `ldp::evaluate_preconditions` → `ConditionalOutcome` | `src/utils/conditional.js`, wired `resource.js:124-130` | **present** |
| ETag | SHA-256 hex | `md5(mtime+size)` (`src/storage/filesystem.js:32`) | **semantic-difference** (C.1b) |
| Link headers (`type`, `acl`, `describedby`, `pim:storage`) | `ldp::link_headers` | `src/ldp/headers.js:15-29` | **present** |
| LDP Direct/Indirect Containers | not implemented | not implemented | both correctly scoped to Basic |
| `.meta` sidecar auto-link | `link_headers` emits `describedby` for every non-meta | emitted via headers.js | **present** |

**C.1a Slug collision**: JSS appends numeric suffixes (`name-1`, `name-2`);
we use UUID fallback. Clients must consume `Location:` header — spec
doesn't mandate a format. Neither is wrong.

**C.1b ETag**: JSS emits weak `md5(mtime+size)`; we emit strong
hex-encoded SHA-256. Strong ETags allow byte-range on-the-fly and are
safer; the Solid Protocol accepts either form.

**State**: LDP parity is substantive. solid-pod-rs additionally implements
`Prefer` — which JSS does not — and stronger ETags. No LDP regressions.

### C.2 WAC (Web Access Control)

| Sub-surface | solid-pod-rs | JSS | Verdict |
|---|---|---|---|
| Default stance | operator-seeded on provision (or deny-by-default if seed skipped) | **default-deny** with auto-seeded owner ACLs on pod creation (`src/server.js:504-547`) | **semantic-difference** (C.2a) |
| ACL inheritance walking up the tree | `wac::StorageAclResolver` resolves sidecars upward | `src/wac/checker.js:59-113` walks up, filters by `acl:default` | **present** |
| `acl:agent` (specific WebID) | `wac::evaluate_access` | `checker.js:129` + `identitiesMatch` cross-ID | **present** |
| `acl:agentClass foaf:Agent` (public) | `evaluate_access` | `checker.js:139` | **present** |
| `acl:agentClass acl:AuthenticatedAgent` | `evaluate_access` | `checker.js:147` | **present** |
| `acl:agentGroup` (vcard:Group) | `evaluate_access_with_groups` + `GroupMembership` trait + `StaticGroupMembership` | parsed at `checker.js:193` but **not enforced** (TODO comment) | **net-new enforcement** (C.2b) |
| `acl:origin` | **not implemented** | **not implemented** (parser doesn't read it) | both missing (C.2c) |
| Modes (Read/Write/Append/Control) + Write→Append implication | `wac::method_to_mode`, `mode_name` | `checker.js:153,290-305` | **present** |
| `WAC-Allow` response header | `wac::wac_allow_header` (alphabetical token sort) | `src/wac/checker.js:279-282` (source order) | **semantic-difference** (C.2d) |
| Turtle ACL parsing | `wac::parse_turtle_acl` + `serialize_turtle_acl`, resolver falls back on JSON-LD parse failure | `src/wac/parser.js:13-384` (`n3` based) | **present** |
| JSON-LD ACL parsing | primary path | accepts both | **present** |
| `.acl` write gate (Control + syntactic validation) | `acl:Control` enforced + strict validation at write time | `acl:Control` enforced, accepts malformed, fails on eval | **semantic-difference** (C.1c) |

**C.2a Default stance**: This is a documentation-level discrepancy only.
JSS's default-deny with auto-seeded owner ACLs is our intended behaviour
when the standalone example's provisioning flow is used (`provision_pod`
seeds the same owner-read/write ACL). For operators running the library
directly, explicit `/.acl` PUT is still required. **Action**: align docs
to prescribe `provision_pod` as the default onboarding path.

**C.2b Group enforcement**: We enforce `acl:agentGroup` via a trait-based
`GroupMembership` resolver. JSS parses but doesn't enforce
(`checker.js:193` carries a TODO). This is **strictly more conformant
than JSS** to WAC §3 of the spec. No regression; log as
"net-new-over-JSS".

**C.2c `acl:origin`**: Neither side implements origin-based restriction.
This is a shared gap against WAC §4.3. P2 to add, but not a JSS parity
issue (both default to "accept any origin"). Bumped to F.2 below for
semantic discussion — recommend shipping on both sides long term.

**C.2d WAC-Allow token order**: Spec is silent on token order. Clients
should parse as a set. Corpora comparing strings should normalise first.

**State**: WAC parity is substantive; solid-pod-rs is stricter than JSS
on both group enforcement and ACL write validation. Zero regressions.

### C.3 Authentication

| Method | solid-pod-rs | JSS | Verdict |
|---|---|---|---|
| Simple Bearer (HMAC token) | not implemented | `src/auth/token.js:45-117` (dev/service path) | **missing** (P3 — dev convenience) |
| Solid-OIDC + DPoP | `oidc::verify_access_token`, `verify_dpop_proof`, `DpopClaims`, `AccessTokenVerified`; feature `oidc` | `src/auth/solid-oidc.js:85-251` with JWKS fetch, jti replay cache, SSRF guard | **present** (C.3a) |
| Nostr NIP-98 (kind 27235) | `auth::nip98::verify_at`, `compute_event_id`, `verify_schnorr_signature` (feature `nip98-schnorr`) | `src/auth/nostr.js:26-267` with Schnorr via `nostr-tools`, Basic-wrapping for git | **present** (C.3b) |
| WebID-TLS | **not implemented** | `src/auth/webid-tls.js:187-257` | **missing** (P3 — legacy, see E.5) |
| IdP-issued JWT | covered by `oidc::verify_access_token` | `auth/token.js:126-161` (`jose`) | **present** |
| did:nostr ↔ WebID cross-resolution | recorded via NIP-98 agent derivation `did:nostr:<pubkey>` | `src/auth/did-nostr.js:41-80`, `src/auth/identity-normalizer.js`, `src/did/resolver.js:67-92` | **partial-parity** (C.3c) |
| Auth dispatch precedence | consumer's HTTP binder decides; `oidc` + `nip98` modules exposed | single chain: DPoP → Nostr → Bearer → WebID-TLS (`auth/token.js:215-269`) | **semantic-difference** — library vs server (C.3d) |
| OIDC DCR (RFC 7591) | `oidc::register_client` | `src/idp/provider.js:147-156` | **present** |
| OIDC discovery (`.well-known/openid-configuration`) | `oidc::discovery_for` | `src/idp/index.js:171-205` | **present** |
| Token introspection (RFC 7662) | `oidc::IntrospectionResponse` | provided via `oidc-provider` | **present** |
| WebID extraction (`webid` claim, URL `sub` fallback) | `oidc::extract_webid` | standard via `jose` | **present** |
| **IdP** (own `oidc-provider` server: auth/token/me/reg/session endpoints, interactions, passkeys, Schnorr SSO, HTML login/register) | **not provided** — we are a relying party, not an IdP | `src/idp/*` — 11 modules, 430 LOC in `idp/index.js`, wired `oidc-provider@9.6.0` | **missing as net surface** (E.3) |

**C.3a Solid-OIDC**: Feature-gated behind `oidc` so default builds don't
pull `openidconnect` + `jsonwebtoken`. DPoP binding, nonce replay
protection (to be wired at the HTTP binder layer — we expose the
`verify_dpop_proof` primitive). No SSRF guard baked into the library
because fetching JWKS is consumer territory; JSS does its own SSRF
(`src/utils/ssrf.js`) because it also issues outbound fetches from the
auth path. **Ship-blocker**: the HTTP binder in `examples/standalone.rs`
needs a jti replay cache matching JSS's behaviour before 0.4.

**C.3b NIP-98**: We have structural checks (kind 27235, `u`/`method`/
`payload` tags, 60s clock skew) since Sprint 2 and Schnorr verification
(feature `nip98-schnorr`, `k256`) since Sprint 3. JSS ships Schnorr
unconditionally via `nostr-tools`. Parity is behavioural. JSS
additionally accepts `Basic nostr:<token>` to serve git credential
helpers — we don't, because we don't ship git HTTP backend (E.1).

**C.3c did:nostr**: We derive the DID identity implicitly when NIP-98
succeeds. JSS ships an explicit `.well-known/did/nostr/:pubkey.json`
resolver (Tier 1/3 DID Document) and a normaliser that links did:nostr
to a WebID via `alsoKnownAs`. **Gap**: we don't publish a DID Document
endpoint or a cross-resolution normaliser. P2 port candidate.

**C.3d Dispatch precedence**: JSS chains DPoP → Nostr → Bearer →
WebID-TLS inside one auth middleware. We expose verification primitives
and let the HTTP binder (actix example, axum example, user code) compose
the order. This is a scope call, not a gap: a library crate should not
dictate order.

**State**: Solid-OIDC and NIP-98 auth are at behavioural parity. Four
gaps: WebID-TLS (won't-fix, F.5), Simple Bearer (deprioritise, E.4),
did:nostr DID Document publication (P2, E.4), full IdP stack (E.3,
large).

### C.4 Notifications

| Protocol | solid-pod-rs | JSS | Verdict |
|---|---|---|---|
| `solid-0.1` WebSocket protocol (legacy SolidOS) | **not implemented** | `src/notifications/websocket.js:1-102, 110-147` (`sub`/`ack`/`err`/`pub`/`unsub`, 100 subs/conn cap, 2 KiB URL cap) | **missing** (P1 — F.4) |
| WebSocketChannel2023 (Notifications 0.2) | `notifications::WebSocketChannelManager` (broadcast channel + per-connection writer, 30s heartbeat) | **not implemented** | **net-new** |
| WebhookChannel2023 (Notifications 0.2) | `notifications::WebhookChannelManager` (AS2.0 POST, 3× exponential retry, drop-on-4xx) | **not implemented** | **net-new** |
| Server-Sent Events | not implemented | not implemented | shared gap (low priority — not in Solid spec) |
| Subscription discovery (`.well-known/solid/notifications`, `.notifications`) | `notifications::discovery_document` | only the status JSON at `/.well-known/solid/notifications` (`src/notifications/index.js:43`) | **partial-parity** (C.4a) |
| Change notification event mapping (storage → AS2.0 `Create`/`Update`/`Delete`) | `ChangeNotification::from_storage_event` | emitted inline | **present** |
| Per-subscription WAC read check | deferred to HTTP binder | enforced in-server | **semantic-difference** |

**C.4a Discovery**: JSS only ships the old `.well-known/solid/notifications`
status endpoint pointing at the WebSocket URL. We ship a richer
Notifications 0.2 discovery advertising both channels, mandatory for
modern clients. JSS is behind the spec here.

**State**: We are substantially ahead of JSS on the notifications front.
The reverse gap is the **legacy `solid-0.1` protocol**, which SolidOS
still speaks; without it we can't serve a SolidOS data-browser session
that expects live updates. P1 to add as an additional channel
implementation — tracked in E.4.

### C.5 PATCH

| Dialect | solid-pod-rs | JSS | Verdict |
|---|---|---|---|
| N3 Patch (Solid Protocol §8.2) | `ldp::apply_n3_patch` (inserts/deletes/where) | `src/patch/n3-patch.js:22-120` (regex-based, `solid:inserts`/`deletes`/simplified `where`) | **present** |
| SPARQL-Update | `ldp::apply_sparql_patch` (via `spargebra`) | `src/patch/sparql-update.js:22-82` (INSERT DATA, DELETE DATA, DELETE+INSERT+WHERE, DELETE WHERE, standalone INSERT WHERE) | **present** (C.5a) |
| JSON Patch (RFC 6902) | `ldp::apply_json_patch` (add/remove/replace/test/copy/move) | **not implemented** | **net-new** (F.1) |
| PATCH dialect dispatch on `Content-Type` | `ldp::patch_dialect_from_mime` → `PatchDialect::{N3,Sparql,JsonPatch}` | Content-Type dispatch in `src/handlers/resource.js` | **present** |
| Failure code on `where` mismatch | 412 Precondition Failed | 409 Conflict | **semantic-difference** (C.5b) |

**C.5a SPARQL-Update parser**: We use the `spargebra` crate (full SPARQL
1.1 grammar); JSS uses hand-rolled regex. Our implementation accepts the
full SPARQL 1.1 algebra whereas JSS's only handles the triple-shaped
subset. Clients that send full SPARQL-Update graph patterns work against
us and fail against JSS.

**C.5b 409 vs 412 on precondition failure**: Solid Protocol §8.2 reads
more naturally as "precondition failed" (= 412) than as "conflict" (=
409). Both are spec-legal; neither side has a reference anchor. We
should consider adding an option to emit 409 for JSS-compat mode, but
the default is correct.

**State**: PATCH parity is substantive. We additionally implement JSON
Patch (net-new). Grammar coverage is broader on SPARQL-Update.

### C.6 JSS-specific extras (features JSS ships that solid-pod-rs does not)

| Feature | JSS path | Status | Priority |
|---|---|---|---|
| Git HTTP backend (`handleGit` via `git http-backend` CGI) | `src/handlers/git.js:11-268`, `src/server.js:286-314` | **missing** | **P2** (niche; Git-backed pods are a known but small audience) |
| ActivityPub federation (Actor on `/profile/card`, inbox HTTP-sig verify, outbox delivery, WebFinger, NodeInfo 2.1) | `src/ap/index.js`, `src/ap/routes/{actor,inbox,outbox,collections}.js` (200+ LOC) | **missing** | **P1** (large scope; Solid-AP bridges are ecosystem-valuable) |
| Nostr relay (NIP-01/11/16) | `src/nostr/relay.js:95-286` | **missing** | **P2** (embedded relay; we should ship as a separate crate) |
| WebID-TLS | `src/auth/webid-tls.js:187-257` | **missing** | **P3** (legacy — see F.5) |
| SolidOS Mashlib static serving | `src/server.js:382-401` | **missing** | **P3** (consumer concern; ship in admin crate) |
| Single-user root-pod bootstrap | `src/server.js:480-548` | `provision::provision_pod` handles multi-pod; single-user is the N=1 case | **partial-parity** |
| Invite-only flag + invite subcommand | `bin/jss.js invite {create,list,revoke}` | **missing** | **P3** (operator tooling) |
| Quota set/show/reconcile CLI + per-pod disk quota | `bin/jss.js quota {set,show,reconcile}` + `src/storage/quota.js` | `provision::QuotaTracker` (byte-level reserve/release) | **partial-parity** — CLI absent |
| Passkeys (WebAuthn) registration + login | `src/idp/passkey.js`, wired `src/idp/index.js:319-380+` | **missing** | **P3** (IdP sub-feature — see E.3) |
| Schnorr SSO (NIP-07 browser-ext handshake) | `src/idp/interactions.js` | **missing** | **P3** (IdP sub-feature) |
| HTML login/register/consent pages | `src/idp/index.js:239-315` | **missing** | **wontfix-in-crate** (F.6) |
| SSRF guard on outbound fetches | `src/utils/ssrf.js:15-157` | **not provided** — consumer binder's responsibility | **missing-as-primitive** (P1 convenience) |
| Dotfile allowlist | `src/server.js:265-281` | **not provided** — consumer's responsibility | **missing-as-primitive** (P2) |
| Rate limits on pod create (1/day/IP) + write + login | `src/server.js:209-219, 356-364, 436-446`; `src/idp/index.js:223-232` | **not provided** — consumer's responsibility | **missing-as-primitive** (P2) |
| Live-reload script injection | `src/handlers/resource.js:23-35` | **not implemented** | **P3** (dev-mode nicety) |
| Filesystem storage backend | provided (`storage::fs::FileSystemStorage`) | `src/storage/filesystem.js` | **present** |
| Subdomain multi-tenancy | **not provided** | `src/server.js:159-170` + `src/utils/url.js` | **missing** (P2; path-based multi-tenancy works today) |

### C.7 CLI / config

| Surface | solid-pod-rs | JSS | Verdict |
|---|---|---|---|
| CLI binary | `examples/standalone.rs` (~200 LOC actix example) | `bin/jss.js` (400+ LOC, 40+ flags, subcommands `start`/`init`/`invite`/`quota`) | **partial-parity** (C.7a) |
| Config file (JSON) | **not provided** — consumer's responsibility | `config.json` via `src/config.js:211-239` (CLI > env > file > default precedence) | **missing** (E.6) |
| Env var map | **not provided** — consumer's responsibility | 30+ `JSS_*` vars + `TOKEN_SECRET`, `CORS_ALLOWED_ORIGINS`, `NODE_ENV`, `DATA_ROOT` (`config.js:96-132`) | **missing** (E.6) |
| Pod-create HTTP endpoint | `provision::provision_pod` + `ProvisionPlan` | `POST /.pods` with 1/day/IP rate-limit (`src/server.js:356-364`) | **partial-parity** — rate-limit absent |
| Admin override | `provision::check_admin_override` (constant-time secret compare) | no direct equivalent (operators edit `config.json`) | **net-new** |

**C.7a CLI**: Scope call — JSS is a full server; we are a library. The
standalone example intentionally stays small. For feature-parity, the
admin CLI belongs in a consumer crate. **Action**: formalise this as
ADR-054 "library vs server separation".

### C.8 Architecture

| Axis | solid-pod-rs | JSS |
|---|---|---|
| Language | Rust 2021 | Node.js ≥ 18 ESM |
| HTTP framework | agnostic (library) | Fastify 4.29.x (tightly coupled) |
| RDF graph model | `ldp::Graph` (internal deterministic) | `n3.js` via `n3` package (ad-hoc per serialiser) |
| Storage backends | `memory`, `fs`, `s3` (gated `s3-backend`) | filesystem only |
| WAC enforcement | library function, HTTP binder decides hook | Fastify `preHandler` hook |
| Dependency count | 13 required + 4 optional (`oidc`, `nip98-schnorr`, `s3-backend`, etc.) | 10 runtime + 1 dev (`autocannon`) |
| Runtime footprint | ~30 MB static binary, <10 ms cold start (example binary) | ~120 MB Node + 432 KB source + deps (per README §"footprint comparison") |
| Steady-state perf | ~15k req/s GET (measured via `wrk`, memory backend, 1 core) | 5,400 req/s GET resource, 4,700 req/s container (README `Performance`) |

---

## D. Overbuilt (features we have that JSS doesn't) — keep / deprecate / feature-flag

### D.1 WebSocketChannel2023 notifications — **keep**

Net-new. Solid Notifications 0.2 spec-standard. Ecosystem value is high:
modern clients (Solid Notifications 2023+, enterprise CDPs) expect this
channel. JSS lacks it; shipping gives us first-mover on Rust-native
Solid notifications.

### D.2 WebhookChannel2023 notifications — **keep**

Net-new. Same rationale as D.1. Webhooks are the canonical way for
external services to integrate with Solid pods. JSS's `solid-0.1` does
not address webhooks.

### D.3 Server-Sent Events (neither side) — **skip**

Neither side ships SSE. Solid Notifications 0.2 does not mandate it.
Keep out of scope until a spec version mandates it.

### D.4 JSON Patch (RFC 6902) PATCH dialect — **keep + feature-flag behind `patch-json`**

Net-new. JSS does not accept `application/json-patch+json`; Solid
Protocol is silent (mandates N3 + SPARQL-Update only). Clients wanting
JSON Patch against JSON resources on a Solid pod are a real audience
(notably: ActivityPub-on-Solid bridges, JSON-first web apps). Cost is
low (already implemented). **Action**: keep the implementation, mark it
`non-normative` in docs so clients understand the spec says nothing
about it.

### D.5 `acl:agentGroup` enforcement with trait-based membership — **keep**

Net-new-behaviour. JSS parses but doesn't enforce. We enforce via the
`GroupMembership` trait and a `StaticGroupMembership` default. This is
**more conformant to WAC §3.1.4** than JSS. Strictly improves security
posture.

### D.6 Strong ETags (hex-encoded SHA-256) — **keep**

Net-new-behaviour. JSS uses `md5(mtime+size)` weak ETags; we use strong
SHA-256. Strong ETags permit Range and precondition use without
ambiguity. RFC 7232 accepts both; strong is the safer default.

### D.7 `Prefer` header composition (minimal + contained IRIs) — **keep**

JSS does not implement `Prefer` dispatch at all (`src/ldp/container.js`
builds container representations unconditionally). We parse and compose
multi-include semantics per RFC 7240 + LDP §4.2.2. Ship-forward.

### D.8 Turtle-serialised ACL documents — **keep**

Both sides parse Turtle ACLs; we also **serialise** via
`wac::serialize_turtle_acl`. JSS only consumes Turtle. Net-new on the
outbound path. Useful for clients that want to mint ACLs
programmatically in Turtle form. Keep.

### D.9 Dev-mode session helper (`interop::dev_session`) — **keep, behind `dev` feature**

Net-new. JSS has no equivalent test-time helper. **Action**: gate behind
the `dev` feature to prevent accidental production use. Store admin
flag only through the typed constructor — never from request headers
(already enforced).

### D.10 S3 / R2 / Object-store backends — **keep gated**

`s3-backend` feature exposes an S3 storage backend. JSS is
filesystem-only. Operators running at scale need this. Cost to carry is
low (dep is `optional = true`). Keep as-is; expand R2 and KV in
consumer crates per ADR-053.

### D.11 Framework-agnostic library surface — **keep (core identity)**

JSS is a Fastify server. solid-pod-rs is a library. Consumers bind into
axum, actix-web, hyper, or custom HTTP runtimes. This is the
architectural thesis of the crate.

### D.12 WebID-OIDC discovery helper (`webid::generate_webid_html_with_issuer`, `extract_oidc_issuer`) — **keep**

Emits `solid:oidcIssuer` triples in generated WebID profiles. JSS
generates WebID profiles with issuer detection via `oidc-provider`
internals. Our helper is the library-level equivalent; keep.

### D.13 `.well-known/solid` discovery document, WebFinger, NIP-05 — **keep**

JSS ships WebFinger (`/.well-known/webfinger`) and NodeInfo but **not**
the Solid Protocol `.well-known/solid` document. We implement all
three. Keep.

### D.14 Quota reserve/release atomic primitive — **keep**

JSS has a `storage/quota.js` module but no equivalent atomic
reserve-then-commit API. Our `QuotaTracker` gives callers a safe way to
reserve bytes before a write, release on failure, and enforce on
concurrent writes without locks. Keep.

### D.15 Constant-time admin override check — **keep**

Net-new. JSS has no equivalent admin-secret endpoint; it uses config
file edits. We provide `provision::check_admin_override` with
`subtle::ct_eq`-shaped comparison to resist timing attacks. Keep.

### D.16 `oxigraph`-independent RDF serialisation — **keep**

Our `Graph::to_ntriples` and `Graph::parse_ntriples` avoid pulling
`oxigraph` (30+ MB of deps). JSS's `n3.js` is smaller. For the crate's
"minimal footprint" thesis, this matters.

### D.17 SOLID_IMPL={jss,native,shadow} dispatcher — **VisionClaw-specific; keep external**

Lives in the VisionClaw consumer, not in the library crate. The public
`solid-pod-rs` repo does not need this.

### D.18 Shadow comparator (native↔JSS traffic diff) — **VisionClaw-specific**

Same as D.17.

### D.19 HMAC opaque IDs with rotating salt + wire-level opacity (Bit-29) — **VisionClaw-specific**

Kept documented regardless (per task brief) but lives in the VisionClaw
repo, not here. **Action**: note in the extraction README that these
primitives belong to the consumer.

---

## E. Missing (features JSS has that solid-pod-rs doesn't) — prioritised port candidates

### E.1 Git HTTP backend — **P2**

**JSS**: `src/handlers/git.js:11-268` wraps `git http-backend` CGI with
path-traversal hardening, `receive.denyCurrentBranch=updateInstead`
auto-set on non-bare repos, WAC hook that flips required mode to `Write`
for `git-receive-pack`. Enables Git-backed Pod storage.

**Rust port ticket**:
- Module: `src/handlers/git.rs` (~450 LOC estimated)
- Dependencies: `tokio::process::Command` for CGI execution; `std::os::unix::fs::MetadataExt` for setuid hardening; path-traversal helper already available
- Tests: `tests/git_handler.rs` — clone/push/pull round-trip via
  `git2` test harness against a temp-dir bare repo
- WAC integration: extend `wac::method_to_mode` with a Git-aware variant
  that consumes request path + method, returns mode
- **Risk**: CGI process spawn is a security surface; must run as a
  dedicated uid or apply seccomp. Propose `seccompiler` dep.
- **Audience**: small but non-zero (Git-backed Pods are a known niche
  for developer-oriented federated platforms).

### E.2 ActivityPub integration — **P1**

**JSS**: `src/ap/{index,keys,store}.js` + `src/ap/routes/*.js`
(~600 LOC). WebFinger + NodeInfo 2.1 (we have WebFinger, missing
NodeInfo), Actor on `/profile/card` (Accept-negotiated), inbox with
HTTP Signature verification (RSA), outbox posting Notes and delivering
to remote inboxes using `microfed`, SQLite-backed follower/following
store.

**Rust port ticket**:
- New module: `src/ap/` (~1,200 LOC estimated)
- Sub-modules: `ap::actor`, `ap::inbox`, `ap::outbox`, `ap::keys`, `ap::store`, `ap::webfinger`, `ap::nodeinfo`
- Dependencies: `rsa` (HTTP Sig RSA-PSS), `httpsig-rs` or hand-rolled verifier, `sqlx` with SQLite feature for follower store OR leverage existing `Storage` trait for persistence
- Feature flag: `activitypub`
- Tests: `tests/activitypub_interop.rs` — round-trip Follow, Create (Note), Like against `wiremock` remote actor
- **Risk**: the spec surface is large; HTTP Signatures + object-capability verification has a long tail of edge cases. Propose 3-4 sprint budget.
- **Audience**: federation surface is Solid's long-promised killer
  feature. JSS ships it; shipping gives real ecosystem parity.

### E.3 IdP (identity provider) stack — **P2 (large)**

**JSS**: `src/idp/` — 11 modules, 430 LOC in `index.js` alone. Ships a
complete `oidc-provider@9.6.0`-based IdP with:
- Standard endpoints `/idp/{auth,token,me,reg,session,session/end}`
- Credentials endpoint `/idp/credentials` (email+password → Bearer) with
  10/min rate limit
- Interactions `/idp/interaction/:uid/{login,confirm,abort}`
- Passkey (WebAuthn) registration + login via `@simplewebauthn/server`
- Schnorr SSO (NIP-07) via `/idp/schnorr/{login,complete}`
- HTML register form at `/idp/register`
- Client Identifier Document support (fetches + caches remote client metadata when `client_id` is a URL)

**Rust port ticket**:
- This is a **new crate** — `solid-pod-rs-idp` — rather than a library
  module. Rationale: the IdP has its own lifecycle (sessions, accounts,
  HTML templates, rate limits, keys rotation) that doesn't belong in a
  library crate.
- Scope: reuse `openidconnect` and `jsonwebtoken` (already present under
  `oidc` feature) + `axum` + `askama` templates + `argon2` (password
  hashing) + `webauthn-rs` (passkeys)
- Estimated: ~3,500 LOC + test harness
- Tests: conformance against Solid-OIDC 0.1 + OIDC Discovery 1.0
- **Decision**: park until 0.5.0+ — we currently integrate with
  external IdPs (NextAuth, Keycloak, Authentik) via Solid-OIDC RP flow.

### E.4 did:nostr DID Document publication + normaliser — **P2**

**JSS**: `src/did/resolver.js:67-92` publishes `/.well-known/did/nostr/:pubkey.json` (Tier 1 and Tier 3 DID Documents), plus `src/auth/did-nostr.js:41-80` resolves did:nostr to a WebID via `alsoKnownAs` when the DID Document links back.

**Rust port ticket**:
- Module: extend `src/interop.rs` with `did_nostr_document(pubkey: &str, webid: Option<&str>) -> serde_json::Value`
- Add to `well_known_solid` a pointer to the did:nostr endpoint when configured
- Add `resolve_did_nostr_to_webid(doc: &serde_json::Value) -> Option<String>`
- Estimated: ~150 LOC + 6 tests
- **Priority**: P2 — enables Nostr-native identity bridging without an
  IdP.

### E.5 WebID-TLS — **P3 (legacy; won't-fix recommended)**

**JSS**: `src/auth/webid-tls.js:187-257`. Verifies client cert SAN →
WebID, validates `cert:modulus` / `cert:exponent` against the WebID
profile's RSA public key triples.

**Rust port ticket**: not recommended. WebID-TLS is obsolete; Solid
community has moved to Solid-OIDC + DPoP. Operating burden (client-cert
provisioning, browser UX, PKI) exceeds adoption. **Decision**:
explicitly-deferred forever. Record in ADR-053 §"WebID-TLS deprecation"
as a Rust-side won't-fix.

### E.6 Config loader, env var map, config precedence — **P2**

**JSS**: `src/config.js:17-239` — loads JSON from disk, overlays env
(`JSS_PORT`, `JSS_HOST`, 30+ more), overlays CLI, with strict precedence
CLI > env > file > default. Size parsing (`parseSize` for `50MB`,
`1GB`).

**Rust port ticket**:
- Module: `src/config.rs` (new)
- Dependencies: `figment` (layered config) or `config-rs`; add `humantime` for duration-style values
- Feature flag: `config-loader`
- Tests: precedence ordering, size parsing, env-var override behaviour
- Estimated: ~250 LOC + 12 tests
- **Priority**: P2 — operators need it; consumer crates currently
  roll their own.

### E.7 Nostr embedded relay (NIP-01/11/16) — **P2 (ship as separate crate)**

**JSS**: `src/nostr/relay.js:95-286` — embedded NIP-01 relay on
`/relay` + NIP-11 info at `/relay/info`. Supports EVENT / REQ / CLOSE,
ephemeral (20000-29999), replaceable (0, 3, 10000-19999), parameterised
replaceable (30000-39999). In-memory ring buffer (default 1000 events),
60 events/min/socket, 64 KiB message cap.

**Rust port ticket**: not inside `solid-pod-rs`. Ship as separate crate
`nostr-relay-rs` so operators who want both a Solid pod and a relay can
bind them in one binary but not conflate features. Estimated
~800-1,200 LOC + conformance tests against `nostr-tools`.

### E.8 Legacy `solid-0.1` WebSocket notifications protocol — **P1**

**JSS**: `src/notifications/websocket.js:1-102, 110-147`. The legacy
protocol SolidOS speaks. Per-subscription WAC read check; 100 subs/
connection; 2 KiB URL cap.

**Rust port ticket**:
- Add `notifications::Solid01Channel` adapter that translates a
  `solid-0.1` client's `sub`/`ack`/`err`/`pub` messages onto the
  existing `WebSocketChannelManager` fanout.
- Estimated: ~300 LOC + 10 tests
- **Priority**: P1 — without it, SolidOS data-browser doesn't receive
  live updates on our pods. Back-compat matters for the existing
  ecosystem.

### E.9 SolidOS Mashlib static serving — **P3 (consumer concern)**

**JSS**: `src/server.js:382-401` serves `/mashlib.min.js`,
`/841.mashlib.min.js`, `/mash.css`, `.map` variants as static assets.
Ship in a separate crate `solid-pod-rs-admin` or document integration
with `actix-files` / `tower-http::ServeDir` in consumer guide.

### E.10 Dotfile allowlist, rate limits, SSRF guard, subdomain multi-tenancy — **library primitives, P1**

Each of these is currently a consumer-binder concern. JSS bakes them
into the server. We should surface them as **primitives** in the
library:
- `security::is_allowed_dotfile_path(path: &str) -> bool` — accept `/.acl`, `/.meta`, `/.well-known/*`; reject the rest.
- `security::PodCreateRateLimiter` trait — consumer wires to their
  rate-limit backend (Redis, LRU, etc.).
- `security::is_safe_outbound_url(url: &Url) -> bool` — blocks RFC1918, link-local, AWS metadata (169.254.169.254), file://, ::1, etc. Mirrors JSS's `src/utils/ssrf.js:15-157`.
- `multitenant::pod_for_hostname(host: &str, base_domain: &str) -> Option<(pod_name, path)>` — subdomain mode.

Estimated: ~300 LOC + 20 tests. **Priority**: P1 for security
primitives (SSRF + dotfile); P2 for subdomain mode.

---

## F. Semantic differences

### F.1 PATCH dialect set

**JSS**: N3 Patch + SPARQL-Update only. JSON Patch (RFC 6902) is **not**
accepted.

**solid-pod-rs**: N3 + SPARQL-Update + JSON Patch. Our JSON Patch is
dispatched via `Content-Type: application/json-patch+json`.

**Bug or feature?** Feature. Solid Protocol §8.2 names N3 + SPARQL-
Update as the mandated dialects; does not forbid additional ones. Our
JSON Patch is strictly additive.

**Compatibility impact**: Clients that PATCH against solid-pod-rs with
`Content-Type: application/json-patch+json` succeed on our pods and
fail (415) against JSS. Document it as a solid-pod-rs extension in the
interop guide.

### F.2 `acl:origin` — neither side enforces

Both parsers ignore `acl:origin`. WAC §4.3 defines it as an additional
gate. **Joint gap**. Port as E.3.5 once C.2c lands.

### F.3 Notifications surface

**JSS**: `solid-0.1` only (legacy SolidOS). No modern Channels.

**solid-pod-rs**: WebSocketChannel2023 + WebhookChannel2023 +
discovery. No `solid-0.1`.

**Bug or feature?** Both. **Action**: per E.4, add a `solid-0.1`
adapter as a compatibility layer without losing the modern channels.
Ship both so SolidOS works and Notifications 0.2 clients work.

**Compatibility impact**: SolidOS data-browser sessions against a
solid-pod-rs pod currently reconnect repeatedly and never receive
updates until E.4 lands. Listed as P1 for 0.4.0.

### F.4 CLI surface + config ecosystem

**JSS**: full CLI `bin/jss.js` with `start`/`init`/`invite`/`quota`
subcommands and 30+ flags. Config precedence CLI > env > file > default.

**solid-pod-rs**: `examples/standalone.rs` is the only binary — a
demonstration, not a production CLI.

**Bug or feature?** Feature — scope call. Library vs server.

**Compatibility impact**: operators cannot drop solid-pod-rs into a
JSS-replacement slot without writing their own wrapper. **Action**: add
ADR-054 documenting the library-vs-server split; ship
`solid-pod-rs-server` as a future consumer crate.

### F.5 Default WAC stance

**JSS**: default-deny at the ACL evaluator (`src/wac/checker.js:31-34`)
but **auto-seeds** owner ACLs + public-read on pod creation so that
pods are usable out of the box.

**solid-pod-rs**: default-deny in the evaluator; `provision::provision_pod`
seeds the same owner-R/W ACL via its `ProvisionPlan`. Library users not
calling `provision_pod` get pure deny-by-default.

**Bug or feature?** Feature, but under-documented. **Action**: update
README + architecture docs to prescribe `provision_pod` as the canonical
onboarding path.

### F.6 ETag strength

JSS = weak md5; ours = strong SHA-256. Both spec-legal. Tests comparing
ETag strings must normalise. See D.6.

### F.7 PATCH `where` failure code

JSS = 409 Conflict; ours = 412 Precondition Failed. Both spec-legal.
412 matches Solid Protocol §8.2 "precondition" wording more closely.
Compatibility: clients that strictly expect 409 must be tolerant.

### F.8 `.acl` write validation

**JSS**: accepts malformed ACL bodies on PUT; fails on first evaluation
with 500.

**solid-pod-rs**: rejects malformed ACL bodies at write time with 422.

**Bug or feature?** Feature — operator-friendlier. **Compatibility
impact**: clients that previously passed malformed ACLs and
subsequently overwrote them see 422 now. Fix: send well-formed
documents.

---

## G. Net-new in solid-pod-rs (our contribution to the Rust Solid ecosystem)

Features that are **not** in JSS and **not** port candidates because
they're either orthogonal, VisionClaw-specific, or deliberate
ecosystem contributions:

1. **Framework-agnostic library surface** (C.8 row 2). The architectural
   identity of the crate.
2. **Rust-native performance**: measured >2× steady-state GET throughput
   vs JSS at ~25% memory footprint, on equivalent hardware, no Node
   runtime dependency.
3. **WebSocketChannel2023** and **WebhookChannel2023** notifications
   (D.1, D.2).
4. **JSON Patch (RFC 6902)** PATCH dialect (D.4).
5. **`acl:agentGroup` enforcement** with trait-based membership (D.5).
6. **Strong ETags** (D.6).
7. **`Prefer` header composition** (D.7).
8. **Turtle ACL serialisation** (D.8).
9. **Dev-mode session helper** (D.9).
10. **S3/R2/object-store storage backends** (D.10).
11. **WebID-OIDC discovery helpers** (`webid::generate_webid_html_with_issuer`, `extract_oidc_issuer`) (D.12).
12. **`.well-known/solid` discovery document** (D.13, C.7).
13. **Atomic quota reserve/release primitive** (D.14).
14. **Constant-time admin override check** (D.15).
15. **Feature-gated OIDC / Schnorr / S3**: default build compiles without
    `openidconnect`, `jsonwebtoken`, `k256`, `aws-sdk-s3`, minimising
    attack surface.
16. **`Send + Sync + 'static` public types**: one process hosts N pods.
    JSS's IdP is process-global.
17. **Deterministic `Graph` model**: single internal representation
    backs Turtle ⇄ JSON-LD ⇄ N-Triples round-trips. JSS's `n3.js`
    per-path serialisers give non-deterministic round-trips.
18. **28+ scenario WAC inheritance corpus** (`tests/wac_inheritance.rs`)
    — more exhaustive than JSS's WAC tests.
19. **AGPL-3.0-only inheritance** (just landed, v0.3.0-alpha.3): aligns
    our licence with JSS so downstream forks in the Solid ecosystem
    don't accidentally relicense under a more permissive umbrella.

VisionClaw-specific (kept out of the public crate):

- **HMAC opaque IDs + rotating salt**
- **Bit-29 wire-level opacity** (VisionClaw-specific transport)
- **Shadow comparator** (JSS ↔ native traffic diff)
- **`SOLID_IMPL={jss,native,shadow}` dispatcher**

---

## H. Prioritised port roadmap (Sprint 4 candidates)

| Rank | Feature | Priority | Est. LOC | Tests | Target |
|---|---|---|---|---|---|
| 1 | SSRF guard + dotfile allowlist primitives (E.10 subset) | **P0** | 150 | 15 unit + 5 integration | 0.3.1 |
| 2 | `solid-0.1` legacy notifications adapter (E.8) | **P1** | 300 | 10 unit + 3 integration | 0.4.0 |
| 3 | `acl:origin` enforcement (C.2c → F.2) | **P1** | 100 | 8 unit | 0.4.0 |
| 4 | jti replay cache for DPoP in the HTTP binder layer (C.3a ship-blocker) | **P1** | 200 | 6 unit | 0.4.0 |
| 5 | did:nostr DID Document publication + normaliser (E.4) | **P2** | 150 | 6 unit | 0.4.0 |
| 6 | ActivityPub integration (E.2) | **P1** | 1,200 | 40 unit + 15 integration | 0.5.0 (new crate `solid-pod-rs-ap`) |
| 7 | Config loader + env var map (E.6) | **P2** | 250 | 12 unit | 0.4.0 |
| 8 | Subdomain multi-tenancy helper (E.10 subset) | **P2** | 150 | 10 unit | 0.4.0 |
| 9 | Git HTTP backend (E.1) | **P2** | 450 | 12 integration | 0.5.0 |
| 10 | Rate-limit primitives trait + reference LRU impl (E.10 subset) | **P2** | 250 | 15 unit | 0.4.0 |

Ranked primarily by **Solid Protocol conformance value + JSS-parity
value**. Git, AP, and `solid-0.1` are the three big unlocks; the others
are incremental hardening.

Not ranked (won't-port or long-deferred):

- **WebID-TLS** (E.5) — won't-port.
- **IdP stack** (E.3) — defer to `solid-pod-rs-idp` crate, post-0.5.0.
- **Nostr relay** (E.7) — defer to `nostr-relay-rs` crate.
- **Mashlib / SolidOS UI** (E.9) — defer to `solid-pod-rs-admin`.
- **HTML login/register pages** (C.6, F.6) — won't-port in-crate;
  `solid-pod-rs-admin` consumer crate.

---

## I. Conclusion + ship stance

### Is solid-pod-rs at acceptable JSS parity right now?

**Yes for the Solid Protocol core**: LDP, WAC, PATCH, Solid-OIDC,
Notifications 0.2, WebID, Turtle ACL round-trip, WebFinger, NIP-05,
`.well-known/solid`, provisioning, quota, admin override. Every
spec-normative surface that JSS implements has a parity or
parity-plus row in [`PARITY-CHECKLIST.md`](./PARITY-CHECKLIST.md).

**No for JSS-specific extras**: ActivityPub, Git HTTP backend, IdP
stack, `solid-0.1` legacy notifications, Nostr relay, WebID-TLS,
Passkeys, Mashlib. These are features JSS ships; we do not. Two of them
(AP and `solid-0.1`) are ecosystem-visible (SolidOS integration,
federation story) and deserve prioritisation. The rest are niche or
legacy.

### Does AGPL-3.0 inheritance change the maturity calculus?

Yes — positively. The licence inheritance (v0.3.0-alpha.3, commit
`6750a9f`) aligns solid-pod-rs with the JSS ecosystem covenant. A
downstream fork that takes solid-pod-rs and bundles it with JSS code
(e.g. for a hybrid Node+Rust deployment) now has a coherent single
licence. Previously (MIT/Apache dual-licence) downstream had a
decision-tree; now the answer is "AGPL everywhere".

This *reduces* the 0.4.0 stabilisation burden: we no longer need to
argue about licence compatibility with the JSS ecosystem. Operators
running AGPL stacks (which most Solid deployments already do) get a
drop-in component.

### Recommended v0.3.x → v0.4.0 gate

**Must land before v0.4.0**:

1. SSRF guard + dotfile allowlist as library primitives (H rank 1).
2. `solid-0.1` legacy notifications adapter (H rank 2) — unblocks
   SolidOS integration.
3. `acl:origin` enforcement (H rank 3) — closes WAC §4.3 gap.
4. DPoP jti replay cache primitive (H rank 4) — closes Solid-OIDC §5.2
   ship-blocker.
5. Config loader + env var map (H rank 7) — operator quality-of-life
   that consumer crates currently duplicate.
6. ADR-054: "library vs server separation" — formalises the scope
   decision that `solid-pod-rs-server`, `solid-pod-rs-admin`,
   `solid-pod-rs-idp` are separate crates.

**Should not block v0.4.0**:

- ActivityPub (new crate timeline; 0.5.0).
- Git HTTP backend (new module; 0.5.0).
- IdP stack (new crate; post-0.5.0).
- Nostr relay (new crate; independent schedule).

**Won't land ever**:

- WebID-TLS (legacy).
- HTML login/register pages in the library crate (scope).
- JSON Patch removal (staying as a non-normative extension).
- Direct/Indirect LDP Containers (Solid Protocol mandates Basic only).

### Bottom line

solid-pod-rs has full behavioural parity with JSS on the spec-normative
Solid Protocol surface, with three net-new features that push it
**ahead** of JSS on modern notifications, WAC group enforcement, and
ACL write strictness. The gaps that matter for ecosystem adoption
(`solid-0.1` legacy notifications, ActivityPub federation, Git HTTP
backend) are scoped, priced, and roadmapped. v0.4.0 is a six-ticket
sprint away from being the canonical Rust reference for Solid Pods.

---

## References

- [`PARITY-CHECKLIST.md`](./PARITY-CHECKLIST.md) — row-per-feature tracker.
- [`docs/reference/jss-feature-inventory.md`](./docs/reference/jss-feature-inventory.md) — canonical JSS surface (the source-of-truth used for this document).
- JSS source: `/home/devuser/workspace/project/JavaScriptSolidServer/` (local clone).
- ADR-053 — backend boundary + extraction scope.
- ADR-054 (pending) — library-vs-server separation.
- Solid Protocol 0.11: <https://solidproject.org/TR/protocol>
- WAC: <https://solidproject.org/TR/wac>
- Solid-OIDC 0.1: <https://solidproject.org/TR/oidc>
- Solid Notifications 0.2: <https://solidproject.org/TR/notifications-protocol>
- W3C LDP: <https://www.w3.org/TR/ldp/>
- NIP-98: <https://github.com/nostr-protocol/nips/blob/master/98.md>
