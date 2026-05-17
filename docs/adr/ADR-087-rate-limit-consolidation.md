# ADR-087 — Rate Limit Consolidation

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-09) |
| Drives | PRD-015 PAR-01 (VisionClaw), PAR-03 (nostr-rust-forum) |
| Companion ADRs | ADR-077 P4 (security scanning), ADR-086 (SOPS secrets) |
| Companion PRDs | PRD-014 (ecosystem productionisation), PRD-015 (ecosystem code hygiene) |
| Affected repos | `VisionClaw`, `nostr-rust-forum` |

## Context

The VisionClaw Rust backend contains **three independent rate-limiting implementations** totalling 1,473 lines:

| File | Lines | Backend | Notes |
|------|-------|---------|-------|
| `src/middleware/rate_limit.rs` | 423 | In-memory `DashMap` | Original; per-IP with fixed windows |
| `src/utils/validation/rate_limit.rs` | 561 | In-memory `DashMap` | Newer; token-bucket with burst support |
| `src/utils/validation/middleware.rs` | 489 | Wraps `validation/rate_limit.rs` | Actix `Transform` adapter; also contains `SecurityHeaders` |

The `middleware/rate_limit.rs` and `utils/validation/rate_limit.rs` modules are both imported in `main.rs`, creating naming ambiguity. Configuration is split across environment variables (`RATE_LIMIT_PER_MINUTE`, `RATE_LIMIT_BURST`) with no single source of truth.

Separately, `nostr-rust-forum` has 3 identical 46-line `rate_limit.rs` files in auth-worker, preview-worker, and search-worker (PAR-03: 138 lines total).

## Decision

### D1 — Single `RateLimitService` in VisionClaw

Consolidate into `src/services/rate_limit_service.rs` implementing a token-bucket algorithm with configurable backend:

```rust
pub struct RateLimitService {
    store: Arc<dyn RateLimitStore + Send + Sync>,
    config: RateLimitConfig,
}

pub trait RateLimitStore: Send + Sync {
    fn check_and_decrement(&self, key: &str, config: &BucketConfig) -> RateLimitResult;
    fn reset(&self, key: &str);
}
```

- **In-memory backend** (default): `DashMap`-based, used in dev and single-instance deploys.
- **Redis backend** (future): For horizontal scaling. Gated behind `feature = "redis-rate-limit"`.
- The Actix `Transform` wrapper stays in `utils/validation/middleware.rs` but delegates to `RateLimitService`.

### D2 — Delete `middleware/rate_limit.rs`

The original 423-line fixed-window implementation is superseded. Remove it and update all imports.

### D3 — Per-scope configuration

Rate limits are configured per API scope in `settings.toml` (or env vars):

| Scope | Default | Rationale |
|-------|---------|-----------|
| `/api/graph` | 120/min | Read-heavy, authenticated |
| `/api/settings` | 30/min | Write, authenticated |
| `/api/auth` | 10/min | Login/token endpoints |
| `/ws` | 5 connections/min | WebSocket upgrade |
| Global fallback | 60/min | Unauthenticated catch-all |

### D4 — Extract `nostr-bbs-rate-limit` crate in forum

The 3 identical copies become a single workspace crate consumed by all workers:

```
crates/nostr-bbs-rate-limit/
├── Cargo.toml
└── src/lib.rs       # ~60 lines: generic token-bucket
```

## Consequences

**Positive:**
- Single rate-limit configuration surface for VisionClaw operators
- Forum workers share one tested implementation
- ~560 lines removed from VisionClaw, ~92 lines removed from forum
- Clear extension point for Redis backend

**Negative:**
- Migration requires auditing all `main.rs` rate-limit wiring
- Existing per-handler rate limits (e.g. auth) must be re-expressed as scope config

**Risks:**
- Token-bucket semantics differ from fixed-window; existing callers may see different burst behaviour during migration. Mitigated by defaulting burst=1 for auth endpoints.
