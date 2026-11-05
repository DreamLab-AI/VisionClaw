# External audit repairs, 2026-11-03

Implementation commit: `6b811f7a1f432f9f880f686a0197ae06a7d05599`.
The November 2 source reconciliation is retained as historical evidence.
These repairs are local code changes; no deployed service or live headset was changed.

| Concern | Implemented boundary |
| --- | --- |
| Release XR upgrade | The transport builds an HTTP upgrade request with a same-authority Origin and a signed NIP-98 subprotocol over the HTTP(S) URL. Malformed configured keys fail before connect. A random signed nonce makes rapid reconnect events distinct. A peer that does not negotiate the signed protocol retains the existing first-message authentication path. |
| Browser V5 acknowledgement | The worker application path no longer discards frames within 16 ms and no longer swallows worker failure. The caller therefore acknowledges only a successfully applied chunk. |
| Release acceptance | Evidence comes from sequence-ordered observer samples after release. A cross-socket displacement, a single old sample, duplicated/out-of-order frames or complete silence cannot prove release. The held node must move between post-release samples; subsequent graph quiet is checked separately. |
| Hold ownership | The orchestrator now keeps node-to-holder ownership, matching the GPU hold boundary. A stale holder's release cannot erase the current holder's no-sleep state. |
| Cross-volume link allowance | The Rust pack admits at most 512 instances including deduplicated endpoint markers. The 512-link input table is not treated as a 512-instance output bound. |
| Portal rearm | The box-distance function now returns world metres, including the box's per-axis scale, and compares them to an arrangement-scaled margin. The old function's documentation said world metres but returned local units; multiplying only the margin would have introduced a second scale error. |
| Topology generation | A topology generation change releases every active hand hold for that connection before its ids can be reused. |
| Annotation provenance | The decoder and join retain the source repository and snapshot revision supplied by Codescape. The revision is Codescape's monotonic graph revision, not a Git commit SHA. Legacy streams retain explicit absent values. |
| Saved API keys | GET and application lookups read the same NostrService user record POST writes; the authenticated GET response is no-store. Caller identity still comes from authentication middleware. |
| Corpus pull freshness | A generation is current only when both buildSha and sourceSha agree, in addition to the existing atomic-generation requirement. Legacy manifests without sourceSha remain comparable to other legacy manifests. |
| Proposal resource identity | An amendment that changes the existing page's resource IRI receives RESOURCE_IMMUTABLE before it can be posted. Creation identity remains governed by its existing collision checks. |
| Grouped proposal consumer gap | Local unsigned grouped assessment remains available, but event construction refuses grouped requests with an actionable atomic-consumer error. Ordinary single-page event construction remains supported. No partially applied group is introduced. |

## Executed validation

- XR Rust library: 503 passed, 2 ignored; includes a real loopback WebSocket
  handshake with signature verification, total-instance budget, metadata parsing
  and rapid reconnect event tests. XR Clippy with warnings denied passed.
- Vault library: 342 passed. Vault Clippy across all targets with warnings denied passed.
- Browser: five focused tests passed, covering the real frame application path
  and acknowledgement ordering/failure behaviour.
- Release-evidence helper: three Node tests passed, including the silent pinned
  and out-of-order counterexamples.
- Server library: 1,809 passed, 11 ignored. Initial new-test setup errors (missing
  Tokio context and a stale fixture checksum) were corrected without changing
  product assertions; the focused holder and source freshness regressions pass.
- Headless GUT on Godot 4.8-beta2: 556 passed, 3 renderer-dependent tests pending,
  across 50 scripts. A newly built extension and tracked source were copied to
  an isolated temporary project on HP with OpenXR disabled. The running headset
  process was untouched. This establishes no stereo, comfort or deployed result.

The worktree used the primary checkout's ignored legacy PTX includes and the
Agentbox federation schema as read-only build fixtures, because these inputs are
not materialised automatically in a new worktree. The GPU crate compiled its own
production kernels. No source or build input was taken from an unverified remote.

## Remaining explicit boundaries

LOOM-A-01 is not closed. Loom ADR-135 D4.6/D7 and PRD-025 section 8 require a
signed distillation result, designated-provider allowlist, submitted-job field
reconciliation, lease fencing and revocation. There is no shared typed
DistillJobResult/31409 producer/verifier in the inspected VisionClaw and Loom
Rust paths, and the illustrative JSON sig does not define canonical signed
bytes. Adding a private verifier here would invent a new wire contract. The
derived-write route still has its graph fence and power_user gate; the signed
distiller contract remains work for both owners.

The independent security review also raised a low-confidence issue in the
unchanged AddIfNoConflict group-index remapping in PhysicsOrchestratorActor.
That hypothesis is tracked separately from these repaired hold messages; this
pass does not claim its closure.
