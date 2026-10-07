---
id: ADR-2013
title: Enterprise federation and delegated-user authority are deferred; public-key identity supports multiple request credentials
date: 2026-08-31
decision_status: accepted
implementation_status: none
activation_status: inactive
supersedes: []                   # legacy ADR-040/ADR-094/ADR-081 distilled — not in this tree; see lineage
superseded_by: []
verified_commit: e0f8cd896
owner: jjohare
review_trigger: an enterprise deployment requiring OIDC/SAML, or a decision to wire NIP-26 agent-on-behalf-of-user delegation
repo: visionclaw
domain: IDENTITY-authority-chain
lineage: Distils legacy ADR-040 (enterprise identity — OIDC-alongside-Nostr, half superseded by ADR-142, half unbuilt) + ADR-094 (NIP-26 phone delegation, frozen 2026-07-03) + ADR-081 (federation key custody, frozen same date).
---

# ADR-2013 — Enterprise federation and delegated-user authority are deferred; public-key identity supports multiple request credentials

## Context

ADR-040 imagined OIDC alongside Nostr; ADR-142 superseded half of it, the rest was never built.
ADR-094 (NIP-26 phone delegation) and ADR-081 (federation key custody) were both frozen
2026-07-03. There is no enterprise-federation realm and no agent-on-behalf-of-user attribution
today. The forum bridge re-signs events under its own key, carrying no original-user authority.
Leaving these as unstated gaps risks code assuming a server-minted or IdP-federated identity.

## Decision

No OIDC/SAML/LDAP/SCIM auth realm exists, and NIP-26 delegation is unwired. Enterprise
federation and agent-on-behalf-of-user attribution are deliberately absent: identity is a
secp256k1 pubkey only until later phases land. This deferral is itself a constraint — no code may
assume a server-minted or IdP-federated identity, nor treat a bridged/agent-signed event as
carrying the original user's authority. It forecloses building features that depend on federated
claims or delegated attribution before those realms are designed and ratified.

## Consequences

- Enterprise SSO is unavailable; onboarding is pubkey-only, which limits large-org adoption until
  a federation ADR lands.
- The forum bridge's events are attributable to the bridge key, not to a user; downstream code
  must not infer user identity from a bridged event.
- When delegation is built, NIP-26 tags must be verified and mapped to authority explicitly —
  this ADR marks that as not-yet-done, not as forbidden forever.
- Merges IDENTITY B5 (OIDC parked) + B6 (NIP-26 deferred).

## Verification

Re-checked at `e0f8cd896`: no OIDC auth verifier exists in `src/` — a grep for
`OidcAuthVerified`/OIDC auth constructs finds only the `solid:oidcIssuer` RDF predicate string in
`src/handlers/solid_proxy_handler.rs:1015` (a Turtle literal, not a verifier) and a stale
claude-flow log, no verifier code. `src/services/nostr_bridge.rs:169` signs via
`sign_with_keys(&self.keys)` where `keys` is the bridge's own `Keys` (`:29`, constructed `:65`),
confirming the bridge carries no original-user authority. Governing doc:
`docs/IDENTITY-authority-chain.md` marks OIDC parked and delegated-agent-signed NOT IMPLEMENTED.

## Closeout extension — 2026-09-04

CP-01/04/05/08. Owner remains jjohare with identity/governance maintainers. None/inactive is retained for the deferred capabilities. The title now distinguishes the public-key identity foundation from ADR-2009's multiple request credentials. The inspected NIP-98 verifier returns the event signer as principal; the bridge verifies an incoming event then re-signs under its service key with source_event correlation. Neither operation establishes delegated-user authority. This record governs VisionClaw; other repositories' identity capabilities need their own assessment.

**Acceptance condition:** Explicitly retain deferral or ratify a bounded issuer/delegation design with principal mapping, audience, operation scope, expiry, revocation, custody and durable audit correlation. Before activation, demonstrate signed grant → mutation → receipt → revocation → denied retry, including restart and negative scope cases. Reopen on enterprise onboarding or any consumer interpreting service signatures as user authority. See the [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md#request-realms-and-deferred-delegation) and [source receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/auth-realms-snapshot.json). No live SSO or delegated mutation was exercised.

## Acceptance progress — 2026-09-05

**No code.** Deferral retained, as the closeout directs. Nothing in this pass
activates issuer or delegation capability; the NIP-98 verifier still returns the
event signer as principal, and the bridge still re-signs under its service key
with `source_event` correlation. Two adjacent changes touch the neighbourhood
without widening it:

* ADR-2002 adds route-declared body binding, which strengthens what a single
  credential proves about one request. It grants no delegated authority.
* ADR-2010 refuses a mutation whose caller authority weakened between admission
  and commit, which narrows rather than widens the authority surface.

**Remains open.** Everything the acceptance condition names: a bounded
issuer/delegation design with principal mapping, audience, operation scope,
expiry, revocation, custody and durable audit correlation, and the signed
grant → mutation → receipt → revocation → denied retry demonstration before any
activation.

## Review — 2026-10-07: NIP-26 proposal declined

An external synthesis proposed replacing bridge re-signing with NIP-26 delegation tokens. Owner decision 2026-10-07: deferral retained, and NIP-26 is not the mechanism to reopen with. Loom ADR-135 retires NIP-26 in favour of per-consumer NIP-59 capabilities, and NIP-26 is marked unrecommended upstream. If the review trigger fires, the bounded design the acceptance condition names should start from that direction. The underlying need, tracing an agent action to its authorising human, is tracked as X-3 in `docs/TODO-unified.md`. No code changed.
