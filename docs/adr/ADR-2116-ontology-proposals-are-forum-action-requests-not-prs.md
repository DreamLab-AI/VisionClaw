---
id: ADR-2116
title: Ontology proposals are forum ActionRequests, not pull requests — /api/ontology-agent/propose is retired
date: 2026-09-22
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: a32abac57f3a7cfe66ab68ea1b0faca013c0d6b2
verified_paths: [src/handlers/ontology_agent_handler.rs, tests/rec1_route_guard.rs]
owner: jjohare
review_trigger: the `vault` binary landing (WS-C), or the first caller reported still POSTing to /api/ontology-agent/propose after the agentbox MCP servers are deleted (WS-G)
repo: visionclaw
---

# ADR-2116 — Ontology proposals are forum ActionRequests, not pull requests

## Context

`POST /api/ontology-agent/propose` was the only sanctioned ontology write path (ADR-120, WS-1): an agent posted a proposal, the Whelk consistency gate ran, and an approved proposal became a pull request against the corpus repository. Its one HTTP consumer was agentbox's `mcp/servers/ontology-propose.js`. VisionFlow `PRD-sovereign-corpus` §3.3 replaces that loop entirely: `vault propose` builds a `PatchProposal`, runs Whelk and the conflict detector as **blockers**, and posts a forum kind-31402; a human's signed kind-31403 carries `Promote{iri}` or `Demote{iri}`; agentbox applies it through a guarded `vault edit`. A pull request is nobody's signature — it carries no `did:nostr`, no rationale requirement and no receipt ladder — so the PR path is not deprecated-but-working. It is gone.

## Decision

1. **`/api/ontology-agent/propose` answers 410 Gone, to every caller, always.** The body names its replacement: the `vault propose` command line, the decision outcomes, the apply path, and the four records that explain them. A 404 would read as a deployment fault and get retried; a 410 is terminal and names its successor.

2. **The auth and rate-limit middleware come off with it.** The nested `/propose` sub-scope, `RequireAuth::authenticated()` and `RateLimit::per_minute(20)` are removed and `/propose` becomes a plain route. A 401 on a route that can only refuse would tell a caller its credentials were the problem, which is false, and would invite exactly the retry loop the 410 exists to stop. Every route on the `/ontology-agent` surface is now anonymous because every route on it is now read-only.

3. **Shared code is untouched.** `OntologyMutationService` and its three error prefixes (`CONFLICT_BLOCKED_PREFIX`, `ENVELOPE_REJECTED_PREFIX`, `IDEMPOTENCY_CONFLICT_PREFIX`) are no longer imported by this handler but are not removed: `decision_handler.rs` and `decision_service.rs` still use them. `ProposeInput` and `AgentContext` stay in `visionclaw-domain`. `OntologyQueryService` and `build_traversal` serve the five surviving read routes.

4. **`ProposeRequest` is deleted, not parked.** It had exactly one reader, and a DTO nothing deserialises is a standing claim that the route still accepts something.

5. **`ontology_propose` is removed from `GET /ontology-agent/status`'s capability list**, because advertising it would point an agent at a 410.

6. **The REC-1 route guard is inverted, not deleted.** It asserted the route stayed auth-gated; it now asserts the route stays retired — 410 to authenticated and unauthenticated callers alike, with a body that names `vault propose`. That is the property a later refactor could silently break by re-introducing a write path. `/ontology/{load,load-axioms}` are unchanged and still gated `power_user().mutations_only()`.

## Consequences

- Ontology writes leave HTTP entirely. There is no authenticated REST mutation of the corpus; there is a signed Nostr event and a CLI. An agent that wants to change the corpus must produce something a human can sign.
- The callers still holding this URL — agentbox's `ontology-propose.js` and `ontology-bridge.js`, the `ontology-augment` and `podcast-knowledge-ingest` skills, the `ontology-curator` agent — get a 410 that tells them what to run instead until WS-G deletes and rewrites them. That is the point of keeping the route.
- Documentation drift is now visible rather than latent: `docs/reference/mcp-tools.md`, `rest-api.md` and `agents-catalog.md` still describe the propose tool, and `docs/reference/rest-api.md:719` already cited the wrong file. Correcting them is WS-G/WS-A work, tracked by this ADR's `review_trigger` rather than done here.
- **Staged, not live**: the replacement the 410 points at (`vault propose`) does not exist yet. Between this change and WS-C landing, the corpus has no agent write path at all. That is deliberate and short-lived — the alternative is leaving a PR path running that the PRD has already replaced.

## Verification

`implementation_status: partial` — the retirement is complete; the replacement it names is WS-C's.

At `verified_commit`:

- `cargo check -p visionclaw-server` — clean, no new warnings; the removed imports and DTO leave nothing dangling.
- `cargo test --test rec1_route_guard` — 5 pass: `ontology_agent_propose_is_retired_and_answers_410` (410, body `error: "route_retired"`, `replacement.command` starting `vault propose`, and an explicit assertion that the status is **not** 401/403 — a retired route must not look like an auth failure), `ontology_agent_propose_is_retired_for_authenticated_callers_too` (410 with an `Authorization` header, proving the middleware is off rather than passing), plus the three unchanged guards for the read side and the `/ontology` ingest routes.

## Re-verification — 2026-09-22 at a32abac57f3a7cfe66ab68ea1b0faca013c0d6b2

**Governed changes since `06dfe97a5`:** `src/handlers/ontology_agent_handler.rs` and `tests/rec1_route_guard.rs` now carry the retirement this record decides: `/propose` answers 410 Gone with a body naming `vault propose`, the `ProposeRequest` DTO and the handler's `OntologyMutationService`/`RequireAuth`/`RateLimit` imports are deleted, `ontology_propose` is gone from the status capability list, and the REC-1 guard asserts the route stays retired for authenticated and anonymous callers alike.

**Decision unaffected — the code caught up with the record.** `verified_commit` moved to the CI-repair commit.
