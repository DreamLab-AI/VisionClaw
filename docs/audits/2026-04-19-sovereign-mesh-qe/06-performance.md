# 06 — Performance Perspective

## HMAC opaque_id cost per node on query

`opaque_id` (`src/utils/opaque_id.rs:166-180`) is a single HMAC-SHA256: one `new_from_slice` call (~0 cost, it accepts any key length), two `update()` calls (pubkey + IRI, typically <200 bytes), one `finalize().into_bytes()`, 12-byte truncation + manual hex. On modern x86 with SHA-NI this is well under 1 µs per node — SHA-256 throughput is >1 GB/s and the message is <1 kB. For a 10k-node graph query that's ~10 ms of HMAC work, which is comparable to a single network round-trip. **Not a bottleneck.** No caching needed for v1; if projected to 100k+ nodes per query, consider caching by `(salt, iri)` tuple with a per-salt-day TTL.

**Observation P1 (INFO)**: the opaque_id is currently **computed at projection time** per ADR-050 and the parser's `KGNodeDraft.opaque_id = None`. This is correct (rotation requires re-derive) but means the projection layer must do the HMAC on every read. Consider lazy computation: only HMAC nodes that will actually be rendered as opaque (i.e. nodes where the caller is not the owner), not every row that comes out of Neo4j.

## Bit 29 encoding / decoding on hot paths

`encode_node_id`, `is_private_opaque`, `node_id_base` (`src/utils/binary_protocol.rs:42-55`) are all single-instruction bit ops. `encode_positions_v3_with_privacy` does a single `private_opaque_ids.map(|s| s.contains(node_id))` per node — a HashSet lookup, O(1) amortised. For a 60 Hz physics broadcast at 10k nodes, this adds ~10k HashSet lookups/frame ≈ tens of microseconds. **Well under frame budget.** The `sovereign_schema_enabled()` env check inside the loop (line 425) is slightly wasteful — hoist the check outside the loop into a single bool.

**Finding P2 (MINOR)**: the `contains(&get_actual_node_id(flagged_id))` double-check at line 453 performs a second HashSet lookup per node to handle both raw and type-flagged ids. This is defensive but doubles the lookup cost. If the caller is consistent about which id form it passes, one lookup suffices. Document the contract.

## Two-pass parser — is Pass 1 O(n²)?

`parse_bundle` (`src/services/parsers/knowledge_graph_parser.rs:152-252`) — Pass 1 walks each file once (O(n·f) where f is file length) to build `adjacency: HashMap<iri, PageMeta>` and `title_index: HashMap<title, iri>`. Pass 2 walks the adjacency map (O(k)) and for each page iterates `wikilinks` with a HashMap lookup (`adjacency.contains_key`, O(1)) + `emitted_iris.insert` (O(1)) + `edge_seen.insert` (O(1)). Total: **O(n·f + k·w)** where w is avg wikilinks per page. Linear, not quadratic. Good.

**Observation P3 (MINOR)**: one subtle cost — `nodes.iter().position(|p| p.node.id == node_id)` in the saga at `ingest_saga.rs:336` is O(plans.len()) and is called once per Pod-successful node, making that loop O(n²) in batch size. With a 100-node batch that's 10k iterations per cycle, negligible. With a 10k-node batch that's 10^8 iterations, noticeable. Replace with a pre-built `HashMap<u32, usize>` id→idx index.

## Saga resumption — 60s poll

`RESUMPTION_INTERVAL = 60s` and `RESUMPTION_BATCH_LIMIT = 200` nodes per tick. For a sustained Neo4j outage recovering at 1 second per 200-node batch, this is throughput-bounded at 200 nodes / 60 s = ~3.3 nodes/s — too slow if the pending queue is in the thousands. **Finding P4 (MEDIUM)**: consider adaptive interval: when `pending_nodes` gauge is non-zero, tick every 5 s; when zero, back off to 60 s. Current behaviour risks a recovery storm after a Neo4j bounce.

## JSON Schema validation (ADR-054, future)

Not yet implemented. When it lands, `jsonschema` crate compilation is the hot cost — compiling a JSON Schema document into a validator tree is tens of milliseconds. The ADR specifies 1 h cache with ETag refresh — adequate. Make sure the cache key is per-(Pod URL, ETag) not per-request to avoid re-compile churn under load.

## Prometheus cardinality

Already covered in Privacy §5. 24 metrics with bounded label sets (3×Saga outcome + 3×Pod container + 4×Nostr kind + unlabelled counters/gauges/histograms). Total active series ~25-30. Scrape-safe at every interval. Histogram bucket counts are modest (14 for saga duration, 10 for bridge confidence). No change needed.

## solid-pod-rs WAC evaluation cost

`evaluate_access` (`crates/solid-pod-rs/src/wac.rs:158-188`) is a linear scan over the `AclDocument.graph` list. For a typical Pod with <10 authorizations per `.acl`, this is <1 µs. The cost dominant component is `find_effective_acl` which walks the path tree and calls `storage.get(acl_key)` once per segment — **O(depth of resource path)** Pod I/Os per WAC decision. For a deep resource like `./public/kg/2024/archive/page-123.ttl` that's 5 storage gets, all cache misses on first access. Add an ACL-resolution cache keyed on the closest ancestor ACL path, invalidated on ACL writes.

**Finding P5 (MEDIUM)**: no ACL cache observed in `solid-pod-rs`. Production PUT/GET latency will be dominated by this until cached.

## Summary

| Area | Assessment |
|---|---|
| HMAC per node | cheap (~1 µs), acceptable at 10k-node scale |
| Bit 29 encode | single HashSet lookup per node, negligible |
| Two-pass parser | O(n·f + k·w), linear |
| Saga resumption loop | adequate baseline, adaptive poll recommended (P4) |
| Prometheus | bounded cardinality, safe |
| WAC evaluation | per-segment storage gets; cache recommended (P5) |

No hard performance bottlenecks. Two optimisation opportunities (P4 adaptive poll, P5 WAC cache) are backlog items. Frame-budget on the binary broadcast path is comfortable.
