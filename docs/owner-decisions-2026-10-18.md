# Owner decisions open at the end of the 2026-10-18 sprint

Each section is one decision the estate register (`docs/TODO-unified.md`) could not close without the owner.

**Correction, same day.** The first version of this brief described four of its six decisions from register rows written before other sessions had changed the code. Re-checked against `main` after the owner answered: S-4 (1) and S-7 (c), (d) were already closed, and ER-28's premise was re-checked twice; the second check confirmed it (push was off). The sections below now carry the current facts and the outcome of each decision.

## ER-6: which key signs dream-engine's governance events — decided, source done

**Decided: an agent-governance signer.** dream-engine signs 31400 (panel) and 31402 (case) in production; its 31403 appears only in tests. agentbox `e07b1a366` makes exactly those two kinds grantable, only on `forum_event`, and only to keys declared `agent_governance`, which sign nothing else; the human-decision kinds stay refused for every key. ADR-2122 records the boundary.

**Your steps:** generate the key independently (never derived from `k_id`), declare it in `config/custody/identity-port-acl.json` with that grant for dream-engine's uid, add its pubkey to the forum's governance allowlist, rebuild the image, and move dream-engine onto the port.

## ER-28: solid-pod-rs git push — decided: deleted

**The first correction to this section was itself wrong.** It said push was live because `handle_git` checks Write on the pod root. It missed that the git service underneath was built with no auth provider, so every push was refused with "no auth provider configured"; clone also returned 404 because the repository root doubled the pod name. The register's original premise, push off, was right.

An engineer working the "per-path checks" answer made push and clone work for the first time to test them. Shown the verified facts, the owner chose the original answer: delete the push path. The engineer's two commits were not shipped; they are kept on the local branch `er28-live-git-archive` in solid-pod-rs. agentbox serves pod git push through its own owner-only route, so nothing is lost.

**Done in solid-pod-rs (unreleased):** `handle_git` refuses `git-receive-pack` and its `info/refs` discovery with 403 for every caller before reading any credential, the route stays registered so the request cannot fall through to LDP, and the server's push-guard module is deleted. A test shows a pod whose ACL lets anyone write still refuses push and creates nothing on disk. Clone remains non-functional, as before; that is recorded, not changed.

## ER-35: exact or range pins between in-family crates — decided, partly done

**Decided: caret ranges.** VisionClaw `250d8d590` and the forum `9341826` replace their `=` pins with caret ranges; the forum now resolves solid-pod-rs 0.5.0-alpha.14 (2889 tests and the wasm check pass). VisionClaw still resolves alpha.13, because the published nostr-bbs-core 1.0.0-beta.16 pins `solid-pod-rs =0.5.0-alpha.13`. That clears with one more nostr-bbs-core release carrying the caret range, which needs your go-ahead to publish.

## S-4 (1): the `/multi-mcp/ws` endpoint — already closed, no decision needed

Another session wired real discovery behind the route on 2026-10-18 (VisionClaw `6a1a74b8b`, with follow-ups `4f4345503` and `9f98e3787`): `MultiMcpAgentDiscovery` is constructed in `app_state.rs`, and the handler no longer simulates failures or delays. The "retire" answer was given on stale facts and is not acted on.

## S-7 (c) and (d): already closed, no decision needed

- **(c)** agentbox `1f7728897` reports whether the engine is accumulating next to the learned count.
- **(d)** VisionFlow `2309f21` makes `dream-build-check.sh` and `dream-link-check.sh` exit with their verdict.

## ADR-VG-004: activate the space and Earth domains

**Choice.** visionGraph ADR-VG-004 is `accepted`, `complete`, `staged`: the two domains (ids 6 and 7) exist in source and the publish workflow already checks them ("Verify the published space and Earth domain contract" passes). Activation means accepting them as live on the published site; S-5 and ER-8 close with it.

**Recommendation: activate after the next green publish.** The latest publish run (`39153534571`, 2026-10-18) did not deploy: it failed a later gate, the supply-chains domain check, because the pinned ontomark 0.4.0 exporter predates that domain. That is separate work in flight (an ontomark release and repin), and the live site stays on its last good deploy until it lands.

## Corrected in this brief: S-3 needs no decision

The register's end-of-sprint table listed S-3's code half as an open decision. It is closed in source: agentbox `e4ff72769` honours `X-Agentbox-Pubkey` only from the proxy's loopback peer, with `AGENTBOX_PROXY_PEERS` as an override. The residual, a process inside the container connecting over loopback, is the same-UID boundary X-1 already tracks.
