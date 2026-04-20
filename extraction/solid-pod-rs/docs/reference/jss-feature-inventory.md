# JSS Feature Inventory (authoritative)

> Source of record for `JavaScriptSolidServer` (JSS). This document replaces any
> earlier Community-Solid-Server-flavoured inventory. Sprint 3 discovered the
> previous parity corpus was mis-attributed to CSS; this file is the corrected
> baseline, built against the real JSS local clone at
> `/home/devuser/workspace/project/JavaScriptSolidServer/` (tracked upstream as
> `jss-upstream`). Citations use `path:line` references into that tree.

## 1. Identification

| Field | Value |
|---|---|
| Canonical name | JavaScriptSolidServer (`jss`) |
| Upstream | `https://github.com/JavaScriptSolidServer/JavaScriptSolidServer` |
| Licence | `AGPL-3.0-only` (`package.json:53`) |
| Language | Node.js ≥ 18, ESM (`package.json:6, 42`) |
| Local tree | `package.json` version `0.0.86`; README self-describes as `v0.0.79` (`README.md:9`). Upstream published tags run `v0.0.26 … v0.0.46`; the local checkout is ahead of the remote at `backup-main-before-huge-refactor-1264-g25b8fae13`. |
| Declared perf | README Performance table: GET resource 5,400+ req/s, GET container 4,700+, PUT 5,700+, POST 5,200+, OPTIONS 10,000+ (`README.md:925–931`). |
| Declared footprint | Comparison table: JSS 432 KB / 10 deps, vs NSS 777 KB / 58 deps, CSS 5.8 MB / 70 deps (`README.md:869–874`). Note: README line ~870 uses "432 KB / 10 deps", not "~1 MB". |
| Solid spec | LDP + Solid Protocol (N3 Patch, SPARQL Update, WAC, Solid-OIDC) with explicit JSS extensions: Nostr relay, NIP-98, ActivityPub, Git HTTP backend, WebID-TLS, Schnorr SSO, Passkeys, did:nostr (`README.md:9–42`). |
| Framework | Fastify 4.29.x (`package.json:32`), confirmed `src/server.js:1`. |

## 2. HTTP / LDP surface

### Routes (wired in `src/server.js`)

| Method | Path | Handler | Citation |
|---|---|---|---|
| GET | `/*` | `handleGet` | `src/server.js:450` |
| HEAD | `/*` | `handleHead` | `src/server.js:451` |
| OPTIONS | `/*` | `handleOptions` | `src/server.js:452` |
| PUT | `/*` | `handlePut` (rate-limited) | `src/server.js:455` |
| DELETE | `/*` | `handleDelete` (rate-limited) | `src/server.js:456` |
| POST | `/*` | `handlePost` (rate-limited) | `src/server.js:457` |
| PATCH | `/*` | `handlePatch` (rate-limited) | `src/server.js:458` |
| POST | `/.pods` | `handleCreatePod` (1/day/IP) | `src/server.js:356–364` |
| GET | `/.notifications` (WS) | WebSocket upgrade | `src/notifications/index.js:36` |
| GET | `/.well-known/solid/notifications` | status JSON | `src/notifications/index.js:43` |
| GET | `/.well-known/openid-configuration` | OIDC discovery | `src/idp/index.js:171` |
| GET | `/.well-known/jwks.json` | JWKS | `src/idp/index.js:208` |
| GET | `/.well-known/webfinger` | AP discovery | `src/ap/index.js:80` |
| GET | `/.well-known/nodeinfo`, `/.well-known/nodeinfo/2.1` | NodeInfo | `src/ap/index.js:116, 130` |
| GET | `/.well-known/did/nostr/:pubkey.json` | did:nostr resolver | `src/did/resolver.js:69` |
| GET | `/profile/card` (`Accept: application/activity+json`) | AP Actor | `src/server.js:238` |
| POST | `/inbox`, `/profile/card/inbox` | AP inbox (HTTP-sig verified) | `src/ap/index.js:163–164` |
| GET/POST | `/profile/card/outbox` | AP outbox | `src/ap/index.js:169–170` |
| GET | `/profile/card/followers`, `/following` | AP collections | `src/ap/index.js:174–175` |
| WS | `/relay`, GET `/relay/info` | Nostr relay (NIP-01/11/16) | `src/nostr/relay.js:239, 270` |
| GET/POST/OPTIONS | `/idp/auth`, `/idp/token`, `/idp/me`, `/idp/reg`, `/idp/session(/end)` | forwarded to `oidc-provider` | `src/idp/index.js:144–168` |
| GET/POST | `/idp/credentials` | email+password → Bearer token (rate-limited 10/min) | `src/idp/index.js:218–233` |
| GET/POST | `/idp/interaction/:uid(/login|/confirm|/abort)` | login/consent UI | `src/idp/index.js:239–279` |
| POST | `/idp/passkey/register/(options|verify)`, `/idp/passkey/login/(options|verify)`, `/idp/passkey/register-new/*` | WebAuthn flow | `src/idp/index.js:319–380+` |
| POST | `/idp/schnorr/login`, `/idp/schnorr/complete` | NIP-07 SSO | `src/idp/interactions.js` (handlers referenced `idp/index.js:19–20`) |
| GET/POST | `/idp/register` | HTML register form / submit | `src/idp/index.js:298–315` |
| Git CGI | `*/info/refs`, `*/git-upload-pack`, `*/git-receive-pack` | `handleGit` via `git http-backend` | `src/handlers/git.js:11–15, 95` |
| GET | `/mashlib.min.js`, `/mash.css`, `/841.mashlib.min.js`, `.map` variants | Mashlib static | `src/server.js:382–401` |
| GET | `/solidos-ui/*` | SolidOS UI static | `src/server.js:411` |

### LDP headers emitted

- `Link: <http://www.w3.org/ns/ldp#Resource>; rel="type"` + `ldp:Container`,
  `ldp:BasicContainer` on directories; `<…/.acl>; rel="acl"` auxiliary link
  (`src/ldp/headers.js:15–29`).
- `Accept-Patch: text/n3, application/sparql-update` (`headers.js:58`).
- `Accept-Post`, `Accept-Put` from `src/rdf/conneg.js:201–216` — with conneg
  off, only `application/ld+json, */*`; with conneg on, Turtle added.
- `Accept-Ranges: bytes` for resources, `none` for containers (`headers.js:59`).
- `Allow`: `GET, HEAD, PUT, DELETE, PATCH, OPTIONS` (+ `POST` on containers)
  (`headers.js:60`).
- `Vary`: `Authorization, Origin` (adds `Accept` when conneg enabled)
  (`headers.js:61`).
- `WAC-Allow`: `user="…", public="…"` with modes `read write append control`
  (`src/wac/checker.js:279–282`).
- `Updates-Via: ws(s)://host/.notifications` when notifications on
  (`src/server.js:229–231`).
- CORS: `Access-Control-Allow-Origin` (echoed or `*`), standard methods,
  `Access-Control-Expose-Headers` listing `Accept-Patch, Accept-Post,
  Accept-Ranges, Allow, Content-Length, Content-Range, Content-Type, ETag,
  Link, Location, Updates-Via, WAC-Allow` (`src/ldp/headers.js:112, 135`).
- `ETag` = md5(mtime+size) (`src/storage/filesystem.js:32`).

### Content-type negotiation

Default **JSON-LD native**: Turtle/N3 paths are feature-flagged by `--conneg`
(`src/rdf/conneg.js:33–61`). Supported output: `application/ld+json`,
`text/turtle`. Supported input: JSON-LD, Turtle, N3. `application/rdf+xml`
recognised but not implemented (`conneg.js:13–25`).

### PATCH dialects

- **N3 Patch** (Solid spec): `src/patch/n3-patch.js:22–56` — supports
  `solid:inserts`, `solid:deletes`, and a simplified `solid:where`. Parser is
  regex-based rather than a full N3 grammar.
- **SPARQL Update**: `src/patch/sparql-update.js:22–82` — `INSERT DATA`,
  `DELETE DATA`, `DELETE { } INSERT { } WHERE { }`, `DELETE WHERE { }`,
  standalone `INSERT { } WHERE { }`. WHERE patterns are treated as
  data-shaped triples, not full graph pattern matching.
- **JSON Patch (RFC 6902)**: not implemented; there is no route path for it
  and no parser. PATCH is `Content-Type`-dispatched to one of the two dialects.

### Prefer / conditional / range

- `If-Match`, `If-None-Match`, `If-None-Match: *` handled in
  `src/utils/conditional.js` → 304 / 412 (`src/handlers/resource.js:124–130`;
  `README.md:263–288`).
- HTTP Range header parsed with suffix-range and open-ended range support, but
  multi-range rejected and the full body served (`resource.js:56–106`).
- `Prefer` header is **not** a first-class dispatch axis in this server — no
  `Prefer: return=representation`, no `include=http://www.w3.org/ns/ldp#PreferMinimalContainer`
  implementation. Container listings are generated unconditionally as JSON-LD
  (`src/ldp/container.js`).

### Server-managed triples

Containers: JSON-LD graph built in `src/ldp/container.js` from filesystem
listing — `@id`, `@type: ldp:BasicContainer`, `ldp:contains` entries. Pod
bootstrap writes `profile/card` (HTML + JSON-LD island), `Settings/Preferences.ttl`,
`publicTypeIndex.ttl`, `privateTypeIndex.ttl`, and per-container `.acl`
(`src/server.js:504–548`, `src/handlers/container.js` `createPodStructure`).

## 3. WAC / auth surface

### Stance

**Default-deny.** `src/wac/checker.js:31–34` — if no `.acl` resolves up the
hierarchy, request is denied with `user="", public=""`. README confirms
(`README.md:876–881`: "Restrictive mode… if no ACL file exists, access is
denied"). Single-user and pod-creation flows auto-write owner + public-read
ACLs so pods work out of the box (`src/server.js:530–547`).

### ACL inheritance

`findApplicableAcl` walks hierarchy: resource-specific `resource.acl` first,
then walks up to each parent container's `.acl` applying only rules with
`acl:default` matching the target URL, falling back to root `/.acl`
(`src/wac/checker.js:59–113`). URL matching handles trailing-slash normalisation
and prefix-match for `acl:default` (`checker.js:204–215`).

### Agent modes

All of the following are implemented in `src/wac/checker.js:129–196`:

- `acl:agent` (specific WebID) — exact match + cross-identity via
  `identitiesMatch(did:nostr ↔ WebID)` (`identity-normalizer.js`).
- `acl:agentClass foaf:Agent` — everyone, including unauthenticated.
- `acl:agentClass acl:AuthenticatedAgent` — any authenticated WebID.
- `acl:agentGroup` — parsed but **not enforced**: `checker.js:193` carries a
  TODO ("requires fetching and parsing group documents"). Group membership is
  never checked.
- `acl:origin` — **not implemented**. The parser does not read `acl:origin`
  and the checker does not gate by request `Origin`.

### Modes

`Read`, `Write`, `Append`, `Control` (`src/wac/parser.js:13–18`). `Write`
implies `Append` (`checker.js:153`). HTTP method → mode in
`getRequiredMode`: GET/HEAD/OPTIONS → Read; POST → Append; PUT/PATCH/DELETE
→ Write (`checker.js:290–305`). `.acl` files gate on `Control` regardless of
method (`src/auth/middleware.js:376–399`) — stricter than spec (which allows
Read to fetch ACLs).

### Authentication methods shipped

| Method | Accepts | Citation |
|---|---|---|
| Simple Bearer (dev) | HMAC-SHA256-signed 2-part token | `src/auth/token.js:45–117` |
| Solid-OIDC DPoP | `Authorization: DPoP …` + `DPoP:` header, JWKS fetched per-issuer, SSRF-validated, jti replay cache, `cnf.jkt` binding required | `src/auth/solid-oidc.js:85–164, 173–251` |
| Nostr NIP-98 | `Authorization: Nostr <base64-event>` or `Basic nostr:<token>` for git clients; kind 27235, validates `u`, `method`, `payload`, ±60s clock skew, Schnorr via `nostr-tools` | `src/auth/nostr.js:26–267` |
| WebID-TLS | Client cert SAN → WebID; verifies `cert:modulus`/`cert:exponent` against WebID profile | `src/auth/webid-tls.js:187–257` |
| IdP-issued JWT | 3-part JWT verified against own JWKS via `jose` | `src/auth/token.js:126–161` |

Order of precedence: DPoP → Nostr → Bearer (simple then JWT) → WebID-TLS
(`src/auth/token.js:215–269`). `WWW-Authenticate: DPoP realm="…", Bearer
realm="…"` on 401 (`middleware.js:117`).

### Dynamic Client Registration

Enabled (`src/idp/provider.js:147–156`) — `registration.enabled=true`,
`initialAccessToken=false` (public registration). Registration endpoint
`/idp/reg`. Solid-OIDC **Client Identifier Document** support: `provider.Client.find`
is overridden to fetch and cache documents when `client_id` is a URL, with
SSRF validation (`src/idp/provider.js:22–85, 429–452`).

### WebID discovery

- Default multi-user: `/:podName/profile/card#me` or `/:podName/#me` (README
  `Pod Structure`).
- Single-user root pod: `/profile/card#me` (`server.js:480`).
- NIP-98: `did:nostr:<hex>` returned when no WebID link; resolved to WebID via
  `alsoKnownAs` when the did:nostr DID Document links back (`did-nostr.js:41–80`,
  `nostr.js:245–266`).

## 4. Notifications

| Protocol | Status | Citation |
|---|---|---|
| Solid WebSocket `solid-0.1` (legacy SolidOS) | Implemented. `protocol solid-0.1`, `sub`, `ack`, `err`, `pub`, optional `unsub`. Per-subscription WAC read check. Limits 100 subs/connection, 2 KiB URL. | `src/notifications/websocket.js:1–102, 110–147` |
| WebSocketChannel2023 (Solid Notifications Protocol) | **Not implemented.** No `type: WebSocketChannel2023` description, no channel subscription endpoint, no `/subscription/…` URLs. |
| WebhookChannel2023 | **Not implemented.** No webhook subscription store, no outbound delivery worker. |
| Server-Sent Events | **Not implemented.** |
| ActivityPub inbox | Implemented as federation endpoint (see §5), not as a Solid notifications transport. | `src/ap/routes/inbox.js:118–190` |

## 5. JSS-specific extras

### Git HTTP backend

- Spawns `git http-backend` CGI (`src/handlers/git.js:174`) with
  `GIT_PROJECT_ROOT`, `GIT_HTTP_EXPORT_ALL`, `GIT_HTTP_RECEIVE_PACK=true`,
  `REMOTE_USER=<webId>`.
- `isGitRequest` detects `/info/refs`, `/git-upload-pack`, `/git-receive-pack`
  (`git.js:11–15`). `isGitWriteOperation` flips required WAC mode to `Write`
  for `git-receive-pack` (`git.js:22–24`, `server.js:293–298`).
- Path-traversal hardened with repeated `..` stripping and realpath containment
  (`git.js:41–62`). Non-bare repos auto-set `receive.denyCurrentBranch=updateInstead`
  (`git.js:137–146`).
- Design doc: `docs/git-support.md`.

### Nostr relay / NIP-98 auth

- Integrated NIP-01 relay on `/relay`, NIP-11 info at `/relay/info`, declares
  `supported_nips: [1, 11, 16]` (`src/nostr/relay.js:270–286`).
- Handles EVENT / REQ / CLOSE. Supports ephemeral (20000–29999), replaceable
  (0, 3, 10000–19999), parameterised replaceable (30000–39999) with `d`-tag
  (`relay.js:66–86, 124–231`). In-memory ring with `maxEvents` (default 1000),
  60 events/min/socket rate limit, 64 KiB message cap (`relay.js:14–20, 107–118`).
- NIP-98 (§3 above) is the HTTP auth surface; auth `Basic nostr:<token>` is
  accepted for git credential helpers (`nostr.js:39–46, 178–200`).

### ActivityPub integration

- WebFinger + NodeInfo 2.1 (`src/ap/index.js:80–153`).
- Actor on `/profile/card` when Accept negotiates `application/activity+json`
  (`server.js:238–259`). Inbox verifies HTTP Signatures by fetching the remote
  actor's `publicKeyPem` (`src/ap/routes/inbox.js:57–110`).
- Outbox posts Notes and delivers to follower inboxes using `microfed`
  primitives with RSA HTTP signatures (`src/ap/routes/outbox.js:110–140`).
- Follower / Following stored in SQLite via `sql.js` (WASM)
  (`src/ap/store.js` referenced from `routes/*.js`; dep `sql.js` in
  `package.json:39`).
- Identity linking: `--ap-nostr-pubkey` writes `alsoKnownAs: ["did:nostr:…"]`
  on the Actor — the "SAND stack" (`README.md:494–502`).

### WebRTC / peer transport

**Not implemented.** No RTCPeerConnection, SDP, ICE, or libp2p code in the
source tree. Peer-to-peer federation is exclusively ActivityPub + Nostr.

### remoteStorage compatibility

**Not implemented.** No remoteStorage draft endpoints (`/storage/`), no
OAuth2-based bearer scope per the rs spec, no webfinger rs-provider hint.
Discovery stops at Solid + AP + Nostr.

### CLI flags (enumerated from `bin/jss.js`)

Commands: `start`, `init`, `invite {create,list,revoke}`, `quota
{set,show,reconcile}`.

Start flags (`bin/jss.js:38–83`): `--port`, `--host`, `--root`, `--config`,
`--ssl-key`, `--ssl-cert`, `--multiuser/--no-multiuser`, `--conneg/--no-conneg`,
`--notifications/--no-notifications`, `--idp/--no-idp`, `--idp-issuer`,
`--subdomains/--no-subdomains`, `--base-domain`, `--mashlib`, `--mashlib-cdn`,
`--no-mashlib`, `--mashlib-version`, `--solidos-ui`, `--git/--no-git`,
`--nostr/--no-nostr`, `--nostr-path`, `--nostr-max-events`,
`--activitypub/--no-activitypub`, `--ap-username`, `--ap-display-name`,
`--ap-summary`, `--ap-nostr-pubkey`, `--invite-only/--no-invite-only`,
`--single-user`, `--single-user-name`, `--webid-tls/--no-webid-tls`,
`--public`, `--read-only`, `--live-reload`, `--quiet`, `--print-config`.

### Environment variables

Map in `src/config.js:96–132`: `JSS_PORT`, `JSS_HOST`, `JSS_ROOT`, `JSS_SSL_KEY`,
`JSS_SSL_CERT`, `JSS_MULTIUSER`, `JSS_CONNEG`, `JSS_NOTIFICATIONS`, `JSS_QUIET`,
`JSS_CONFIG_PATH`, `JSS_IDP`, `JSS_IDP_ISSUER`, `JSS_SUBDOMAINS`,
`JSS_BASE_DOMAIN`, `JSS_MASHLIB`, `JSS_MASHLIB_CDN`, `JSS_MASHLIB_VERSION`,
`JSS_SOLIDOS_UI`, `JSS_GIT`, `JSS_NOSTR`, `JSS_NOSTR_PATH`,
`JSS_NOSTR_MAX_EVENTS`, `JSS_ACTIVITYPUB`, `JSS_AP_USERNAME`,
`JSS_AP_DISPLAY_NAME`, `JSS_AP_SUMMARY`, `JSS_AP_NOSTR_PUBKEY`,
`JSS_INVITE_ONLY`, `JSS_SINGLE_USER`, `JSS_SINGLE_USER_NAME`, `JSS_WEBID_TLS`,
`JSS_DEFAULT_QUOTA`, `JSS_PUBLIC`, `JSS_READ_ONLY`, `JSS_LIVE_RELOAD`. Plus
non-prefixed `TOKEN_SECRET` (mandatory in production — `auth/token.js:17–34`),
`CORS_ALLOWED_ORIGINS` (`ldp/headers.js:98–102`), `NODE_ENV`, `DATA_ROOT`
(used by filesystem storage — `src/utils/url.js`, set by the CLI from `--root`).

## 6. Configuration

- **Format**: JSON file (`config.json`) plus `JSS_*` env plus CLI. Precedence:
  CLI > env > file > defaults (`src/config.js:211–239`). Size values
  (`50MB`, `1GB`) parsed by `parseSize` (`config.js:137–145`).
- **Storage backend**: filesystem only (`src/storage/filesystem.js`). No
  memory-only, S3, or Git-object backend — `sql.js` is used solely for
  ActivityPub state, not as an LDP store. IdP state uses a custom filesystem
  `oidc-provider` adapter (`src/idp/adapter.js`).
- **Multi-tenant**: path-based (default) or subdomain-based (`--subdomains
  --base-domain example.com`). Subdomain mode extracts pod name from
  `request.hostname` and remaps storage paths (`src/server.js:159–170`,
  `src/utils/url.js`). Pod creation endpoint is central (`POST /.pods`), rate-
  limited to 1/IP/day to stop namespace squatting (`server.js:356–364`).
- **Service discovery**: `.well-known/openid-configuration`,
  `.well-known/jwks.json`, `.well-known/webfinger`, `.well-known/nodeinfo`,
  `.well-known/nodeinfo/2.1`, `.well-known/did/nostr/<pubkey>.json`,
  `.well-known/solid/notifications` (status). **No** NIP-05 `/.well-known/nostr.json`
  endpoint; **no** Solid `/.well-known/solid` TypeRegistration document.

## 7. Architecture

- **Framework**: Fastify 4.29.x. Plugins: `@fastify/middie` (for oidc-provider
  Koa-style mounting), `@fastify/rate-limit`, `@fastify/websocket`
  (`package.json:26–32`).
- **Dependencies** (10, per README): `fastify`, `@fastify/middie`,
  `@fastify/rate-limit`, `@fastify/websocket`, `@simplewebauthn/server`,
  `bcryptjs`, `commander`, `fs-extra`, `jose`, `microfed`, `n3`,
  `nostr-tools`, `oidc-provider`, `sql.js`. (Dev: `autocannon`.)
- **`src/` subtree responsibilities**:
  - `server.js` (577 lines) — Fastify wiring, hooks (CORS, dotfile block,
    WAC, git preHandler), route registration, single-user bootstrap.
  - `handlers/resource.js` (850+ lines) — GET/HEAD/PUT/DELETE/PATCH/OPTIONS
    including range, conditional, live-reload injection.
  - `handlers/container.js` — POST/create-pod; ACL seeding.
  - `handlers/git.js` — git http-backend CGI bridge.
  - `storage/filesystem.js` — fs-extra-backed ops, md5 ETags.
  - `storage/quota.js` — per-pod byte quotas, reconcile from disk.
  - `auth/middleware.js` — WAC hook; ACL-file special casing.
  - `auth/{token,solid-oidc,nostr,webid-tls,did-nostr,identity-normalizer}.js`
    — auth backends.
  - `wac/{parser,checker}.js` — ACL parsing + hierarchy resolution.
  - `ldp/{headers,container}.js` — LDP metadata.
  - `notifications/{index,websocket,events}.js` — `solid-0.1` fanout +
    filesystem watcher.
  - `idp/` (11 modules) — `oidc-provider` config, accounts, keys, interactions,
    passkeys, invites, views (HTML), credentials endpoint.
  - `ap/{index,keys,store}.js` + `routes/{actor,inbox,outbox,collections}.js`
    — ActivityPub federation.
  - `nostr/relay.js` — NIP-01 relay.
  - `rdf/{turtle,conneg}.js` — Turtle ↔ JSON-LD via `n3`, conneg dispatcher.
  - `patch/{n3-patch,sparql-update}.js` — patch dialects.
  - `webid/profile.js` — HTML+JSON-LD profile generator.
  - `did/resolver.js` — did:nostr Tier 1/3 DID Documents.
  - `mashlib/index.js` — SolidOS data-browser wrapper.
  - `utils/{url,conditional,ssrf}.js` — path mapping, conditional requests,
    SSRF guard (blocks RFC1918, link-local, AWS metadata etc.
    — `utils/ssrf.js:15–50`).

## 8. Test surface

- **Runner**: Node built-in `node --test --test-concurrency=1`
  (`package.json:21`).
- **Count**: 21 top-level `test/*.test.js` files, 6,527 test-file lines in
  total. README claims "223 tests (including 27 conformance tests)"
  (`README.md:944`).
- **Categories**:
  - Unit/integration (in-process): `auth`, `conditional`, `conneg`,
    `did-nostr`, `identity-normalizer`, `idp`, `ldp`, `live-reload`,
    `nip98-method`, `notifications`, `passkey-login-token`, `patch`, `pod`,
    `range`, `solid-oidc`, `sparql-update`, `ssrf` (590 lines — largest),
    `wac`, `webid`, `webid-tls`.
  - Conformance: `conformance.test.js` (349 lines) plus `test/interop/`
    scripts that probe NSS, CSS, SolidCommunity, rdflib, webid discovery
    (`test/interop/*.js`).
  - CTH harness compatibility: `scripts/test-cth-compat.js` + `npm run test:cth`.
- Benchmarks: `autocannon`-based, invoked via `npm run benchmark` →
  `benchmark.js` (182 lines).

## 9. Documentation

- `README.md` (1,056 lines) is the primary reference — feature summary, CLI
  flag table, env table, pod structure, auth tutorials, performance, CTH
  instructions, project-tree appendix.
- `docs/git-support.md` (7,481 bytes) — how-to for adding git HTTP backend.
- `docs/design/nostr-relay-integration.md`,
  `docs/design/nostr-solid-browser-extension.md` — design explainers.
- `AGENTS.md`, `CTH.md`, `LOCAL_CHANGES.md`, `SECURITY-AUDIT-2026-01-03.md`,
  `SECURITY-AUDIT-2026-01-05.md` — operational / audit notes.
- No Diátaxis-style tutorial/how-to/reference/explanation split; no API
  reference generator (JSDoc comments are inline but not published as a site).
  Documentation site referenced at
  `https://javascriptsolidserver.github.io/docs/` (`README.md:5`).

## 10. Community + release cadence

- **Tags** (local `git tag`): `v0.0.26, v0.0.27, v0.0.31, v0.0.32, v0.0.33,
  v0.0.34, v0.0.35, v0.0.46` — semver-esque but clearly pre-1.0 alpha,
  patch-level only.
- **Current local version**: `package.json` `0.0.86`; README body references
  `v0.0.79`, `v0.0.77`, `v0.0.15`, `v0.0.12` as feature introductions. Tag
  coverage lags the code.
- **Commit cadence**: local mirror is the VisionClaw fork, so raw commit counts
  include our own work. Verification of upstream cadence requires
  `git fetch jss-upstream main` + `git log jss-upstream/main --since='6 months
  ago'`, which was not executed (repo is read-only from this agent's side and
  the task requires a local clone).
- **Open issues / PRs**: no offline snapshot in the working tree. README
  references Issue #32 for root-ACL semantics (`README.md:919`).
- **Security advisories**: two in-tree audits, `SECURITY-AUDIT-2026-01-03.md`
  and `SECURITY-AUDIT-2026-01-05.md`. No GitHub Security Advisory stream
  visible without network.

## 11. As-built citation index

All paths relative to `/home/devuser/workspace/project/JavaScriptSolidServer/`.

| Feature | File:line |
|---|---|
| CLI surface | `bin/jss.js:27–446` |
| Config loader + env map | `src/config.js:17–239` |
| Fastify wiring | `src/server.js:45–562` |
| LDP headers, ACL URL, CORS | `src/ldp/headers.js:15–190` |
| WAC parser (ACL shapes, agent classes) | `src/wac/parser.js:13–384` |
| WAC checker (default-deny, hierarchy, WAC-Allow) | `src/wac/checker.js:21–305` |
| Auth dispatch order | `src/auth/token.js:215–269` |
| Solid-OIDC DPoP verification | `src/auth/solid-oidc.js:85–251` |
| Nostr NIP-98 | `src/auth/nostr.js:133–267` |
| WebID-TLS | `src/auth/webid-tls.js:187–270` |
| did:nostr ↔ WebID | `src/auth/did-nostr.js:41–80`, `src/did/resolver.js:67–92` |
| Identity normalizer (cross-ID) | `src/auth/identity-normalizer.js:*` |
| IdP plugin + discovery | `src/idp/index.js:35–205` |
| IdP `oidc-provider` config, DCR | `src/idp/provider.js:92–455` |
| Credentials endpoint | `src/idp/index.js:218–233` |
| Passkeys | `src/idp/passkey.js`, wiring `src/idp/index.js:319–380` |
| Schnorr SSO | `src/idp/interactions.js`, referenced `src/idp/index.js:19–20` |
| N3 Patch | `src/patch/n3-patch.js:22–120` |
| SPARQL Update | `src/patch/sparql-update.js:22–82` |
| Container JSON-LD | `src/ldp/container.js:*` |
| Notifications `solid-0.1` | `src/notifications/websocket.js:39–145, 203–245` |
| Live-reload script injection | `src/handlers/resource.js:23–35` |
| Filesystem storage + ETag | `src/storage/filesystem.js:13–60` |
| Quota | `src/storage/quota.js:*` |
| Git HTTP backend | `src/handlers/git.js:11–268` |
| Git WAC hook | `src/server.js:286–314` |
| Nostr relay NIP-01/11/16 | `src/nostr/relay.js:95–286` |
| ActivityPub plugin | `src/ap/index.js:27–176` |
| AP inbox (HTTP sig verify) | `src/ap/routes/inbox.js:57–248` |
| AP outbox + delivery | `src/ap/routes/outbox.js:17–147` |
| WebFinger + NodeInfo | `src/ap/index.js:80–153` |
| Rate limits (pod create, write, login) | `src/server.js:209–219, 356–364, 436–446`; `src/idp/index.js:223–232, 304–315` |
| SSRF guard | `src/utils/ssrf.js:15–157` |
| Dotfile allowlist | `src/server.js:265–281` |
