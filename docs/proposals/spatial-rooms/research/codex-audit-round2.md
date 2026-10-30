**Verdict: retain as PROPOSED/inactive records; BLOCK ACTIVATION.** This is a decision-text audit, not implementation verification. There are **nine** supplied ADRs, not eight.

**Round-1 closure**

“CLOSED” below means the binding remedy is encoded in Decision text; it does not mean implemented or tested.

| Finding | Status | Closing text and remaining gap |
|---|---|---|
| **B1 — ordering** | **PARTIAL** | ADR-2031 D2: “authorises each against the state current at admission”; snapshots have “`created_at` strictly greater”. D3 requires a “persisted per-group, per-kind watermark”; D7 requires the storage repair. However, ADR-2157 D5 specifies a **per-room** watermark across 39000 and 39002, and increasing timestamps can outrun the future-date limit. See findings 7–8. |
| **B2 — freshness** | **PARTIAL** | ADR-2031 D4: “re-signs 39002 at least every 30 s”, stale sessions “are closed”, admission checked “after the session is registered”. But its **70-second** stale threshold contradicts “effective everywhere within R” (**60 seconds**). ADR-2157 D11 explicitly permits R + S. |
| **B3 — voice** | **PARTIAL** | ADR-2160 D6 requires “`RemoveParticipant` … at once”; D3 sets TTL “at most R”; grace “never applies” to kicks. However, D6 stops retrying when the token expires without establishing that expiry terminates an existing media session or prevents replay before expiry. |
| **B4 — membership authority** | **PARTIAL** | ADR-2030 D3 routes 9000/9001/9005/9021/9022, refuses 9009, and says “Projection lag denies”. But this denial is local: ADR-2157 D13 assumes lag makes 39002 stale, while ADR-2031 D4 requires periodic re-signing without prohibiting refresh of a lagging projection. |
| **M5 — moves** | **CLOSED** | ADR-2031 D8: “There is no `moved_to` tag”; a fork has “fresh membership; nothing carries over”. ADR-2157 D6 likewise prohibits aliases and carries nothing over. |
| **M6 — registry** | **CLOSED** | ADR-2157 D2 makes `room_from_group()` “the only writer” of the binding registry and rejects an absent entry. D1 specifies length-prefixed input and a golden vector; D7 and ADR-2160 D2 deny voice to unbound rooms. The vector remains an unverified activation prerequisite. |
| **M7 — bridge grant** | **CLOSED** | ADR-2030 D5: “separate from membership”, “never confers write”, “never appears in 39002”, and “the forum issues and revokes it”. ADR-2157 D10 repeats this and requires denial after revocation. The general revocation-bound defect still applies. |
| **M8 — pinning/audience** | **PARTIAL** | ADR-2031 D1 pins out of band; D6 separates the signing key and holds it in Worker secrets. ADR-2160 D3 binds audience to the configured SFU. But D4 independently selects the client endpoint from 30312 without requiring signature/group/revision validation or equality with server configuration; SFU audience enforcement is not specified. |
| **M9 — provenance** | **CLOSED** | ADR-2030 D9: outbound AGPL is “not evidence of where the code came from”; derived files retain notices and upstream commits; unlicensed repositories are “ideas-only”. This closes the requested policy wording, not verification of future source provenance. |
| **M10 — source offer** | **CLOSED** | ADR-2030 D10 requires a “prominent link, in its footer and on its about page, to the Corresponding Source for the exact deployed commit”; NIP-11 is “a supplement”. Activation should verify complete source/build material and unauthenticated retrieval, not merely a resolving commit link. |

**New and residual findings**

1. **BLOCKER — ADR-2162 D3–D4; ADR-2161 D1–D3, D6: two node keys, no authority mapping.**  
   **Claim:** ADR-2162 gives the mesh service a node federation key, then gives VisionClaw “its own node federation key”. Peer entries contain only one `pubkey`.  
   **Why:** It is unclear which key names dataspaces, signs catalogues, authenticates envelopes and populates `allowed_remote_dids`. Implementations must either reject legitimate messages or invent an unreviewed delegation rule. Rotation also changes URN identity, not merely peer pins.  
   **Fix:** Define one canonical key and signing interface, or explicit separately pinned key roles and their authenticated mapping. Specify rotation’s effect on existing URNs, links and tombstones.

2. **BLOCKER — ADR-2161 D5–D7; ADR-2162 D6: a pinned peer is not authorised to retract every URN.**  
   **Claim:** Receiving “a `retraction` for a URN” triggers tombstoning, without an origin-authority check.  
   **Why:** An otherwise trusted peer can impersonate another home’s withdrawal. `asserting node key` is likewise just a record field unless bound to authenticated authorship. Gift wrapping alone does not establish authority over the subject.  
   **Fix:** Bind authenticated envelope sender to the canonical home authority for each retracted URN. Validate asserting-node identity and local-subject ownership for link notices. Specify recipient, message type, replay handling and durable ordering. Reject notices claiming another node’s authority.

3. **BLOCKER — ADR-2161 D4, D9: the dataspace boundary is not a complete server-side stream contract.**  
   **Claim:** The design describes upgrade authentication, existing visibility filtering and an ACL, but does not specify binding every graph operation to a requested dataspace. Read-only behaviour is stated through client `BackendCaps`.  
   **Why:** A modified client can attempt raw `/wss` queries, subscriptions or mutations outside that projection. Initial-load checks do not cover subsequent nodes, edges, counts, lookups or visibility changes.  
   **Fix:** Bind each server-side subscription to an authenticated viewer and dataspace. Apply **selector ∩ object visibility ∩ ACL** to every output and lookup; reject mutations server-side for this connection mode. Specify live revocation for `dids`, selectors and object visibility as well as `space:` membership.

4. **BLOCKER — ADR-2161 D6–D7: “time-bounded” retraction is bounded only after receipt.**  
   **Claim:** The 24-hour purge begins when a consumer receives a retraction or encounters 410.  
   **Why:** Offline consumers, lost messages and restart gaps can retain references indefinitely. A replayed link notice can resurrect a purged link if tombstones disappear with it. “Erasure … completes once its 410 is live” also omits already-rendered remote content.  
   **Fix:** Require durable retraction delivery/catch-up, acknowledgements or expiring reference leases, and persistent anti-resurrection state. Specify withdrawal from active render stores and reconnection behaviour. Define separate deadlines for home erasure, display removal and reference purge.

5. **BLOCKER — ADR-2031 D4; ADR-2157 D4/D11; ADR-2160 D3/D6: incompatible revocation clocks.**  
   **Claim:** A 60-second guarantee coexists with acceptance until snapshot age exceeds 70 seconds.  
   **Why:** A removal immediately after the last snapshot can remain hidden beyond 60 seconds even with perfectly synchronised clocks. Relative clock error can extend this further. A voice refresh during that window can add another credential lifetime.  
   **Fix:** Define one end-to-end deadline measured from authoritative removal, with an explicit relative-clock-error budget. Make snapshot leases, timers and SFU enforcement fit it. Reconcile the 60-second verification test with the 70-second decisions.

6. **BLOCKER — ADR-2160 D3/D6: credential expiry is not an established-session revocation mechanism.**  
   **Claim:** D6 treats token expiry as the backstop when `RemoveParticipant` is unreachable.  
   **Why:** The supplied design establishes no SFU rule that disconnects an already-connected participant at expiry. Nor does it specify invalidation of a still-valid token after eviction. Stopping retries at expiry can abandon a live participant.  
   **Fix:** Require and test an SFU-enforced session lease or equivalent fail-closed enforcement, including control-plane partitions, token reuse, reconnect and restart. Serialize token issuance with revocation so a racing mint cannot escape a completed kick.

7. **MAJOR — ADR-2031 D2/D4: monotonic timestamps can invalidate fresh state.**  
   **Claim:** Every same-second update increments `created_at`; consumers reject timestamps more than ten seconds ahead.  
   **Why:** A burst of membership changes can advance snapshots beyond the acceptance window. If “current state revision” uses those timestamps, legitimate moderation can then appear backdated.  
   **Fix:** Separate durable admission sequence from wall-clock freshness. Coalesce or schedule snapshot publication within the future-date bound while preserving the revocation deadline. Define the revision used for backdating checks.

8. **MAJOR — ADR-2157 D5/D12 versus ADR-2031 D3: watermark granularity and restart recovery conflict.**  
   **Claim:** VisionClaw uses one “per-room watermark” for 39000 and 39002 and persists no payload.  
   **Why:** A membership heartbeat can suppress valid older metadata. After restart, rejecting equal-timestamp snapshots prevents rebuilding unchanged metadata already represented by the watermark.  
   **Fix:** Persist per-room, **per-kind** revision and event identity. Permit verified rehydration of the exact accepted event without treating it as a newer revision, or persist the necessary signed state under an explicit retention policy.

9. **BLOCKER — ADR-2030 D3; ADR-2031 D4; ADR-2157 D13: cohort lag can be freshly signed indefinitely.**  
   **Claim:** Forum access denies while projection lags, but consumers rely on snapshot age.  
   **Why:** Re-signing the old member set every 30 seconds makes it fresh without making it current. Remote presence, voice and dataspace admission can continue admitting a removed cohort member.  
   **Fix:** Prohibit lease renewal for a lagging projection and communicate invalidation, or carry an authenticated coherence mechanism consumers enforce. Test projection failure while the signer and network remain healthy.

10. **MAJOR — ADR-2161 D2/D4 and expectation D2: catalogue leaks protected counts.**  
    **Claim:** Catalogues include a node-count hint and optional descriptive metadata, published to relays and a well-known endpoint. D2 promises an unauthorised viewer receives nothing “including its count”.  
    **Why:** The catalogue publication rules contain no viewer ACL gate.  
    **Fix:** Omit sensitive hints/descriptions from public discovery or serve them only under the same viewer authorisation. State which existence metadata is intentionally public.

11. **MAJOR — ADR-2161 D2–D4: catalogue replacement and endpoint binding are underspecified.**  
    **Claim:** Acceptance requires a pinned signature, but no catalogue revision, replay, withdrawal or endpoint-equality rule.  
    **Why:** An old signed catalogue can restore withdrawn discovery. The signed endpoint and configured endpoint are competing authorities. A viewer’s self-supplied Origin does not prove that viewer came from a pinned node.  
    **Fix:** Define durable catalogue replacement/withdrawal rules and require an exact configured key-to-endpoint binding. Treat Origin solely as browser-origin policy; use an actual authenticated mechanism if node provenance is required.

12. **MAJOR — ADR-2161 D5; ADR-2159 D1: link resolution and connection scope are ambiguous.**  
    **Claim:** Links resolve through per-source `id↔urn` tables, while the records alternate between one connection per source and one per remote space.  
    **Why:** The same node can occur in overlapping spaces with different ACLs. Unqualified resolution can bind a link to the wrong source or expose existence through a stub.  
    **Fix:** Define connection and subscription identities separately. Bind tables to authenticated source authority and stream generation; authorise link records themselves and define whether inaccessible endpoint identifiers/existence may be disclosed.

13. **MAJOR — ADR-2030 D8; ADR-2158/2159 dependency headers; ADR-2161 activation text: activation gates remain inconsistent.**  
    **Claim:** ADR-2030 still restores NIP-29 advertising after E1 alone. ADR-2158 and ADR-2159 mutually `depends_on` each other. ADR-2161 unconditionally lists ADR-2157 as a dependency while claiming public/dids activation needs neither its registry nor admission.  
    **Why:** There is no unambiguous feature-level activation DAG or complete gate. This repeats round-1 finding 11. No inherent three-repository cycle is necessary, but the current declarations invite one.  
    **Fix:** Separate code dependencies from activation prerequisites; make `space:` conditional; implement the two-client prerequisite and connection layer before the volume trial, or activate them as one tested unit. Restore advertising only after the complete applicable security gate passes.

14. **MAJOR — ADR-2160 D3–D4; ADR-2032 D4: SFU discovery still has two authorities.**  
    **Claim:** Clients use 30312 `service`; servers use configuration.  
    **Why:** There is no explicit mismatch rejection or consumed-event validation contract. Writing an audience claim also does not establish that the SFU validates it.  
    **Fix:** Validate signature, pinned home, group and replacement state; require the discovered endpoint to match configuration; test audience enforcement at the SFU.

15. **MINOR — ADR-2032 D1/D6; ADR-2158 D5: earlier presentation defects remain.**  
    **Claim:** Claims omit home identity; “node’s world frame” lacks a shared transform; fitting “by its smallest axis” does not define containment.  
    **Why:** Same-named groups can collide, different nodes can disagree about placement, and anisotropic content can overflow.  
    **Fix:** Namespace claims by full space identity; define frame ownership/transforms; use the minimum bounds-to-content axis ratio with degenerate-axis handling. Correct ADR-2032’s remaining “one signed event for both membership and layout” statement.

**Activation disposition**

The proposals can remain recorded for revision, but they are **not ready to be accepted as enforceable designs**.

A workable activation order is:

1. Agentbox key-role contract, authenticated control rules, peer projection and kind-map fixtures.
2. VisionClaw public/dids federation, after server-side stream isolation, catalogue and erasure gates pass.
3. Forum authoritative state, coherent cohort projection, freshness and read grants.
4. VisionClaw `RoomAdmission`, then `space:` dataspaces.
5. Voice, only after end-to-end SFU revocation tests pass.

The XR connection/volume work can proceed independently once its dependency cycle and prerequisite test are resolved. Forum rendering of dataspace references consumes federation; it must not become federation’s prerequisite.

Shared-memory search and storage were attempted but denied by the tool’s approval policy; no audit result was persisted.