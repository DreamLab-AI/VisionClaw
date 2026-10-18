# Owner decisions open at the end of the 2026-10-18 sprint

Each section is one decision the estate register (`docs/TODO-unified.md`) cannot close without the owner. Each gives the choice, the evidence, a recommendation and what follows. Nothing here is implemented; pick an option and the work is a normal change.

## ER-6: which key signs dream-engine's governance events

**Choice.** The identity port refuses to grant kinds 31400–31405 and 38414 to any key (`services/nostr-pod-bridge/src/identity_port/acl.rs`, `NEVER_GRANTABLE_KINDS`). dream-engine publishes 31400 (panel) and 31402 (case) as JunkieJarvis, so it cannot move onto the port as the W3b cutover planned.

1. **An agent signer for agent-authored kinds.** A separate `junkiejarvis-agent` key may be granted 31400 and 31402 only; the human-decision kinds stay never-grantable. The forum can then tell agent from human authorship by key.
2. **dream-engine keeps its own key file.** No port change; W3b is restated to exclude it, and the same-UID boundary (X-1) stays open for that key.

**Recommendation: option 1.** It is the one that serves the identity port's purpose, and the rationale gate (ER-7) already wants agent and human authorship distinguishable. Cost: a new key in the custody split, a forum allowlist entry, a per-key exception list in the ACL schema. Recorded in agentbox ADR-2122.

## ER-28: solid-pod-rs git push

**Choice.** Push is off: `handle_git` builds `GitHttpService` with no auth provider, so every push is refused. If it were enabled, authorisation is Write on the pod root only, so a pusher could overwrite files under stricter sub-container ACLs (for example `/private/privkey.jsonld`); clone and fetch check Read on the root only, so history is readable; the git panel handlers sit outside the push guard; and the repository root is one directory too deep.

1. **Keep push off and delete the push path** until a consumer needs it.
2. **Enable it properly:** per-path WAC on every pushed file, Read checks on clone and fetch per path, the panel handlers inside the guard, and the root fixed.

**Recommendation: option 1 now.** Pod git push already exists in the estate, but through a different implementation: agentbox's management API serves `/pods/:npub/.git/*` itself (`management-api/routes/pod-git`, owner-only push, gated by `[sovereign_mesh.git].enabled`), not through solid-pod-rs-server's `handle_git`. So nothing depends on the solid-pod-rs path, and option 2 is a security design in its own right. Deleting the latent path removes the trap of someone switching it on.

## ER-35: exact or range pins between in-family crates

**Choice.** Published crates pin each other exactly (`=0.5.0-alpha.13`). A security release of one crate then forces a republish of every crate that pins it: `nostr-bbs-core` had to ship `beta.16` only to move its pin. It is happening again today: solid-pod-rs `0.5.0-alpha.14` is published, but the forum workspace and VisionClaw both pin `=0.5.0-alpha.13`, so neither picks it up until their pins move.

1. **Caret ranges** (`^0.5.0-alpha.14`). For a pre-release, Cargo's caret accepts later pre-releases of the same `0.5.0` and nothing past it, so a consumer resolves the next alpha without a republish.
2. **Keep exact pins** and accept the republish chain.

**Recommendation: option 1 for published crates, keeping exact pins only in binaries' lockfiles.** Alphas may change API, so a caret is a mild risk; the lockfile still fixes what a build uses, and the gain is that a security release propagates on `cargo update`.

## S-4 (1): the `/multi-mcp/ws` endpoint

**Choice.** `send_discovery_data` in `multi_mcp_websocket_handler.rs` simulates discovery (a 20% synthetic failure and a 100 ms sleep); `MultiMcpAgentDiscovery` is never constructed. The route is registered and authenticated. No client in VisionClaw, agentbox skills or MCP servers calls it; the only mention is a test fixture.

1. **Retire the route**, its module and the demo test.
2. **Wire `MultiMcpAgentDiscovery`** so the endpoint reports real agents.

**Recommendation: option 1.** It has no caller, and the estate rule is that dead code is deleted, not kept on the chance of use. Re-adding a real endpoint later costs no more than wiring this one now.

## S-7 (c) and (d): two hygiene items

- **(c) The SONA feeder's headline count.** `scripts/ruvector-sona-feeder.mjs` reports `fed` (items marked learned) as its headline although the engine stores nothing at 384 dimensions; it already reports `engine_stored` beside it. **Recommendation:** make `engine_stored` the headline and keep `fed` as a secondary figure. It is a metrics contract, so it is yours to change.
- **(d) Exit codes of the dream check scripts.** `dream-build-check.sh` and `dream-link-check.sh` always exit 0; `deploy.yml` greps their output for the OK marker, which works. Making them exit non-zero changes the contract of both `deploy.yml` and the dream-cycle annexe dispatch. **Recommendation:** leave them, since the grep contract is enforced today; change both consumers together only if a third consumer appears.

## ADR-VG-004: activate the space and Earth domains

**Choice.** visionGraph ADR-VG-004 is `accepted`, `complete`, `staged`: the two domains (ids 6 and 7) exist in source and the publish workflow already checks them ("Verify the published space and Earth domain contract" passes). Activation means accepting them as live on the published site; S-5 and ER-8 close with it.

**Recommendation: activate after the next green publish.** The latest publish run (`39153534571`, 2026-10-18) did not deploy: it failed a later gate, the supply-chains domain check, because the pinned ontomark 0.4.0 exporter predates that domain. That is separate work in flight (an ontomark release and repin), and the live site stays on its last good deploy until it lands.

## Corrected in this brief: S-3 needs no decision

The register's end-of-sprint table listed S-3's code half as an open decision. It is closed in source: agentbox `e4ff72769` honours `X-Agentbox-Pubkey` only from the proxy's loopback peer, with `AGENTBOX_PROXY_PEERS` as an override. The residual, a process inside the container connecting over loopback, is the same-UID boundary X-1 already tracks.
