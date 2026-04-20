# solid-pod-rs-idp

Solid-OIDC identity provider for [`solid-pod-rs`](../solid-pod-rs/).

**Not yet implemented. Target milestone: v0.5.0.**

This crate reserves the namespace under the workspace for the Solid-OIDC
IDP surface per ADR-056 §D2 and
[`docs/design/jss-parity/06-library-surface-context.md`](../../docs/design/jss-parity/06-library-surface-context.md).

Scope when populated: OIDC authorization endpoints, dynamic client
registration, JWKS, Client Identifier Documents, credentials + passkeys
+ Schnorr SSO flows, and pluggable templated UI. See PARITY-CHECKLIST
rows 74–82 and GAP-ANALYSIS §E.3.

## Licence

**AGPL-3.0-only**.
