---
id: ADR-2106
title: The published ontology is pulled into the embedded pod, not pushed from CI
date: 2026-09-06
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 4d1a698e70f60c19d8c83fa8c6caef193866978e
verified_paths: [.github/workflows/ontology-publish.yml, src/services/ontology_pull.rs, src/main.rs, scripts/ontology/pack-pod-resources.py, client/src/features/ontology/services/jss/contextLoader.ts, client/src/features/ontology/services/jss/schemaParser.ts, env.example]
owner: jjohare
review_trigger: A pod that becomes reachable from CI (self-hosted runner or public endpoint); a change to the /public/ontology/ resource set; the release channel moving off GitHub (e.g. to the Loom or narrativegoldmine.com).
repo: visionclaw
domain: BASELINE-architecture
lineage: completes ADR-2098 (SOLID_POD_URL names the in-process endpoint); relates to ADR-032 M3 (embedded solid-pod-rs), ADR-2040 (vault format), ADR-2005 / ADR-2006 in VisionFlow (substrate-sourced, canon owns the cross-repo view); diagram docs/diagrams/estate/09-build-deploy-and-ci-estate.md ES-09.13
---

# ADR-2106 — The published ontology is pulled into the embedded pod, not pushed from CI

## Context

`ontology-publish.yml` builds the pod resources for `/public/ontology/`
(`visionflow.ttl`, `context.jsonld`, `ontology.jsonld`, `index.jsonld`) and
its `deploy-jss` job PUTs them to `SOLID_POD_URL`. Since ADR-032 M3 the pod is
served in-process by this server under `/solid` (ADR-2098), on a LAN host. A
GitHub-hosted runner cannot reach it; no self-hosted runner is registered for
the organisation or the repository; `vars.SOLID_POD_URL` is unset. The job
had never succeeded.

On 2026-09-06 the workflow was made to run end to end for the first time
(`ONTOLOGY_SOURCE_TOKEN`, archived-source default, pip-cache failure) and the
converter it carried was found to be wrong for the ADR-2040 vault: an inline
`md_to_ttl.py` read Logseq `key:: value` lines and never the `json-ld` fence,
shipping 0 `owl:Class` from 380 pages under an `example.org` vocabulary with
valid syntax and matching digests (run 34045488066). It was replaced by the
vault's own `python -m pipeline.build` and `scripts/ontology/pack-pod-resources.py`
(8,434 classes, 265,455 triples, substance floor 4000 / 100k; run 34046473943).

That left one question: how the resources reach a pod no runner can see.

Three options were weighed. A LAN self-hosted runner puts a GitHub-controlled
agent on the trusted server segment for a public repository. A public pod URL
exposes a write surface that only CI needs. A pull from the server keeps the
LAN inbound-closed and matches how this server already ingests the vault
(`GitHubSyncService` pulls markdown from GitHub at runtime into Oxigraph).

## Decision

**Delivery is inverted.** The workflow attaches the resources plus a
`SHA256SUMS` to a rolling GitHub release `ontology-latest` on this public
repository (`publish-release` job; tag moved to the publishing commit, assets
replaced). The server, on `solid-pod-embed`, spawns
`services::ontology_pull::spawn_boot_pull` right after the pod state is
initialised (`src/main.rs`): it fetches `index.jsonld`, compares
`visionflow:buildSha` with the manifest already in the pod, and only if they
differ fetches `SHA256SUMS` and every content file, verifies each digest, then
writes through the `Storage` trait: containers, a public-read WAC ACL at
`/public/ontology/.acl` (an observed existing ACL is preserved; an existence-probe error is currently treated as absence), the four content files, and the manifest last. It repeats every
`ONTOLOGY_PULL_INTERVAL_SECS` (default one hour, one small GET when nothing
changed). Fetch and digest failures occur before resource writes. Storage failures are logged, but publication is sequential and can leave a partially updated generation; manifest-last ordering is a retry signal, not an atomic commit. The ACL absence check currently treats a storage error as absence.

`deploy-jss` remains as the push path for a deployment whose pod a runner can
reach, gated on `vars.SOLID_POD_URL`. `deploy-target-missing` states in the
run view that the pull model is in effect.

The client's JSS-era contract of one negotiated URL is corrected at the same
time: `schemaParser.ts` fetches `/public/ontology/ontology.jsonld` and
`/public/ontology/visionflow.ttl` (`getOntologyJsonLdUrl`,
`getOntologyTurtleUrl`), because on the embedded pod a GET of the bare
container path is a container listing, not the ontology.

Configuration: `ONTOLOGY_PULL_URL` (default the release download base),
`ONTOLOGY_PULL_ENABLED`, `ONTOLOGY_PULL_TIMEOUT_SECS`,
`ONTOLOGY_PULL_INTERVAL_SECS` (0 = boot only). Documented in `env.example`.

## Consequences

- After a successful complete pull, the pod holds the selected release generation, identified
  by the vault source SHA and the workflow build SHA in `index.jsonld`. Nothing
  on the LAN accepts inbound traffic for it and no token is involved anywhere
  in the read path.
- A container that runs for weeks converges on the current release within the
  interval; a container that boots offline serves what it last held and says so
  in the log.
- Integrity is transfer integrity: `SHA256SUMS` comes from the same origin as
  the files, so it defends against truncation and corruption, not against a
  compromised origin. A signed manifest (the server already carries Nostr keys;
  CI could sign with the organisation key and the puller verify Schnorr) is the
  full-strength version and is deliberately not in this record.
- The release tag is mutable by design. Anyone who wants an immutable
  generation pins `ONTOLOGY_PULL_URL` at a fixed release or at the Loom.
- `notify-websocket` cannot fire for a pulled update; the pod's own LDP
  notifications do not fire either, because the puller writes through
  `Storage`, not the HTTP handler. Clients re-read on their cache TTL.
- The `./` semantics in the pre-existing per-pod root ACL (`build_pod_root_acl`)
  normalise to `/`; this record's ACL uses absolute paths so its grant is
  exactly the `/public/ontology/` subtree.

## Verification

`cargo test --no-default-features --features solid-pod-embed --lib services::ontology_pull`
exercises the sequence against `MemoryBackend` and a map-backed `Fetch`: first
pull writes all resources with the ACL and the manifest last; an unchanged
`buildSha` fetches only the manifest; a new build replaces content and keeps an
operator's ACL; a digest mismatch, a missing `SHA256SUMS` entry and an
unreachable release write nothing; `ONTOLOGY_PULL_ENABLED=off` short-circuits.
`ontology-publish.yml` run on the publishing commit: `publish-release` diffs
the public `SHA256SUMS` against the one it uploaded. Live: the server log line
`ontology pull: /public/ontology/ updated to build <sha>` and
`GET /solid/public/ontology/index.jsonld` returning that `visionflow:buildSha`.

## Estate audit — 2026-09-07

The pull direction and boot/interval wiring are source-supported. The earlier blanket failure guarantee was not: `src/services/ontology_pull.rs:345-373` mutates containers, ACL and content sequentially, and `exists(&acl_path).await.unwrap_or(false)` treats a failed ACL probe as absence. A later write error does not roll back earlier writes. The existing operator ACL is preserved only when its existence is successfully observed. The implementation axis is therefore partial for the full contract; the recorded live activation of pull delivery is retained as historical evidence, not re-observed here.

Closeout: publish to an immutable generation and switch an authoritative pointer atomically, or implement a proven equivalent transaction/rollback boundary; abort on an indeterminate ACL check. Inject failure at every write and ACL probe, verify prior-generation readability and operator ACL preservation, restart, and demonstrate convergence without mixed resources. Bind the result to the tested storage backend and reader path. CP-02/04/08; owner remains the maintainer named above. See the [source audit](../../../VisionFlow/docs/estate-review/2026-09-07-visionclaw-audit.md), VC-A01/A02.
