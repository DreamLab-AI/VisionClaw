# Systemscape, messaging and Jev cutover — 2026-11-18

Activation completed at **14:26:38.485 UTC** from Agentbox main
`79b1580db543b0721751e7e68fbaad62eb3552fa`.
Image: `sha256:fd082ea35c49ea39c9aab08d2cd87d0bd6a7ab8201acf036760a54f010364f55`.

## Source and gates

- Systemscape main was pushed at `fbd780511de2c3d264d53b8cb097f983b8ec156c`;
  Agentbox pins 0.4.0 with NAR hash
  `sha256-hvAQpf+9SfBfSvHL70wmDyfD3YxTgvKxSVwwUCTapko=`. The lock is
  byte-identical to upstream. No crates.io publication was needed or performed.
- The hosts-mount branch was merged. Host Nix package tests: 49 passed.
  Real isolated terminal smoke passed, including host selection, missing-probe
  status, off-screen pausing, resize and probe cleanup. Hosts-layer checks: eight;
  actual boot-block fixture: one; lifecycle guard tests: 24.
- Later consolidated main also includes multipart DMs, marked cuts, newest-tail
  peek, long tmux-buffer sends and the Jev score-scale fixes. A stale consumer
  test was corrected to compare against the explicitly selected vendor's locality.
- A genuine noninteractive-locale defect surfaced during the sessions tests:
  tmux sanitized tab separators to underscores without UTF-8 mode. The tmux
  client now forces `-u`; a regression checks it. The complete sessions suite
  then passed **66 tests**, including 40 KB multi-byte/multi-line delivery.
- System One workspace: 310 passed; Jev plugin: 88 passed; focused message
  tests: 14 passed; consumer-activation tests: two passed. All 162 Agentbox
  ADR records validated. Those are source/contract gates, not a paid model eval.

## Delivery costs and cache retention

| Preparation | Changed layer bytes | Nix | Push | Pull/unpack | Activation |
|---|---:|---:|---:|---:|---|
| Initial Systemscape 0.4 image (`3159051e7`) | 33.998 GB | 549.361 s | 313.261 s | 1,524.501 s | Held at operator request; never activated |
| Consolidated messaging/Jev image (`79b1580db`) | 3.600 GB | 192.071 s | 33.932 s | 170.002 s | Activated |

The small Systemscape package change invalidated the **30.399 GB platform
layer**, because the monitor is in `basePackages`. Unpacking was mostly
single-core CPU work, not an I/O lockup. The second preparation reused that
already imported layer and changed only services/application layers. These
measurements explain the cost; they are not a promised duration for another run.

Follow-up: classify frequently changing monitoring tools outside the platform
layer. No such layout change was made during this rollout. No cache, Nix
output, recovery image or data volume was pruned. Roughly 247 GiB remained free
on the shared NVMe after the final cutover.

## Controlled mount migration and runtime verification

- Ordinary fast activation correctly rejected a different mount set. A separate
  operator-authorised procedure permitted exactly one added read-only bind:
  `config/systemscape` at `/etc/agentbox/systemscape`. Every pre-existing mount
  source, target and access mode matched before and after; private settings
  contents were unchanged. The generic lifecycle guard was not weakened.
- The private hosts file remained gitignored, mode 0600, and was absent from
  shipped application paths and the isolated candidate. Boot created
  `~/.config/systemscape/hosts.toml` pointing to the managed read-only file.
- The installed binary reports 0.4.0. The configured remote returned versioned
  probe frames, and an isolated default-path terminal showed it live and quit
  cleanly. The existing System pane initially lacked the host strip; sending
  `q` restarted only that telemetry pane onto the config. It then showed the
  configured host, and one persistent probe was observed on HP. No headset
  performance benchmark was rerun in this deployment.
- System One was rebuilt for the score legend/probabilities fix:
  `sha256:ef0820564188bf38ef8346906fccebf19b043bd0a0ab3b25c03c59da67fe3947`.
  It and OpenJev were recreated together (shared network namespace), retaining
  model volumes. The email service and other independent GPU sidecars were not
  rebuilt or restarted in this round.
- Installed Jev cache hooks match their baked source byte-for-byte. The cached
  reader successfully parsed both a real local facade response with three
  score bands and a legacy score-only response with the caller's scale hint.
  Runtime bridge server and turn-sink files also match the baked copies.
- Agentbox boot restarted tab0-bridge, the Nostr gateway and the sessions daemon;
  they are RUNNING. The sessions API returned eight structured session records.
  Memory MCP health, synthetic terminal frames, CUDA, NVENC and QGIS checks
  passed. No unrelated paid cloud inference was added to these checks.

## Explicitly unresolved

- `/ready` is true with the existing owner-parked elevation-publisher warning:
  `no-signing-stack`, zero skipped/failed proposals. That is not an unqualified
  empty-degradation claim; no signing stack or keys were provisioned.
- The pre-existing simultaneous stale-lock takeover race is not fixed here.
- The main model setting was `opus` before cutover. The boot permissions
  projector applied the manifest's `sonnet` default. Only the persisted model
  field was restored to `opus`, with a concurrent-edit/hash guard; every other
  settings field was preserved. The temporary settings staging copy was removed.
  The manifest/ADR model-policy mismatch remains undecided and can recur on
  another boot until the owner chooses the canonical policy.
- No paid 100-session evaluation, harness run, budget decision or model-policy
  change was authorised or performed. Automatic Codex audits remain disabled.
