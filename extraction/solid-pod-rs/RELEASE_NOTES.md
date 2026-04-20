# v0.4.0-alpha.1

JSS-parity migration. solid-pod-rs is at 76 % strict parity with
the reference JavaScriptSolidServer implementation, the six prior
audit findings are closed, and the workspace now cleanly separates
the library from the transport.

## Highlights

- **Workspace split.** `solid-pod-rs` (library) and
  `solid-pod-rs-server` (drop-in binary) replace the previous
  all-in-one crate. Four reserved sibling crates —
  `solid-pod-rs-{activitypub, git, idp, nostr}` — hold the
  v0.5.0 extension namespaces.
- **Security hardening.** SSRF guard with IP classification plus
  allow/deny lists; dotfile allowlist enforced at the storage
  boundary; DPoP `jti` replay cache per Solid-OIDC §5.2 and
  RFC 9449 §11.1.
- **WAC `acl:origin`.** Origin-based authorisation per the Web
  Access Control spec §4.3, gated behind a feature flag.
- **Legacy notifications.** `solid-0.1` WebSocket adapter for
  SolidOS data-browser compatibility.
- **JSS-compatible config loader.** Layered loader
  (defaults → file → env) with `JSS_*` variable names identical
  to the reference implementation.

## Install

```bash
cargo install solid-pod-rs-server
solid-pod-rs-server --config config.json
```

Minimal config:

```json
{
  "server":  { "host": "127.0.0.1", "port": 3000 },
  "storage": { "kind": "fs", "root": "./pod-root" },
  "auth":    { "nip98": { "enabled": true } }
}
```

## Upgrading from 0.3.0-alpha.3

- Add `solid-pod-rs-server` to your deployment if you were
  constructing `actix-web::HttpServer` from the library. The
  library no longer mounts HTTP routes.
- `verify_dpop_proof` gained an optional replay-cache argument;
  existing call sites compile unchanged by passing `None`.
- `evaluate_access` gained an optional request-origin argument;
  existing call sites compile unchanged by passing `None`.

See [CHANGELOG](crates/solid-pod-rs/CHANGELOG.md) for the full
change list and
[PARITY-CHECKLIST](crates/solid-pod-rs/PARITY-CHECKLIST.md) for
the row-by-row parity tracker.

## Licence

AGPL-3.0-only, inherited from the JavaScriptSolidServer ecosystem
covenant.
