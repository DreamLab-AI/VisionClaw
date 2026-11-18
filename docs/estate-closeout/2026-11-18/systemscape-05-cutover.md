# Systemscape 0.5 cutover — 2026-11-18

Agentbox activation completed at **21:27:51.906 UTC** from clean main
`2062be880309614051bf66878618fcd9523c9cdc`.
Image: `sha256:8f97ed0672e54e5d9e2132575426c6dfac6463d92af7dc70928ec370f0c1993b`.
The parent gitlink is recorded by `81c57c711`.

## Source and validation

- Systemscape upstream main is pushed at
  `7faffcd29a13f5d0d217434a6e7fafe8407492a1`, version 0.5.0. Host Nix
  prefetch independently confirms
  `sha256-t+7REjvDMzmAzQhJJB6VxDn/ni0BRHR0e2Ti7V8bA/Q=`. The vendored
  Cargo.lock is byte-identical; no dependency version changed.
- The standalone Nix package passed **183 tests**, with one upstream timing
  benchmark ignored. The isolated real-terminal smoke passed activity controls,
  telemetry, alerts/banner/tmux option, off-screen idling, narrow/tiny resizing,
  host switching, missing-probe status and renderer/probe cleanup.
- A separate real recorder run wrote a complete 150-second slot to an isolated
  temporary state directory. The history was mode 0600 and contained only the
  expected numeric/version fields. Startup's partial slot is deliberately
  discarded: allow up to two slots (five minutes), not only 150 seconds, before
  expecting a new file.
- Three guarded recorder-launch tests pass: current binary, older binary and
  failed help probe. Hosts-layer checks: eight; actual boot-link fixture: one;
  Notes launcher: 17; vault precedence: 10; consumers: 11; lifecycle/persistence:
  25; offline launch contract: six (daemon/live-provider cases not rerun).
- All 162 ADRs and their generated index validate. Four records received scoped
  source verification without promoting their acceptance/status axes.
- The candidate's binary hash matched the tested derivation. Its application
  tree and isolated container contained no private hosts file. All 6,518 actual
  shipped store paths were checked: no local QtWebEngine/WebKit/QGIS/OrcaSlicer.

## Delivery cost and retention

Preparation took **2,294.381 seconds (38 min 14 s)**: Nix/package/evidence
376.533 s, registry setup 0.227 s, push 272.198 s, Docker pull/unpack
1,644.833 s, smoke/verification 0.590 s. This excludes earlier standalone
package checks and subsequent activation checks.

Only platform and application layer digests changed: 31,482,639,872 bytes
uncompressed (platform 30,400,152,064; application 1,082,487,808). Toolchain,
agent-CLI and service blobs were reused, but their snapshots still needed
extraction after the platform parent changed. The small monitor remains in
`basePackages`; moving frequently updated monitors out of that platform group
remains a build-speed follow-up, not an implemented change.

The build used one Nix job with 16 cores, selected from 28 available CPUs by the
60% budget. A Docker unpack sample used roughly 115% CPU; observed I/O pressure
was far below the earlier sustained lockup levels. Approximately 198 GiB was
free after cutover. No Nix GC, image/cache prune, volume deletion or Cargo-cache
cleanup was performed; recovery images and data volumes remain.

## Runtime verification and limits

- Normal guarded activation succeeded with unchanged persistent mounts; no
  mount migration was needed. All 16 other running container IDs matched the
  pre-activation snapshot. Neither System One nor OpenJev needed a rebuild.
- Both Systemscape views execute the exact tested 0.5.0 binary. The System pane
  has one `--record` process; its state directory is owned by devuser, mode 0700,
  on the retained `.local` volume. The private hosts symlink is intact.
- Remote frames and an isolated default-path viewer passed; the HP's 0.4.0
  version-1 probe remains compatible and was not upgraded. After the targeted
  pane reload below, the System pane includes the configured host and one
  persistent remote probe is running.
- Supervisor services settled. Readiness is true, with only the existing
  owner-parked elevation publisher `no-signing-stack` warning and zero skipped
  or failed proposals. Memory MCP reports healthy RuVector PostgreSQL. Synthetic
  terminal truecolor, CUDA scalar, NVENC and QGIS ping probes pass.
- The pre-activation main model was `fable`. Boot projected the manifest's
  `sonnet`; only the model field was restored, guarded against concurrent
  settings edits and with all other fields compared unchanged. The temporary
  full settings copy was removed. No model-policy decision, paid evaluation or
  unrelated cloud inference was performed; automatic Codex auditing stays off.

### Boot-order follow-up (not fixed in source here)

The System pane launched before Stage B projected the hosts symlink:
tmux-autostart finished at 21:27:42.636; the link was created at 21:27:43.139.
The initially local-only viewer therefore did not spawn a remote probe.
Sending `q` only to the System telemetry pane at 21:30:18 let its existing
two-second wrapper reopen the projected config. Agent/work panes, the bridge
and the container were not restarted again.

This is a reproducible startup race, not a broken 0.5 probe protocol. A future
source fix should project before consumers or gate only the System launcher
with a bounded wait, retaining missing-file and hand-written-file behavior.
The current runtime is remediated; this report does not claim future boots
are race-free. The live recorder's first complete slot after that reload was
not yet written at the 21:30 runtime inspection (expected until about 21:35).

### Independent fleet activity

VisionClaw and Codescape stopped during preparation, before this activation:
Codescape finished at 20:59:59.270 with exit 0; VisionClaw at 21:00:17.730
with exit 137. Neither reports OOMKilled. This workflow issued no stop/restart
for either and left them untouched, rather than overriding possible concurrent
maintenance. They are excluded from the 16 running-container comparison above.
