# Owner decisions open at the end of the 2026-10-18 sprint

Each section is one decision the estate register (`docs/TODO-unified.md`) could not close without the owner.

**Correction, same day.** The first version of this brief described four of its six decisions from register rows written before other sessions had changed the code. Re-checked against `main` after the owner answered: S-4 (1) and S-7 (c), (d) were already closed, and ER-28's premise (push off) was false. The sections below now carry the current facts and the outcome of each decision.

## ER-6: which key signs dream-engine's governance events — decided, source done

**Decided: an agent-governance signer.** dream-engine signs 31400 (panel) and 31402 (case) in production; its 31403 appears only in tests. agentbox `e07b1a366` makes exactly those two kinds grantable, only on `forum_event`, and only to keys declared `agent_governance`, which sign nothing else; the human-decision kinds stay refused for every key. ADR-2122 records the boundary.

**Your steps:** generate the key independently (never derived from `k_id`), declare it in `config/custody/identity-port-acl.json` with that grant for dream-engine's uid, add its pubkey to the forum's governance allowlist, rebuild the image, and move dream-engine onto the port.

## ER-28: solid-pod-rs git push — needs a fresh decision

The first version said push was off. It is not. agentbox builds the pod server with the `git` feature in its baseline set, and `handle_git` serves push: it resolves the caller from a `Basic nostr:` or NIP-98 credential and requires Write on the pod root (Read for clone and fetch). The pre-receive guard refuses any push touching `.acl` sidecars or server-managed paths, and the repository root is correct. So deleting the path would break a working agentbox feature; the "delete" answer was given on false facts and is not acted on.

**What is still weak:** a pushed file is not checked against its own ACL, so a holder of Write on the pod root can write under a sub-container with a stricter ACL; and clone needs only Read on the root, so the whole history is readable to anyone who can read the root.

1. **Per-path checks** in the existing push guard (it already lists every changed path) and in clone, so each path is judged by its own ACL.
2. **Accept as is**: pod-root Write is normally the owner alone, which makes the gap narrow.

**Recommendation: option 1**, scoped to the guard you already have.

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
