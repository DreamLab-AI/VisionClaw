# Solid primer for Rust developers

This page gives you enough Solid background to read the rest of the
solid-pod-rs documentation. It is written for developers who are
comfortable with Rust and HTTP but new to the Solid ecosystem.

If you are already a Solid implementer coming from JSS, skip to
[comparison-vs-jss.md](comparison-vs-jss.md).

## What Solid is

Solid is a protocol built on top of HTTP + Linked Data that gives
users control over their data through **Pods** — personal data
stores. Think "the filesystem is a network service, but every file
has a semantic type and every directory is addressable".

A Solid Pod is:

- An HTTP server with a known root URL (e.g.
  `https://alice.example/`).
- Every resource is addressable by URL and has a content type
  (including RDF formats like Turtle or JSON-LD).
- Directories are called *containers*; every container lists its
  children via RDF triples using `ldp:contains`.
- Access to each resource is controlled by a sidecar ACL document
  (Web Access Control, "WAC").
- Authentication uses Solid-OIDC (a DPoP-bound OAuth 2 flow) or
  (in this crate) NIP-98 (Nostr-signed HTTP auth).

solid-pod-rs implements the **server side** of the above.

## The four key specs

1. **LDP — Linked Data Platform** ([W3C Rec, 2015](https://www.w3.org/TR/ldp/)).
   Defines how RDF resources are served over HTTP: content types,
   containers, `Link` headers, `Prefer` headers, POST-as-create with
   `Slug`.
2. **WAC — Web Access Control**
   ([spec](https://solid.github.io/web-access-control-spec/)). Defines
   how `.acl` documents grant access modes to agents or classes.
3. **Solid Protocol** ([spec](https://solidproject.org/TR/protocol)).
   The binding layer — what methods a Pod must support, Link header
   requirements, PATCH with N3 / SPARQL Update, how auth slots in.
4. **Solid-OIDC** ([spec](https://solid.github.io/solid-oidc/)). The
   authentication profile — DPoP-bound tokens, WebID discovery, dynamic
   client registration.

The solid-pod-rs crate is structured so each spec maps to one module:

| Spec | Module |
|---|---|
| LDP | [`ldp`](../reference/api.md#ldp) |
| WAC | [`wac`](../reference/api.md#wac) |
| Solid Protocol (PATCH) | [`ldp::apply_n3_patch`, `apply_sparql_patch`](../reference/patch-semantics.md) |
| Solid-OIDC | [`oidc`](../reference/api.md#oidc-types) (feature-gated) |
| NIP-98 | [`auth::nip98`](../reference/api.md#authnip98) |
| Notifications 0.2 | [`notifications`](../reference/api.md#notifications) |

## Concepts Rust developers find surprising

### Every resource is RDF-shaped, even if it's binary

LDP containers enumerate their children as RDF triples. A container
response looks like this (JSON-LD):

```json
{
  "@context": { "ldp": "http://www.w3.org/ns/ldp#" },
  "@id":   "/notes/",
  "@type": ["ldp:Container","ldp:BasicContainer","ldp:Resource"],
  "ldp:contains": [ {"@id":"/notes/a.txt"} ]
}
```

Binary files (images, videos) still live inside the container and are
addressable — you just don't get RDF when you GET the body; the
container still reports them via `ldp:contains`.

### Paths are canonical

- Containers end with `/`.
- `GET /notes` ≠ `GET /notes/` — the latter is a container; the former
  would be a resource named "notes" (if it existed). solid-pod-rs
  enforces this.

### Metadata is out-of-band

Every resource has a `.meta` sidecar for metadata and a `.acl`
sidecar for access control. The sidecar path is computed, not stored:
for `/profile/card` you find the ACL at `/profile/card.acl`. Clients
discover these via `Link: rel="acl"` and `rel="describedby"` — see
[reference/link-headers.md](../reference/link-headers.md).

### Deny-by-default

Unlike a filesystem, a Solid pod with no `.acl` document denies every
request. This is a WAC invariant. The first thing you do on a new pod
is install `/.acl`. See
[tutorial 3](../tutorials/03-adding-access-control.md).

### Authentication is request-bound

Both Solid-OIDC (DPoP) and NIP-98 bind the auth token to:

- The HTTP method.
- The full URL.
- (Usually) a hash of the request body.

Replay of a token at a different URL, with a different method, or
against a modified body is rejected. This is stronger than plain
bearer tokens.

### Notifications are AS2.0

When a resource changes, the pod emits an Activity Streams 2.0
notification with `type: "Create"`, `"Update"`, or `"Delete"`. Clients
subscribe via WebSocket or Webhook. See
[reference/api.md §notifications](../reference/api.md#notifications).

## Minimal mental model

```
  Client                                        Pod (solid-pod-rs)
  ──────                                        ──────────────────
  HTTP request                                  HTTP framework
      │                                                │
      ▼                                                ▼
  Auth middleware  ←── NIP-98 or OIDC ────►  auth::nip98 / oidc
      │                                                │
      ▼                                                ▼
  WAC check        ←── .acl resolution ──►    wac::evaluate_access
      │                                                │
      ▼                                                ▼
  LDP handler      ←── body + metadata ──►  ldp::* (render, PATCH)
      │                                                │
      ▼                                                ▼
  Storage layer    ←── get/put/delete ───►  storage::Storage trait
      │
      ▼
  Notify subscribers ←── StorageEvent ──►   notifications::*
```

Each horizontal arrow crosses one of the trait or function APIs this
crate exposes. Swap any layer without touching the others.

## What solid-pod-rs is not

- **Not a client library.** If you're *consuming* a Pod from Rust,
  use a different crate. solid-pod-rs is server-side.
- **Not an HTTP server.** It's a library you drop into actix-web /
  axum / hyper / your favourite framework.
- **Not a WebID-OIDC identity provider.** It consumes tokens issued
  by an OP; running an OP is a different concern.
- **Not a quota / accounting system.** Pod-level quotas are a
  consumer-crate concern (see
  [PARITY-CHECKLIST.md](../../PARITY-CHECKLIST.md)).

## Next steps

- Tutorial 1: [your first Pod](../tutorials/01-your-first-pod.md).
- Reference: [Rust API surface](../reference/api.md).
- Explanation: [architecture decisions](architecture-decisions.md).
