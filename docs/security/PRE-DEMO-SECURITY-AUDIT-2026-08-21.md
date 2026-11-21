# Pre-Demo Security Audit — junkiejarvis.com Public Demo

**Date:** 2026-08-21
**Scope:** Public exposure of the VisionFlow/VisionClaw backend at `junkiejarvis.com` via cloudflared → nginx (`nginx.production.conf`, port 3001) → Rust backend, plus the Vite client bundle.
**Method:** Read-only source review across four dimensions — HTTP/WS endpoint authentication, auth-bypass + secrets posture, client/browser surface, and availability. Every finding below carries a source-verified verdict. No endpoints were exercised.

---

## 1. Executive call: **NO-GO**

Do **not** expose `junkiejarvis.com` to the public in its current configuration.

The deployment has one structural defect that collapses the whole authorisation model: **authentication is self-service.** Any visitor can generate a Nostr keypair in-browser and sign a NIP-98 header; the backend auto-registers that key on first use and treats "holds a valid signature" as "is an authorised Authenticated user". Every mutating route that gates on the bare `AuthenticatedUser` extractor (85 of them) is therefore open to any anonymous visitor — including agent/swarm spawn, task submission, server-global settings writes, and GPU compute triggers.

Layered on top of that, several write and compute endpoints have **no authentication at all** (mock-agent injection, canary latch, KG governance writeback, OWL reasoner, NL→Cypher translation, and three secondary WebSocket upgrades that accept any non-empty token).

The good news, and the reason this is fixable before a demo rather than a rebuild:
- The old `SETTINGS_AUTH_BYPASS` / `dev-session-token` bypass is **genuinely remediated** — compile-time stripped from release builds and boot-refused by `enforce_release_env_hygiene()`. It is not a live vector.
- The `power_user()` allowlist tier still holds: `WriteSettings`/`Admin` routes correctly reject self-minted keys. The blast radius is bounded to the `Authenticated` tier and the unauthenticated routes.
- No secrets are committed; the tunnel token is env-injected.

The fix is concentrated: stop equating a self-signed identity with write authorisation, and close the handful of no-auth routes. A minimal, well-scoped fix list (Section 4) makes the demo safe to expose. Everything else is hardening (Section 5).

---

## 2. Confirmed findings by severity

### CRITICAL

#### C1 — Self-service NIP-98 auth turns 85 mutating routes into anonymous agent-spawn / task-submission / settings-write
*(dimension: auth bypass + secrets)*

**Evidence**
- `src/services/nostr_service.rs:634-680` `get_or_create_user_from_pubkey()` — a valid NIP-98 signature over *any* self-generated keypair creates an authenticated user + session; there is **no** allowlist check in the auth path. `validate_nip98_token` (`src/utils/nip98.rs:205`) only checks signature/timestamp/url/method.
- `src/config/feature_access.rs:38-52` `register_new_user()` unconditionally pushes every new pubkey into `approved_pubkeys` **and** `openai_enabled` **and** `ragflow_enabled`. The `APPROVED_PUBKEYS` env allowlist is a seed only; the register path auto-grants paid-API feature access to any caller.
- 85 handler params take the bare `AuthenticatedUser` extractor and discard it (`_auth`/`_user`); only 8 sites call `require_power_user()`.
- `src/handlers/bots_handler.rs:401` `spawn_agent_hybrid`, `:251` `initialize_hive_mind_swarm`, `:700` `submit_task`, `:791` `interrupt_task`, `:597` `remove_task` — all `_auth: AuthenticatedUser` (discarded), mounted at `/api/bots/*`.
> Note 2026-11-21: the `settings_handler` module cited in the next bullet was never mounted and has been deleted.
- `src/handlers/settings_handler/write_handlers.rs:20` `update_settings` takes `_user: AuthenticatedUser` (discarded), never calls `verify_write_settings` — despite the `AccessLevel::WriteSettings` machinery existing and being unit-tested (`src/utils/auth.rs:284-300`).
- Reachable: `config.yml` tunnels `junkiejarvis.com` → `visionclaw-server:3001`; `nginx.production.conf:106` proxies all `/api/` to the backend.

**Attacker story.** A hostile visitor loads `junkiejarvis.com`, generates a Nostr keypair in-browser (zero cost, no registration), signs a fresh NIP-98 `Authorization` header, and drives the agent-orchestration control plane (`POST /api/bots/spawn-agent-hybrid`, `/initialize-swarm`, `/submit-task`) — anonymous agent execution with attacker-authored task text. The same passport rewrites server-global settings (`POST /api/settings/*`) and auto-grants OpenAI + RAGFlow access, burning the operator's paid-API budget. This is **not** the old `SETTINGS_AUTH_BYPASS` (that is genuinely fixed) — it is the ordinary NIP-98 path, which survives release builds.

**Fix.** (1) Enforce an explicit allowlist / `feature_access.has_access()` check inside `get_or_create_user_from_pubkey` so an unknown key is authenticated-but-**unauthorised**, and stop `register_new_user()` auto-appending to `approved`/`openai`/`ragflow`. (2) Replace every discarded `_auth`/`_user` on destructive routes with an explicit `auth.require_power_user()?` (or a `RequirePowerUser` extractor). (3) For the public demo specifically, run all mutating scopes behind a **server-side** `PUBLIC_DEMO=read-only` flag that returns 403 on non-safe methods — the client demo gate is cosmetic only.

> **Note on granularity.** Dimension-1 findings HIGH-1 through HIGH-4 below are the specific route families exposed *through* this single primitive. Fixing C1 (gate mutations at `power_user()` or a server-side read-only demo mode) closes all four at once. They are listed separately because each also warrants route-level hardening.

---

### HIGH

#### H1 — Any self-registered visitor can WriteGraph via the Authenticated tier
*(dimension: HTTP/WS auth)*

`src/services/nostr_service.rs:405-437` (login) and `:637-660` (`verify_nip98_auth`, auto-register on first use) mint an `Authenticated` session for any keypair. `src/utils/auth.rs` `verify_access` assigns non-power users `AccessLevel::Authenticated`, and `src/middleware/auth.rs:237-239` confirms `Authenticated.has_permission(&WriteGraph)` is `true`. Real mutating endpoints gate at exactly this tier: `workspace_handler.rs:35`, `decision_handler.rs:327` (`/propose`), `ontology_agent_handler.rs:446`, `graph_export_handler.rs:321`, `analytics/mod.rs:170`.
**Attacker story.** Three lines of `nostr-tools` in-browser, per-request NIP-98 signing → immediately Authenticated with WriteGraph, no approval or shared secret.
**Fix.** Gate mutating endpoints at `power_user()` for the demo, or add the `PUBLIC_DEMO=read-only` middleware. Do not equate a valid self-signed identity with write authorisation.

#### H2 — Global visual/physics/rendering settings are mutable by any visitor and broadcast live to every viewer
*(dimension: HTTP/WS auth)*

`src/settings/api/settings_routes.rs:1714-1727` registers `PUT physics/constraints/rendering/node-filter/quality-gates/visual` under `/api/settings`. `update_physics_settings` (`:472`) and siblings take `auth: AuthenticatedUser` and never call `require_power_user()` (grep confirms zero hits in `settings_routes.rs`/`write_handlers.rs`). Handlers mutate the **server-global** snapshot via `state.settings_addr` `GetSettings`/set — not a per-user record — and writes reheat the layout and broadcast morphing to all clients. Only rate-limited (`main.rs:927-931`, `RateLimit::per_minute(60)`), not authorised.
**Attacker story.** Self-register, then `PUT /api/settings/visual` / `/physics` with hostile values (extreme spring/gravity, hidden nodes, garish colours) → every viewer sees the shared graph deface itself / collapse in real time. Repeated writes = persistent live-defacement + physics-DoS.
**Fix.** Escalate all `/api/settings/*` write handlers to `require_power_user()` (extractor already present), or make settings per-session, or block writes in the demo profile.

#### H3 — Agent-control / swarm endpoints drive real backend agent execution for any visitor
*(dimension: HTTP/WS auth)*

`src/handlers/api_handler/bots/mod.rs:12-30` registers POST `/api/bots/data`, `/update`, `/initialize-swarm`, `/settings-command`, `/spawn-agent-hybrid`, `/submit-task`, `/interrupt`. All handlers gate only at `_auth: AuthenticatedUser` (`bots_handler.rs:401/700/792/508/251/192`); zero `require_power_user`/`is_power_user` in the file. `spawn_agent_hybrid` builds a `CreateTask` with attacker-controlled `agent_type`/`swarm_id` → `TaskOrchestratorActor` → agentbox `/v1/tasks`; `submit_task` dispatches arbitrary attacker `req.task` prose.
**Attacker story.** Self-register, loop `POST /api/bots/spawn-agent-hybrid` + `/submit-task` → spawns agents and queues attacker-authored tasks, consuming host CPU/GPU/agent-runtime and any downstream LLM/MCP cost.
**Fix.** Move all bots spawn/submit/interrupt/initialize/update handlers to `require_power_user()`; in the demo profile expose only the read-only `/status`, `/agents`, `/task-status` GETs.

#### H4 — GPU analytics/compute triggers reachable by any visitor (shared-GPU exhaustion DoS)
*(dimension: HTTP/WS auth)*

`src/handlers/api_handler/analytics/mod.rs:169-170` wraps the whole `/analytics` scope in `RequireAuth::authenticated().mutations_only()` — every POST needs only a self-minted session. Expensive compute POSTs under that gate: `/clustering/run`, `/sssp/compute`, `/stress-majorization/trigger`, `/pagerank/compute`, `/pathfinding/sssp`, `/pathfinding/apsp` (all-pairs, O(V²) on GPU), `/pathfinding/path`, `/pathfinding/connected-components`, `/kernel-mode` (switches GPU compute mode). Handlers carry no own extractor. The same GPU serves the live position broadcast for all viewers, and **there is no rate limit on `/analytics`** (`RateLimit` is on `/settings` only; the settings-driven limiter is "not yet wired", `app_state.rs:932-934`).
**Attacker story.** Self-register and loop `POST /api/analytics/pathfinding/apsp` + `/pagerank/compute` → heavy GPU work on the single shared device that also runs the live layout, starving the real-time stream and freezing the visualisation for every visitor. `/kernel-mode` flips the GPU mode out from under the live graph.
**Fix.** Gate compute-triggering POSTs at `power_user()`, and/or rate-limit + queue-bound per-IP and cap APSP/pagerank by node count; demo profile exposes only read-only analytics GETs.

#### H5 — Fully anonymous (no NIP-98 at all) mutating/compute endpoints: KG governance writeback, OWL reasoner, LLM translation
*(dimension: auth bypass + secrets)*

No auth extractor at all on these:
- `src/handlers/ingest_writeback_handler.rs:75-100` `writeback()` — takes the approver identity from the request body (`attribution_pubkey(decision.approved_by)`; `attribution_pubkey` only format-checks `is_pubkey_hex` — no signature/ownership check) then performs a real fenced Oxigraph `:summary` write via `apply_decision()`. An anonymous caller approves/rejects any `case_id` and forges the `approved_by` pubkey.
- `src/handlers/inference_handler.rs:69/111/285` `run_inference`/`batch_inference`/`invalidate_cache` (DELETE) — drives the Whelk OWL reasoner and cache invalidation anonymously.
- `src/handlers/natural_language_query_handler.rs:90` `translate_query` — invokes the LLM (Loom/model) on attacker text.
- `src/handlers/multi_mcp_websocket_handler.rs:848` `refresh_mcp_discovery` — triggers MCP rediscovery.
- No global `/api` auth middleware (`main.rs:849-852` = Logger/cors/Compress/Timeout only); rate limit is scoped to `/settings` alone.

**Attacker story.** Unauthenticated loop of `POST /api/nl-query/translate` + `POST /api/inference/run` burns LLM/reasoner/model-budget compute with no throttle. `POST /api/ingest/writeback` applies governance decisions to any `case_id` stamped as "approved_by" any pubkey, silently corrupting the KG governance/broker state the demo showcases.
**Fix.** Require power-user auth on writeback/inference/nl-query/mcp-refresh. For writeback, derive the approver from the **verified** NIP-98 pubkey, never the request body. Move `RateLimit` up to the whole `/api` scope (or per-endpoint on LLM/reasoner routes). Disable reasoner/LLM/writeback behind the server-side demo flag.

---

### MEDIUM

#### M1 — Unauthenticated mock-agent injection pollutes the shared graph
*(dimension: HTTP/WS auth)*

`src/handlers/bots_visualization_handler.rs:512` registers `POST /api/bots/mock-agents → inject_mock_agents` with **no** `RequireAuth` and **no** extractor (handler sig `:340` is body + `app_state` only). It mutates shared state — `graph_service_addr.do_send(UpdateBotsGraph{agents})` (`:450`) plus per-edge `AddEdge` (`:477`) — and injected nodes surface in `get_agent_visualization_snapshot`. Mounted in the public `/api` scope (`main.rs:975`), which has no scope-level auth.
**Attacker story.** With no credentials at all, `POST /api/bots/mock-agents` with attacker-labelled agents/positions → they appear as agent nodes in the graph every viewer sees. Repeatable → spam.
**Correction to prior wording.** The sibling `POST /api/visualization/swarm/initialize` (`:287`) is a pure stub (logs + canned JSON, no state mutation) — an unauthenticated no-op, not itself a pollution vector. Gate it anyway.
**Fix.** Wrap `/api/bots/mock-agents` in `RequireAuth::power_user()` or compile mock-injection out of release builds.

#### M2 — Unauthenticated canary latch tampering leaks build SHA and corrupts liveness/gap-closure evidence
*(dimension: HTTP/WS auth)*

`src/handlers/liveness_harness_handler.rs:129-135` registers `/canary/register` (POST), `/canary/observe/{id}` (POST), `/canary/status` (GET) with **no** `RequireAuth`. `observe` (`:94`) records attacker-supplied `evidence` via the one-shot fire latch (`liveness_harness.rs:314-318`). `status` (`:118`) leaks `kg_backend_up`, current SHA, and the full canary registry to anonymous callers. Mounted in the public `/api` scope (`main.rs:1012`); it is the only unguarded write scope in the codebase (every comparable surface gates auth). Reachable via `nginx.conf:188` / `nginx.production.conf:106`.
**Attacker story.** Anonymous `GET /api/canary/status` reads the registry, then `POST /api/canary/observe/{id}` with fabricated evidence prematurely fires canaries backing the project's gap-closure/liveness reporting, or floods `/canary/register` — corrupting the internal QE truth source while leaking build SHA + backend health.
**Fix.** Gate `/api/canary/*` writes at `power_user()` and treat as internal-only — bind to an internal listener or add `location = /api/canary { deny all; }` in the nginx/cloudflared configs.

#### M3 — Production CSP is neutered by `'unsafe-inline'` + wildcard `connect-src`/`img-src`
*(dimension: client + browser)*

`nginx.production.conf:100` (deployed via `Dockerfile.production:224`): `script-src 'self' 'unsafe-inline' https://esm.sh https://javascriptsolidserver.github.io https://cdn.jsdelivr.net https://unpkg.com https://getalby.com https://goal.ruv.io`; `connect-src 'self' wss: https: https://esm.sh`; `img-src 'self' data: https:`. `'unsafe-inline'` means CSP provides no XSS mitigation; bare `https:`/`wss:` permit fetch/XHR/WS exfiltration to any host; no `object-src 'none'`, no `upgrade-insecure-requests`. Three of the six trusted script origins (esm.sh, unpkg, jsdelivr) serve arbitrary npm — a poisoned package runs with full origin privilege on the public demo (this part is exploitable without an app-side foothold). The line-99 `# TODO: Replace 'unsafe-inline'` comment acknowledges the debt.
**Attacker story.** Any injection foothold runs (`'unsafe-inline'`) and streams stolen state to any attacker `https`/`wss` endpoint (wildcard). The CDN anchors are standing supply-chain risk.
**Fix.** Drop `'unsafe-inline'` (nonces/hashes); self-host or SRI-pin exact CDN URLs; constrain `connect-src`/`img-src` to specific hosts; add `object-src 'none'`.
*(Defence-in-depth: no independent app-side foothold is asserted here, hence MEDIUM.)*

#### M4 — Secondary WebSocket endpoints accept any non-empty token — unbounded unauthenticated actor allocation
*(dimension: availability)*

`speech_socket_handler.rs:808-834`, `mcp_relay_handler.rs:451-477`, `client_messages_handler.rs:113-143` all share one weak gate: `token.as_deref().unwrap_or("").is_empty()` — any non-empty value (e.g. `?token=x`) passes with zero cryptographic check, then unconditionally `ws::start(...)` spawns a per-connection actor + uuid socket id. Contrast `/wss` (`socket_flow_handler/http_handler.rs:152-256`) which validates the token via `nostr_service.get_session(t)` and rejects in release builds. The weak gate is **not** build-gated (present in release). Mounted at root scope (`main.rs:909-915`) with no auth wrapper and no per-IP connection cap. In-source comments ("Currently allows but logs unauthenticated connections") overstate enforcement.
**Attacker story.** Open thousands of `wss://junkiejarvis.com/ws/speech?token=x` (or `/ws/mcp-relay`, `/ws/client-messages`) with bogus tokens → every upgrade succeeds, allocating actors/sockets/buffers with no ceiling, exhausting fds/memory.
**Fix.** Validate the token cryptographically at upgrade (reuse the `/wss` `get_session` path) and reject on failure; add a per-client connection cap and per-IP rate limiter.

#### M5 — Unauthenticated, unthrottled NL→Cypher translation endpoint
*(dimension: availability)*

`natural_language_query_handler.rs:90-105` `translate_query` — no auth extractor, only `web::Json`; runs `translate_to_cypher()`/`suggest_queries()` on attacker text. Model-backed: `NaturalLanguageQueryService` → `PerplexityService.chat_completion` → external LLM. Registered under `/api` (`main.rs:959`) with no `RateLimit` wrapper; `request.query` bounded only by the default Json body limit.
**Attacker story.** Flood `POST /api/nl-query/translate` → unauthenticated, unthrottled schema-context build + prompt construction every request; external inference cost if Perplexity creds are configured (compute-DoS regardless).
**Fix.** Require auth (or a strict per-caller rate limit + small max query length) on `/api/nl-query/*`; treat model-backed translation as an expensive endpoint behind the corrected rate limiter.

---

### LOW

#### L1 — Unauthenticated client-log ingestion with log-forging (`POST /api/client-logs`)
*(dimension: HTTP/WS auth)*

`main.rs:926` registers `POST /api/client-logs → handle_client_logs` directly under `/api`, before any auth wrap, no extractor, no rate limit (registered before the `/settings` sub-scope that carries `RateLimit`). The handler (`client_log_handler.rs:110-168`) appends caller-controlled `message`/`stack`/`namespace`/`url`/`data`/`session_id` into newline-delimited `/app/logs/client.log` with **no** escaping of embedded newlines/control chars — a genuine CWE-117 log-forging surface. `MAX_LOG_ENTRIES=1000` caps entries per request (413 over) but per-entry length is unbounded up to the Json body limit and there is no per-IP throttle.
**Fix.** Sanitise newline/control chars in log contents; add per-IP rate limiting; cap payload size; optionally require a session in the demo profile. (The entry-count DoS cap is already implemented.)

#### L2 — `nodePageUrl` passes attacker-influenceable metadata URL verbatim into `window.open()`
*(dimension: client + browser)*

`client/src/features/graph/utils/pageLinks.ts:38-39` returns the raw `meta.page_url ?? meta.pageUrl ?? meta.url` with no protocol validation (the slug branch is `encodeURIComponent`-safe; this explicit branch bypasses it). Sinks: `GraphManager.tsx:504` and `NodeDetailPanel.tsx:79` `window.open(url, '_blank', 'noopener,noreferrer')`. Node metadata originates from the GitHub KG corpus. **LOW for the demo**: a visitor cannot write the read-only corpus, and modern browsers already neuter `javascript:` in `window.open` / open `data:` as an opaque origin. Becomes material only if a KG/ontology write path is exposed. Contrast the correct allowlist in `MarkdownRenderer.tsx:47-54`.
**Fix.** Apply the `InteractiveLink` allowlist (`new URL()`, permit only `http:`/`https:`/`mailto:`, else return null) to the explicit branch.

#### L3 — Committed `config.yml`: debug tunnel logging + internal topology disclosure (no secrets baked)
*(dimension: auth bypass + secrets)*

`config.yml` (committed) sets `loglevel: debug`, hardcodes ingress (`junkiejarvis.com → http://visionclaw-server:3001`, tunnel `logseqXR`, `noTLSVerify: true`). **No credential is committed** — `CLOUDFLARE_TUNNEL_TOKEN` is env-injected with a required `:?` guard and the image is digest-pinned. A targeted secret grep over the committed nginx/compose/Dockerfile configs found zero tokens; `env.production.template` ships all secret fields blank. Separately, `nginx.production.conf` CORS map (lines 60-66) allowlists `visionclaw.info`/`localhost` but **not** `junkiejarvis.com`, so `/solid/*` CORS silently fails for the real demo domain (functional, not a leak).
**Fix.** Set `config.yml` `loglevel` to `info`/`warn`; move internal topology to dashboard-managed ingress (the compose file already documents this path); align the `$cors_origin` map to `junkiejarvis.com`.

#### L4 — CORS prod hardening guard is inert because `APP_ENV` is never set (config-drift nit)
*(dimension: auth bypass + secrets — downgraded from the original MEDIUM claim)*

`main.rs:808` gates the same-host `allowed_origin_fn` on `APP_ENV == "production"` (case-sensitive), but no config sets `APP_ENV` (prod sets `NODE_ENV`, which this check does not read), so the same-host origin fn stays enabled and `localhost:3000/3001` sit in a credentialed allowlist. **Not an executable theft vector**: the same-host fn only reflects an Origin whose hostname equals the request Host (an `evil.com` page is rejected), and the app authenticates with NIP-98 `Authorization` headers, not ambient cookies, so `.supports_credentials()` confers no CORS theft. (The prior claim that this also disables a required-env hard-fail is **wrong** — `JWT_SECRET`/`CORS`/`MANAGEMENT_API_KEY` are in the `recommended` array that only warns regardless.)
**Fix.** Set `APP_ENV=production` (better: key prod-detection off the release `cfg` used by `enforce_release_env_hygiene`) and drop `localhost` from the prod `CORS_ALLOWED_ORIGINS` default.

---

### Informational (verified state, no live vector)

- **INFO-1 — `SETTINGS_AUTH_BYPASS` / dev-session-token: VERIFIED FIXED.** `auth_extractor.rs:20-47`/`:187-201` dev bypasses are `#[cfg(any(debug_assertions, feature="dev-auth"))]` — release compiles a None-returning stub. `main.rs:114-160` `enforce_release_env_hygiene()` hard-exits (status 2) if `SETTINGS_AUTH_BYPASS`/`ALLOW_INSECURE_DEFAULTS`/`VISIONCLAW_DEV_MODE` are present in a release binary, before logging init. `Dockerfile.production:153` builds `cargo build --release` with no `dev-auth`; `tests/auth_bypass_release.rs` asserts the strings are absent. No attacker path for a correctly-built image. *Recommendation:* add a CI/startup build-provenance assertion so the demo can never ship a debug binary.
- **INFO-2 — Client XSS/secret-leak vectors: near-clean bill.** No `dangerouslySetInnerHTML` in `client/src`; labels/metadata render as auto-escaped React text or WebGL glyphs; `react-markdown` runs without `rehype-raw`. Dev-auth bypass paths are dead in the production build (`import.meta.env.DEV` is false under `vite build`). **Standing item:** any `VITE_*_TOKEN` set at build time (e.g. `VITE_VIRCADIA_AUTH_TOKEN`) inlines into the public bundle — keep them empty and move real secrets to non-`VITE` backend env.

---

## 3. Refuted-findings appendix (so they are not re-found)

| ID | Claim | Verdict | Why |
|----|-------|---------|-----|
| R1 | "Auth posture is deployment-contingent; a debug container behind the tunnel gives `dev-session-token` power-user + tokenless `/wss`" (originally INFORMATIONAL) | **Refuted as executable** | Dev bypass and `/wss` anonymous acceptance are `#[cfg(debug_assertions/dev-auth)]`-stripped in release; `enforce_release_env_hygiene()` boot-refuses suspect env; `Dockerfile.production` builds release. Requires deploying the `launch.sh up dev` debug container behind the prod tunnel — not the configured demo. **Latent gap kept:** `docker-compose.unified.yml` gives dev and prod services the same `visionclaw-server` network alias, so the tunnel resolves to whichever answers — pin the tunnel to the prod container (see backlog). |
| R2 | "APP_ENV gap enables credentialed-CORS theft + disables required-env hard-fail" (originally MEDIUM) | **Refuted / downgraded to L4** | Same-host fn never trusts a cross-origin attacker origin; NIP-98 header auth is not ambient-credential so no CORS theft. The required-env hard-fail claim is factually wrong (`recommended` array only warns). Survives as a LOW config nit. |
| R3 | "VITE_PUBLIC_DEMO gate = anonymous read/write" (originally HIGH) | **Refuted / downgraded to LOW hardening** | Client claims are accurate (demo mode mounts live-but-inert mutating controls, sends no `Authorization` header), but the exploitable part is self-admittedly conditional on surviving backend gaps and does not prove a single unauthenticated mutating endpoint against this build. Real defence-in-depth/UX weakness (make demo a genuine read-only client posture), not an executable vuln on its own. |
| R4 | "`/api/physics/optimize` unbounded `max_iterations` = CRITICAL GPU/CPU DoS" | **Refuted as CRITICAL → actual LOW** | The actor `Handler<SimulateUntilConvergenceMessage>` (`actix_physics_adapter.rs:373-391`) is a **no-op stub**: logs one line, returns `converged:true`, never reads `max_iterations`; `compute_forces`/`update_positions` return empty vecs so the O(N²) loop iterates over nothing; a 30s timeout bounds every call and the await yields the worker. No compute, no exhaustion. **Real residual (LOW):** `optimize_layout`/`perform_step`/`unpin_nodes` genuinely omit the `require_power_user()` their siblings use — an auth-consistency defect worth fixing before a real engine lands, plus clamp `max_iterations`. |
| R5 | "Per-IP rate limiting collapses to 127.0.0.1 because CF-Connecting-IP is ignored" (originally HIGH) | **Refuted** | The evidence cites `nginx.conf` (dev). The public demo runs the prod profile: cloudflared depends on `visionclaw-production` (`docker-compose.unified.yml:202-235`), which ships `nginx.production.conf` setting `X-Real-IP $http_cf_connecting_ip` on every route (lines 118/147/165/210/239). `extract_client_id` reads `X-Real-IP` first → distinct real IP per client. The prod config already implements the recommended fix; the XFF-spoof fallback is unreachable because nginx unconditionally sets `X-Real-IP`. |

---

## 4. Minimal pre-demo fix list — MUST land before exposure

These close every CONFIRMED CRITICAL/HIGH/MEDIUM live vector. Ordered by leverage.

1. **Kill self-service authorisation for mutations (closes C1, H1, H2, H3, H4).** Either (a) add a server-side `PUBLIC_DEMO=read-only` middleware on the `/api` scope that rejects all non-safe (non-GET/HEAD/OPTIONS) methods with 403; **or** (b) escalate every mutating handler from the discarded `_auth: AuthenticatedUser` to an explicit `auth.require_power_user()?` — settings writes, bots spawn/submit/interrupt/initialize/update, and analytics compute POSTs. Option (a) is the smallest, most reliable change for a demo.
2. **Enforce an allowlist at authentication (closes the C1 auto-grant).** In `get_or_create_user_from_pubkey`, reject or mark authenticated-but-unauthorised any pubkey not in `APPROVED_PUBKEYS`; stop `register_new_user()` auto-appending to `approved_pubkeys`/`openai_enabled`/`ragflow_enabled`.
3. **Add auth to the no-auth write/compute routes (closes M1, M2, H5, M5).** Gate `/api/bots/mock-agents`, `/api/visualization/swarm/initialize`, `/api/canary/*` writes, `/api/ingest/writeback`, `/api/inference/*`, `/api/nl-query/*`, and `refresh_mcp_discovery` at `power_user()` — or disable them in the demo profile. For `writeback`, derive the approver from the **verified** NIP-98 pubkey, never `decision.approved_by`.
4. **Secure the secondary WebSocket upgrades (closes M4).** Replace the `is_empty()` check in `speech_socket_handler`, `mcp_relay_handler`, `client_messages_handler` with `nostr_service.get_session(t)` validation (the `/wss` path) and add a per-client connection cap.
5. **Rate-limit the whole `/api` scope, not just `/settings`.** Move/extend `RateLimit` (with per-endpoint caps on the LLM/reasoner/analytics/compute routes) so unauthenticated floods are bounded even where a route stays public.
6. **Restrict the canary surface at the edge.** Add `location = /api/canary { deny all; }` (or bind it to an internal listener) in `nginx.production.conf` / cloudflared so the tunnel never exposes liveness internals + build SHA.

---

## 5. Hardening backlog (post-demo, not blocking)

- **CSP (M3):** drop `'unsafe-inline'` for nonces/hashes; SRI-pin or self-host the six CDN script origins; constrain `connect-src`/`img-src` to real hosts; add `object-src 'none'`.
- **Client read-only demo posture (R3):** when `isPublicDemo`, disable/hide mutating controls (settings, command palette, graph edits) rather than only skipping the login screen.
- **`nodePageUrl` scheme allowlist (L2):** apply the `InteractiveLink` `new URL()` + `http/https/mailto` allowlist to the explicit branch.
- **Client-log ingestion (L1):** sanitise newline/control chars, add per-IP rate limit + payload cap, optionally require a session in the demo.
- **`config.yml` (L3):** `loglevel: info`; move ingress topology to dashboard-managed; align the nginx CORS `$cors_origin` map to `junkiejarvis.com`.
- **CORS/env (L4):** set `APP_ENV=production` (or key prod-detection off the release `cfg`); drop `localhost` from the prod CORS default.
- **Physics auth-consistency (R4 residual):** add `require_power_user()` to `optimize_layout`/`perform_step`/`unpin_nodes` and hard-clamp `max_iterations` before the (future real) engine lands.
- **Deployment pinning (R1 latent gap):** give dev and prod distinct network aliases so the cloudflared tunnel can only resolve the prod container; add a CI/startup build-provenance assertion that the shipped image is a release build without `dev-auth` (INFO-1); add edge `limit_except GET` on the exposed `/api/` location for the read-only demo.
- **Build hygiene (INFO-2):** keep every `VITE_*_TOKEN` empty in the demo build; move any real secret to a non-`VITE` backend env var; optional build-time assert that `DEV===false` and no `VITE_DEV_*` is set.
- **Defence-in-depth (R5):** keep `X-Real-IP $http_cf_connecting_ip` in the prod nginx (already correct) and prefer `CF-Connecting-IP` in `extract_client_id` as belt-and-braces; never trust the first `X-Forwarded-For` element.

---

*Prepared read-only. No endpoints were exercised against the live demo. Line references are to the source tree as reviewed on 2026-08-21.*
