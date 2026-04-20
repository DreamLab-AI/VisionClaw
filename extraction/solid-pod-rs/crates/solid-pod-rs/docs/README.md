# solid-pod-rs documentation

Rust implementation of a Solid Pod server: WAC + LDP + NIP-98 +
Solid-OIDC + Notifications, on a pluggable `Storage` trait.

This site follows the [Diátaxis](https://diataxis.fr/) framework: four
distinct documentation modes, each optimised for a different user
need. Pick the quadrant that matches what you want to do *right now*.

| | Practical | Theoretical |
|---|---|---|
| **Study (acquire skill)** | [Tutorials](#tutorials) | [Explanation](#explanation) |
| **Work (apply skill)** | [How-to guides](#how-to-guides) | [Reference](#reference) |

## Tutorials

Learning-oriented. Start here if you have never touched the crate.
Each tutorial is a complete, copy-paste-able walkthrough and takes
≤ 15 minutes.

- [01. Your first Pod](tutorials/01-your-first-pod.md) — spin up the
  example server and make your first request.
- [02. Storing your first resource](tutorials/02-storing-your-first-resource.md)
  — `PUT`, `GET`, `DELETE` over HTTP; read ETag and Link headers.
- [03. Adding access control](tutorials/03-adding-access-control.md) —
  author a `.acl` sidecar and verify the WAC-Allow response.
- [04. Subscribing to changes](tutorials/04-subscribing-to-changes.md)
  — attach a WebSocket client and see live change notifications.

## How-to guides

Goal-oriented. You know what you want to accomplish and need the
shortest procedural recipe.

- [Configure NIP-98 authentication](how-to/configure-nip98-auth.md)
- [Enable Solid-OIDC](how-to/enable-solid-oidc.md)
- [Swap storage backends](how-to/swap-storage-backends.md)
- [Migrate from JSS (JavaScriptSolidServer)](how-to/migrate-from-jss.md)
- [Enable webhook notifications](how-to/enable-notifications-webhook.md)
- [Enable WebSocket notifications](how-to/enable-notifications-websocket.md)
- [Scale with an S3 backend](how-to/scale-with-s3-backend.md)
- [Deploy to production](how-to/deploy-to-production.md)
- [Debug ACL denials](how-to/debug-acl-denials.md)

## Reference

Information-oriented. Dry, exhaustive, accurate. Look things up here.

- [Public Rust API](reference/api.md) — traits, structs, functions.
- [HTTP endpoints](reference/http-endpoints.md) — method matrix per
  path kind.
- [`Link` headers](reference/link-headers.md) — every value the server
  emits and why.
- [`Prefer` header](reference/prefer-headers.md) — LDP representation
  selection.
- [WAC modes](reference/wac-modes.md) — `acl:Read` / `Write` /
  `Append` / `Control`.
- [Content types](reference/content-types.md) — Turtle, JSON-LD,
  N-Triples, RDF/XML negotiation matrix.
- [PATCH semantics](reference/patch-semantics.md) — N3 Patch +
  SPARQL-Update subset.
- [Environment variables](reference/env-vars.md) — every config knob.
- [Error codes](reference/error-codes.md) — HTTP status → `PodError`
  mapping.

## Explanation

Understanding-oriented. Why did we build it this way? What are the
tradeoffs?

- [Solid primer for Rust developers](explanation/solid-primer.md)
- [Architecture decisions](explanation/architecture-decisions.md) —
  the big "why X over Y" calls.
- [Comparison vs JSS](explanation/comparison-vs-jss.md) — what's the
  same, what's different, what's deliberately missing.
- [Security model](explanation/security-model.md) — threat model +
  auth layering.
- [Storage abstraction](explanation/storage-abstraction.md) — the
  trait shape and why.
- [Ecosystem integration](explanation/ecosystem-integration.md) — how
  this crate sits alongside URN-Solid, solid-schema, and Solid-Apps.

## Reading order

- **New to Rust + Solid?** Tutorials 01 → 02 → 03, then
  [Solid primer](explanation/solid-primer.md).
- **Experienced Solid implementer (from JSS)?** Start with
  [Comparison vs JSS](explanation/comparison-vs-jss.md) then
  [migrate-from-jss.md](how-to/migrate-from-jss.md).
- **Integrating into an existing actix/axum service?** Jump to
  [Rust API reference](reference/api.md) and
  [HTTP endpoints](reference/http-endpoints.md).

## Related material

- [README.md](../README.md) — crate-level overview.
- [PARITY-CHECKLIST.md](../PARITY-CHECKLIST.md) — feature-by-feature
  status against JSS.
- [CHANGELOG.md](../CHANGELOG.md) — release history.
- [CONTRIBUTING.md](../CONTRIBUTING.md) — how to contribute.
- [diagrams/](diagrams/) — architecture diagrams (Mermaid + rendered).
- [benchmarks.md](benchmarks.md) — performance numbers.
- [examples-index.md](examples-index.md) — runnable examples.
