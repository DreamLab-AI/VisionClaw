# ADR-028-ext: NIP-98 as Enterprise Auth — Optional-Auth Extension

## Status

Ratified 2026-04-19

Extension of: ADR-011 (Universal Authentication Enforcement), ADR-040 (Enterprise
Identity Strategy), ADR-048 (Dual-Tier Identity Model).

This ADR does not extend ADR-028 (SPARQL PATCH for Ontology Mutations) — the
shared ADR number reflects the NIP-98 auth primitive lineage, not the SPARQL
topic. The file is intentionally named `-ext-` to disambiguate.

## Context

NIP-98 Schnorr auth is already the primary authentication path in the backend
at `src/utils/auth.rs:104-168`. Every request that carries
`Authorization: Nostr <base64-event>` is verified for signature validity, URL
match, method match, 60-second freshness window, and body hash binding. The
primitive is sound.

Coverage is not. Audit of the current router wiring shows material gaps:

- `/api/graph/data` (the read path that feeds the 3D graph viewer) is
  unauthenticated. The handler returns the full graph regardless of caller.
- The entire `/ontology-agent/*` scope is completely unwrapped — no middleware
  of any kind. Agents can call ontology endpoints with no identity assertion.
- Analytics auth is explicitly disabled with a `// for testing` comment and was
  never re-enabled.
- A legacy `X-Nostr-Pubkey + X-Nostr-Token` session path (lines 170-248) is
  still live. It has no body binding and no freshness window, making it
  weaker than NIP-98 and vulnerable to replay.

The sovereign-private-node model (ADR-048) requires three visibility tiers at
the read boundary:

1. **Anonymous visitors** see `visibility = public` nodes only.
2. **Signed users** see public nodes plus their own private nodes.
3. **Other users' private nodes** appear as opacified stubs (existence
   acknowledged, content redacted) — never as full content and never
   silently dropped.

None of these tiers are expressible under the current wiring, which is binary:
either the route is authenticated (and anonymous visitors are locked out of the
public graph entirely) or it is unauthenticated (and private data leaks).

## Decision Drivers

- Single auth primitive (NIP-98) must cover all entry points; no forked
  auth models inside the same process.
- Anonymous visitors must retain access to the public graph tier for the
  public-showcase use case.
- Caller identity must be available to handlers whenever present, so read
  paths can filter at the Neo4j boundary.
- Legacy auth must be deprecable without a flag-day breaking change.
- Rollout must be reversible by a single environment variable flip.

## Considered Options

### Option 1: Add `AccessLevel::Optional` + wrap missing scopes (chosen)

Introduce an `Optional` variant to `AccessLevel` and an `optional()`
constructor on `RequireAuth`. Semantics:

- No `Authorization` header → request passes through; caller pubkey is empty.
- Invalid `Authorization` header (bad sig, stale, URL mismatch, etc.) → 401.
- Valid `Authorization` header → request passes through with caller pubkey
  bound to the request extensions.

Wrap `/api/graph/data` with `RequireAuth::optional()` so anonymous visitors
get the public tier and signed users get public + own private. Wrap
`/ontology-agent/*` with `RequireAuth::authenticated()` — this scope is a
critical vulnerability today. Re-enable analytics auth. Gate the legacy
`X-Nostr-Pubkey + X-Nostr-Token` path behind `APP_ENV != production`.

Handler-side caller filtering uses a parameterised Cypher clause:

```cypher
MATCH (n:KGNode)
WHERE COALESCE(n.visibility, 'public') = 'public'
   OR ($caller IS NOT NULL AND n.owner_pubkey = $caller)
```

Gate the entire optional-auth change behind feature flag
`NIP98_OPTIONAL_AUTH=true` (default `false`).

- **Pros**: Single primitive. Anonymous public access preserved. Identity
  available when present. Legacy path has a clean deprecation route.
  Feature-flagged rollback is cheap.
- **Cons**: `/api/graph/data` response shape now depends on caller identity,
  so any cache keyed on the URL alone will return the wrong data for the
  wrong tier. Cache keys must include pubkey (or be disabled for this
  endpoint during rollout).

### Option 2: Two endpoints — `/api/graph/data/public` and `/api/graph/data/private`

Split the read path in two. Anonymous visitors hit the public endpoint;
signed users hit the private endpoint.

- **Pros**: No Optional variant needed. Cache keys remain URL-stable.
- **Cons**: Doubles handler surface. Clients must branch on auth state
  before issuing requests. Cross-user opacification requires a third
  endpoint or mixed-tier response assembly — either way the simplification
  is illusory. Route configuration duplication across analytics, graph,
  and future sovereign endpoints.

### Option 3: Require auth on everything, accept that anonymous visitors see nothing

Wrap `/api/graph/data` with `RequireAuth::authenticated()`, matching
`/ontology-agent/*`.

- **Pros**: Simplest. Single code path.
- **Cons**: Breaks public-showcase use case. VisionClaw's public graph is a
  marketing and demonstration surface; locking it behind NIP-98 means
  anonymous browser visitors see 401. Directly contradicts the
  sovereign-private-node tier model from ADR-048.

## Decision

**Option 1: `AccessLevel::Optional` + scope-level wrapping + caller-side
Cypher filter, all gated by `NIP98_OPTIONAL_AUTH`.**

Concrete changes:

1. Add `AccessLevel::Optional` variant to `src/utils/auth.rs`.
2. Add `RequireAuth::optional()` middleware constructor to
   `src/middleware/auth.rs`. Missing `Authorization` → pass-through with
   empty pubkey. Invalid `Authorization` → 401.
3. Wrap `/api/graph/data` with `RequireAuth::optional()`. Handler reads
   `caller_pubkey` from request extensions and injects into the Cypher
   `$caller` parameter.
4. Wrap `/ontology-agent/*` with `RequireAuth::authenticated()`.
5. Re-enable analytics auth (remove the `// for testing` bypass).
6. Gate the legacy `X-Nostr-Pubkey + X-Nostr-Token` path behind
   `APP_ENV != production`. In production, the legacy path returns 401
   regardless of payload.
7. Filter at Neo4j:

   ```cypher
   MATCH (n:KGNode)
   WHERE COALESCE(n.visibility, 'public') = 'public'
      OR ($caller IS NOT NULL AND n.owner_pubkey = $caller)
   ```

   Other users' private nodes are materialised as opacified stubs in a
   separate query stage, not merged into this MATCH — see the handler-side
   opacification contract in ADR-048.

8. Feature flag: `NIP98_OPTIONAL_AUTH=true|false`. Default `false` for
   rollout safety. When `false`, `optional()` behaves as `authenticated()`
   so pre-flag behaviour is preserved exactly.

## Consequences

### Positive

- Single auth primitive (NIP-98) now covers every entry point — read,
  write, ontology, analytics. No more forked auth models in the same
  process.
- Anonymous visitors keep the public-tier graph view, preserving the
  showcase use case.
- Private-data leakage at `/api/graph/data` is closed: the Cypher filter
  returns only public nodes for anonymous callers and only public + own
  private for signed callers. Cross-user private nodes are opacified at
  the handler layer, not leaked.
- `/ontology-agent/*` now requires authentication — previously this scope
  was a critical vulnerability.
- Legacy `X-Nostr-Pubkey + X-Nostr-Token` has a deprecation path: rejected
  in production, still usable in dev for tooling that has not yet migrated.

### Negative

- Clients currently relying on the legacy path in production will break
  when `NIP98_OPTIONAL_AUTH=true` ships. Mitigated by: feature flag,
  staged rollout, advance comms, and the `APP_ENV != production` gate
  that only activates the deprecation in the prod tier.
- `/api/graph/data` response content now depends on caller identity.
  Any HTTP cache (CDN, reverse proxy, browser cache) keyed on URL alone
  will serve the wrong tier. Cache keys must include a pubkey hash or
  the endpoint must set `Cache-Control: private, no-store` during
  rollout.
- Additional middleware variant grows the `AccessLevel` enum. Must be
  exhaustively matched in tests.

### Neutral

- The Cypher `COALESCE(n.visibility, 'public')` handles the case of
  existing nodes without a `visibility` property; they default to public
  and are visible to anonymous callers. This is the safe direction — no
  node accidentally becomes invisible.
- Opacified-stub generation is handled by the existing handler-layer
  visibility pipeline introduced in ADR-048; no new domain logic is
  required here.

## Compliance Criteria

- [ ] `AccessLevel::Optional` variant exists in `src/utils/auth.rs`.
- [ ] `RequireAuth::optional()` constructor exists in
      `src/middleware/auth.rs`.
- [ ] `/api/graph/data` is wrapped with `RequireAuth::optional()` and
      filters by `$caller` at the Cypher boundary.
- [ ] `/ontology-agent/*` is wrapped with `RequireAuth::authenticated()`.
- [ ] Analytics routes have authentication re-enabled.
- [ ] Legacy `X-Nostr-Pubkey + X-Nostr-Token` returns 401 when
      `APP_ENV = production`, regardless of payload.
- [ ] `tests/auth_sovereign_mesh.rs` passes the three-tier matrix:
      anonymous → public only, signed → public + own private, cross-user
      private → opacified stub.
- [ ] With `NIP98_OPTIONAL_AUTH=false`, every previously-401 response
      code path remains 401 (no behavioural drift when flag is off).

## Rollback

- Set `NIP98_OPTIONAL_AUTH=false`. `optional()` degrades to
  `authenticated()` behaviour; missing `Authorization` on wrapped routes
  returns 401 as before. Effective immediately, no restart beyond
  config reload.
- If the flag is insufficient, revert the commit. The `AccessLevel::Optional`
  variant and `optional()` constructor remove cleanly. No database migration
  is required. No client breaking change is introduced when the flag is
  `false`, so the revert window is open indefinitely.

## Related Decisions

- ADR-011: Universal Authentication Enforcement — establishes `RequireAuth`
  middleware pattern and the fail-closed rule this ADR extends.
- ADR-040: Enterprise Identity Strategy — dual-stack OIDC + Nostr model;
  NIP-98 remains the on-wire primitive for both identity paths.
- ADR-048: Dual-Tier Identity Model — defines the visibility tiers this
  ADR enforces at the read boundary.
- ADR-052: Pod Default WAC + Public Container Model — paired decision in
  the sovereign-mesh Wave 1; Pod-side WAC enforces what this ADR enforces
  at the backend read boundary.

## Implementation Note (2026-05-12)

The enterprise RBAC middleware (`src/middleware/enterprise_auth.rs`) now implements
a compile-time dual-path model complementary to this ADR's optional-auth
extension:

- **`nip98-auth` feature gate**: When enabled, the `RequireRole` middleware reads
  `Authorization: Nostr <base64>`, verifies the NIP-98 signature, and resolves
  the pubkey to an `EnterpriseRole` via `Nip98RoleResolver`. This path replaces
  the `X-Enterprise-Role` header extraction when a resolver is attached.

- **Default path**: Without the feature gate, `RequireRole` reads
  `X-Enterprise-Role` from request headers (dev/gateway mode).

This is orthogonal to the `AccessLevel::Optional` variant described in this ADR:
the optional-auth extension controls whether authentication is required or
optional on a given route, while the `nip98-auth` feature gate controls how
the enterprise role is determined once the caller is authenticated.

## References

- `src/utils/auth.rs:104-168` — NIP-98 primary verifier
- `src/utils/auth.rs:170-248` — legacy session path
- `src/middleware/auth.rs` — `RequireAuth` middleware
- `src/middleware/enterprise_auth.rs` — `RequireRole` middleware with dual NIP-98 / header path
- `src/handlers/graph_handler.rs` — `/api/graph/data` read path
- `src/handlers/ontology_agent_handler.rs` — `/ontology-agent/*` scope
- `tests/auth_sovereign_mesh.rs` — three-tier matrix
