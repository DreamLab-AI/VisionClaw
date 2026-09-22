---
title: Corpus ingest from the local vault
description: How VisionClaw ingests the sovereign corpus from the mounted vault volume, and how to switch back to GitHub
category: how-to
diataxis: how-to
tags:
  - operations
  - corpus
  - ingest
updated-date: 2026-09-22
---

# Corpus ingest from the local vault

VisionClaw ingests the knowledge corpus through the `CorpusSource` port
(ADR-2114). The default source is the **local vault** mounted into the
container; GitHub is a second implementation, off by default (ADR-2115).

## How the mount works

The vault lives in the agent workspace named volume
`multi-agent-docker_workspace`, which holds `visionGraph/` at its root. The
dev service in `docker-compose.unified.yml` declares that volume as
`external: true` and mounts it **read-only**:

```yaml
volumes:
  agent-workspace:
    name: multi-agent-docker_workspace
    external: true

services:
  visionclaw:
    volumes:
      - agent-workspace:/vault:ro
    environment:
      CORPUS_SOURCE: "local"
      VAULT_ROOT: "/vault/visionGraph"
      VAULT_BASE_PATHS: "knowledge/pages,working/pages"
```

Ingest then walks `${VAULT_ROOT}/knowledge/pages` and
`${VAULT_ROOT}/working/pages` — the same dual-source split `GITHUB_BASE_PATHS`
used to express. `.obsidian/`, `.trash/`, `_misc`, `journals/`, `bak/`, dot-files
and anything that is not `.md` are skipped at listing time. The incremental
filter keys on `mtime:size` per page instead of a blob SHA; an untouched vault
re-syncs nothing.

Read-only is deliberate: the ingest path never writes to the corpus. Corpus
writes go through the `vault` CLI, not through the server.

## The host-path caveat

`multi-agent-docker_workspace` is a **named volume**, not a bind mount, precisely
because bind paths resolve on the *host*, not inside the agent container. A
`launch.sh` run started from inside a container would resolve
`/home/devuser/workspace/visionGraph` against the host filesystem and silently
mount the wrong tree (or nothing). Always bring the stack up from the host
shell, and never replace the named volume with a host path in this compose file.

If the volume does not exist yet (fresh machine, agent stack never started):

```bash
docker volume create multi-agent-docker_workspace
```

Docker refuses to start the service rather than creating an empty external
volume, so a missing volume is a loud failure, not a silent empty corpus.

## Verifying

```bash
docker compose exec visionclaw ls /vault/visionGraph/knowledge/pages | head
docker compose logs visionclaw | grep 'Corpus source:'
# → Corpus source: local corpus at /vault/visionGraph (sources: knowledge/pages, working/pages)
```

`GET /api/ontology/classes` after the first sync reports the ingested class
count.

## Switching back to GitHub

Set the source explicitly and supply credentials:

```bash
CORPUS_SOURCE=github
PRIVATE_REPO_GITHUB_PAT=<token>
GITHUB_OWNER=<owner>
GITHUB_REPO=<repo>
GITHUB_BASE_PATHS=knowledge/pages,working/pages
```

With `CORPUS_SOURCE=local` (or unset with `VAULT_ROOT` present) none of the
GitHub variables are read, and the server boots without them.

## Changing source forces a full re-sync

The sync database records the source identity (`kind:location:base_paths`) under
`corpus_source_identity`. When it changes, stale data is cleared and the next
sync is a full one — a store built from GitHub is never topped up incrementally
from the vault, or vice versa.

A store built **before** ADR-2114 carries no identity record, so the first local
sync after the upgrade looks like a first run. Force the rebuild once:

```bash
FORCE_FULL_SYNC=1 docker compose up -d visionclaw
# or, without a restart:
curl -X POST 'http://localhost:4000/api/admin/sync?force_full=true'
```

## The CLI

`sync_corpus` (formerly `sync_github`) runs one sync against the configured
source and prints the statistics:

```bash
cargo run --bin sync_corpus
```

`load_ontology` reads the same source for a one-shot OntologyBlock load; it takes
an optional vault-root argument that overrides `VAULT_ROOT`.
