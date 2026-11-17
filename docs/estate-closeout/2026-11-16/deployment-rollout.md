# Selected estate rollout — 2026-11-16

This records executed deployment checks, not a general security qualification.
Unrelated authority/governance, Ontomark and other WIP branches remain parked.

## Agentbox

- Selected journal/review work merged and pushed to main at `c0d8382a2`.
- Image: `sha256:7652f551ce73a3e72895af8f738bd0a8103a3809f41fdbf26b127a45d1dd7fb4`.
- Preparation: Nix 384.809 s; push 25.132 s; pull 173.074 s. Changed layers
  total 3.599 GB; platform, toolchains and agent-CLI layers were reused.
  The affinity-limited client selected one job with sixteen cores (16 total
  build slots). The earlier two-jobs/eight-cores description was incorrect;
  `nixBuildBudget(28)` prefers the larger per-job allocation on equal totals.
- Candidate isolated smoke and actual shipped-path inventory passed: no local
  QtWebEngine, WebKitGTK, QGIS, diff-pdf or OrcaSlicer was brought back.
- Readiness settled without degraded services. Persistent mount identity was
  checked before and after activation. RuVector MCP health, actual Gemini
  Hydrogen inference, synthetic true-colour terminal frames, CUDA, NVENC and
  the sidecar QGIS ping passed. All 125 skills remain present.
- Automatic Codex review remains explicitly disabled (`audit_monthly_usd=0`),
  including zero-estimate requests, until process turn/spend confinement is
  enforced. The review skill's off-switch regression tests were extended; its
  provider transport is available, not automatically enabled.
- The two declared journal boundaries matched their exact original bytes before
  installation. The baked verifier checked 8,575 records and reported
  `verified_with_boundaries`, with one gap and one direct resumption. Original
  historical bytes were not rewritten. Cross-process journal locking remains
  an open item; this is not a claim of uninterrupted historical continuity.
- Gates: journal tests 43; review tests 83; cron 21; delivery 24; supervisor
  isolation 25; instruction layers 12. Skill lint: 125 clean, nine existing
  warnings. All 161 Agentbox ADR records validated.

## VisionClaw and Codescape

- Local ontology feed and backup/checkpoint branches were combined on main.
  The checkpoint task does not change profile flags or ontology-pull authority;
  the new mounts do not change ingress, memory or typed-decision contracts.
  ADR-2008/2119 also cover the tested recovery-preserving launcher fix.
- Deployment used cached `scripts/launch.sh up dev --no-prompt`, not its
  cache-deleting `rebuild` route. Existing Cargo caches were retained. Runtime
  CPU quotas were capped at 24 CPUs for VisionClaw and 12 for Codescape.
- The old dev Solid root held 66 MB outside a volume. With its backend stopped,
  it was archived independently and copied into `visionclaw-solid:/data/solid`.
  The sorted file-content digest matched before and after copying:
  `25650d13dc91c0bd520e9d9d2168763feea3d89b29278b988505f43cf6bd7a31`.
  The archive is retained privately on the host and NAS.
- The legacy launcher removed the renamed recovery container through its
  project-wide orphan cleanup. Data volumes, the Solid archive and the verified
  store backup survived. The launcher now reconciles only known conflicts;
  a regression test proves a renamed recovery container is not targeted.
- Focused checkpoint/pull/reconciliation tests: 25 passed. Backup unit,
  integration and documentation tests: 20 passed. Launcher tests: three passed.
- New containers expose cloud-judge flags as `0`. The local file release was
  hash-checked before boot and the live pull reported
  `local-a5b754af4445-vault0.1.0-2cbfd79b`: 9,595 classes, 352,196 triples.
  Codescape health reports the CUDA engine; VisionClaw HTTP health succeeded
  and actual GPU kernel launches were validated.

## Real backup and restore drill

- First run: NAS `visionclaw-backups/20261116T210719Z` (private directory).
  The backend was stopped; only this cold backup opened the live store with
  its exclusive writer lock. Nightly backups use server checkpoints instead.
- All five SQLite members were present (including optional liveness). Oxigraph
  N-Quads: 333,418,304 bytes, 2,093,326 quads. Manifest hashes verified.
- Restore into a new empty scratch directory succeeded in **19.512 s** using
  eight CPU quota and 16 GiB memory. The tool checked quad counts and SQLite
  integrity, row counts and content digests. This measures restore execution,
  not incident response, deployment or network recovery time.
- The restored store occupied 588 MiB versus approximately 29 GiB for the
  existing store. The live store was **not** replaced or compacted. Investigate
  that amplification separately; neither store is disposable build cache.
- Deployed backup executable SHA-256:
  `a6811f77fb95b49ac77db1bddecdee5a14f5c08d3df893bd15840056388031e1`.
  Its immutable compatibility runtime is retained as
  `visionclaw-backup-runtime:nov16`. The user service mounts only the executable,
  read-only stores/checkpoints and private NAS output; it has no network.
- First live checkpoint completed in 110.0 s. The exact user service then
  completed successfully against it, producing run `20261116T212245Z` with
  checkpoint metadata and the same 2,093,326-quads count. The 03:15 nightly
  timer is enabled with up to five minutes of jitter; user lingering is enabled.
- The six-hour checkpoint interval plus nightly backup permits up to roughly
  30 hours of Oxigraph age before failures/delays. A checkpoint older than
  24 hours is refused by the backup tool. There is no claim of cross-store
  transactional consistency or continuous PITR; ADR-2103 remains partial.

## Loom on HP

- Main and the development/serving checkouts advanced to `b7fe382`.
  Existing dirty model configuration and rollback data were preserved.
- Built from a clean committed source export and the pinned RuVector source,
  with Cargo jobs capped at 12. Image:
  `sha256:b99c1f329f01262d01bc58632ce93cf9d30c24e8b0cb752c6cb96450b3cd684c`.
  Only the facade was recreated; the model service was not restarted.
- Facade library/promotion/reload tests: 97 passed. The runtime target was
  probed against the real read-only 9,532-class bundle rather than the obsolete
  smoke stage's hard-coded 8,146-class fixture.
- Manual nightly promotion and reload succeeded. The reload receipt says
  `served` for `visionGraph@a5b754af444521206c84b6d31a4922b9eba6b52c`;
  health reports 9,595 classes and semantic search ready, with the previous
  generation recorded. Installed bundle modes are 0755 directories/0644 files.
- Both nightly-promotion and generation-reload user timers are enabled;
  lingering was already enabled. Previous facade image and data copy remain
  available. The separate model image, weights and ledger volume were retained.

No Nix GC, Docker prune, Cargo cache deletion or data-volume deletion was run.
