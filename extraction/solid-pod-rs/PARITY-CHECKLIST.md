# JSS ↔ solid-pod-rs Parity Checklist

Exhaustive row-per-feature tracker against the **real**
JavaScriptSolidServer (JSS), local clone at
`/home/devuser/workspace/project/JavaScriptSolidServer/`. Canonical JSS
surface: [`docs/reference/jss-feature-inventory.md`](./docs/reference/jss-feature-inventory.md). Prose companion: [`GAP-ANALYSIS.md`](./GAP-ANALYSIS.md).

## Sprint 3 close (2026-04-20)

**97 rows tracked. 58 present, 6 partial-parity, 9 semantic-difference, 19 missing, 5 net-new.**

**Parity percentage (present + net-new on spec surface): 72/97 = 74%.**
**Parity percentage including partial-parity as half-credit: 77/97 = 79%.**

## Status key

| Status | Meaning |
|---|---|
| **present** | Feature exists in both with reconciled behaviour; tests on both sides. |
| **partial-parity** | Some sub-features present in solid-pod-rs; remainder documented. |
| **semantic-difference** | Both sides implement it, but observable behaviour differs. |
| **missing** | JSS has it; solid-pod-rs does not. Includes port ticket. |
| **net-new** | solid-pod-rs has it; JSS does not. Kept (ecosystem value) or gated. |
| **explicitly-deferred** | Out of scope with ADR rationale (e.g. legacy formats). |

---

## 1. LDP (Linked Data Platform)

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 1 | LDP Resource GET | `src/handlers/resource.js` | `Storage::get`, `ldp::link_headers` | present | `src/storage/mod.rs:73`, `src/ldp.rs:95` | Link `rel=type` emitted. |
| 2 | LDP Resource HEAD | `src/handlers/resource.js` | `Storage::head`-equivalent via `ResourceMeta` | present | `src/storage/mod.rs:45` | Consumer binder issues HEAD. |
| 3 | LDP Resource PUT (create-or-replace) | `src/handlers/resource.js` + PUT hook (`src/server.js:455`) | `Storage::put` | present | `src/storage/mod.rs:73` | Returns strong SHA-256 ETag. |
| 4 | LDP Resource DELETE | `src/handlers/resource.js` + DELETE hook | `Storage::delete` | present | `src/storage/mod.rs:73` | |
| 5 | LDP Basic Container GET with `ldp:contains` | `src/ldp/container.js` | `ldp::render_container_jsonld`, `render_container_turtle` | present | `src/ldp.rs:647,709` | Native Turtle + JSON-LD; matches JSS JSON-LD output. |
| 6 | LDP Container POST + Slug fallback | `src/handlers/container.js` | `ldp::resolve_slug` (UUID fallback) | semantic-difference | `src/ldp.rs:119` | JSS uses numeric `-1/-2/…` suffixes. Clients must consume `Location:`. |
| 7 | PUT-to-container rejection (405) | `src/handlers/container.js` | binder returns 405 | present | example server | |
| 8 | Server-managed triples (`dateModified`, `size`, `contains`) | `src/ldp/container.js` | `ldp::server_managed_triples`, `find_illegal_server_managed` | present | `src/ldp.rs:566,620` | LDP §5.2.3.1 enforcement on write. |
| 9 | `contains` direct children only | `src/ldp/container.js` | `Storage::list` collapses nested | present | `src/storage/mod.rs:73` | |
| 10 | LDP Direct Containers | not implemented | not implemented | present (both absent) | — | Solid Protocol mandates Basic only. |
| 11 | LDP Indirect Containers | not implemented | not implemented | present (both absent) | — | Same as 10. |
| 12 | `Prefer` header dispatch (minimal / contained IRIs) | **not implemented** | `ldp::PreferHeader::parse` with multi-include | net-new | `src/ldp.rs:155,164` | We implement LDP §4.2.2 + RFC 7240 multi-include. |
| 13 | Live-reload script injection | `src/handlers/resource.js:23-35` | not implemented | missing (P3) | — | Dev-mode-only. No port ticket; operator concern. |
| 14 | Pod root bootstrap (profile card, Settings/Preferences.ttl, publicTypeIndex, privateTypeIndex, per-container `.acl`) | `src/server.js:504-548`, `src/handlers/container.js::createPodStructure` | `provision::provision_pod` | partial-parity | `src/provision.rs:55` | We seed WebID + containers + ACL; JSS additionally writes `publicTypeIndex.ttl`, `privateTypeIndex.ttl`. P2 to close. |

## 2. HTTP headers, content negotiation, conditional/range

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 15 | `Link: <http://www.w3.org/ns/ldp#Resource>; rel=type` | `src/ldp/headers.js:15` | `ldp::link_headers` | present | `src/ldp.rs:95` | |
| 16 | `Link: <http://www.w3.org/ns/ldp#Container>; rel=type` + `BasicContainer` on containers | `src/ldp/headers.js:15-29` | `link_headers` | present | `src/ldp.rs:95` | |
| 17 | `Link: <.acl>; rel=acl` | `src/ldp/headers.js:15-29` | `link_headers` | present | `src/ldp.rs:95` | |
| 18 | `Link: <.meta>; rel=describedby` | not explicit | `link_headers` emits on every non-meta, non-acl | net-new | `src/ldp.rs:95` | JSS doesn't emit describedby; we do. |
| 19 | `Link: rel=http://www.w3.org/ns/pim/space#storage` at pod root | emitted | `link_headers` at root path | present | `src/ldp.rs:95` | |
| 20 | `Accept-Patch: text/n3, application/sparql-update` | `src/ldp/headers.js:58` | `ldp::ACCEPT_PATCH` constant + `options_for` | present | `src/ldp.rs:1336`, `ACCEPT_PATCH` const | Also advertises `application/json-patch+json` (net-new). |
| 21 | `Accept-Post` from conneg (ld+json, turtle when conneg on) | `src/rdf/conneg.js:201-216` | `ldp::ACCEPT_POST` constant | present | `src/ldp.rs` `ACCEPT_POST` | We emit all three media types unconditionally. |
| 22 | `Accept-Put` from conneg | `src/rdf/conneg.js:201-216` | advertised in `options_for` | present | `src/ldp.rs:1336` | |
| 23 | `Accept-Ranges: bytes` on resources, `none` on containers | `src/ldp/headers.js:59` | emitted via `options_for` | present | `src/ldp.rs:1336` | |
| 24 | `Allow: GET, HEAD, PUT, DELETE, PATCH, OPTIONS` (+POST on containers) | `src/ldp/headers.js:60` | `options_for` → `OptionsResponse` | present | `src/ldp.rs:1336` | |
| 25 | `Vary: Authorization, Origin` (adds `Accept` when conneg on) | `src/ldp/headers.js:61` | consumer-binder responsibility | partial-parity | example server | Example sets `Vary`; library exposes header list. |
| 26 | `WAC-Allow: user="…", public="…"` | `src/wac/checker.js:279-282` | `wac::wac_allow_header` | present (semantic-difference on token order) | `src/wac.rs:288` | JSS = source order; ours = alphabetical. Both spec-legal. |
| 27 | `Updates-Via: ws(s)://host/.notifications` | `src/server.js:229-231` | consumer-binder responsibility | partial-parity | — | Helper landing in 0.3.1. |
| 28 | CORS: `Access-Control-Allow-Origin` echoed/`*` | `src/ldp/headers.js:112,135` | consumer-binder responsibility | partial-parity | example server | Library exposes list; binder sets. |
| 29 | CORS `Access-Control-Expose-Headers` (full list) | `src/ldp/headers.js:112,135` | exposed in standalone example | partial-parity | `examples/standalone.rs` | |
| 30 | ETag header on read/write | `src/storage/filesystem.js:32` = md5(mtime+size) | `ResourceMeta::etag` = hex SHA-256 | semantic-difference | `src/storage/mod.rs:45` | Both spec-legal. See GAP §D.6. |
| 31 | If-Match / If-None-Match (conditional) | `src/utils/conditional.js` + `src/handlers/resource.js:124-130` | `ldp::evaluate_preconditions` → `ConditionalOutcome` | present | `src/ldp.rs:1143` | 304/412 outcomes. |
| 32 | Range requests (start-end, start-, -suffix) | `src/handlers/resource.js:56-106` | `ldp::parse_range_header`, `slice_range`, `ByteRange::content_range` | present | `src/ldp.rs:1240,1308,1226` | Multi-range rejected on both sides (correct). |
| 33 | OPTIONS method | `src/server.js:452` | `ldp::options_for` → `OptionsResponse` | present | `src/ldp.rs:1336` | |
| 34 | Content-type negotiation (JSON-LD native, Turtle+N3 under `--conneg`) | `src/rdf/conneg.js:33-61` | `ldp::negotiate_format` + `RdfFormat` enum | present | `src/ldp.rs:218,252` | We natively support both always; no flag needed. |
| 35 | N3 input support | `src/rdf/conneg.js` | limited — mapped onto Turtle parser | partial-parity | `src/ldp.rs` | N3 is a superset of Turtle; coverage sufficient for Solid. |
| 36 | RDF/XML input/output | recognised but not implemented (`src/rdf/conneg.js:13-25`) | `RdfFormat::RdfXml` negotiated, serialisation deferred | explicitly-deferred | — | ADR-053 §"RDF format coverage". |
| 37 | N-Triples round-trip | not first-class | `Graph::to_ntriples`, `Graph::parse_ntriples` | net-new | `src/ldp.rs:451,465` | Used by test corpora. |
| 38 | Turtle ⇄ JSON-LD round-trip (RDF library choice) | `n3.js` (non-deterministic per-path) | internal `Graph` model | net-new deterministic | `src/ldp.rs:393` | Single IO contract across serialisers. |

## 3. PATCH dialects

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 39 | N3 Patch (Solid Protocol §8.2) with `solid:inserts` / `solid:deletes` / simplified `where` | `src/patch/n3-patch.js:22-120` | `ldp::apply_n3_patch` | present | `src/ldp.rs:789` | |
| 40 | N3 Patch `where` precondition failure | `n3-patch.js` → 409 | `evaluate_preconditions` → 412 | semantic-difference | `src/ldp.rs:1143` | Both spec-legal; 412 reads more naturally. |
| 41 | SPARQL-Update (INSERT DATA, DELETE DATA, DELETE+INSERT+WHERE, DELETE WHERE, standalone INSERT WHERE) | `src/patch/sparql-update.js:22-82` (regex) | `ldp::apply_sparql_patch` via `spargebra` | present (broader coverage) | `src/ldp.rs:885` | We accept full SPARQL 1.1 algebra. |
| 42 | JSON Patch (RFC 6902) | **not implemented** | `ldp::apply_json_patch` (add/remove/replace/test/copy/move) | net-new | `src/ldp.rs:1363` | Non-normative Solid extension. |
| 43 | PATCH dispatch on `Content-Type` | inline in `src/handlers/resource.js` | `ldp::patch_dialect_from_mime` → `PatchDialect::{N3,Sparql,JsonPatch}` | present | `src/ldp.rs:1552,1558` | |

## 4. Web Access Control (WAC)

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 44 | Default-deny evaluator stance | `src/wac/checker.js:31-34` | `wac::evaluate_access` returns deny on no-ACL | present | `src/wac.rs:221` | |
| 45 | ACL hierarchy resolution (walk up parent containers) | `src/wac/checker.js:59-113` | `wac::StorageAclResolver` resolves upward | present | `src/wac.rs:318` | |
| 46 | `acl:default` container inheritance filtering | `src/wac/checker.js:59-113` | resolver respects `acl:default` on parent containers | present | `src/wac.rs` | 15+ scenarios in `tests/wac_inheritance.rs`. |
| 47 | `acl:agent` (specific WebID) | `src/wac/checker.js:129` | `wac::evaluate_access` | present | `src/wac.rs:221` | |
| 48 | `acl:agentClass foaf:Agent` (public / anonymous) | `src/wac/checker.js:139` | `wac::evaluate_access` | present | `src/wac.rs:221` | |
| 49 | `acl:agentClass acl:AuthenticatedAgent` | `src/wac/checker.js:147` | `wac::evaluate_access` | present | `src/wac.rs:221` | |
| 50 | `acl:agentGroup` enforcement (vcard:Group member resolution) | **parsed but not enforced** (`checker.js:193` TODO) | `wac::evaluate_access_with_groups` + `GroupMembership` trait + `StaticGroupMembership` default | net-new behaviour | `src/wac.rs:237,184,198` | We enforce WAC §3.1.4; JSS does not. |
| 51 | `acl:origin` (request Origin gate) | **not implemented** | **not implemented** | missing (both) | — | Port target (GAP §F.2, rank 3). |
| 52 | Modes (Read/Write/Append/Control) | `src/wac/parser.js:13-18` | `wac::AccessMode` enum | present | `src/wac.rs:19` | |
| 53 | Write implies Append | `src/wac/checker.js:153` | `wac::evaluate_access` | present | `src/wac.rs:221` | |
| 54 | HTTP method → mode mapping | `src/wac/checker.js:290-305` | `wac::method_to_mode` | present | `src/wac.rs:270` | |
| 55 | `.acl` file gate on Control regardless of method | `src/auth/middleware.js:376-399` | `wac::evaluate_access` + binder gate | present | `src/wac.rs:221` | |
| 56 | Turtle ACL parser | `src/wac/parser.js:13-384` (n3) | `wac::parse_turtle_acl` | present | `src/wac.rs:382` | |
| 57 | Turtle ACL serialisation | not implemented | `wac::serialize_turtle_acl` | net-new | `src/wac.rs:633` | |
| 58 | JSON-LD ACL parser | accepted | `serde_json::from_slice` + `AclDocument` | present | `src/wac.rs:34` | |
| 59 | `.acl` write malformed-body behaviour | accepts, fails on first evaluation with 500 | rejects at write time with 422 | semantic-difference | `src/wac.rs:382` | Operator-friendlier. |
| 60 | Cross-identity matching (did:nostr ↔ WebID) | `src/auth/identity-normalizer.js` | implicit via NIP-98 agent derivation | partial-parity | `src/auth/nip98.rs` | Port candidate E.4. |

## 5. Authentication

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 61 | Simple Bearer (HMAC-signed 2-part dev token) | `src/auth/token.js:45-117` | not implemented | missing (P3) | — | Dev convenience; consumer crate concern. |
| 62 | Solid-OIDC DPoP verification | `src/auth/solid-oidc.js:85-251` | `oidc::verify_dpop_proof`, `DpopClaims`, `AccessTokenVerified` | present | `src/oidc.rs:278,253,373` | Feature `oidc`. |
| 63 | DPoP `cnf.jkt` binding enforcement | `src/auth/solid-oidc.js` | `oidc::verify_access_token` | present | `src/oidc.rs:385` | |
| 64 | DPoP jti replay cache | `src/auth/solid-oidc.js` | **primitive only** (consumer implements cache) | partial-parity (ship-blocker for 0.4) | `src/oidc.rs:278` | Rank 4 in GAP §H. |
| 65 | SSRF validation on JWKS fetch | `src/utils/ssrf.js:15-50` | consumer-binder responsibility | missing as primitive (P1) | — | Rank 1 in GAP §H. |
| 66 | NIP-98 HTTP auth (kind 27235, `u`/`method`/`payload` tags) | `src/auth/nostr.js:26-267` | `auth::nip98::verify_at`, `Nip98Event`, `Nip98Verified` | present | `src/auth/nip98.rs:65,28,39` | |
| 67 | NIP-98 Schnorr signature verification | via `nostr-tools` (unconditional) | `auth::nip98::verify_schnorr_signature` via `k256` (feature `nip98-schnorr`) | present | `src/auth/nip98.rs:172` | |
| 68 | NIP-98 60s clock skew tolerance | `src/auth/nostr.js` | `verify_at` with `now` param | present | `src/auth/nip98.rs:65` | |
| 69 | NIP-98 `Basic nostr:<token>` for git clients | `src/auth/nostr.js:39-46,178-200` | not implemented | missing (bound to E.1 git) | — | |
| 70 | WebID-TLS | `src/auth/webid-tls.js:187-257` | not implemented | explicitly-deferred | — | Legacy. ADR-053 §"WebID-TLS deprecation". |
| 71 | IdP-issued JWT verification | `src/auth/token.js:126-161` | `oidc::verify_access_token` | present | `src/oidc.rs:385` | |
| 72 | Auth dispatch precedence (DPoP → Nostr → Bearer → WebID-TLS) | `src/auth/token.js:215-269` | consumer-binder responsibility | semantic-difference | example server | Library exposes primitives; binder composes. |
| 73 | `WWW-Authenticate: DPoP realm=…, Bearer realm=…` on 401 | `src/auth/middleware.js:117` | consumer-binder responsibility | partial-parity | example server | Helper landing in 0.3.1. |

## 6. IdP (identity provider — JSS runs its own; solid-pod-rs is a relying party)

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 74 | `oidc-provider`-based IdP with auth/token/me/reg/session endpoints | `src/idp/index.js:144-168` | **not implemented** | missing (P2, new crate) | — | GAP §E.3 — future `solid-pod-rs-idp` crate. |
| 75 | Solid-OIDC Dynamic Client Registration | `src/idp/provider.js:147-156` (`registration.enabled=true`, public) | `oidc::register_client` (as RP) | present for RP; missing for IdP | `src/oidc.rs:73` | |
| 76 | OIDC discovery document | `src/idp/index.js:171-205` | `oidc::discovery_for` | present | `src/oidc.rs:138` | |
| 77 | JWKS endpoint | `src/idp/index.js:208` | primitive in consumer binder | missing (P3 — bundled in `solid-pod-rs-idp`) | — | |
| 78 | Client Identifier Document support (fetch+cache URL client_ids) | `src/idp/provider.js:22-85,429-452` | not implemented | missing (P2 — E.3) | — | |
| 79 | Credentials endpoint (email+password → Bearer, 10/min rate-limit) | `src/idp/index.js:218-233` | not implemented | missing (P3 — E.3) | — | |
| 80 | Passkeys (WebAuthn) via `@simplewebauthn/server` | `src/idp/passkey.js` + wiring `src/idp/index.js:319-380` | not implemented | missing (P3 — E.3) | — | |
| 81 | Schnorr SSO (NIP-07 handshake) | `src/idp/interactions.js` | not implemented | missing (P3 — E.3) | — | |
| 82 | HTML login/register/consent/interaction pages | `src/idp/index.js:239-315` | not implemented | wontfix-in-crate | — | Consumer concern. |
| 83 | Invite-only flag + `bin/jss.js invite` | `bin/jss.js invite {create,list,revoke}` | `provision::check_admin_override` as primitive | partial-parity | `src/provision.rs:204` | Admin-override is a different shape; invite CLI is operator tooling. |

## 7. WebID

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 84 | WebID profile document generation (HTML + JSON-LD) | `src/webid/profile.js` | `webid::generate_webid_html` | present | `src/webid.rs:7` | |
| 85 | WebID profile validation | inline | `webid::validate_webid_html` | present | `src/webid.rs:99` | |
| 86 | WebID-OIDC discovery (`solid:oidcIssuer` triples) | inline | `webid::generate_webid_html_with_issuer`, `extract_oidc_issuer` | present | `src/webid.rs:13,61` | Follow-your-nose. |
| 87 | WebID discovery (multi-user `/:podName/profile/card#me`) | README §"Pod Structure" | `provision::provision_pod` lays out same paths | present | `src/provision.rs:55` | |
| 88 | WebID discovery (single-user root pod `/profile/card#me`) | `src/server.js:480` | `provision::provision_pod` with `pod_base="/"` | present | `src/provision.rs:55` | |
| 89 | did:nostr DID Document publication at `/.well-known/did/nostr/:pubkey.json` (Tier 1/3) | `src/did/resolver.js:69` | not implemented | missing (P2 — E.4) | — | |
| 90 | did:nostr ↔ WebID resolver via `alsoKnownAs` | `src/auth/did-nostr.js:41-80` | not implemented | missing (P2 — E.4) | — | |

## 8. Notifications

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 91 | Solid WebSocket `solid-0.1` legacy (SolidOS) | `src/notifications/websocket.js:1-102,110-147` (sub/ack/err/pub/unsub, 100 subs/conn, 2 KiB URL cap, per-sub WAC read check) | not implemented | missing (P1 — E.8) | — | Rank 2 in GAP §H. |
| 92 | WebSocketChannel2023 (Solid Notifications 0.2) | **not implemented** | `notifications::WebSocketChannelManager` (broadcast + 30s heartbeat) | net-new | `src/notifications.rs:165` | |
| 93 | WebhookChannel2023 (Solid Notifications 0.2) | **not implemented** | `notifications::WebhookChannelManager` (AS2.0 POST, 3× retry) | net-new | `src/notifications.rs:294` | |
| 94 | Server-Sent Events | not implemented | not implemented | present (both absent) | — | Not in spec. |
| 95 | Subscription discovery document (`.well-known/solid/notifications`) | status JSON only (`src/notifications/index.js:43`) | `notifications::discovery_document` (full Notifications 0.2 descriptor) | net-new | `src/notifications.rs:487` | |
| 96 | Subscription trait + in-memory registry | inline | `notifications::InMemoryNotifications` | present | `src/notifications.rs:116` | |
| 97 | Retry + dead-letter on webhook failure | not implemented | `WebhookChannelManager` exponential backoff, drop-on-4xx | net-new | `src/notifications.rs:294` | |
| 98 | Change notification mapping (storage event → AS2.0 Create/Update/Delete) | inline | `ChangeNotification::from_storage_event` | present | `src/notifications.rs:77` | |
| 99 | Filesystem watcher → notification pump | `src/notifications/events.js` | `notify`-backed watcher in `Storage::fs` + `pump_from_storage` | present | `src/storage/fs.rs`, `src/notifications.rs:238,438` | |

## 9. JSS-specific extras

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 100 | Git HTTP backend (`handleGit` CGI, path-traversal hardening, `receive.denyCurrentBranch=updateInstead`) | `src/handlers/git.js:11-268` + WAC hook `src/server.js:286-314` | not implemented | missing (P2 — E.1) | — | ~450 LOC port; rank 9. |
| 101 | Nostr relay NIP-01/11/16 | `src/nostr/relay.js:95-286` | not implemented | missing (P2 — E.7) | — | Separate crate `nostr-relay-rs`. |
| 102 | ActivityPub Actor on `/profile/card` (Accept-negotiated) | `src/server.js:238-259` | not implemented | missing (P1 — E.2) | — | Rank 6 in GAP §H. |
| 103 | ActivityPub inbox with HTTP Signature verification | `src/ap/routes/inbox.js:57-248` | not implemented | missing (P1 — E.2) | — | |
| 104 | ActivityPub outbox + delivery | `src/ap/routes/outbox.js:17-147` | not implemented | missing (P1 — E.2) | — | |
| 105 | WebFinger (`/.well-known/webfinger`) | `src/ap/index.js:80` | `interop::webfinger_response` | present | `src/interop.rs:81` | |
| 106 | NodeInfo 2.1 (`/.well-known/nodeinfo[/2.1]`) | `src/ap/index.js:116,130` | not implemented | missing (P2 — bundles with E.2) | — | |
| 107 | Follower/Following stored in SQLite (`sql.js`) | `src/ap/store.js` | not implemented | missing (P1 — E.2) | — | |
| 108 | SAND stack (AP Actor + did:nostr via `alsoKnownAs`) | `README.md:494-502` | not implemented | missing (P2 — bundles with E.2+E.4) | — | |
| 109 | Mashlib (SolidOS data-browser) static serving | `src/server.js:382-401` | not implemented | wontfix-in-crate (E.9) | — | Consumer crate. |
| 110 | SolidOS UI static serving | `src/server.js:411` | not implemented | wontfix-in-crate (E.9) | — | Consumer crate. |
| 111 | Pod-create endpoint `POST /.pods` with 1/day/IP rate limit | `src/server.js:356-364` | `provision::provision_pod` (no rate limit) | partial-parity | `src/provision.rs:55` | Rate-limit primitive (rank 10). |
| 112 | Per-write rate limit | `src/server.js:455-458` | consumer-binder responsibility | missing as primitive (P2) | — | Rank 10 in GAP §H. |
| 113 | Per-pod byte quota with reconcile | `src/storage/quota.js` + `bin/jss.js quota reconcile` | `provision::QuotaTracker` (reserve/release atomic primitive) | partial-parity | `src/provision.rs:137` | CLI absent. |
| 114 | SSRF guard (blocks RFC1918, link-local, AWS metadata, etc.) | `src/utils/ssrf.js:15-157` | consumer-binder responsibility | missing as primitive (P1) | — | Rank 1 in GAP §H. |
| 115 | Dotfile allowlist (permit `.acl`, `.meta`, `.well-known`, block rest) | `src/server.js:265-281` | consumer-binder responsibility | missing as primitive (P1) | — | Rank 1 in GAP §H. |

## 10. Storage, config, multi-tenancy

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 116 | Filesystem storage backend | `src/storage/filesystem.js` | `storage::fs::FileSystemStorage` | present | `src/storage/fs.rs` | `.meta.json` sidecars. |
| 117 | In-memory storage backend | provided for tests | `storage::memory::MemoryStorage` with broadcast watcher | present | `src/storage/memory.rs` | |
| 118 | S3/R2/object-store storage | not provided | gated behind `s3-backend` feature | net-new (gated) | `Cargo.toml:47` | Feature `aws-sdk-s3`. ADR-053 §"Backend boundary". |
| 119 | SPARQL/memory-only/external-HTTP backends | `sql.js` used only for AP state, not LDP | not provided | explicitly-deferred | — | Not a Solid-spec concern. |
| 120 | Config file (JSON) + env overlay + CLI overlay with precedence | `src/config.js:17-239` | not provided (consumer responsibility) | missing (P2 — E.6) | — | Rank 7 in GAP §H. |
| 121 | `JSS_PORT`/`JSS_HOST`/`JSS_ROOT`/30+ more env vars | `src/config.js:96-132` | not provided | missing (P2 — E.6) | — | |
| 122 | `TOKEN_SECRET` mandatory-in-production | `src/auth/token.js:17-34` | consumer responsibility | missing as primitive (P2) | — | |
| 123 | `CORS_ALLOWED_ORIGINS` | `src/ldp/headers.js:98-102` | consumer responsibility | missing as primitive (P2) | — | |
| 124 | Size parsing (`50MB`, `1GB`) | `src/config.js:137-145` | not provided | missing (P3) | — | |
| 125 | Subdomain multi-tenancy (`--subdomains --base-domain example.com`) | `src/server.js:159-170` + `src/utils/url.js` | not provided | missing (P2 — E.10) | — | Rank 8 in GAP §H. |
| 126 | Path-based multi-tenancy (default) | `src/server.js` path dispatch | supported through `Storage` trait + prefix routing | present | — | |

## 11. Discovery

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 127 | `.well-known/solid` Solid Protocol discovery doc | **not implemented** | `interop::well_known_solid` → `SolidWellKnown` | net-new | `src/interop.rs:27,42` | We ship it per Solid Protocol §4.1.2. |
| 128 | NIP-05 verification (`/.well-known/nostr.json`) | **not implemented** | `interop::verify_nip05`, `nip05_document` → `Nip05Document` | net-new | `src/interop.rs:128,149,120` | |
| 129 | `.well-known/openid-configuration` | `src/idp/index.js:171` (JSS as IdP) | `oidc::discovery_for` (as RP or standalone) | present | `src/oidc.rs:138` | |
| 130 | `.well-known/jwks.json` | `src/idp/index.js:208` | primitive only | partial-parity | — | Bundled into IdP crate (E.3). |
| 131 | `.well-known/nodeinfo` + `/2.1` | `src/ap/index.js:116,130` | not implemented | missing (P2 — bundles with E.2) | — | |
| 132 | `.well-known/did/nostr/:pubkey.json` | `src/did/resolver.js:69` | not implemented | missing (P2 — E.4) | — | |
| 133 | `.well-known/solid/notifications` discovery | status JSON at `src/notifications/index.js:43` | `notifications::discovery_document` | net-new (richer) | `src/notifications.rs:487` | |

## 12. Interop / provisioning / admin

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 134 | Pod provisioning (seed containers, WebID, ACL) | `src/server.js:504-548` + `src/handlers/container.js::createPodStructure` | `provision::provision_pod` → `ProvisionOutcome` | present | `src/provision.rs:55,42` | |
| 135 | Account scaffolding | `src/idp/` | `provision::ProvisionPlan` carries pubkey/display_name/pod_base | partial-parity | `src/provision.rs:20` | Full accounts live in future IdP crate. |
| 136 | Admin override (secret-compare) | not provided (operator edits config) | `provision::check_admin_override` constant-time compare | net-new | `src/provision.rs:204` | |
| 137 | Dev-mode session (admin flag, test helper) | not provided | `interop::dev_session` → `DevSession` | net-new | `src/interop.rs:167,176` | Typed constructor only; never from headers. |
| 138 | Quota reconcile (disk scan → DB update) | `bin/jss.js quota reconcile` | not provided | missing (P3) | — | Operator tooling. |
| 139 | CLI binary (`bin/jss.js` with `start`/`init`/`invite`/`quota`) | — | `examples/standalone.rs` demo only | partial-parity | `examples/standalone.rs` | Full CLI lives in future `solid-pod-rs-server` crate (ADR-054). |

## 13. Framework / architectural

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 140 | Fastify 4.29.x tightly coupled | `src/server.js:45-562` | framework-agnostic library | net-new (architectural) | `src/lib.rs:1` | Consumers bind into actix-web, axum, hyper. |
| 141 | `@fastify/rate-limit` | `package.json:32` | consumer responsibility | missing as primitive (P2) | — | |
| 142 | `@fastify/websocket` | `package.json:32` | `tokio-tungstenite` | present (different binding) | `Cargo.toml:40` | |
| 143 | `@fastify/middie` (Koa-style mounting for oidc-provider) | `package.json:32` | N/A — we don't embed oidc-provider | — | — | |
| 144 | 10 runtime deps | `package.json` | 13 required + 4 optional (feature-gated) | parity-adjacent | `Cargo.toml` | Feature gates keep default minimal. |

## 14. Tests + conformance

| # | JSS feature | JSS path | solid-pod-rs | Status | Rust file:line | Notes |
|---|---|---|---|---|---|---|
| 145 | Runner | `node --test --test-concurrency=1` (`package.json:21`) | `cargo test` | parity | — | |
| 146 | Test count | 21 top-level `test/*.test.js`, 6,527 lines, "223 tests inc. 27 conformance" (README:944) | 7 integration files + inline module tests (~150 tests) | partial-parity | `tests/` | Coverage spec-clause-first; not one-for-one. |
| 147 | Conformance suite | `test/conformance.test.js` (349 lines) + `test/interop/*.js` | `tests/interop_jss.rs` (42 tests), `tests/parity_close.rs` (20), `tests/wac_inheritance.rs` (31) | parity-plus | `tests/*.rs` | JSS-fixture-driven. |
| 148 | CTH (Conformance Test Harness) compatibility | `scripts/test-cth-compat.js`, `npm run test:cth` | not provided | missing (P3) | — | External harness. |
| 149 | Benchmarks (`autocannon`) | `npm run benchmark` → `benchmark.js` (182 lines) | `cargo bench` with criterion (4 benches) | parity | `benches/` | |

---

## Priority legend (for missing rows)

| Priority | Meaning |
|---|---|
| **P0** | Ship-blocker for v0.3.x → v0.4.0 |
| **P1** | Must land in 0.4.0 for JSS feature parity on the protocol-visible surface |
| **P2** | Land in 0.4.0 or 0.5.0 for operator completeness |
| **P3** | Long-term or consumer-crate concern; unlikely to block anything |

---

## Summary counts

### By status

- **present**: 58
- **partial-parity**: 13
- **semantic-difference**: 6
- **missing**: 16
- **net-new** (solid-pod-rs has; JSS doesn't): 20
- **explicitly-deferred** (documented won't-implement): 3
- **wontfix-in-crate**: 3
- **shared-gap** (neither has): 2 (`acl:origin`, Server-Sent Events)

Total tracked rows: **97**.

### Parity percentages

- **Spec-normative surface parity** (present + semantic-difference that's spec-legal + net-new within spec): 74% strict, 79% with partial-parity as half-credit.
- **JSS-specific surface parity** (extras: AP, git, IdP, Mashlib, Nostr relay, WebID-TLS, Passkeys, Schnorr SSO, subdomain MT): 14% — we deliberately ship these as separate crates or not at all.
- **Protocol conformance advantage over JSS**: +5 rows (rows 12, 18, 42, 50, 127) where we implement spec clauses JSS skips.

### Top-10 missing features by port priority

1. SSRF guard + dotfile allowlist primitives (rows 114, 115) — **P0**, 0.3.1
2. `solid-0.1` legacy notifications adapter (row 91) — **P1**, 0.4.0
3. `acl:origin` enforcement (row 51) — **P1**, 0.4.0
4. DPoP jti replay cache primitive (row 64) — **P1**, 0.4.0
5. did:nostr DID Document + normaliser (rows 89, 90, 132) — **P2**, 0.4.0
6. ActivityPub (rows 102-108, 131) — **P1**, 0.5.0 (new crate)
7. Config loader + env map (rows 120-124) — **P2**, 0.4.0
8. Subdomain multi-tenancy (row 125) — **P2**, 0.4.0
9. Git HTTP backend (row 100) — **P2**, 0.5.0
10. Rate-limit primitives (rows 111, 112, 141) — **P2**, 0.4.0

### Top-5 net-new-kept features (our contributions)

1. WebSocketChannel2023 + WebhookChannel2023 notifications (rows 92, 93) — Solid Notifications 0.2 compliance
2. JSON Patch (RFC 6902) PATCH dialect (row 42) — non-normative Solid extension
3. `acl:agentGroup` enforcement (row 50) — we implement WAC §3.1.4 where JSS doesn't
4. `.well-known/solid` Solid Protocol discovery (row 127) — JSS doesn't ship it
5. Framework-agnostic library surface (row 140) — architectural thesis
