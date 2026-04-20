# PATCH semantics reference

solid-pod-rs supports two PATCH dialects: solid-protocol N3 PATCH
and a subset of SPARQL 1.1 Update. JSON Patch (RFC 6902) is **not**
supported; see
[PARITY-CHECKLIST.md](../../PARITY-CHECKLIST.md#patch).

## Dialect detection

```rust
pub enum PatchDialect { N3, SparqlUpdate }
pub fn patch_dialect_from_mime(mime: &str) -> Option<PatchDialect>;
```

| `Content-Type`                     | `PatchDialect` |
|------------------------------------|----------------|
| `text/n3`, `application/n3`        | `N3`           |
| `application/sparql-update`, `application/sparql-update+update` | `SparqlUpdate` |
| anything else                      | `None` → 415   |

## N3 PATCH

```rust
pub fn apply_n3_patch(target: Graph, patch: &str) -> Result<PatchOutcome, PodError>;
```

Recognised body shape (per
[Solid Protocol §8.2](https://solidproject.org/TR/protocol#n3-patch)):

```turtle
_:rename a solid:InsertDeletePatch ;
  solid:inserts { <#s> <#p> <#o> . } ;
  solid:deletes { <#s> <#p> <#o> . } ;
  solid:where   { <#s> <#p> ?var . } .
```

The parser hunts for blocks delimited by curly braces after any of:

- `insert`, `inserts`, `solid:inserts`
- `delete`, `deletes`, `solid:deletes`
- `where`, `solid:where`

Contents of each block are parsed as N-Triples. Full Turtle prefix
declarations inside the block are **not** supported — the parser
expects absolute IRIs or simple literals.

### WHERE semantics

Every triple in the WHERE block must be present in the target graph.
If any triple is missing, the operation fails with
`PodError::PreconditionFailed` (HTTP 412).

Variables (`?var`) are permitted in the WHERE block but are not
bound — the matcher requires an exact triple match. Clients that
need real SPARQL-style WHERE resolution should use
`apply_sparql_patch` with `DELETE ... WHERE` instead.

### Order of operations

1. WHERE preconditions are verified.
2. DELETE triples are removed (only triples actually present are
   counted toward `PatchOutcome.deleted`).
3. INSERT triples are added.

The combined result is deterministic: INSERT's `inserted` count is
the size of the insert block; DELETE's `deleted` count is the set
intersection of the delete block with the target graph before the
patch.

### Example

```rust
use solid_pod_rs::ldp::{apply_n3_patch, Graph, Term, Triple};

let mut g = Graph::new();
g.insert(Triple::new(
    Term::iri("http://s/a"),
    Term::iri("http://p/drop"),
    Term::literal("old"),
));

let patch = r#"
    _:r a solid:InsertDeletePatch ;
      solid:deletes {
        <http://s/a> <http://p/drop> "old" .
      } ;
      solid:inserts {
        <http://s/a> <http://p/new> "shiny" .
      } .
"#;

let outcome = apply_n3_patch(g, patch).unwrap();
assert_eq!(outcome.inserted, 1);
assert_eq!(outcome.deleted, 1);
```

## SPARQL-Update PATCH

```rust
pub fn apply_sparql_patch(target: Graph, update: &str) -> Result<PatchOutcome, PodError>;
```

Parses via `spargebra`. Supports:

| Operation             | Behaviour |
|-----------------------|-----------|
| `INSERT DATA { ... }` | Each ground quad in the default graph is inserted. |
| `DELETE DATA { ... }` | Each ground quad in the default graph is removed. |
| `DELETE { ... } INSERT { ... } WHERE { ... }` | Deletes + inserts — only ground templates (no variables) are applied. |

Unsupported (returns `PodError::Unsupported`):

- `LOAD`, `CLEAR`, `CREATE`, `DROP`, `COPY`, `MOVE`, `ADD`.
- Named graphs — only the default graph is handled.
- Variables in template positions (WHERE-binding resolution is not
  implemented).

### Example

```rust
let update = r#"INSERT DATA { <http://s> <http://p> "v" . }"#;
let outcome = apply_sparql_patch(Graph::new(), update).unwrap();
assert_eq!(outcome.inserted, 1);
```

## `PatchOutcome`

```rust
pub struct PatchOutcome {
    pub graph:    Graph,   // post-patch graph
    pub inserted: usize,
    pub deleted:  usize,
}
```

The caller serialises `graph` back to the storage layer (typically as
N-Triples or the resource's original format).

## Error mapping

| `PodError`                 | HTTP |
|----------------------------|------|
| `PreconditionFailed(msg)`  | 412  |
| `Unsupported(msg)`         | 400 or 415 depending on the layer |
| (parse / decode)           | 400  |

## Atomicity

Both `apply_n3_patch` and `apply_sparql_patch` operate on an in-memory
graph and return a complete replacement. The HTTP handler that calls
these must itself make the "read-graph → patch → write-graph" cycle
atomic with respect to concurrent requests (typically via `If-Match`
or a storage-level lock).

## Tests

- `n3_patch_insert_and_delete`
- `n3_patch_where_failure_returns_precondition`
- `sparql_insert_data`
- `sparql_delete_data`
- `patch_dialect_detection`

in [`src/ldp.rs`](../../src/ldp.rs).

## See also

- [reference/content-types.md](content-types.md)
- [reference/api.md §ldp::apply_n3_patch](api.md#ldp)
- [Solid Protocol §8.2 N3 Patch](https://solidproject.org/TR/protocol#n3-patch)
- [SPARQL 1.1 Update](https://www.w3.org/TR/sparql11-update/)
