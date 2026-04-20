# solid-pod-rs-nostr

did:nostr resolver and embedded Nostr relay for
[`solid-pod-rs`](../solid-pod-rs/).

**Not yet implemented. Target milestone: v0.5.0.**

This crate reserves the namespace under the workspace for the did:nostr
+ relay surface per ADR-056 §D2 and
[`docs/design/jss-parity/06-library-surface-context.md`](../../docs/design/jss-parity/06-library-surface-context.md).

Scope when populated: did:nostr DID Document publication at
`/.well-known/did/nostr/:pubkey.json`, did:nostr ↔ WebID resolver, and
an embedded NIP-01/11/16 relay. See PARITY-CHECKLIST rows 89, 90, 101,
132 and GAP-ANALYSIS §E.4, §E.7.

## Licence

**AGPL-3.0-only**.
