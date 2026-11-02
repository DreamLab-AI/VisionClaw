---
id: ADR-2058
title: Accept WebSocket bearer tokens only from the Authorization header
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: b00c28a0d766c8cf46cd00b100dab60ef2dd74a4
verified_paths: []
owner: jjohare
review_trigger: Any new WebSocket endpoint, or any request to re-enable query-string auth for a client that cannot set headers
repo: visionclaw
---

# ADR-2058 — Accept WebSocket bearer tokens only from the Authorization header

## External review note, 2026-11-02

The [reconciliation](../reference/external-review-2026-11-02.md) records the remaining XR
consumer mismatch at `e78210c4d`: `transport.rs:95` supplies a URL to connect,
then authenticates after upgrade; release `http_handler.rs:124-175` requires an
Origin and an accepted upgrade credential. The later signed NIP-98 subprotocol
path is implemented server-side. This observation does not relax the release
gate or claim a deployed headset regression.

## Context

The `/wss` upgrade accepted a bearer token from either the `Authorization` header or a
`?token=` query parameter. There are **four** such extraction sites in this domain, not one:
`socket_flow_handler/http_handler.rs` (the upgrade-time check, and a second query parse later
in the same handler), `fastwebsockets_handler.rs` (the fastwebsockets `/wss` path), and
`api_handler/analytics/websocket_integration.rs` (`/analytics/ws`). The first revision of this
ADR closed only the first and claimed the endpoint was closed; vc-clients reported `/wss` still
accepting `?token=`, which was correct — the fastwebsockets path was still open. Query strings are recorded by access logs, reverse-proxy logs and
`Referer` headers, so a token in the URL leaks to every hop that sees the request line —
the token is cryptographically validated, but its confidentiality is lost before
validation happens. `docs/PROTOCOL-registry.md` recorded this as a live divergence against
legacy ADR-011 and classed it medium severity, log-hygiene. The estate posture is
fail-closed (ADR-2026). Diagram VC-13.1 carries the DIVERGENCE note on the connect path.

## Decision

In a release build the `Authorization: Bearer <token>` header is the only accepted carrier
for WebSocket authentication. The `?token=` query parameter is not read. This applies to
**every** WebSocket upgrade path in this domain — all four sites listed above — not to one
handler. A rule applied to one of four entrances is not a rule.

The query-string path survives only in development builds, behind the same
`#[cfg(any(debug_assertions, feature = "dev-auth"))]` gate that guards the other auth
relaxations, and it logs a `SECURITY:` warning naming this ADR whenever it is used. A
release build compiles the fallback out entirely, so a production deployment cannot accept
it however it is configured. A release build that receives a `token=` query parameter logs
a `SECURITY:` rejection warning so the operator learns why the client failed rather than
seeing a bare 401.

Cryptographic validation via `NostrService` is unchanged, as is the existing
`ALLOW_INSECURE_DEFAULTS` dev bypass, which remains dev-gated and out of scope here.

## Consequences

Any client that authenticated by putting the token in the URL breaks against a release
build and must move the token to the header. Browser `WebSocket` constructors cannot set
headers, so a browser client that relied on `?token=` needs a different mechanism — the
existing post-connect NIP-98 `authenticate` message (kind 27235, 60-second freshness
window, single-use replay cache, `src/utils/nip98.rs:20,169`) is that mechanism and is
already live, so the practical migration is to connect and then authenticate rather than
to authenticate in the URL.

The dev shim is a deliberate, compiled-out-of-release exception rather than a runtime flag,
so it cannot be switched on in production by configuration error. It should be removed
once no development workflow depends on it; that is the review trigger.

## Verification

`cargo check -p visionclaw-server` — **exit 0, zero errors**, with all four sites gated.

`grep -rn 'k == "token"\|parts\[0\] == "token"'` across `socket_flow_handler/`,
`fastwebsockets_handler.rs` and `api_handler/analytics/` returns four hits, and each was
confirmed to sit inside a `#[cfg(any(debug_assertions, feature = "dev-auth"))]` block by
inspecting the 16 lines above it — so a release build reads none of them.

Three further `?token=` sites exist outside this domain and are **not** covered by this ADR:
`multi_mcp_websocket_handler.rs` and `mcp_relay_handler.rs` (estate), and
`client_messages_handler.rs` (vc-core). They were reported to their owners rather than
edited.

Both `cfg` arms were written and type-check under the dev configuration used in this
container (`debug_assertions` on). The release arm is text-verified only, because a
`--release` build was out of scope per the Phase 2 code rules; it is a straight-line
binding of `header_token` with a warning branch and no other behaviour.

Verification ran on the uncommitted working tree above the recorded SHA and must be re-run
at the landing commit; `verified_paths` is empty for that reason.
