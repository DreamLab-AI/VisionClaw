# Relation-weight tables (ADR-2191)

A relation-weight table gives each relation type its own spring stiffness in the
force pass. The factor multiplies the edge weight the force kernels already use,
so it takes effect in the default LinLog mode and in Hooke mode, with no kernel
change. Label propagation, degree, SSSP, Louvain and Leiden keep the unscaled
weights. See [ADR-2191](../adr/ADR-2191-per-relation-stiffness-is-a-spring-weight-table-on-the-existing-weight-path.md).

| File | Provenance | Effect |
|---|---|---|
| [`uniform.toml`](uniform.toml) | `uniform` | None. The force pass gets weights bit-identical to the feature being off. |
| [`hand-set-two-relations.toml`](hand-set-two-relations.toml) | `hand-set` | `rdfs:subClassOf` (is-a) pulls 4×, `ngm:relatedTo` pulls 0.25×. |

## Format

```toml
format = "visionclaw.relation-weights"
version = 1

[provenance]
source = "hand-set"          # uniform | hand-set | learned
note = "free text"
# learned only, all required: producer_crate, commit (40-hex), preregistration_result

[composition]
registry_weight = "compose"  # compose: weight x factor; ignore: factor alone for matched edges

[[relation]]
match = "iri"                # iri (owl_property_iri) | edge_type (normalised edge_type)
key = "http://www.w3.org/2000/01/rdf-schema#subClassOf"
stiffness = 4.0              # finite, 0 to 4
```

An edge first matches a row by its predicate IRI, then by its normalised edge
type (`depends-on`, `Depends On` and `depends_on` are one key). If neither matches,
the edge keeps its weight unchanged. A `learned` table loads only with
`VISIONCLAW_RELATION_WEIGHTS_ALLOW_LEARNED=true`, and none is authorised. ADR-2174's
experiment did not support learned per-relation geometry (H1 not supported,
`77f6702b9`), so ADR-2191 keeps the flag false and ships no learned table.

## Switching it on (dev stack, from the host shell)

The table is read once at start, so every change needs `launch.sh up`, which
recreates the container when the environment changes.

1. Check out a revision that contains ADR-2191 in the host project root.
2. Add `VISIONCLAW_RELATION_WEIGHTS=/app/docs/relation-weights/uniform.toml` to the
   env file `launch.sh` loads (`.env.dev` if present, else `.env`).
3. Run `./scripts/launch.sh up dev --no-prompt`.
4. Check the log:
   `docker exec visionclaw_container sh -c 'grep -h "relation weights" /app/logs/rust*.log | tail -5'`.
   Expect `relation weights: bound /app/docs/relation-weights/uniform.toml (0 rows, provenance uniform ...)`
   and `relation weights applied to N CSR slots: 0 edges by IRI, 0 by edge type, M unchanged`.
   The layout should not change.
5. Point the variable at `/app/docs/relation-weights/hand-set-two-relations.toml`,
   then run `up dev` again. Expect `2 rows, provenance hand-set` and a non-zero
   `edges by IRI` count. As the layout settles, class hierarchies should draw
   tighter and related-to pairs should sit looser.
6. To switch it off, remove the variable and run `up dev`. The log then says
   `relation weights: off (VISIONCLAW_RELATION_WEIGHTS unset)`.

The prod image has no `docs/` mount, so a prod table needs a path baked into the
image or a mounted volume.
