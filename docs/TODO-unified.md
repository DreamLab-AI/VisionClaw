# Unified TODO: VisionClaw and Agentbox estate

**Live board, rebuilt 2026-11-16.** One row per open item, grouped by what blocks it. Every row was checked against the repositories on 2026-11-15; "state" is what that check found on `main`.

The full history, with every dated reconciliation from 2026-09-07 to 2026-11-15 and the definition of each row id, is in the [archived board](archive/TODO-unified-2026-09-07-to-2026-11-15.md). Row ids here are the ids used there. Rows labelled by date (for example `11-08 #1a`) are items from that reconciliation's ranked list.

**Scope.** This board covers VisionClaw and agentbox, plus the audit findings rated high that were still open on 15 November (W2-01, W3-01, AW1-09, CONTRACTS-F3, AW1-01); CONTRACTS-F1 and F2 are W2-01 seen from the contract side. It does not cover parked work, the gap register or the duplicate-implementations report. Those lists, and estate-wide context, are indexed from [current focus](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/current-focus.md).

**Rules for this page.** Edit rows in place; history lives in git. A change that fixes a row updates the row in the same pull request. "State" is dated: a row whose state is older than a month is due a re-check. A row leaves when it is done and observed, or when it is parked with a date. A closure made in source but not yet seen running stays here, marked "source only".

**Counts:** 76 open rows. 21 ordinary engineering; 26 waiting on an owner decision; 8 waiting on an owner act (key ceremony, credential, push); 8 needs a live deployment; 4 needs a device; 6 waiting on an external party; 3 stale.

## Ordinary engineering: can start now

| Row | Item | State on main, 2026-11-15 | Fix on an unmerged branch |
|---|---|---|---|
| 11-08 #1a | Browsercontainer publishes VNC 5903, MCP 8931, CDP 9222 on 0.0.0.0 | FIX ON UNMERGED BRANCH | yes: agentbox `fix/estate-review-hardening` 721836d53 (compose files only: browsercontainer, gui-tools, xr-runtime; no `flake.nix` change) |
| 11-08 #1b | Xvnc runs `-SecurityTypes None` | STILL OPEN |  |
| 11-08 #1d / W3-01 | Relay consumer admits every signed author when allowlist unset | FIX ON UNMERGED BRANCH | yes: agentbox `fix/estate-review-contracts` 2c1f85706 |
| 11-08 #1e / W3-05 | NIP-98 door fails open if its boot step fails | CANNOT VERIFY |  |
| 11-08 #7 | Dependabot backlog untriaged; no `dependabot.yml` | STILL OPEN | config only: project `feat/golden-thread-2026-11-15` 65eb0c588; agentbox `fix/estate-review-hardening` 08b24eeab (adds config, does not triage) |
| AW1-09 | `/ontology/derived` gated `power_user` only (Loom ADR-135 D7) | STILL OPEN |  |
| W2-01 | Agent events sent bare; VisionClaw drops them as Malformed | FIX ON UNMERGED BRANCH | yes: agentbox `fix/estate-review-contracts` 9040efc6f; project `fix/estate-review-2026-11-15` eafa7d3e4 (test only) |
| W3-02 | Saved API keys never read back | CANNOT VERIFY |  |
| 11-15 new | RUSTSEC-2026-0459: `libsqlite3-sys 0.28` via direct `rusqlite 0.31` | STILL OPEN |  |
| cheap: forum-tweaks | Delete remote `fix/forum-tweaks-2026-10-22` | STILL OPEN |  |
| X-1 | Same-UID boundary; per-role accounts and `/run/secrets` | STILL OPEN |  |
| X-2 | Actor bypass audit (direct store reads, `execute_in_thread`) | PARTLY DONE |  |
| X-3 | One request id across seven layers; change locked 52-byte frame | PARTLY DONE |  |
| L-3 | Clamp-fire receipt | STILL OPEN |  |
| E-5 | Direct axiom writes need forum apply path (waits on N-2) | STILL OPEN |  |
| N-2 | Forum `vault propose` apply path | STILL OPEN | adjacent: project `feat/golden-thread-2026-11-15` |
| N-6 | Key-custody surfaces on shared UID | STILL OPEN |  |
| CY-A3 | Three green scheduled nights of the estate check | PARTLY DONE | partly: VisionFlow `fix/estate-review-2026-11-15` f4a5db5 |
| ER-18 | ontomark NGG2 relation codes/resolvers | CANNOT VERIFY |  |
| ER-29 | Cross-process anchoring locks (three in-process maps) | STILL OPEN | no (the solid-pod-rs fix branch touches other things) |
| CONTRACTS-F3 | agentbox's task-list response omits fields VisionClaw's client requires, so the call fails (audit finding, 2 November) | STILL OPEN | yes: agentbox `fix/estate-review-contracts` |

## Waiting on an owner decision

| Row | Item | State on main, 2026-11-15 | Fix on an unmerged branch |
|---|---|---|---|
| 11-08 #1c | Supersplat listens 0.0.0.0:8795 unauthenticated (ADR-2177) | STILL OPEN |  |
| 11-08 #4 | Audit journal: torn records `events/2026-10-12.jsonl` lines 35, 64; refused-revoke not journalled (B4/B5) | STILL OPEN |  |
| 11-08 #6 | `VISIONCLAW_JEV_ALLOW_CLOUD` defaults to 1 | STILL OPEN | no; adjacent: agentbox `fix/estate-review-hardening` 283d85acd makes the System One judge local by default (`agentbox.toml` only, different setting) |
| 11-15 new | Re-enable GitHub Actions on demand? (#43 merged on local verification) | OVERTAKEN |  |
| cheap: nix-flake-update | Merge bot's `deps/nix-flake-update` | STILL OPEN |  |
| cheap: FORGE_MIRROR_FOLLOW | Owner-chosen follow list (VisionFlow ADR-2019, solid-pod-rs ADR-2009/2010) | STILL OPEN |  |
| cheap: ADR-OM-003 | ontomark ADR-OM-003: status text disagrees with front matter | STILL OPEN |  |
| ratify: VC 2134/2138 | VisionClaw ADR-2134, ADR-2138 'reserve acceptance' | STILL OPEN |  |
| ratify: VC 2039/2108 | VisionClaw ADR-2039, ADR-2108 empty `verified_commit` | STILL OPEN |  |
| ratify: VF 2015/2018 | VisionFlow ADR-2015 (proposed) and ADR-2018 ruling | STILL OPEN |  |
| ratify: VF 2021 | VisionFlow ADR-2021 business roadmap: accept, amend or reject | STILL OPEN |  |
| ratify: AB 2080/2162/2177 | agentbox ADR-2080 item 3 (router.sh:79), ADR-2162, ADR-2177 supersplat listener | STILL OPEN |  |
| G-4 | Public-recipient set for mandatory recipient enumeration | STILL OPEN |  |
| G-7 | Backup policy per store | STILL OPEN |  |
| G-23 | `ESTATE_READ_TOKEN` read on visionGraph, or mark it exempt | STILL OPEN |  |
| EA-06 | Per-store erasure mappings and backend adapters | STILL OPEN |  |
| M-3 | HP image candidate failed readiness (propose close, superseded) | OVERTAKEN |  |
| V-1 | Matching semantic bundle/server activation | CANNOT VERIFY |  |
| N-3 | Host ADR-2111: delete FsPaymentStore | STILL OPEN |  |
| N-10 | Estate adoption of sidestr capabilities | STILL OPEN |  |
| CY-B7 / CY-B8 | Augmentation-conditions activation; ADR-2095 measurement run | STILL OPEN |  |
| CY-C | Fresh-host runbook / Trust letter MVP | STILL OPEN |  |
| ER-7 | VisionClaw's own 31402 builders declare task properties | STILL OPEN |  |
| ER-11 | Memory-cloud tombstones | STILL OPEN |  |
| `feat/upgrade-phase-0-1` | Forum branch: inbox-key attestation fix and Phases 2-4: merge or retire | STILL OPEN |  |
| AW1-01 | The pod server's forge mirror refuses the loopback relay it ships with as its default; needs a feature-wiring decision (audit finding, 2 November) | STILL OPEN |  |

## Waiting on an owner act (key ceremony, credential, push)

| Row | Item | State on main, 2026-11-15 | Fix on an unmerged branch |
|---|---|---|---|
| 11-08 #5 | visionGraph third-party attribution (SLSO, SWEET, SAC-CG): NOTICE and `/third-party-notices/` | FIX ON UNMERGED BRANCH | partly: visionGraph `fix/estate-review-2026-11-15` a5b754af4 (NOTICE only; page not covered) |
| cheap: Gemini key | Gemini audit key 401; ADR-2131 audit leg never ran | CANNOT VERIFY |  |
| X-4 | Upgrade rehearsal built, never run | STILL OPEN |  |
| G-5 | Replacement publisher identity, recovery custody (key ceremony) | STILL OPEN |  |
| AC-FORUM-1 | Three blank `audited_by`; deepsec gate store reset; nothing deployed | PARTLY DONE |  |
| ER-6 | agent_governance key ceremony, ACL grant, forum allowlist, dream-engine onto port | STILL OPEN |  |
| ER-23 | Website 5-site admin-key rotation | STILL OPEN |  |
| ER-36 | Trail-key hygiene (agentbox image half done) | STILL OPEN |  |

## Needs a live deployment

| Row | Item | State on main, 2026-11-15 | Fix on an unmerged branch |
|---|---|---|---|
| EA-04 | Live external mutation journey | CANNOT VERIFY |  |
| EA-05 | Matching bundle/server generations (loom/visionGraph) | CANNOT VERIFY |  |
| M-4 | Multi-peer delivery accepted/rejected | CANNOT VERIFY |  |
| M-5 | Completed live nightly receipt with sibling HEAD evidence | CANNOT VERIFY |  |
| L-2 | Second-model-family traffic canary | CANNOT VERIFY |  |
| L-4 | Failure-path telemetry liveness | CANNOT VERIFY |  |
| DR-1 | Complete nightly agent-runtime journey | PARTLY DONE | partly: VisionFlow `fix/estate-review-2026-11-15` f4a5db5 (stale snapshot names its cause; nightly fetches first) |
| N-13 | Dream governance: first rebuilt-engine night receipt; ledger silent since 10-24 | PARTLY DONE | partly: VisionFlow f4a5db5 |

## Needs a device

| Row | Item | State on main, 2026-11-15 | Fix on an unmerged branch |
|---|---|---|---|
| L-1 | Signed model/mobile session canaries | CANNOT VERIFY |  |
| L-5 / N-4 | Headset acceptance, keystore, voice/XR receipts | STILL OPEN |  |
| L-6 | Amethyst/Amber mobile session | CANNOT VERIFY |  |
| ER-13 | Gaze binding premise re-test on Godot 4.8 | PARTLY DONE |  |

## Waiting on an external party

| Row | Item | State on main, 2026-11-15 | Fix on an unmerged branch |
|---|---|---|---|
| 11-08 #9 | Production-image receipt; VisionClaw ADR-2037/2038/2012/2119 (parked) | STILL OPEN | adjacent: project `feat/golden-thread-2026-11-15` c1a407a42 turns dev auth bypass off and dev ports loopback by default (touches the ADR-2012 DEV_AUTH_LOOPBACK residual) |
| X-5 | OWL/RDF vs flat retrieval experiment (loom PRD-028) | CANNOT VERIFY |  |
| D-1 | Seven-day observation window with daily recall receipts | CANNOT VERIFY |  |
| ER-17 | SONA 384-dim | CANNOT VERIFY |  |
| RUSTSEC-0387 residue | `age 0.12.1` to p256 0.13 to crypto-bigint 0.5.5 in agentbox secret-backup; sidestr-evm k256 0.13 | STILL OPEN |  |
| S-6 | Podkey pin re-pin | STILL OPEN |  |

## Stale: close or reword

| Row | Item | State on main, 2026-11-15 | Fix on an unmerged branch |
|---|---|---|---|
| cheap: night 10-27 | Dream night 2026-10-27 has no events file | OVERTAKEN |  |
| ratify: dream-engine #7 | dream-engine PR #7 'still open' | OVERTAKEN |  |
| X-6 | Dream engine 'mostly paper' | OVERTAKEN |  |

## Scope notes carried from the archived board

Scope exclusions, the added XR visual scope and the frozen and historical scope are unchanged; see the last sections of the [archived board](archive/TODO-unified-2026-09-07-to-2026-11-15.md#explicit-scope-exclusions).
