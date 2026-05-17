# 02 — Privacy Perspective

How much private data actually stays private?

## Caller-aware Neo4j filter

The filter lives at the Rust handler layer, not Cypher: `visibility_allows` in `src/handlers/api_handler/graph/mod.rs:115-134`. Legacy rows missing `visibility` default to `"public"` — matching the Cypher `COALESCE(n.visibility, 'public')` form. A `visibility` value that is neither `"public"` nor `"private"` is default-deny (line 126-131), so future enum extensions fail safe. Anonymous callers (empty pubkey normalised to `None` at line 147) see only `visibility=public`. Signed callers see `visibility=public ∪ (visibility=private WHERE owner_pubkey == caller)`. **Finding P1 (HIGH)**: the filter drops private nodes it cannot reveal rather than opacifying them. Compare with ADR-028-ext §"three-tier read boundary" and the `is_opaque_to` method in `src/models/node.rs:423-432` — both describe an opacified-stub semantic where other users' private nodes appear as bit-29-flagged placeholders. Today the JSON `/api/graph/data` handler returns only the nodes the caller can fully see; there is no stub emission. `is_opaque_to` is defined but never called. This is a deviation from the ADR contract and removes a topology-preserving property of the sovereign model.

## Binary V5 protocol string-freeness

`encode_positions_v3_with_privacy` (`src/utils/binary_protocol.rs:389-516`) correctly ORs `PRIVATE_OPAQUE_FLAG` (bit 29) into the wire id when a node id is in `private_opaque_ids`. Wire format is 48 bytes per node: id + pos + vel + sssp + analytics. **No string fields ever touch the wire.** Labels and metadata live in the JSON `/api/graph/data` response, not the binary broadcast. On that axis, V5 is string-free for opacified nodes by construction. **Finding P2 (CRITICAL for intent)**: `encode_positions_v3_with_privacy` is defined but **never called with a populated `private_opaque_ids` set**. Every call site (`encode_node_data_extended_with_sssp`, `encode_node_data_with_live_analytics`, the WebSocket broadcast path in `client_coordinator_actor.rs`) passes `None`. The bit-29 flag path therefore never fires on production broadcasts — the "clients render opacified shape without receiving the real label" guarantee from ADR-050 is currently only enforced by the absence-of-label-in-binary invariant, not by an explicit opacity signal. Given the binary format never carried labels to begin with, this is *sufficient for privacy* but does not deliver the authoring intent (the client cannot distinguish "this private node is opaque to you" from "this node has no metadata yet"). Wire the request-context caller pubkey into the broadcast pipeline.

## Private stubs and wikilink edges

`KnowledgeGraphParser::parse_bundle` emits a `KGNodeDraft` stub for every wikilink target not in the ingest batch (`src/services/parsers/knowledge_graph_parser.rs:219-222`). Stubs have empty `label`, empty `metadata_id`, `visibility=private`, `owner_pubkey=owner`, no `pod_url`. **Finding P3 (MEDIUM)**: `metadata.insert("stub_source_wikilink", wikilink_text)` at line 345 preserves the **original authored text of the wikilink** on the stub. If an anonymous API surface ever returns this metadata (e.g. the generic `/api/graph/data` metadata field), it leaks the private page's presumed title even though `label` is empty. `visibility_allows` today drops the stub for anonymous callers — so the leak does not reach the wire — but the field is still in Neo4j and would leak if a future handler bypasses the filter. Rename this metadata key to something unambiguously internal (`_internal_wikilink_hint`) or strip it at read time.

When an anonymous caller requests a public page that wikilinks to a private stub: the stub is dropped from the node set (`visibility_allows` → false), then edges are dropped because their target is no longer in `filtered_node_ids` (`src/handlers/api_handler/graph/mod.rs:238-248`). Edge dropping is symmetric: the anonymous caller never learns the stub exists. This is correct.

## Pod ACL default

`render_owner_only_acl` emits owner-only (`acl:agent`, `acl:Read/Write/Control`) with **zero `foaf:Agent` grants**. The root ACL provisioning (`src/handlers/solid_proxy_handler.rs:692-696`), `./private/.acl` (line 710-711), `./shared/.acl` (line 737-738) all use the owner-only template. Only `./public/.acl` (line 724-727, `render_public_container_acl`) and `./profile/.acl` (`render_profile_container_acl` — public read on `card` only) grant `foaf:Agent`. Migration matches: `acl_is_sovereign` rejects anything containing `<#publicread>` or `agentclass foaf:agent` at the root (`src/handlers/solid_proxy_migration.rs:52-58`).

## `corpus.jsonl` (ADR-054)

**Finding P4 (INFO)**: `corpus.jsonl` generation is **not yet implemented** in `src/services/ingest_saga.rs` or `src/services/pod_client.rs`. No grep hit for `corpus.jsonl`. ADR-054 compliance criterion "publish saga writes `./public/kg/corpus.jsonl`" is unmet. This is a scope gap, not a privacy bug — no leak occurs because no artefact exists. Flag for the ADR-054 rollout milestone.

## Prometheus label cardinality (`src/services/metrics.rs`)

24+ metrics, all label sets are enumerated (`SagaOutcomeLabel::{Complete|Pending|Failed}`, `PodContainer::{Public|Private|Config}`, `NostrKind::{K14|K30023|K30100|K30200|K30300|K30301|K31400|K31402}`). **No pubkey, IRI, or path label anywhere.** Cardinality is bounded at roughly 20-35 distinct time series across the full registry. Scrape body stays small; no secret-adjacent values ever appear as labels. Compliance is good.

## Verdict

P2 (binary opacity signal never fires) is the headline privacy observation. Not a leak — the binary format is string-free by construction — but the stated intent of ADR-050 (bit 29 as "authoritative opacity signal") is not yet wire-true. P1 (opacified-stub emission missing from the JSON API) is the second-order observation: graph topology is not preserved across the trust boundary. Both are fixable by plumbing the request-context caller pubkey into the broadcast and JSON response paths.
