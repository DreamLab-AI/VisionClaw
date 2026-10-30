Check 6 (code citations) skipped as requested; this audit uses only the supplied text.

**BLOCKER**

1. **Files: ADR-2030, ADR-2031 — “Home orders” by client-controlled `(created_at, id)`.**  
   **Why:** A seven-day-backdated moderation event can change the fold before already-authorised actions. Sorting provides deterministic replay, not authoritative admission order. Separately, successive home snapshots within one second can have decreasing precedence: the later removal loses if its event id is higher. Verbatim replication preserves this error.  
   **Fix:** Specify a durable home admission sequence and authorisation-at-admission rule; prevent late events from retroactively acquiring authority. Emit snapshots with strictly increasing replacement timestamps, coalescing or delaying same-second updates. Test backdated admin grants/removals and same-second membership changes.

2. **Files: ADR-2031, ADR-2157, PLAN E4–E5 — Signed cached membership allegedly ensures fail-closed removal.**  
   **Why:** Signatures prove origin, not freshness. Mirrors explicitly serve during outages without a stale-state deadline; existing presence sessions may remain indefinitely. Replayed snapshots can look freshly received after restart. An admission can also race a removal before the actor registers. “One subscription round trip” gives no bound during partitions.  
   **Fix:** Define a numerical revocation bound, authenticated freshness/lease mechanism, persisted revision watermark and coherent snapshot handling. Expire existing sessions and mirror read/write authority when freshness cannot be established. Make admission and session registration revision-safe; test partitions, replay after restart and concurrent removal.

3. **Files: ADR-2160, ADR-2157 — “Membership removal ends both presence and voice through one gate.”**  
   **Why:** Closing presence does not revoke an SFU credential or disconnect media. Token expiry alone does not establish a bound on an already-connected participant. The 25-second timer, reconnect behaviour and kick handling are unresolved; E10 tests issuance only.  
   **Fix:** Distinguish voluntary disconnect from removal. On removal, enforce server-side SFU eviction, prevent token reuse/rejoin and stop refresh immediately. Specify grants, lifetime, restart-safe enforcement and an end-to-end revocation deadline. Make these activation gates.

4. **File: ADR-2030 — Zone cohorts are the “one membership authority”.**  
   **Why:** Rejecting manual 9000/9001 does not close 9021 joins, 9022 leaves, 9009 invitations or other membership-changing paths. Cohort projection ordering, failure and lag are unspecified. A removed cohort member can retain access through stale projected state.  
   **Fix:** Enumerate every mutation path for zone-backed groups; reject or route each through the cohort authority. Commit removals and their enforcement atomically, or deny access while projection is behind. Test invitations, joins, leaves, replay and projection failure.

**MAJOR**

5. **Files: ADR-2031, ADR-2157 — A final `moved_to` is enough to move membership and rooms.**  
   **Why:** No finality rule, destination acceptance, state handover, cutover revision, alias-cycle handling or old-session policy exists. An ordinary replaceable 39000 is not inherently final. Following an alias could inadvertently carry old membership into a destination with different membership. A fork is a new identity, but copied authority is unspecified.  
   **Fix:** Define a two-home authenticated handover with frozen old revision, explicit destination group and membership snapshot, bounded acyclic aliases, and re-admission at the destination. Revoke old voice/presence authority at cutover. Treat fork membership as separately authorised.

6. **Files: ADR-2157, ADR-2160 — Typed minting prevents hand-typed bound room IDs; all calls pass membership admission.**  
   **Why:** A parser accepts client-supplied bytes regardless of which constructor produced them. The hash cannot recover its group input. No authoritative URN-to-group registry is specified, while legacy rooms remain open and the voice rule does not explicitly exclude them.  
   **Fix:** Resolve bound URNs only through a server-controlled mapping to pinned home/group identities. Reject unknown IDs and constrain aliases. Explicitly deny space-call tokens to unbound rooms, or define a separate policy that cannot reach a bound call.

7. **Files: ADR-2030, ADR-2157 — Private-group subscriptions need a bridge that “carries no user authority”.**  
   **Why:** ADR-2030 restricts membership lists to members; ADR-2157 both forecloses bridge membership and says it needs privileged membership. NIP-42 authentication alone supplies no read permission. Private-space admission therefore has no consistent implementable path.  
   **Fix:** Specify a narrowly scoped service read grant for selected groups, separate from membership/admin powers, or explicitly change the membership policy. Gate private-space activation on this grant and test denied reads.

8. **Files: ADR-2031, ADR-2032, ADR-2160 — Pinned signatures establish placement and call discovery.**  
   **Why:** Initial pin establishment is unspecified; accepting whatever `self` an arbitrary endpoint advertises permits identity substitution. The admission adapter does not explicitly require signature, author, group and replacement validation. Client discovery and server token minting can also select different SFUs.  
   **Fix:** Define trusted space provisioning and pin persistence. Validate every consumed state/discovery event against its pinned identity and revision rules. Bind token audience, configured SFU, discovered service and room identity; reject mismatches.

9. **Files: PLAN §2, ADR-2030 — AGPL designation resolves source exposure; a fresh implementer enables permissive publication.**  
   **Why:** Reading code does not automatically make a new implementation derivative. Conversely, choosing AGPL does not cure copied expression, missing notices or incompatible upstream rights. An implementer who avoids the source can still receive derivative material through reports.  
   **Fix:** Describe AGPL as the chosen outbound licence, not proof of provenance compliance. Maintain an expression/provenance review, including upstream headers and report contents. For permissive publication, require documented independent implementation from screened specifications; implementer identity alone is insufficient.

10. **Files: PLAN §2.6, ADR-2030 — NIP-11 `source_url`, licence and commit satisfy AGPL §13.**  
    **Why:** These fields alone do not establish a prominent offer to all remote users or access to the deployed version’s complete Corresponding Source. Cloudflare Worker deployment does not itself remove the network-source obligation where §13 applies.  
    **Fix:** Provide a prominent, usable source offer through the actual user interaction surfaces, backed by the exact deployed source and required build/install material, including modifications. Test unauthenticated retrieval and source-to-deployment correspondence. Do not treat field presence as the compliance test.

11. **Files: PLAN §6, ADR-2030 verification, ADR-2157, ADR-2160 — Activation criteria establish the security claims.**  
    **Why:** The plan omits ordering E3 and removal E5 from blocking counterexamples; ADR-2030 restores NIP-29 advertising after E1 alone. Known unresolved revocation behaviour can therefore pass the stated gates. “One round trip” and unspecified TTLs cannot establish a measurable removal guarantee.  
    **Fix:** Define a single cross-repository activation checklist covering read isolation, all admission paths, snapshot ordering/freshness, removal and voice revocation, including adversarial outage tests with numerical deadlines.

**MINOR**

12. **Files: ADR-2032, PLAN E11 — Placement is canonical; claims identify the winning occupant.**  
    **Why:** Canonical numeric coordinates do not establish a shared world frame. Claims are explicitly vulnerable to backdating and differing reader histories, and their d-tag omits home identity. These are acceptable only as advisory UI, not authoritative occupancy.  
    **Fix:** Define the coordinate frame and distinguish numeric agreement from physical alignment. Namespace claims by full space identity and visibly label provisional occupancy. Keep claims and self-published 10312 presence outside access decisions.

13. **Files: PLAN, ADR-2032, ADR-2158, ADR-2159, ADR-2160 — Several summary claims exceed the detailed records.**  
    **Why:** PLAN calls 13k/20k triangle ceilings; ADR-2159 explicitly corrects them to benchmark counts. ADR-2032 says one event provides membership and layout, although these are 39002 and 39000. ADR-2160 says the forum “publishes” 30312 despite implementation being absent. Runtime-support absence is inferred from limited searches. “Scales by its smallest axis” is insufficient to guarantee box containment.  
    **Fix:** Reconcile the plan with the ADRs; use proposed tense and evidence-qualified runtime statements. Define fit as the minimum per-axis bounds-to-content ratio, with degenerate-axis handling.

**Verdict: BLOCK activation; resolve authoritative ordering, bounded revocation and all membership paths before implementation approval.**