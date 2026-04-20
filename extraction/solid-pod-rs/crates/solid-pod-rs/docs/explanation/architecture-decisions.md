# Architecture decisions

This page collects the major design calls made when building
solid-pod-rs, with the alternatives considered and the reasons for
the final choice. It is not a changelog — it is a "why the crate
looks like this" document for people reading or extending the code.

Unless noted otherwise, the status of each decision is **stable** —
we don't plan to revisit it.

## Why a framework-agnostic library, not a ready-to-run server

**Alternatives considered:**

1. Ship an actix-web binary with everything baked in.
2. Ship a trait-based library with example HTTP wiring.

**Decision:** ship the library. The example lives in `examples/`.

**Why:**

- Solid pods show up in many runtime contexts: standalone actix
  services, axum microservices inside VisionClaw, Cloudflare Workers
  (via `worker-rs`), Azure Functions. A single binary would have to
  compromise all of them.
- Every production deployment ends up needing middleware the library
  can't anticipate (rate limiting, tracing layers, tenant routing).
  Baking an HTTP framework in would force consumers to bypass or wrap
  the pod, defeating the purpose.
- `Storage`, `AclResolver`, and `Notifications` are all async traits
  with `Send + Sync + 'static` bounds. That composes cleanly with
  actix, axum, and hyper tower services.

**Consequence:** consumers write ~120 lines of HTTP glue per
deployment. `examples/standalone.rs` is a usable starting point.

## Why a single `Storage` trait

**Alternatives considered:**

1. A single `Storage` trait (what we did).
2. Three separate traits: `ResourceRead`, `ResourceWrite`, `Watcher`.
3. Object-oriented hierarchy (`Storage`, `WatchableStorage`, …).

**Decision:** one trait.

**Why:**

- Every realistic backend implements all three concerns: FS, S3, R2,
  IPFS, an in-memory store. Splitting would force every backend to
  implement three traits.
- The `watch` method returns an `mpsc::Receiver<StorageEvent>`, which
  gives callers a first-class handle — no broadcast contention if they
  only ever have one watcher per path.
- Trait splitting would make `LdpContainerOps` and `AclResolver`
  over-generic (they'd need multi-trait bounds everywhere).

See [explanation/storage-abstraction.md](storage-abstraction.md) for a
deeper dive.

## Why NIP-98 is first-class, Solid-OIDC is feature-gated

**Alternatives considered:**

1. Solid-OIDC on by default (matches JSS).
2. Both on by default.
3. NIP-98 on by default, OIDC opt-in.

**Decision:** NIP-98 is always compiled; OIDC requires
`features = ["oidc"]`.

**Why:**

- NIP-98 is 256 lines, pulls in only `base64`, `sha2`, `hex`, and
  `serde`. Solid-OIDC needs `openidconnect` + `jsonwebtoken` +
  their transitive deps (jsonrpc shim, base64 variants, pem parsers).
  A NIP-98-only pod doesn't pay for OIDC.
- The crate's origin (`community-forum-rs`) is a Nostr-native app.
  NIP-98 was the primary auth path from day one. OIDC is there for
  interop with existing Solid clients.
- Both protocols solve the same problem (request-bound auth). Running
  both in parallel is routine; running only one is the common case.

## Why the deny-by-default WAC policy

**Alternatives considered:**

1. Allow public read when no ACL exists (JSS's historical default).
2. Deny when no ACL exists (WAC spec wording).

**Decision:** deny.

**Why:**

- The [WAC spec](https://solid.github.io/web-access-control-spec/) says
  explicitly: "if no ACL resource is effective for the requested
  resource, access should be denied". We follow the letter of the
  spec.
- Silent public-reads are a recurring class of vulnerability on JSS
  pods (people paste "wrong" configs).
- Forcing the operator to commit an explicit `/.acl` makes the access
  policy legible.

**Consequence:** new pods look "broken" until an ACL is installed.
Tutorial 3 covers this as the first step after setup.

## Why our own tiny RDF model instead of `sophia` / `oxigraph`

**Alternatives considered:**

1. Use `oxigraph` for full RDF store, PATCH, and negotiation.
2. Use `sophia` for RDF model + parsing.
3. Minimal in-crate `Term`, `Triple`, `Graph` types.

**Decision:** minimal in-crate model.

**Why:**

- `oxigraph` brings an entire SPARQL engine — megabytes of code and
  a 40 % longer build time — for the handful of PATCH operations we
  need.
- `sophia` is lighter but its parsers pull in full Turtle 1.1 support;
  PATCH bodies are a tiny N-Triples-like subset.
- The in-crate `Graph` is 400 lines. It serialises N-Triples,
  round-trips through it, and that's enough to back N3 PATCH and
  SPARQL DELETE-DATA / INSERT-DATA via `spargebra` (which only parses
  — no storage).
- Downstream consumers who need a real triple store can wrap our
  storage backend with `oxigraph`'s in-memory store. We don't
  constrain them.

**Trade-off:** we do not support Turtle 1.1 shortforms inside PATCH
bodies. The PATCH corpus is N-Triples-shaped. Real-world Solid
clients already emit N-Triples inside PATCH blocks — this is an
accepted loss.

## Why Activity Streams 2.0 for notifications

**Alternatives considered:**

1. Custom JSON shape.
2. Server-Sent Events with a bespoke event type.
3. Activity Streams 2.0 (Solid Notifications 0.2 mandates this).

**Decision:** AS2.0.

**Why:**

- Solid Notifications 0.2 §7 says so.
- AS2.0 has a published JSON-LD context; consumers can embed and
  validate with off-the-shelf tooling.
- Clients that already subscribe to JSS/NSS pods consume the same
  shape — we keep interop.

## Why `broadcast::channel` inside `WebSocketChannelManager`

**Alternatives considered:**

1. `broadcast::channel` + per-client `mpsc`.
2. `async-channel::Sender<ChangeNotification>`.
3. Single producer, fan-out in the HTTP handler.

**Decision:** `tokio::sync::broadcast`.

**Why:**

- Every connected WebSocket needs the same event. `broadcast` is
  exactly that primitive.
- `broadcast::Receiver` can lag (drop messages under backpressure),
  which is the correct failure mode for live notifications —
  catch-up is the client's problem, not ours.
- Capacity `1024` is a tunable; we chose the default because a busy
  pod at the 100-op/s level still has ~10 s of buffer per client.

**Consequence:** slow clients drop frames rather than backpressuring
the producer. Clients that need catch-up must re-fetch the resource.

## Why exponential backoff for webhook delivery

**Alternatives considered:**

1. Fixed retry interval.
2. Exponential backoff (500 ms, 1 s, 2 s, drop).
3. Queue with a scheduler daemon.

**Decision:** exponential backoff, bounded by `max_retries`.

**Why:**

- Transient 5xx responses are overwhelmingly short-lived (seconds).
  Exponential backoff with a 3-retry cap covers 99 % of them without
  a scheduler.
- `max_retries = 3` + `retry_base = 500 ms` means total retry budget
  is ~3.5 s. Well inside any reasonable HTTP-handler timeout.
- 4xx is a permanent signal ("your subscription is invalid") — drop
  the subscription, don't retry.

**Trade-off:** a target that returns 5xx for longer than 3.5 s gets
a dropped delivery. Consumers who need at-least-once delivery must
run their own queue in front.

## Why `Storage::put` is create-or-replace

**Alternatives considered:**

1. `put` fails on existing paths; separate `update`.
2. `put` is create-or-replace.

**Decision:** create-or-replace.

**Why:**

- HTTP `PUT` semantics (RFC 7231 §4.3.4) match.
- Callers that want insert-only semantics can call `exists` then
  `put` — at the cost of a lost atomicity window, which is fine for
  the rare cases where it matters.
- Reduces the trait surface by one method.

## Why strong ETags (SHA-256 hex)

**Alternatives considered:**

1. Weak ETags (timestamp-based).
2. Strong content-hash ETags.

**Decision:** strong hash.

**Why:**

- `If-Match` is meaningful only with strong validators (RFC 7232
  §3.1).
- SHA-256 is cheap (µs per KB), deterministic, and ports trivially to
  every backend.
- Hex encoding avoids Base64 padding ambiguity and is trivially
  inspectable.

## Still open

- **JSON Patch (RFC 6902).** Deliberately out of scope now; may be
  added if users ask. Ported from pod-worker on demand.
- **If-Match enforcement.** Storage layer returns canonical ETags;
  HTTP-layer enforcement is P2. See
  [PARITY-CHECKLIST.md](../../PARITY-CHECKLIST.md).
- **Quota enforcement.** Not in this crate. Downstream concern.

## See also

- [explanation/storage-abstraction.md](storage-abstraction.md)
- [explanation/security-model.md](security-model.md)
- [explanation/comparison-vs-jss.md](comparison-vs-jss.md)
