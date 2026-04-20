# Content-type reference

solid-pod-rs negotiates and serialises four RDF syntaxes. This page
lists the full matrix: MIME types recognised, how they map to the
internal `RdfFormat` enum, and which operations support which
format.

## Format enum

```rust
pub enum RdfFormat { Turtle, JsonLd, NTriples, RdfXml }

impl RdfFormat {
    pub fn mime(&self)                  -> &'static str;
    pub fn from_mime(mime: &str)        -> Option<Self>;
}
```

## MIME recognition matrix

| Accept / Content-Type value | `RdfFormat` |
|---|---|
| `text/turtle`                 | `Turtle`   |
| `application/turtle`          | `Turtle`   |
| `application/x-turtle`        | `Turtle`   |
| `application/ld+json`         | `JsonLd`   |
| `application/json+ld`         | `JsonLd`   |
| `application/n-triples`       | `NTriples` |
| `text/plain+ntriples`         | `NTriples` |
| `application/rdf+xml`         | `RdfXml`   |
| any other                     | `None` (defaults to Turtle) |

## Accept-header negotiation

```rust
pub fn negotiate_format(accept: Option<&str>) -> RdfFormat;
```

- `*/*`, `application/*`, `text/*`: `Turtle`.
- `None` / empty: `Turtle`.
- q-values respected; on ties `Turtle` wins.
- Unknown MIME types are skipped; the next candidate wins.

Examples:

| `Accept` header                                                                  | Result      |
|----------------------------------------------------------------------------------|-------------|
| (missing / empty)                                                                | `Turtle`    |
| `*/*`                                                                            | `Turtle`    |
| `text/turtle`                                                                    | `Turtle`    |
| `application/ld+json`                                                            | `JsonLd`    |
| `application/ld+json, text/turtle;q=0.5`                                         | `JsonLd`    |
| `application/ld+json;q=0.5, text/turtle;q=0.9`                                   | `Turtle`    |
| `application/rdf+xml, text/turtle;q=0.8`                                         | `RdfXml`    |
| `application/n-triples;q=1.0, text/turtle;q=0.9`                                 | `NTriples`  |

## Operation support matrix

| Operation | Turtle | JSON-LD | N-Triples | RDF/XML |
|---|---|---|---|---|
| `render_container_jsonld`                          | —   | ✓ | — | — |
| `render_container_turtle`                          | ✓   | — | — | — |
| `render_container` (alias)                         | —   | ✓ | — | — |
| `Graph::to_ntriples` / `Graph::parse_ntriples`     | —   | — | ✓ | — |
| `apply_n3_patch`                                   | partial (N3 subset for inserts/deletes/where) | — | — | — |
| `apply_sparql_patch`                               | — (SPARQL 1.1 text, not RDF) | — | — | — |
| Content negotiation (`negotiate_format`)           | ✓   | ✓ | ✓ | ✓ |
| Accept-Post advertisement (`ACCEPT_POST`)          | ✓   | ✓ | ✓ | — |
| RDF/XML serialisation                              | —   | — | — | partial (negotiated; serialisation deferred to consumer crate) |

## `ACCEPT_POST`

```rust
pub const ACCEPT_POST: &str = "text/turtle, application/ld+json, application/n-triples";
```

This is the `Accept-Post` header a server advertises on container
responses. Callers that accept other formats must emit their own
header.

## PATCH content-type matrix

| `Content-Type`                         | `PatchDialect` |
|----------------------------------------|----------------|
| `text/n3`                              | `N3`           |
| `application/n3`                       | `N3`           |
| `application/sparql-update`            | `SparqlUpdate` |
| `application/sparql-update+update`     | `SparqlUpdate` |
| anything else                          | `None` → 415   |

See [reference/patch-semantics.md](patch-semantics.md).

## Storage of content types

`ResourceMeta.content_type` carries the content type as supplied on
`PUT`. It is returned verbatim on `GET` — solid-pod-rs does **not**
transcode resource bodies between formats.

If you need server-side conversion (e.g. render a Turtle-stored
resource as JSON-LD to match `Accept`), do it at the HTTP layer. The
library's `Graph` type is the canonical conversion point: parse
N-Triples in, serialise any supported format out.

## Content types solid-pod-rs does not handle

- RDF/XML serialisation (negotiated, but emitting RDF/XML is left to
  the consumer).
- JSON Patch (RFC 6902) — deliberately not ported from the
  pod-worker source; see
  [PARITY-CHECKLIST.md](../../PARITY-CHECKLIST.md).
- Binary bodies: served verbatim with their declared content type; no
  RDF semantics applied.

## Tests

- `negotiate_prefers_explicit_turtle`
- `negotiate_falls_back_to_turtle`
- `negotiate_picks_jsonld_when_highest`
- `ntriples_roundtrip`
- `patch_dialect_detection`

in [`src/ldp.rs`](../../src/ldp.rs).

## See also

- [reference/prefer-headers.md](prefer-headers.md)
- [reference/patch-semantics.md](patch-semantics.md)
- [reference/api.md §RdfFormat](api.md#rdf-format--content-negotiation)
