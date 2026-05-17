# ADR-030-ext: GitHub Credentials in Pod — Sovereign Per-User Auth

## Status

Ratified

## Date

2026-04-19

## Related Documents

- ADR-030 — Agent memory in Solid Pods (base ADR this extends)
- ADR-050 — Pod-backed `:KGNode` schema (sovereign-private-node model)
- ADR-051 — Visibility transitions (publish/unpublish saga)
- ADR-028-ext — NIP-98 auth extension (used to read per-user Pod creds)
- ADR-052 — WAC default-private container policy (governs `./private/config/`)
- ADR-053 — solid-pod-rs sidecar

## Context

Current VisionClaw ingest uses a single shared GitHub token in the server's
`.env` file. Source of truth today:

```
GITHUB_OWNER=jjohare
GITHUB_REPO=logseq
GITHUB_BRANCH=main
GITHUB_BASE_PATH=mainKnowledgeGraph/pages      # plus workingGraph/pages
```

This works for a single-user deployment but fails the multi-tenant sovereign
model introduced in Wave 2:

- Each user's private graph lives in their own GitHub repo, under their own
  account. A shared server token cannot reach user A's repo and user B's repo
  without a sprawl of server-held tokens.
- Credentials must be **sovereign** (user-owned, stored in the user's Pod)
  rather than server-owned. The user grants the backend time-bounded read
  access; the user can revoke it by rotating the token in their Pod.
- The power user (jjohare) must be bootstrappable from the existing `.env`
  values so the migration does not strand the operator.

ADR-030 established Pods as the sovereign store for agent memory. This ADR
extends that pattern to GitHub credentials.

## Decision

Store per-user GitHub credentials in the owner's Pod at `./private/config/github`
as a JSON document with the following schema:

```json
{
  "owner": "jjohare",
  "repo": "logseq",
  "branch": "main",
  "base_paths": [
    "mainKnowledgeGraph/pages",
    "workingGraph/pages"
  ],
  "token": "ghp_xxx...",
  "token_storage": "plain"
}
```

- `owner`, `repo`, `branch` mirror the current `.env` fields.
- `base_paths` is a **list** (replaces the single `GITHUB_BASE_PATH` env var) so
  a user can source from multiple roots inside one repo, matching jjohare's
  `mainKnowledgeGraph/pages` + `workingGraph/pages` setup.
- `token` is a GitHub PAT.
- `token_storage` is a versioning field. `"plain"` is the v1 value. `"nip44"`
  is reserved for v1.5 when we add NIP-44 at-rest encryption.

The schema is registered at `docs/schemas/pod-github-config.json` for
validation at both write time (admin CLI) and read time (backend ingest).

### Bootstrap for the power user

An admin CLI command performs a one-shot, auditable, idempotent migration from
the existing `.env` to the Pod:

```
claude-flow vc bootstrap-power-user --env .env
```

The command:

1. Reads `GITHUB_OWNER`, `GITHUB_REPO`, `GITHUB_BRANCH`, and
   `GITHUB_BASE_PATH` (splitting on commas for the list form).
2. Reads `GITHUB_TOKEN`.
3. Writes the JSON document to `./private/config/github` in the operator's
   Pod via an authenticated-as-owner `PATCH`.
4. Emits a kind-30301 audit event (distinct from the kind-30300 visibility
   events in ADR-051) recording the bootstrap.
5. Exits non-zero if the Pod resource already exists and its contents differ;
   idempotent when contents match.

### Access model

- **WAC**: `./private/config/` is owner-only by default (per ADR-052). No
  other user and no anonymous read can touch the file.
- **Backend read**: ingest reads the creds via **authenticated-as-owner** Pod
  access. On the first ingest request for a user, the user signs a NIP-98
  authentication token (ADR-028-ext); the backend uses that token to fetch
  the creds file just like any other owner-authenticated Pod request.
- **Token lifetime in memory**: the backend holds the PAT in memory only for
  the duration of the ingest run; it is never written to server disk.

### v1 token storage

Plain text, inside the owner-ACL container. Sufficient for v1 because the Pod
ACL closes external access and the backend handles the token only in memory.

Defence-in-depth encryption is deferred to v1.5 and v2:

- **v1.5**: NIP-44 encryption of the `token` field. `token_storage` flips to
  `"nip44"`; backend decrypts using the user's ephemeral session key.
- **v2**: replace the PAT with a GitHub App OAuth flow. Removes the long-lived
  token entirely; token_storage becomes irrelevant.

### Feature flag

`GITHUB_CREDS_IN_POD=true|false`:

- `true` (new path): ingest reads per-user creds from the user's Pod. A user
  without creds in their Pod cannot ingest.
- `false` (legacy path): ingest falls back to the shared server `.env` token.
  Used during migration and for single-user deployments.

## Consequences

### Positive

- **Sovereign**: GitHub auth is user-owned, not server-owned. Users rotate
  their own tokens; operators do not hold long-lived user credentials.
- **Multi-tenant ingest**: each user's graph is pulled from their own repo.
  The server reaches user A's repo and user B's repo using each user's own
  PAT, with no shared token.
- **No shared server-side token sprawl**: the operator does not accumulate a
  growing set of PATs for each onboarded user.
- **Auditable bootstrap**: the kind-30301 audit event records who bootstrapped
  whom, when.

### Negative

- **PAT leakage risk if Pod ACL is misconfigured**. Mitigated by ADR-052
  (default-private container policy) and integration tests that assert a
  different user cannot read the creds file.
- **Token rotation requires user action**. A stale PAT blocks ingest until
  the user updates it in their Pod. Resolved by the v2 GitHub App OAuth path.
- **First-run latency**: the initial NIP-98 signing handshake adds one
  round-trip before the first ingest can fetch creds.

### Neutral

- No change to the ingest pipeline's downstream behaviour; only the credential
  source changes.
- `base_paths` generalisation is backwards-compatible with the single-path
  `.env` form via the bootstrap CLI's comma-split.

## Compliance criteria

- [ ] JSON schema defined at `docs/schemas/pod-github-config.json`
- [ ] `claude-flow vc bootstrap-power-user --env .env` CLI reads `.env` and writes the Pod resource
- [ ] Backend ingest reads Pod creds via authenticated-as-owner NIP-98 (no shared server-token code path when flag is `true`)
- [ ] Pod ACL enforcement test: a second user cannot read the first user's creds file
- [ ] `GITHUB_CREDS_IN_POD=false` preserves existing shared-token behaviour
- [ ] kind-30301 audit event emitted on bootstrap
- [ ] Backend holds the PAT in memory only (never written to server disk)

## Rollback

- Set `GITHUB_CREDS_IN_POD=false`. Ingest falls back to the `.env`-supplied
  token; the Pod resource remains untouched and can be deleted at leisure
  from the operator's Pod.
- The bootstrap CLI is idempotent and non-destructive; no cleanup required on
  the Pod side.

## References

- `docs/schemas/pod-github-config.json` — schema definition
- `src/cli/vc_bootstrap.rs` — bootstrap CLI implementation
- `src/ingest/github_creds.rs` — Pod-backed cred reader
- `src/audit/nostr_events.rs` — kind-30301 bootstrap event
