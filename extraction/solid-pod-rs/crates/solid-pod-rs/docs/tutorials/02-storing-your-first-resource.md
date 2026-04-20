# Tutorial 2 — Store your first resource

**Goal:** `PUT` a resource into the pod, read it back, inspect its
ETag, and delete it. ≤ 10 minutes.

## Prerequisites

- Tutorial 1 complete: the example server runs on
  `http://127.0.0.1:8765`.
- `curl` and `jq` (optional, for formatting output).

## Step 1 — Upload a JSON-LD resource

```bash
curl -i -X PUT \
     -H 'Content-Type: application/ld+json' \
     --data '{"@id":"#me","name":"Ada"}' \
     http://127.0.0.1:8765/notes/hello.jsonld
```

Expected:

```text
HTTP/1.1 201 Created
etag: "c7a2c1d3..."
link: <http://www.w3.org/ns/ldp#Resource>; rel="type", </notes/hello.jsonld.acl>; rel="acl", </notes/hello.jsonld.meta>; rel="describedby"
content-length: 0
```

A few things happened:

1. The pod created the container `/notes/` implicitly.
2. It stored the body and computed a SHA-256 ETag (hex-encoded, 64
   chars).
3. It wrote a sidecar `.meta.json` next to the body file (open
   `/tmp/solid-pod-rs-example/notes/hello.jsonld.meta.json` to see).
4. It responded with the `Link` header for the new resource — note
   that `rel="acl"` points to `hello.jsonld.acl` (not created yet).

## Step 2 — Read it back

```bash
curl -i http://127.0.0.1:8765/notes/hello.jsonld
```

Expected:

```text
HTTP/1.1 200 OK
content-type: application/ld+json
etag: "c7a2c1d3..."
wac-allow: user="", public=""
link: <http://www.w3.org/ns/ldp#Resource>; rel="type", </notes/hello.jsonld.acl>; rel="acl", </notes/hello.jsonld.meta>; rel="describedby"

{"@id":"#me","name":"Ada"}
```

The ETag is identical to the PUT response — the body hasn't changed.
The `Content-Type` was preserved verbatim because it was stored in the
meta sidecar.

## Step 3 — See the resource in its container

```bash
curl -s http://127.0.0.1:8765/notes/ | jq .
```

Output:

```json
{
  "@context": {
    "ldp": "http://www.w3.org/ns/ldp#",
    "dcterms": "http://purl.org/dc/terms/",
    "contains": { "@id": "ldp:contains", "@type": "@id" }
  },
  "@id": "/notes/",
  "@type": ["ldp:Container", "ldp:BasicContainer", "ldp:Resource"],
  "ldp:contains": [
    {
      "@id": "/notes/hello.jsonld",
      "@type": ["http://www.w3.org/ns/ldp#Resource"]
    }
  ]
}
```

The container response is LDP-compliant: it lists each child as an
`ldp:contains` entry with its LDP `@type`. Sub-containers (trailing
slash) carry the container types; plain resources carry only
`ldp:Resource`.

## Step 4 — Conditional update with `If-Match`

The ETag is the pod's concurrency token. Let's use it:

```bash
# Capture the current ETag
ETAG=$(curl -sI http://127.0.0.1:8765/notes/hello.jsonld \
       | awk -F'"' '/^etag:/ {print $2}')

# Replace only if nothing else has changed
curl -i -X PUT \
     -H 'Content-Type: application/ld+json' \
     -H "If-Match: \"$ETAG\"" \
     --data '{"@id":"#me","name":"Ada Lovelace"}' \
     http://127.0.0.1:8765/notes/hello.jsonld
```

> **Note:** the example server in `examples/standalone.rs` does not
> yet honour `If-Match`; the storage trait returns the ETag and the
> example accepts the write unconditionally. See
> [PARITY-CHECKLIST.md §Metadata / Link headers](../../PARITY-CHECKLIST.md)
> — If-Match enforcement is a P2 item. Production integrations should
> enforce it themselves; the ETag the storage layer returns is
> canonical.

## Step 5 — Post to a container (Slug → child)

```bash
curl -i -X POST \
     -H 'Content-Type: text/turtle' \
     -H 'Slug: note-1' \
     --data '<#n> <http://purl.org/dc/terms/title> "A note" .' \
     http://127.0.0.1:8765/notes/
```

The example currently proxies POST to PUT (see
[reference/http-endpoints.md](../reference/http-endpoints.md)), but
the `solid_pod_rs::ldp::resolve_slug` helper produces a safe
filename: it rejects slashes and `..`, and falls back to a UUID if
the Slug is missing or unsafe.

## Step 6 — Delete

```bash
curl -i -X DELETE http://127.0.0.1:8765/notes/hello.jsonld
```

Expected:

```text
HTTP/1.1 204 No Content
```

The body file and its `.meta.json` sidecar are both removed.

## What just happened — under the hood

Every HTTP method maps to a single `Storage` trait call:

| HTTP method | Storage call |
|---|---|
| `GET` | `storage.get(path)` |
| `HEAD` | `storage.head(path)` |
| `PUT` | `storage.put(path, body, ct)` |
| `DELETE` | `storage.delete(path)` |
| `GET` on container | `storage.list(container)` then `ldp::render_container` |

The FS backend stores each resource as two files:

- `<root>/<path>` — the body
- `<root>/<path>.meta.json` — `{ "content_type": ..., "links": [...] }`

See [reference/api.md §Storage](../reference/api.md#storage-trait) for
the full trait.

## Where to next

- Tutorial 3: [add access control](03-adding-access-control.md).
- Reference: [`Link` headers emitted per path kind](../reference/link-headers.md).
- How-to: [swap the FS backend for in-memory](../how-to/swap-storage-backends.md).
