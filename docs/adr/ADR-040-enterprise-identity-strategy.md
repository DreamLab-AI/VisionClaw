# ADR-040: Enterprise Identity Strategy

## Status

Accepted

## Date

2026-04-14

## Context

VisionClaw's identity model is built on Nostr NIP-98 authentication (secp256k1 keypairs, Schnorr signatures, browser extensions like Alby or nos2x). This is a genuine differentiator for provenance: every bead, every debrief, every workflow action is cryptographically signed by its author. No central auth server can be compromised. No password database exists.

Enterprise customers cannot adopt this model. Their IT departments mandate federated SSO via OIDC (Microsoft Entra ID, Okta, Google Workspace) or SAML. They will not permit browser extensions on managed devices. They require central user provisioning, group-based access control, and audit logs tied to corporate identity. Without enterprise identity support, VisionClaw cannot enter regulated sectors (pharma, finance, advanced manufacturing) regardless of how strong the platform is technically.

The core tension is that Nostr signatures provide VisionClaw's provenance guarantees. Every `BrokerDecision`, `WorkflowProposal`, and bead carries a cryptographic author signature. Replacing Nostr with OIDC would destroy this differentiator. The platform needs both: OIDC for enterprise user access and session management, Nostr for signed provenance events.

The critical UX question identified in the PRD analysis (section 18, risk C) is: how does an enterprise user logging in via Entra ID get a secp256k1 keypair to sign NIP-98 events without installing a browser extension?

### Current State

- BC1 (Authentication) has `NostrSession` as its aggregate root
- `auth_extractor.rs` validates NIP-98 signed requests
- `nostr_service.rs` manages session lifecycle
- ADR-011 enforces authentication on all mutating endpoints
- Solid Pods store user-owned data with NIP-26 delegation
- No OIDC, SAML, LDAP, or SCIM support exists anywhere in the codebase
- The DDD Identity Contexts document already defines `OidcAuthVerified(webId, issuer)` as a domain event, but it is not implemented

## Decision Drivers

- Enterprise customers require OIDC SSO as a prerequisite for procurement
- Cryptographic provenance (Nostr signatures) is a strategic differentiator that must be preserved
- Enterprise IT will block browser extensions on managed devices
- SCIM compatibility is needed for user lifecycle management at scale
- The Judgment Broker role (Workstream 3) depends on enterprise identity being resolved first
- Open/community deployments must continue to work with Nostr-only authentication
- Solid Pod integration must work with both identity paths

## Considered Options

### Option 1: Dual-stack with server-side ephemeral Nostr keypairs (chosen)

OIDC handles enterprise user access (login, session, group membership). On OIDC session creation, the server generates an ephemeral secp256k1 keypair mapped to the OIDC session. This keypair is stored in either the user's Solid Pod (preferred, preserving data sovereignty) or a server-side secure enclave (fallback for deployments without Solid). The ephemeral keypair signs provenance events on behalf of the OIDC-authenticated user. The mapping between OIDC subject (`sub` claim) and ephemeral Nostr pubkey is recorded in the user's identity record.

- **Pros**: Enterprise users get standard SSO. Provenance chain remains cryptographically signed. No browser extension required. Nostr-native users keep their existing flow unchanged. The ephemeral keypair model is already conceptually aligned with NIP-26 delegation.
- **Cons**: Server holds delegated signing keys, which is a weaker trust model than client-side Nostr keys. Key rotation on session expiry adds complexity. Auditors must understand the delegation chain (OIDC identity -> ephemeral Nostr key -> signed event).

### Option 2: OIDC-only, drop Nostr signatures for enterprise users

Replace Nostr signatures with OIDC session assertions for provenance. Enterprise events would carry an OIDC subject identifier rather than a cryptographic signature.

- **Pros**: Simpler implementation. No key management. Standard enterprise pattern.
- **Cons**: Destroys the provenance differentiator. Events are no longer independently verifiable. Auditors cannot verify authorship without trusting the OIDC provider. Two fundamentally different provenance models in the same system. Breaks the bead lifecycle model (ADR-034) which assumes Nostr-signed events.

### Option 3: Require browser extensions for all users

Keep Nostr-only authentication. Require enterprise IT to whitelist a NIP-07 extension.

- **Pros**: No architecture changes. Strongest cryptographic model.
- **Cons**: Non-starter for enterprise procurement. IT departments will not whitelist unfamiliar browser extensions on managed Chrome/Edge deployments. Blocks the entire enterprise strategy.

### Option 4: OIDC proxy that translates to Nostr internally

An API gateway translates OIDC tokens to Nostr-signed requests before they reach the application layer. The application only sees Nostr.

- **Pros**: Application code unchanged. Clean separation.
- **Cons**: Gateway must hold signing keys with no user association. Key-per-user management at the proxy layer is equivalent to Option 1 but in a worse location (outside the application's security boundary). Harder to audit.

## Decision

**Option 1: Dual-stack with server-side ephemeral Nostr keypairs mapped to OIDC sessions.**

### Authentication Flow

```
Enterprise User                    VisionClaw Server               OIDC Provider
     |                                   |                              |
     |-- GET /auth/login/oidc ---------->|                              |
     |                                   |-- redirect to provider ----->|
     |<---------- OIDC redirect ---------|                              |
     |-- auth code callback ------------>|                              |
     |                                   |-- exchange code ------------>|
     |                                   |<-- id_token + userinfo ------|
     |                                   |                              |
     |                                   |-- generate ephemeral keypair |
     |                                   |-- store key in Solid Pod     |
     |                                   |   (or secure enclave)        |
     |                                   |-- create session with both   |
     |                                   |   OIDC sub + Nostr pubkey    |
     |                                   |                              |
     |<-- session token (JWT) -----------|                              |
     |                                   |                              |
     |-- API request + session token --->|                              |
     |                                   |-- server signs provenance    |
     |                                   |   event with ephemeral key   |
```

### Identity Model

The `NostrSession` aggregate root in BC1 is extended to support two authentication methods:

```rust
pub enum AuthMethod {
    /// User authenticated via NIP-98 signed request (community/open deployments)
    NostrNative { pubkey: String },
    /// User authenticated via OIDC, with server-managed ephemeral keypair
    OidcDelegated {
        oidc_subject: String,
        oidc_issuer: String,
        ephemeral_pubkey: String,
        delegation_token: Option<String>,  // NIP-26 if Pod-stored
    },
}
```

### OIDC Integration

OIDC integration uses actix-web middleware, consistent with the existing `RequireAuth` pattern (ADR-011):

1. **Discovery**: Standard OIDC discovery (`/.well-known/openid-configuration`) for provider metadata
2. **Token validation**: RS256/ES256 JWT verification against provider JWKS
3. **Claims mapping**: `sub` -> user identity, `groups` or custom claims -> role mapping
4. **Session creation**: OIDC-verified identity creates a VisionClaw session with both the OIDC subject and the ephemeral Nostr pubkey

### Role Model

| Role | Description | Default Permissions |
|------|-------------|-------------------|
| Broker | Reviews escalations, approves workflows, sets precedents | Read all, decide on assigned cases, promote workflows |
| Admin | Manages connectors, policies, user provisioning, platform config | Full platform configuration, user management, policy editing |
| Auditor | Inspects provenance, exports compliance evidence | Read all, export, no mutation |
| Contributor | Submits workflow proposals, uses approved workflows | Read assigned scope, propose workflows, execute approved workflows |

Roles are mapped from OIDC group claims. A mapping configuration defines which OIDC groups correspond to which VisionClaw roles:

```toml
[identity.oidc.role_mapping]
broker = ["oidc-group:visionclaw-brokers"]
admin = ["oidc-group:visionclaw-admins"]
auditor = ["oidc-group:visionclaw-auditors"]
contributor = ["oidc-group:visionclaw-users"]
```

### SCIM Compatibility Roadmap

Phase 1 (this ADR): OIDC login with group-based role mapping. User records created on first login (JIT provisioning).

Phase 2 (future): SCIM 2.0 `/Users` and `/Groups` endpoints for automated provisioning and deprovisioning. Deprovisioned users have their ephemeral keypairs revoked and sessions invalidated.

### Deployment Modes

| Mode | Authentication | Provenance Signing |
|------|---------------|-------------------|
| Open (default) | Nostr NIP-98 only | Client-side keypair |
| Enterprise | OIDC primary, Nostr optional | Server-side ephemeral keypair |
| Hybrid | Both enabled, user chooses | Depends on auth method used |

Mode is selected via configuration. The application layer is auth-method-agnostic after session creation.

### Ephemeral Key Storage

Preferred: stored in the user's Solid Pod under a protected container (`/identity/nostr-delegate/`), encrypted at rest, accessible only to the VisionClaw server via the user's Pod delegation.

Fallback: stored in a server-side PostgreSQL table (RuVector) with AES-256 encryption, scoped to the user's OIDC subject and rotated on session refresh.

Key lifecycle:
- Created on first OIDC login
- Rotated on configurable interval (default: 30 days)
- Revoked on OIDC session termination or SCIM deprovisioning
- Old keys retained in read-only archive for provenance verification

## Consequences

### Positive

- Enterprise customers can use standard OIDC SSO without browser extensions
- Cryptographic provenance is preserved for all users regardless of auth method
- Nostr-native users are unaffected; their flow is unchanged
- Role model aligns with PRD target users (Broker, Admin, Auditor, Contributor)
- Solid Pod integration is preserved and extended for enterprise key storage
- The delegation chain is auditable: OIDC identity -> ephemeral key -> signed event

### Negative

- Server-side key management is a weaker trust model than client-held Nostr keys. An attacker with server access could sign events as any enterprise user. Mitigation: key encryption at rest, Pod-preferred storage, audit logging of all signing operations.
- Ephemeral key rotation means a single user may have multiple Nostr pubkeys over time. Provenance queries must resolve the OIDC subject -> pubkey mapping, not just match on pubkey.
- OIDC provider outages block enterprise login. Mitigation: cached session validation for existing sessions; Nostr fallback for emergency access if configured.
- SAML is not directly supported in this ADR. Mitigation: most modern SAML IdPs (ADFS, PingFederate) support OIDC as well. A SAML-to-OIDC bridge (e.g., Keycloak) can be documented as the supported path.

### Neutral

- Existing NIP-98 authentication code remains unchanged
- WebSocket authentication (ADR-011) applies identically to both auth methods after session creation
- Binary protocol (BC10) is unaffected
- Graph data model (BC2) stores the signing pubkey on provenance nodes regardless of how the key was obtained

## Implementation Status (2026-05-12)

The `RequireRole` middleware in `src/middleware/enterprise_auth.rs` now implements
dual-path authentication as described in the Decision section:

- **NIP-98 path** (compile-time `nip98-auth` feature): `RequireRole` reads a
  `Nostr <base64>` Authorization header, verifies the NIP-98 Schnorr signature
  via `extract_role_from_nip98()`, and resolves the signer's pubkey to an
  `EnterpriseRole` through the `Nip98RoleResolver` trait. A `Nip98IdentityExt`
  request extension carries the verified pubkey and role to downstream handlers.
  `InMemoryRoleMap` provides a dev/test resolver; production deployments should
  implement `Nip98RoleResolver` against a persistent store.

- **Header path** (default, no feature gate): `RequireRole` reads the
  `X-Enterprise-Role` header for role extraction. This is the dev/gateway mode
  described in the original Decision and remains the default when the
  `nip98-auth` feature is not compiled in.

Both paths enforce the hierarchical role model defined above: Admin (4) >
Broker (3) > Auditor (2) > Contributor (1). A per-handler helper
`require_role_nip98()` is also available for handlers that need direct NIP-98
verification without the middleware.

### Agent Control Surface Protocol

The `ServerNostrActor` now publishes governance events (kinds 31400, 31402) on
behalf of the `BrokerActor`, bridging VisionClaw's broker workflow with the
Forum Kit's Agent Control Surface Protocol. These events carry the server's
Nostr identity and are subject to the same role-based access control when
submitted via the HTTP API.

## Related Decisions

- ADR-011: Universal Authentication Enforcement (extended, not replaced)
- ADR-028-ext: NIP-98 as Enterprise Auth -- Optional-Auth Extension
- ADR-034: Needle Bead Provenance (provenance signing model preserved)
- ADR-027: Pod-Backed Graph Views (Solid Pod used for ephemeral key storage)
- ADR-041: Judgment Broker Workbench Architecture (depends on role model defined here)
- ADR-045: Policy Engine Approach (policies reference roles defined here)
- ADR-088: Auth Service Extraction (proposed consolidation of auth modules)

## References

- PRD Section 18, Risk C: OIDC-Nostr Identity Collision
- PRD Workstream 2: Enterprise Identity & Access
- PRD FR6: Enterprise Identity
- `docs/explanation/security-model.md`
- `docs/explanation/ddd-identity-contexts.md`
- `docs/explanation/ddd-bounded-contexts.md` (BC1: Authentication & Authorization)
- `src/settings/auth_extractor.rs`
- OpenID Connect Core 1.0 specification
- SCIM 2.0 (RFC 7643, RFC 7644)
- Nostr NIP-26: Delegated Event Signing
