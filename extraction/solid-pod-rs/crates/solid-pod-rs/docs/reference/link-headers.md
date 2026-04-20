# `Link` header reference

The `link_headers(path)` function in [`src/ldp.rs`](../../src/ldp.rs)
computes the full `Link` header set emitted on every response. Each
returned string is a single `Link` *value* (no outer commas); the
caller concatenates them with `", "` when composing the header.

## Emission matrix

| Condition           | Emitted value                                                        |
|---------------------|-----------------------------------------------------------------------|
| always              | `<http://www.w3.org/ns/ldp#Resource>; rel="type"`                     |
| path is a container | `<http://www.w3.org/ns/ldp#BasicContainer>; rel="type"` **and** `<http://www.w3.org/ns/ldp#Container>; rel="type"` |
| path is not `.acl`  | `<{path}.acl>; rel="acl"`                                             |
| path is neither `.acl` nor `.meta` | `<{path}.meta>; rel="describedby"`                     |
| path is `/`         | `</>; rel="http://www.w3.org/ns/pim/space#storage"`                  |

"Container" here means the path ends with `/` (or is `/`).

## Full output per path kind

### Pod root `/`

```
<http://www.w3.org/ns/ldp#BasicContainer>; rel="type"
<http://www.w3.org/ns/ldp#Container>; rel="type"
<http://www.w3.org/ns/ldp#Resource>; rel="type"
</.acl>; rel="acl"
</.meta>; rel="describedby"
</>; rel="http://www.w3.org/ns/pim/space#storage"
```

### Sub-container `/notes/`

```
<http://www.w3.org/ns/ldp#BasicContainer>; rel="type"
<http://www.w3.org/ns/ldp#Container>; rel="type"
<http://www.w3.org/ns/ldp#Resource>; rel="type"
</notes/.acl>; rel="acl"
</notes/.meta>; rel="describedby"
```

### Resource `/notes/hello.jsonld`

```
<http://www.w3.org/ns/ldp#Resource>; rel="type"
</notes/hello.jsonld.acl>; rel="acl"
</notes/hello.jsonld.meta>; rel="describedby"
```

### ACL sidecar `/notes/hello.jsonld.acl`

```
<http://www.w3.org/ns/ldp#Resource>; rel="type"
```

No `rel="acl"` (it is the ACL), no `rel="describedby"` (by
convention, ACL docs are not described by a meta).

### Meta sidecar `/notes/hello.jsonld.meta`

```
<http://www.w3.org/ns/ldp#Resource>; rel="type"
<.../hello.jsonld.meta.acl>; rel="acl"
```

No `rel="describedby"` on meta resources themselves.

## IRI constants

All IRIs are exposed via `ldp::iri`:

| Constant | Value |
|---|---|
| `LDP_RESOURCE`          | `http://www.w3.org/ns/ldp#Resource` |
| `LDP_CONTAINER`         | `http://www.w3.org/ns/ldp#Container` |
| `LDP_BASIC_CONTAINER`   | `http://www.w3.org/ns/ldp#BasicContainer` |
| `LDP_CONTAINS`          | `http://www.w3.org/ns/ldp#contains` |
| `PIM_STORAGE_REL`       | `http://www.w3.org/ns/pim/space#storage` |
| `PIM_STORAGE`           | `http://www.w3.org/ns/pim/space#Storage` |
| `DCTERMS_MODIFIED`      | `http://purl.org/dc/terms/modified` |
| `STAT_SIZE`             | `http://www.w3.org/ns/posix/stat#size` |
| `STAT_MTIME`            | `http://www.w3.org/ns/posix/stat#mtime` |
| `XSD_DATETIME`          | `http://www.w3.org/2001/XMLSchema#dateTime` |
| `XSD_INTEGER`           | `http://www.w3.org/2001/XMLSchema#integer` |
| `ACL_NS`                | `http://www.w3.org/ns/auth/acl#` |

## Why `BasicContainer` + `Container` + `Resource`

LDP §5.2.1.4 requires a basic container to advertise all three types.
Clients that only understand `BasicContainer` will still read its
children via `ldp:contains`; clients that only speak `Container`
get the representation abstract type; clients that only speak
`Resource` can at least perform metadata discovery.

## Why the `pim:storage` marker on `/`

Solid clients use `rel="http://www.w3.org/ns/pim/space#storage"` to
discover the pod's storage root (useful when the pod is rooted at a
non-`/` path, e.g. `https://host/user/` — in our case root is
literally `/`, but the marker is still correct and enables clients
that rely on it).

This marker is emitted **only** when the path is exactly `/`.

## Tests

`link_headers` behaviour is covered by:

- `link_headers_include_acl_and_describedby`
- `link_headers_root_exposes_pim_storage`
- `link_headers_skip_describedby_on_meta`
- `link_headers_skip_acl_on_acl`

in [`src/ldp.rs`](../../src/ldp.rs).

## See also

- [reference/http-endpoints.md](http-endpoints.md)
- [reference/api.md §ldp](api.md#ldp)
- [LDP §5.2.1](https://www.w3.org/TR/ldp/#ldpc-serialization)
