# ADR-088 — Auth Service Extraction

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-09) |
| Drives | PRD-015 PAR-02, PRD-014 SEC-01 (auth bypass), O1 (NIP-98 convergence), O6 (key management) |
| Companion ADRs | ADR-074 (DID:nostr canonicalisation), ADR-076 (nostr-core absorption), ADR-078 (library convergence), ADR-081 (key custody) |
| Companion PRDs | PRD-010, PRD-014, PRD-015 |
| Affected repos | `VisionClaw`, `nostr-rust-forum`, `solid-pod-rs` |

## Context

VisionClaw has **6 auth-related modules** (2,349 lines) that evolved independently:

| Module | Lines | Mechanism | Used by |
|--------|-------|-----------|---------|
| `middleware/auth.rs` | 312 | Nostr pubkey extraction from NIP-98 | Most API handlers |
| `middleware/enterprise_auth.rs` | 603 | Dual-path: NIP-98 Schnorr auth (`nip98-auth` feature) or `X-Enterprise-Role` header | Enterprise routes |
| `utils/auth.rs` | 289 | Auth utility functions | Internal services |
| `utils/nip98.rs` | 640 | NIP-98 token generation/validation | Middleware, services |
| `settings/auth_extractor.rs` | 134 | Settings-aware auth extractor | Settings endpoints |
| `socket_flow_handler/filter_auth.rs` | 488 | WebSocket auth | WS handlers |

These share no common trait or error type. Handler authors must know which auth module to import for their use case. The `SETTINGS_AUTH_BYPASS` environment variable gates `auth_extractor.rs` independently of other auth paths, creating inconsistent protection.

Cross-substrate: NIP-98 is implemented 3 times (VisionClaw 640 lines, nostr-rust-forum 1,075 lines, solid-pod-rs 484 lines) per PRD-015 O1.

## Decision

### D1 — `AuthService` trait

```rust
#[async_trait]
pub trait AuthService: Send + Sync + 'static {
    async fn authenticate(&self, req: &HttpRequest) -> Result<AuthIdentity, AuthError>;
    async fn authorize(&self, identity: &AuthIdentity, resource: &str, action: Action) -> Result<(), AuthError>;
}

pub enum AuthIdentity {
    Nostr { pubkey: [u8; 32], delegation: Option<DelegationTag> },
    Enterprise { subject: String, claims: Claims },
    Anonymous,
}
```

### D2 — Three concrete implementations

| Impl | Source | Priority |
|------|--------|----------|
| `NostrAuthService` | Extracted from `middleware/auth.rs` + `utils/nip98.rs` | Default |
| `EnterpriseAuthService` | Extracted from `middleware/enterprise_auth.rs` | When OIDC configured |
| `CompositeAuthService` | Chains Nostr → Enterprise → Anonymous | Production default |

### D3 — Single Actix middleware

Replace all 6 modules with one `AuthMiddleware` that wraps `CompositeAuthService`:

```rust
pub struct AuthMiddleware {
    service: Arc<dyn AuthService>,
}
```

Injected via `app_data` in `main.rs`. Handlers extract `AuthIdentity` from request extensions.

### D4 — Remove `SETTINGS_AUTH_BYPASS`

The bypass is a security risk (PRD-014 SEC-01). Replace with:
- Dev mode: `CompositeAuthService` returns `AuthIdentity::Anonymous` for localhost origins when `APP_ENV=development`.
- Prod: No bypass path exists.

### D5 — Cross-substrate NIP-98 convergence (deferred to Sprint 3)

Per PRD-015 O1, the forum's 1,075-line NIP-98 implementation (most complete, includes replay store) becomes the canonical source. Extract to `solid-pod-rs-nostr` crate. VisionClaw and solid-pod-rs consume it as a dependency.

This ADR covers D1-D4 (VisionClaw internal). D5 is tracked separately as a Sprint 3 deliverable.

## Consequences

**Positive:**
- Single auth entry point for all handlers
- Consistent auth error responses (currently 4 different error shapes)
- `SETTINGS_AUTH_BYPASS` eliminated
- Clear extension point for future auth mechanisms (WebAuthn, mTLS)
- ~800 lines net reduction after consolidation

**Negative:**
- All handler tests that mock auth must be updated to use `AuthIdentity` extraction
- Enterprise auth customers need migration guide for any header changes

**Risks:**
- WebSocket auth has different lifecycle (upgrade-time vs per-request). Mitigated by `AuthMiddleware` checking `Connection: Upgrade` and delegating to WS-specific flow.
