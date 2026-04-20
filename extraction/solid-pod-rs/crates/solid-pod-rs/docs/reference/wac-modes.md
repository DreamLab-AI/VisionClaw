# WAC modes reference

Web Access Control defines four access modes. solid-pod-rs encodes
them in the `AccessMode` enum and `map_mode` function in
[`src/wac.rs`](../../src/wac.rs).

## The four modes

| Mode | IRI | Meaning |
|---|---|---|
| `Read`    | `http://www.w3.org/ns/auth/acl#Read`    | GET / HEAD a resource |
| `Write`   | `http://www.w3.org/ns/auth/acl#Write`   | PUT / DELETE / PATCH a resource |
| `Append`  | `http://www.w3.org/ns/auth/acl#Append`  | POST to a container (create child) |
| `Control` | `http://www.w3.org/ns/auth/acl#Control` | Read or write the ACL sidecar |

## Mode implication

solid-pod-rs implements the WAC spec's single implication rule:

| Granted in ACL | Expands to |
|---|---|
| `acl:Read`    | `Read` |
| `acl:Write`   | `Write` + `Append` |
| `acl:Append`  | `Append` |
| `acl:Control` | `Control` |

Critically:

- `Write` implies `Append`. A rule granting `acl:Write` also permits
  `POST`.
- `Read` does **not** imply `Append` or `Write`.
- `Control` does **not** imply `Read` or `Write`. A rule granting
  only `Control` lets you read/modify `.acl` but not the resource
  itself.

See `fn map_mode` in `src/wac.rs`:

```rust
fn map_mode(mode_ref: &str) -> &'static [AccessMode] {
    match mode_ref {
        "acl:Read"    | "http://www.w3.org/ns/auth/acl#Read"    => &[AccessMode::Read],
        "acl:Write"   | "http://www.w3.org/ns/auth/acl#Write"   => &[AccessMode::Write, AccessMode::Append],
        "acl:Append"  | "http://www.w3.org/ns/auth/acl#Append"  => &[AccessMode::Append],
        "acl:Control" | "http://www.w3.org/ns/auth/acl#Control" => &[AccessMode::Control],
        _ => &[],
    }
}
```

Prefix or IRI form is accepted (`acl:Read` ≡ the full IRI).

## HTTP method → required mode

```rust
pub fn method_to_mode(method: &str) -> AccessMode;
```

| HTTP method | Required mode |
|---|---|
| `GET`    | `Read`   |
| `HEAD`   | `Read`   |
| `PUT`    | `Write`  |
| `DELETE` | `Write`  |
| `PATCH`  | `Write`  |
| `POST`   | `Append` |
| (other)  | `Read`   (fallback) |

Note: `POST` to a container requires only `Append`. `PUT` at the
child path (same net effect) requires `Write`. A resource owner who
wishes to accept public uploads but not public overwrites should grant
`acl:Append` to `foaf:Agent` and keep `acl:Write` restricted.

## Mode naming

```rust
pub fn mode_name(mode: AccessMode) -> &'static str;
```

- `Read`    → `"read"`
- `Write`   → `"write"`
- `Append`  → `"append"`
- `Control` → `"control"`

Used by `wac_allow_header` when serialising the `WAC-Allow` response.

## `WAC-Allow` header

```rust
pub fn wac_allow_header(
    acl_doc:       Option<&AclDocument>,
    agent_uri:     Option<&str>,
    resource_path: &str,
) -> String;
```

Shape: `user="<modes>", public="<modes>"`. `user` reflects what the
requesting agent can do; `public` reflects what an anonymous requester
could do. Both fields come from the same ACL document.

Examples:

- `user="read write append", public="read"` — authenticated writers,
  public readers.
- `user="read write append control", public=""` — owner-only resource.
- `user="", public=""` — no ACL in effect, deny-by-default.

## `acl:accessTo` vs `acl:default`

| Predicate | Applies to |
|---|---|
| `acl:accessTo` | The exact resource named by the object. Also matches children when the object is a container. |
| `acl:default`  | Every descendant of the named container. Inherited down the tree. |

Both are normalised via `normalize_path` — trailing slashes don't
matter, leading `./` is stripped.

A typical pattern: for a container, emit one rule with both
`acl:accessTo` (so the container itself is accessible) and
`acl:default` (so descendants inherit). See
[tutorial 3](../tutorials/03-adding-access-control.md).

## Agent matchers

| Field                                     | Matches |
|-------------------------------------------|---------|
| `acl:agent <uri>`                         | The specific agent URI, exactly. |
| `acl:agentClass foaf:Agent`               | Everyone (including anonymous). |
| `acl:agentClass acl:AuthenticatedAgent`   | Anyone with a non-None `agent_uri`. |
| `acl:agentGroup <uri>`                    | Any member of the named group document, resolved via `GroupMembership`. |

Multiple matchers in a single authorization are OR'd: a rule with
both `acl:agent X` and `acl:agentClass foaf:Agent` matches everyone.

## Tests

- `public_read_grants_anonymous`
- `write_implies_append`
- `method_mapping`
- `wac_allow_shape`
- `no_acl_denies_all`
- `tests/wac_basic.rs`
- `tests/wac_inheritance.rs` (28 scenarios)

## See also

- [reference/api.md §wac](api.md#wac)
- [explanation/security-model.md](../explanation/security-model.md)
- [WAC spec](https://solid.github.io/web-access-control-spec/)
