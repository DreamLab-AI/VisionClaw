# `Prefer` header reference

The `Prefer` header (RFC 7240) allows a client to ask for a specific
container representation. solid-pod-rs parses preferences via
[`ldp::PreferHeader::parse`](../../src/ldp.rs).

## Supported preferences

Only `return=representation` preferences with `include=` or `omit=`
tokens are honoured. All other tokens are ignored (RFC 7240 §2 permits
this).

| IRI | Effect |
|---|---|
| `http://www.w3.org/ns/ldp#PreferMinimalContainer` | `representation = MinimalContainer`; container metadata only, no `ldp:contains`. |
| `http://www.w3.org/ns/ldp#PreferContainedIRIs`    | `representation = ContainedIRIsOnly`; list of IRIs only, no container metadata. |
| `http://www.w3.org/ns/ldp#PreferMembership`       | (in `omit=`) `omit_membership = true` — not currently enforced independently; merged with `MinimalContainer`. |

## Parsed shape

```rust
pub enum ContainerRepresentation { Full, MinimalContainer, ContainedIRIsOnly }

pub struct PreferHeader {
    pub representation:         ContainerRepresentation,
    pub include_minimal:        bool,
    pub include_contained_iris: bool,
    pub omit_membership:        bool,
}
```

`PreferHeader::default()` yields `representation = Full` — all four
fields are `false` / `Full`.

## Worked examples

### `Prefer: return=representation; include="http://www.w3.org/ns/ldp#PreferMinimalContainer"`

Parsed as:

```rust
PreferHeader {
    representation: ContainerRepresentation::MinimalContainer,
    include_minimal: true,
    include_contained_iris: false,
    omit_membership: false,
}
```

Response body (JSON-LD):

```json
{
  "@context": {
    "ldp":     "http://www.w3.org/ns/ldp#",
    "dcterms": "http://purl.org/dc/terms/"
  },
  "@id":   "/notes/",
  "@type": ["ldp:Container", "ldp:BasicContainer", "ldp:Resource"]
}
```

### `Prefer: return=representation; include="http://www.w3.org/ns/ldp#PreferContainedIRIs"`

```rust
PreferHeader {
    representation: ContainerRepresentation::ContainedIRIsOnly,
    include_minimal: false,
    include_contained_iris: true,
    omit_membership: false,
}
```

Response body:

```json
{
  "@id": "/notes/",
  "ldp:contains": [
    { "@id": "/notes/a.txt" },
    { "@id": "/notes/b.txt" }
  ]
}
```

### No `Prefer` header (default)

`PreferHeader::default()` → `representation = Full`. Full JSON-LD
document with `@context`, types, and `ldp:contains` entries carrying
both `@id` and `@type`.

## Multi-token parsing

The parser tolerates comma-separated preference lists:

```
Prefer: handling=strict,
        return=representation; include="http://www.w3.org/ns/ldp#PreferMinimalContainer"
```

`handling=strict` is ignored; the `return=representation` block is
applied.

## Tests

- `prefer_minimal_container_parsed`
- `prefer_contained_iris_parsed`
- `render_container_minimal_omits_contains`

in [`src/ldp.rs`](../../src/ldp.rs).

## See also

- [reference/content-types.md](content-types.md)
- [reference/api.md §ldp](api.md#ldp)
- [LDP §4.2.2](https://www.w3.org/TR/ldp/#ldp-prefer-parameters)
- [RFC 7240](https://datatracker.ietf.org/doc/html/rfc7240)
