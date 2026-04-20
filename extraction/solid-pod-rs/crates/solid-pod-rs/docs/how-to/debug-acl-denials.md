# How to debug ACL denials

**Goal:** systematically diagnose why a request is being denied by
WAC.

## Symptoms

- `WAC-Allow: user="", public=""` where you expected modes.
- 403 responses on methods that ought to succeed.
- "It worked on JSS" complaints after a migration.

## Step 1 — Confirm an ACL is found

```rust
use solid_pod_rs::wac::{StorageAclResolver, AclResolver};

let resolver = StorageAclResolver::new(storage.clone());
let doc = resolver.find_effective_acl("/notes/foo").await?;
match doc {
    Some(_) => tracing::info!("ACL resolved for /notes/foo"),
    None    => tracing::warn!("no ACL found above /notes/foo — deny-by-default"),
}
```

The resolver walks up the path tree: `/notes/foo` → `/notes/foo.acl`
→ `/notes/.acl` → `/.acl`. First hit wins. If none exist, you are in
deny-by-default territory.

See [reference/api.md §StorageAclResolver](../reference/api.md#storageaclresolver).

## Step 2 — Dump the ACL your resolver actually reads

```bash
curl -s http://pod.example/.acl | jq .
```

Check the `@graph` is non-empty and each authorization has:

- At least one of `acl:agent`, `acl:agentClass`, `acl:agentGroup`.
- An `acl:accessTo` **or** `acl:default`.
- At least one `acl:mode`.

A rule missing any of these is silently ignored — it will never match.

## Step 3 — Evaluate the rule manually

```rust
use solid_pod_rs::wac::{evaluate_access, AccessMode};

let granted = evaluate_access(
    doc.as_ref(),
    Some("did:nostr:aaaa...aaaa"),   // agent WebID or None for anonymous
    "/notes/foo",                     // resource path
    AccessMode::Write,                // mode you expect
);
tracing::info!(granted, "evaluate_access");
```

If `granted == false`, one of the following is true:

1. No authorization contains the requested mode. (`acl:Write` is not
   implied by `acl:Read`. `acl:Write` **does** imply `acl:Append`.)
2. The agent doesn't match. `acl:agent` is exact-URI; `acl:agentClass
   foaf:Agent` matches everyone; `acl:agentClass acl:AuthenticatedAgent`
   matches any logged-in agent; `acl:agentGroup` requires a group
   resolver (see Step 6).
3. The path doesn't match. `acl:accessTo` must be exactly the
   resource, or a container that contains the resource. `acl:default`
   applies to the container's descendants.

## Step 4 — Check for path mismatches

The resolver normalises paths as follows (see
[`wac.rs::normalize_path`](../../src/wac.rs)):

- Strip leading `./` or `.`.
- Strip trailing `/` unless the path is `/`.

So `acl:accessTo` values of `./`, `/`, `./foo/`, `/foo` all normalise
to the same path as `/foo`. That is intentional: JSON-LD serialisers
emit base-relative IRIs, and we accept them.

Common mistakes:

- `acl:accessTo` of `/notes` with no trailing slash on a container —
  fine, normalised.
- `acl:accessTo` of `notes/` missing the leading slash — fine,
  normalised to `/notes`.
- `acl:accessTo` of `https://pod.example.com/notes/` (absolute URI)
  — **will not match** relative paths. Pick one style and stick to
  it across your ACL corpus.

## Step 5 — Check mode implication

solid-pod-rs encodes the WAC mode hierarchy:

| If `acl:mode` is… | Grants modes |
|---|---|
| `acl:Read` | `Read` |
| `acl:Write` | `Write` + `Append` |
| `acl:Append` | `Append` |
| `acl:Control` | `Control` |

`Read` does **not** imply anything else. `Write` does **not** imply
`Read` — you must grant both separately. See
[reference/wac-modes.md](../reference/wac-modes.md).

## Step 6 — Groups not resolving?

`acl:agentGroup` requires a group-membership resolver. The stock
`evaluate_access` uses a no-op resolver that returns `false` for
every call.

Pass a real resolver via `evaluate_access_with_groups`:

```rust
use solid_pod_rs::wac::{evaluate_access_with_groups, StaticGroupMembership};

let mut groups = StaticGroupMembership::new();
groups.add(
    "https://pod.example/groups/editors",
    vec!["did:nostr:alice".into(), "did:nostr:bob".into()],
);

let ok = evaluate_access_with_groups(
    doc.as_ref(),
    Some("did:nostr:alice"),
    "/shared/",
    AccessMode::Write,
    &groups,
);
```

For dynamic group documents, implement the `GroupMembership` trait
with a fetcher.

## Step 7 — Is the request actually authenticated?

`evaluate_access` takes `Option<&str>`. If you pass `None`:

- `acl:agent`: never matches.
- `acl:agentClass acl:AuthenticatedAgent`: never matches.
- `acl:agentClass foaf:Agent`: matches.
- `acl:agentGroup`: never matches.

So if a rule says "agent Alice, mode Write" and Alice's NIP-98 token
was rejected earlier in the pipeline, `agent_uri == None` and the
rule silently doesn't apply. Log the authentication result.

## Step 8 — Reproduce with a unit test

Fastest diagnostic loop: pull the offending ACL into a test.

```rust
#[test]
fn regression_public_read_on_notes() {
    let doc: AclDocument = serde_json::from_str(include_str!("fixtures/acl.json")).unwrap();
    assert!(evaluate_access(Some(&doc), None, "/notes/foo", AccessMode::Read));
}
```

## Common root causes

| Symptom | Likely cause |
|---|---|
| All requests denied on a brand-new pod | No `/.acl` installed; deny-by-default |
| Write fails, read works | Missing `acl:Write` mode |
| Works for one specific agent only | Rule uses `acl:agent`, not `acl:agentClass` |
| Works on `/notes/` but not `/notes/foo` | Missing `acl:default` on the container rule |
| Anonymous access denied, authenticated works | Rule uses `acl:AuthenticatedAgent`, not `foaf:Agent` |

## See also

- [tutorial 3: Adding access control](../tutorials/03-adding-access-control.md)
- [reference/wac-modes.md](../reference/wac-modes.md)
- [explanation/security-model.md](../explanation/security-model.md)
