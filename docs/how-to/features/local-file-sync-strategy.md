---
title: Local File Sync Strategy with Two-Pass Parser + Pod-First Saga
description: Two-pass knowledge graph parser + visibility classification + Pod-first graph-commit saga. Pass 1 extracts wikilinks and visibility. Pass 2 emits nodes + stubs. Pod write precedes graph commit with 60s pending retry.
category: how-to
tags:
  - tutorial
  - api
  - docker
  - database
  - backend
updated-date: 2026-04-19
difficulty-level: advanced
---


# Local File Sync Strategy with Two-Pass Parser + Pod-First Saga

> **ADR-2004 update**: The graph-commit phase writes to the embedded Oxigraph store, not
> Neo4j. "Pod-first / graph-second" naming below refers to the Oxigraph commit. There is
> no Neo4j container, Bolt URI, or `cypher-shell`; verification is via the REST API.

## Problem Solved

**Visibility Model**: Public vs private pages; private wikilink targets → stubs in graph  
**Crash Safety**: Pod succeeds but the graph commit fails → pending marker for 60s retry  
**Data Integrity**: Never emit graph node before Pod has content  
**New Approach**: Two-pass parser (commit 227f5b57a) + Pod-first saga (commit c939242f4)

## Architecture

### Two-Pass Parser (ADR-051)

**Pass 1**: Scan all files, classify visibility (line-anchored `public:: true`), extract wikilinks, build adjacency map.

**Pass 2**: For every page in the batch → emit `KGNodeDraft`. For every wikilink target NOT in batch → emit private stub. Every `WikilinkRef` edge stamped with `last_seen_run_id` UUID for orphan retraction.

```
File: knowledge/Foo.md (public:: true)
    ↓ Pass 1: Visibility::Public + wikilinks [[Bar]], [[Baz]]
    ↓ Pass 2: emit KGNodeDraft(Foo, visibility=Public)
             emit KGNodeDraft(Bar, visibility=Private, is_stub=true)
             emit KGNodeDraft(Baz, visibility=Private, is_stub=true)
             emit WikilinkRef(Foo→Bar, last_seen_run_id=UUID-X)
             emit WikilinkRef(Foo→Baz, last_seen_run_id=UUID-X)

File: knowledge/Private.md (no public:: true)
    ↓ Pass 1: Visibility::Private (not marked public)
    ↓ Pass 2: emit KGNodeDraft(Private, visibility=Private)
             (and wikilink edges if any)

Background orphan retraction job:
    scan WikilinkRef edges where last_seen_run_id != current UUID
    delete stale edges
    for each stub: if zero refs remain, delete stub node
```

### Visibility Classification (ADR-050 / ADR-051)

**Rule**: `classify_visibility(raw_content)` returns `Visibility::Public` **if and only if** the page has line-anchored `public:: true` in page properties block (before first `- ` or `#`).

**OWL Property Disambiguation**: `public-access:: true` inside an `### OntologyBlock` does **NOT** trigger public visibility. Reference commit b501942b1 for the regression that conflated them.

**Canonical IRI** (ADR-050): `visionclaw:owner:{npub}/kg/{sha256(relative_path)}` — rename-proof.

**Private Stubs**: No label, no content, canonical IRI + HMAC opaque_id generated at query-time (never at write-time).

### Pod-First / Graph-Second Saga (ADR-051; graph store = embedded Oxigraph, ADR-2004)

**Phase ordering ensures crash safety:**

```
1. Pod write phase
   For each KGNodeDraft:
     PUT {pod_base}/{owner}/[public|private]/kg/{slug} ← content
     if error: skip this node, no marker
   
2. Graph commit phase (embedded Oxigraph store)
   save_graph() with Pod-successful nodes only
   if error: write saga_pending=true marker on those nodes
   
3. Pending marker clearance
   for each committed node: clear saga_pending flag
```

**Recovery**: Background task `IngestSaga::resume_pending` wakes every 60s, finds nodes with `saga_pending=true`, retries their graph commit (idempotent — upsert by canonical IRI).

**Feature flag**: `POD_SAGA_ENABLED=true|false` — when false, legacy direct graph-write path used.

### Container Routing (ADR-052)

Pod URL construction depends on visibility:

```
Public node:  {pod_base}/{owner}/public/kg/{slug}
Private node: {pod_base}/{owner}/private/kg/{slug}
```

**Default Pod base**: `http://jss:3030` (in-cluster); override via `POD_BASE_URL` env var.

### WikilinkRef Edges + Orphan Retraction

Every wikilink edge carries `metadata["last_seen_run_id"]` from the ingest run UUID.

**Background job**:
```sql
MATCH (n:KGNode)-[r:WIKILINK_REF]->()
WHERE r.metadata.last_seen_run_id != $current_run_id
DELETE r

MATCH (stub:KGNode {is_stub: true})
WHERE NOT (stub)<-[WIKILINK_REF]-()
DELETE stub
```

This ensures stale references are pruned without manual intervention.

### GitHubSyncService Integration

`GitHubSyncService` now invokes the saga instead of direct graph writes:

```rust
let parser = KnowledgeGraphParser::new_with_owner(owner_pubkey);
let parse_output = parser.parse_bundle(files)?;

let saga = IngestSaga::new(pod_client, graph_repo); // graph_repo = OxigraphGraphRepository
let outcome = saga.execute_batch(parse_output)?;
// outcome: Complete | PendingRetry | Failed
```

## Feature Flags

| Flag | Default | Behavior |
|------|---------|----------|
| `POD_SAGA_ENABLED` | `false` | When true, Pod-first saga. When false, legacy direct graph writes. |
| `VISIBILITY_CLASSIFICATION` | `true` | When true, two-pass parser with visibility gating. When false, all pages treated as public. |
| `POD_DEFAULT_PRIVATE` | `false` | When true, pages without `public:: true` skip Pod write (content stays local). |
| `POD_BASE_URL` | `http://jss:3030` | Pod API base URL for ingest writes. |
| `FORCE_FULL_SYNC` | `false` | When true, bypass SHA1 delta filter, reprocess all files. |

## Implementation Details

### Two-Pass Parser Location

**File**: `src/services/parsers/knowledge_graph_parser.rs`

**Key types**:
- `FileBundle` — input file + metadata
- `KGNodeDraft` — node with visibility + is_stub flag + pod_url
- `ParseOutput` — nodes + edges + run_id UUID

**Entry point**: `parse_bundle(files: Vec<FileBundle>) -> ParseOutput`

### Visibility Module

**File**: `src/services/parsers/visibility.rs`

**Function**: `classify_visibility(raw: &str) -> Visibility`

Scans page-properties block (before first `- ` or `#`), returns `Visibility::Public` iff exactly `public:: true` found on a single line.

### Ingest Saga

**File**: `src/services/ingest_saga.rs`

**Types**:
- `SagaStep::PodWrite` — upload to Pod
- `SagaStep::GraphCommit` — commit to the embedded Oxigraph store
- `SagaOutcome` — Complete | PendingRetry | Failed

**Background task**: `spawn_resumption_task()` wakes every 60s, retries pending nodes.

### Pod Client

**File**: `src/services/pod_client.rs`

Routes requests to `{pod_base}/{owner}/[public|private]/kg/{slug}` based on node visibility.

## Testing & Verification

### Check Parser Output

```bash
# Emit parse output + visibility classification
cargo run --bin parse_files -- \
  --input /app/data/pages \
  --owner npub1... \
  --output /tmp/parse_output.json
```

### Check Saga Execution

```bash
# View pending markers via the admin/diagnostics API (graph store is embedded — no cypher-shell)
curl -s http://localhost:4000/api/graph/data | \
  jq '[.nodes[] | select(.metadata.saga_pending == true) | {iri: .canonical_iri, visibility: .visibility}]'

# Check Pod writes succeeded
curl -s http://jss:3030/{owner}/public/kg/Foo.md | head -20
```

### Monitor Orphan Retraction

```bash
# Count WikilinkRef edges by ingest run (embedded Oxigraph — query via REST, not cypher-shell)
curl -s http://localhost:4000/api/graph/data | \
  jq '[.edges[] | select(.edge_type == "WIKILINK_REF") | .metadata.last_seen_run_id]
      | group_by(.) | map({run_id: .[0], edge_count: length}) | sort_by(-.edge_count)'
```

## Configuration

### Environment Variables

```bash
# Two-pass parser flags
VISIBILITY_CLASSIFICATION=true
POD_SAGA_ENABLED=true
POD_DEFAULT_PRIVATE=false

# Pod ingest
POD_BASE_URL=http://jss:3030

# Sync options
FORCE_FULL_SYNC=false
SYNC_BATCH_SIZE=50

# GitHub credentials (for GitHubSyncService)
# GITHUB_REPO is the corpus repo NAME on GitHub (unchanged); its contents are an
# Obsidian vault — see docs/VAULT-corpus-format.md. GITHUB_BASE_PATH lists the
# vault roots; pages sit at each root.
GITHUB_OWNER=jjohare
GITHUB_REPO=visionGraph
GITHUB_BRANCH=main
GITHUB_BASE_PATH=knowledge,working
GITHUB_TOKEN=github_pat_...
```

## References

- **ADR-050**: Sovereign schema — canonical IRI, visibility, Pod URLs, opaque_id
- **ADR-051**: Ingest saga — Pod-first write ordering, pending markers, orphan retraction
- **ADR-052**: Container routes + public/private segregation
- **ADR-030-ext**: GitHub credentials in secure storage
- **Commit 227f5b57a**: Two-pass parser + visibility classification implementation
- **Commit c939242f4**: Pod-first saga implementation
- **Commit b501942b1**: Visibility classification regression fix (public-gating rule)

## Troubleshooting

### Pages not appearing in graph

1. Check visibility classification: does page have `public:: true` as line-anchored property?
2. Check parse output: `cargo run --bin parse_files -- --input /app/data/pages`
3. Check Pod write status: look for `saga_pending=true` in the graph store (via `/api/graph/data`)
4. Check pending retry: background task runs every 60s; wait or trigger manually

### Orphaned stubs not cleaned up

1. Verify orphan retraction job is running (background task spawned on startup)
2. Check stale `last_seen_run_id` values via the graph-store query above
3. Manually trigger cleanup: `curl -X POST http://localhost:4000/api/admin/orphan-retraction`

### Pod base URL not reachable

1. Verify `POD_BASE_URL` env var is set correctly
2. Check in-cluster DNS: `nslookup jss:3030`
3. Fallback to localhost for local testing: `POD_BASE_URL=http://localhost:3030`

---

**Status**: Implemented and tested (ADR-051 merged)  
**Next steps**: Monitor Pod write latency, tune resumption interval if needed, backport visibility classification to legacy single-file parser
