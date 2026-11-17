# Jev and gateway rebuild — 2026-11-17

## Source and build

- Agentbox main `100d44cad89226a4c3749d96457a52df0b399746`, including the
  already-committed Jev batching and gateway PID fixes. Main was clean and stable
  before cutover; the independent embedding workload was left running.
- Candidate image
  `sha256:17a8c2e9f80e74af708b831279ad2a2c18919f99912d61b0257990a0689a72bb`.
  Nix: 398.057 s; registry push: 27.134 s; pull: 178.131 s; offline smoke:
  0.477 s. Only services and application layers changed (3.599 GB total);
  platform, toolchains and CLI layers were reused. Nix's client affinity
  selected one job with sixteen cores, not two jobs with eight cores.
- System One image
  `sha256:9f5357b6ea0645c020461a48b4093dada9b0f83b83afbc4036542684c9a81b9c`.
  Its named Rust context previously copied a 2.9 GB workspace `target/`.
  Explicit source-only COPY statements reduced transfer to about 60 kB.
  Cargo builds are locked and use at most twelve jobs. No target cache was
  deleted. Facade tests: 31 unit, 22 integration, nine documentation tests.
- Gateway PID/thread identification and build-context regression tests:
  seven passed. All 161 Agentbox ADRs validated before preparation.

## System One and email sidecars

- System One and OpenJev were recreated together because the latter shares
  the former's network namespace. Both became healthy on CUDA. Model volumes
  and mount access modes were compared before/after and matched exactly.
- A real synthetic 100-question request returned all 100 answers in 0.563 s;
  a 1,025-question request was refused by the facade's own ceiling.
  The implementation splits engine batches at 64 and sends them sequentially.
- Email's host checkout lacked four remote Jev/embedding-pin commits. They
  were merged with the corrected runtime guide and pushed to its existing
  private repository (`f5d31f9`). No raw mail, identities or credentials were
  printed by the checks or committed.
- Email's requirements file matched the running image byte-for-byte. The
  application-only image reused dependency image
  `sha256:eca494c6d060fce42eca2f139813a33e1d9118e5b9fcc9ea4ba1597c204bf207`
  and baked the current app, entrypoint, health check and Bridge script.
  Result:
  `sha256:e5bb2ab66255acb8ef2376d8b232442dbbb73e2a95515cf2c8ec0671a229e80a`.
  CUDA, llama-cpp and Proton Bridge were not recompiled. The canonical full
  Dockerfile remains the path when dependencies change; this refresh does not
  claim a new dependency/base security upgrade.
- All mail/index/Bridge/policy/model mounts and the reference salt,
  authentication settings and reasoner route were checked unchanged before
  recreation. Email's Jev route is explicitly local at
  `http://systemone:8097/v1/systemone`; answer abstention remains unset.
- Twenty-one email Jev tests passed in an isolated container. A live synthetic
  ranking call passed. The private MCP smoke passed both before and after
  recreation: index, bearer denial, 1,024-dimensional embeddings, two read-only
  mailbox logins, sanitized query pipeline and unauthorized raw-access denial.
  The reasoner guide now names the observed runtime route rather than the
  retired `.48` host; dotenv files were not opened or edited.

## Agentbox cutover and live checks

- Activation completed at **2026-11-17 22:14:00.781 UTC**. The active container
  image and persistent mounts matched the prepared receipt before and after
  replacement. This is a new cutover, not the previous day's deployment.
- The same synthetic 100-question request passed through Agentbox's local
  vendor proxy in 0.214 s. Backend metadata reported `laya`, local and
  `laya-typed-decisions`. The Nostr gateway stayed RUNNING after restart.
- Supervised services settled. RuVector MCP health, actual Gemini Hydrogen
  inference, both synthetic terminal frame modes, CUDA, NVENC and the QGIS
  sidecar ping passed. Automatic Codex audit remains off and 125 skills are
  present. No local QtWebEngine/WebKit/QGIS dependency returned.
- Core `/ready` is true with the single documented signing warning below;
  no elevation publications had been skipped or failed. Other sidechain
  startup warnings cleared. It is not an unqualified all-green readiness claim.
- About 306 GiB remained available on the shared NVMe after deployment.

## Retained limitations

The new elevation-publisher status honestly reports `no-signing-stack` on
`/ready`. ADR-2066 leaves that provisioning owner-parked; the rebuild did not
enable it, provision keys or suppress its warning. Core readiness is distinct
from a completely empty degraded list. The post-cutover probe permits only
this exact documented warning with zero skipped/failed proposals; all other
warnings and supervisor faults remain failures.

The gateway's reused-PID fix is not an atomic stale-lock takeover fix. Two
simultaneous takers can still race; that pre-existing issue remains open.
Automatic Codex audits remain disabled pending process turn/spend confinement.
No data volumes, Nix outputs, Cargo caches or recovery images were pruned.
