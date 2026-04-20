# Tutorial 1 — Your first Pod

**Goal:** bring up a Solid Pod server on `localhost:8765`, make a
first request, and read back an LDP-compliant response. ≤ 10 minutes.

## You will learn

- How the crate is wired up in a minimal actix-web example.
- What headers a Solid Pod emits on the first `GET /`.
- Where the Pod's data lives on disk.
- How to terminate the server cleanly.

## Prerequisites

- Rust toolchain 1.74 or newer (`rustup default stable`).
- `curl` (or any HTTP client).
- The `solid-pod-rs` workspace checked out.

No Docker. No database. No TLS. Everything is local.

## Step 1 — Build and run the example server

From the workspace root:

```bash
cargo run --example standalone -p solid-pod-rs
```

On first run the crate compiles; subsequent runs take a few seconds.
You will see a line like:

```text
solid-pod-rs example running on http://127.0.0.1:8765 (root: /tmp/solid-pod-rs-example)
```

The server is now accepting requests. The Pod root directory is
`/tmp/solid-pod-rs-example` (your OS's temp directory).

## Step 2 — Make your first request

In a second terminal:

```bash
curl -i http://127.0.0.1:8765/
```

Expected response:

```text
HTTP/1.1 200 OK
content-type: application/ld+json
wac-allow: user="", public=""
link: <http://www.w3.org/ns/ldp#BasicContainer>; rel="type", <http://www.w3.org/ns/ldp#Container>; rel="type", <http://www.w3.org/ns/ldp#Resource>; rel="type", </.acl>; rel="acl", </.meta>; rel="describedby", </>; rel="http://www.w3.org/ns/pim/space#storage"
content-length: 248

{"@context":{"ldp":"http://www.w3.org/ns/ldp#","dcterms":"http://purl.org/dc/terms/","contains":{"@id":"ldp:contains","@type":"@id"}},"@id":"/","@type":["ldp:Container","ldp:BasicContainer","ldp:Resource"],"ldp:contains":[]}
```

Let's walk through what happened.

### The body

The body is JSON-LD. It represents the pod root as an empty LDP
`BasicContainer`. The `ldp:contains` array is empty because there are
no resources yet.

### The `Link` headers

Every Solid resource carries four categories of `Link` header. See
[reference/link-headers.md](../reference/link-headers.md) for the full
matrix. For the root you get:

- Three `rel="type"` values identifying the LDP class of the resource.
- `rel="acl"` pointing to `/.acl` — the ACL sidecar for this resource.
- `rel="describedby"` pointing to `/.meta` — the metadata sidecar.
- `rel="http://www.w3.org/ns/pim/space#storage"` pointing to `/` — this
  marks `/` as the [pim:Storage](https://www.w3.org/ns/pim/space#Storage)
  root. Only the pod root has this.

### The `WAC-Allow` header

`user=""` + `public=""` means no one has any access. That sounds
scary but it is correct: we have not yet installed an ACL document.
No ACL = no access. The server's deny-by-default posture is deliberate
and is one of the [architectural
decisions](../explanation/architecture-decisions.md).

## Step 3 — Inspect the filesystem layout

```bash
ls -la /tmp/solid-pod-rs-example
```

It is empty. The pod root exists implicitly — the server synthesises
an empty `BasicContainer` for `GET /` when the directory has nothing
in it. That is the normal LDP behaviour: containers are always
*addressable*, even if empty.

You will watch this directory fill up in the next tutorial.

## Step 4 — Stop the server

`Ctrl+C` in the terminal running `cargo run`. The temp directory is
**not** cleaned up; it persists until your OS clears `/tmp`.

## Where to next

- Tutorial 2: [store your first resource](02-storing-your-first-resource.md)
  — `PUT` a JSON-LD document and read back its ETag.
- Reference: [HTTP endpoint matrix](../reference/http-endpoints.md).
- Explanation: [why we built a framework-agnostic crate](../explanation/architecture-decisions.md).

## Troubleshooting

- **Port 8765 is already in use.** Edit `examples/standalone.rs`,
  change the `.bind(...)` port, and re-run.
- **Compile error about missing `actix-web`.** `actix-web` is a
  dev-dependency — make sure you are running `cargo run --example
  standalone -p solid-pod-rs`, not `cargo run -p solid-pod-rs`.
- **Permission denied writing to `/tmp`.** Run with `TMPDIR=$HOME/tmp
  cargo run --example standalone -p solid-pod-rs` (the example server
  honours `TMPDIR`).
