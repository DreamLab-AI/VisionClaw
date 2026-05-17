# ADR-060: Owner-pubkey-filtered binary position encoder

**Status:** Proposed
**Date:** 2026-04-28
**Author:** VisionClaw platform team
**Supersedes:** —
**Related:**
- ADR-050 (sovereign model: visibility, owner_pubkey, opaque_id, bit-29)
- ADR-059 §6 (Phase 4 of bidirectional agent channel — calls out this ADR)
- Pair on agentbox side: ADR-014 (subscriber + OPF inbound policy)

## TL;DR

Phase 4 of ADR-059 adds a stricter visibility filter at the binary
position encoder: when `PUBKEY_VISIBILITY_FILTER=true`, nodes that
are private and not owned by the current session pubkey are **dropped
from the wire frame entirely**, rather than just opacified by bit-29.
Default behaviour is unchanged (opacification via ADR-050). Fail-closed:
anonymous + flag enabled ⇒ public-only graph.

## Decision

`src/handlers/socket_flow_handler/position_updates.rs` checks
`PUBKEY_VISIBILITY_FILTER` env var at every position broadcast. When
enabled, the existing `private_opaque_ids` set (computed per-caller
via `compute_private_opaque_ids(&nta, session_pubkey)`) is used as a
**drop set**: any `(id, data)` whose `id` appears there is removed
before `encode_node_data_with_live_analytics_and_privacy` is called.

Visible debug line on every frame where ≥1 node was dropped:

```
Sent position snapshot with N nodes (M dropped by PUBKEY_VISIBILITY_FILTER)
```

## Consequences

- Defence in depth: even a malicious client cannot correlate opaque IDs across sessions if private-of-others nodes never reach the wire.
- Default-off ⇒ no behaviour change for current production. Enable per environment via Docker env / launch.sh.
- One frame's worth of extra allocation when flag is on (the filtered Vec). Negligible.

## Phasing

Land alongside ADR-059 Phase 4. The `PUBKEY_VISIBILITY_FILTER` flag
exists today (default false). Promotion-to-default-on ships once the
test matrix in ADR-059 §QE has been exercised against a multi-user
session for ≥1 release.
