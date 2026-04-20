# Tutorial 3 — Adding access control

**Goal:** author a JSON-LD `.acl` document, grant `acl:Read` to the
public, and observe the `WAC-Allow` response header change. ≤ 15
minutes.

## Prerequisites

- Tutorial 2 complete.
- A running example server (`cargo run --example standalone -p
  solid-pod-rs`).

## Step 1 — Observe the baseline (no ACL)

```bash
curl -sI http://127.0.0.1:8765/notes/ | grep -i wac-allow
```

Expected:

```text
wac-allow: user="", public=""
```

No one can do anything. Deny-by-default. This matches the Solid WAC
specification: if no ACL is effective for a resource, no access is
granted. See [explanation/security-model.md](../explanation/security-model.md).

## Step 2 — Write a root ACL

The WAC specification says ACL lookup walks up the path tree:
`/notes/` → `/` → ... until it finds an `.acl` document. We'll install
one at the pod root so it covers everything.

Create a file `root.acl.json`:

```json
{
  "@context": {
    "acl": "http://www.w3.org/ns/auth/acl#",
    "foaf": "http://xmlns.com/foaf/0.1/"
  },
  "@graph": [
    {
      "@id": "#public-read",
      "@type": "acl:Authorization",
      "acl:agentClass": { "@id": "foaf:Agent" },
      "acl:accessTo":   { "@id": "/" },
      "acl:default":    { "@id": "/" },
      "acl:mode":       { "@id": "acl:Read" }
    }
  ]
}
```

Install it at `/.acl`:

```bash
curl -i -X PUT \
     -H 'Content-Type: application/ld+json' \
     --data-binary @root.acl.json \
     http://127.0.0.1:8765/.acl
```

The path convention is: the ACL for `/foo` lives at `/foo.acl`; the
ACL for a container `/bar/` lives at `/bar/.acl`; the pod root's ACL
lives at `/.acl`.

## Step 3 — Observe the change

```bash
curl -sI http://127.0.0.1:8765/notes/ | grep -i wac-allow
```

Expected:

```text
wac-allow: user="read", public="read"
```

The `WAC-Allow` header now advertises:

- `user="read"` — whichever agent authenticated would have read.
- `public="read"` — anonymous clients have read.

Both values come from evaluating the `.acl` against the request. The
`foaf:Agent` class covers everyone, including anonymous requests.
See [reference/wac-modes.md](../reference/wac-modes.md).

## Step 4 — Add a per-agent write rule

Extend `root.acl.json`:

```json
{
  "@context": {
    "acl": "http://www.w3.org/ns/auth/acl#",
    "foaf": "http://xmlns.com/foaf/0.1/"
  },
  "@graph": [
    {
      "@id": "#public-read",
      "@type": "acl:Authorization",
      "acl:agentClass": { "@id": "foaf:Agent" },
      "acl:accessTo":   { "@id": "/" },
      "acl:default":    { "@id": "/" },
      "acl:mode":       { "@id": "acl:Read" }
    },
    {
      "@id": "#owner-write",
      "@type": "acl:Authorization",
      "acl:agent":    { "@id": "did:nostr:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" },
      "acl:accessTo": { "@id": "/" },
      "acl:default":  { "@id": "/" },
      "acl:mode":     [
        { "@id": "acl:Read" },
        { "@id": "acl:Write" },
        { "@id": "acl:Control" }
      ]
    }
  ]
}
```

Re-PUT it:

```bash
curl -i -X PUT \
     -H 'Content-Type: application/ld+json' \
     --data-binary @root.acl.json \
     http://127.0.0.1:8765/.acl
```

Now:

- Anonymous requests: `read`.
- `did:nostr:a…a` requests: `read write append control` (`Write`
  implies `Append` — see [reference/wac-modes.md](../reference/wac-modes.md#mode-implication)).

The example server authenticates via NIP-98. To exercise the authenticated
path you need a NIP-98 token — covered in
[how-to/configure-nip98-auth.md](../how-to/configure-nip98-auth.md).

## Step 5 — Understand ACL inheritance

- `acl:accessTo` applies to the exact resource (and, for containers,
  the container's representation).
- `acl:default` applies to everything inside the container,
  recursively.

In the example above both point to `/`, so the rule applies to the
root and to every descendant.

If you wanted to lock down `/notes/` but leave the rest open:

```json
{
  "@id": "#notes-owner-only",
  "@type": "acl:Authorization",
  "acl:agent":    { "@id": "did:nostr:owner" },
  "acl:accessTo": { "@id": "/notes/" },
  "acl:default":  { "@id": "/notes/" },
  "acl:mode":     [ { "@id": "acl:Read" }, { "@id": "acl:Write" } ]
}
```

...install at `/notes/.acl`. The walk-up resolver will pick this ACL
for anything under `/notes/` before continuing to `/.acl`.

See [reference/api.md §StorageAclResolver](../reference/api.md#storageaclresolver)
for the walk-up algorithm.

## Step 6 — Observe the ACL in server logs

Re-request the container with a trace:

```bash
RUST_LOG=solid_pod_rs=debug cargo run --example standalone -p solid-pod-rs
# in another terminal:
curl -sI http://127.0.0.1:8765/notes/
```

The tracing output shows which ACL was resolved and which rules
matched. If a request is denied see
[how-to/debug-acl-denials.md](../how-to/debug-acl-denials.md).

## Where to next

- Tutorial 4: [subscribe to changes](04-subscribing-to-changes.md).
- How-to: [debug ACL denials](../how-to/debug-acl-denials.md).
- Reference: [WAC modes](../reference/wac-modes.md).
- Explanation: [security model](../explanation/security-model.md).
