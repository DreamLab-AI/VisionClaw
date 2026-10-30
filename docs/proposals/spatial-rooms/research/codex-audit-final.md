1. **WORDING — PLAN retains the superseded revocation clock.**  
   **Files and quotes:** PLAN §4 says “a snapshot older than R is treated as stale” and “Token TTL is at most R”; E4 repeats “older than R”; E5 requires “within R = 60 s”; the activation checklist repeats “R = 60 s”. AUDIT-DECISIONS C1 instead fixes **L = 60 s, S = 10 s, C = 10 s, D = 90 s**; C4 fixes token TTL ≤ L.  
   **Fix:** Use **L + S = 70 s** for snapshot freshness, **L** for token TTL, **C** for rechecking and **D** for end-to-end revocation. Do not mechanically replace every `R` with `D`.

2. **WORDING — Unqualified revocation guarantees contradict the accepted LiveKit partition exception.**  
   **Files and quotes:** PLAN E5 promises presence and voice revocation “including across a partition”. Forum BASELINE’s federation paragraph promises “closure everywhere”; its pending invariants require removal closed “everywhere within D = 90 s”. Forum IDENTITY-keys-and-trust also states an unqualified “end-to-end revocation guarantee”. ADR-2031 D4, ADR-2157 D4 and ADR-2160 D6 expressly exempt voice enforcement during a LiveKit API partition.  
   **Fix:** Qualify every estate-wide guarantee: presence and dataspace streams remain bounded by D; voice requires a reachable LiveKit API, with the documented recovery requirement and operator sign-off.

3. **WORDING — Snapshot expiry is confused with session-closure time.**  
   **Files and quotes:** ADR-2157 Consequences says an outage “closes bound rooms within L + S (70 s) of the last good snapshot”. Its D4 permits session checks “every C”, while ADR-2031 D4 budgets freshness expiry **plus C plus margin**.  
   **Fix:** Say the snapshot becomes inadmissible after L + S; existing sessions close on the next check, within C thereafter. Retain D as the end-to-end guarantee.

4. **WORDING — PLAN still requires a LiveKit audience claim.**  
   **Files and quotes:** PLAN E10 prohibits a token “with an audience other than the configured SFU”. ADR-2160 D3 says “LiveKit does not enforce an `aud` claim”; AUDIT-DECISIONS C4 binds the SFU through its API key, `iss`, and secret.  
   **Fix:** Replace the audience test with per-SFU signing-key/`iss` validation, room-scoped grants and TTL ≤ L.

5. **WORDING — PLAN grants the disconnect grace too broadly.**  
   **Files and quotes:** PLAN §4, ADR-2160 summary: “leaving the room starts a 25 s grace timer”. ADR-2160 D6 and AUDIT-DECISIONS B3 explicitly restrict grace to **voluntary** disconnects and require immediate removal attempts for kicks, bans and stale membership.  
   **Fix:** Add “voluntary”; explicitly exclude removal, ban, kick and stale-authority closure.

6. **WORDING — The restart policy incorrectly relies on token expiry to end voice.**  
   **Files and quotes:** ADR-2160 Consequences permits a best-effort grace timer and says a lost timer drops the participant “never later than the TTL”. Its D6 and AUDIT-DECISIONS C4 say token expiry does **not** eject connected participants.  
   **Fix:** Require a persisted leave deadline or conservative removal after restart. Remove TTL as an enforcement deadline; specify an actual `RemoveParticipant` action.

7. **DESIGN — Reusable tokens contradict permanent revocation within D and refusal of rejoin.**  
   **Files and quotes:** PLAN E5 requires “rejoin is refused”. ADR-2157 D4 guarantees closure of every voice participant within D. ADR-2160 Consequences explicitly allows: “An unexpired token can still rejoin, so the reconciler removes the participant again within C.” D3 permits refreshed tokens while cached `RoomAdmission` still passes.  
   **Conflict:** With a withheld removal, a token can be issued shortly before the membership lease expires and remain usable beyond D. A participant can therefore rejoin after the deadline. The mint/revoke lock does not invalidate an already issued credential.  
   **Fix:** Decide between an SFU admission/revocation mechanism that prevents post-deadline rejoin, or a weaker guarantee allowing repeated unauthorised intervals of up to C until outstanding tokens expire. The latter changes the binding D/rejoin requirement and needs an explicit ruling.

8. **WORDING — Several summaries prohibit the timestamp tie-breaker required for heartbeats.**  
   **Files and quotes:** ADR-2030 D1 calls `rev` “the only ordering key”. VisionClaw PROTOCOL-registry says consumers order by it “never by `created_at`”; IDENTITY-authority-chain says “not `created_at`”; forum IDENTITY-keys-and-trust says “never `created_at`”. ADR-2031 D2 and ADR-2157 D5 require ordering by **(`rev`, `created_at`)**, with identical content for equal-revision heartbeats.  
   **Fix:** Distinguish state revision from snapshot replacement: `rev` orders state changes; `created_at` orders identical equal-revision heartbeats.

9. **DESIGN — Revision-ordered 30312 and catalogue events conflict with the storage rule for non-3900x kinds.**  
   **Files and quotes:** ADR-2031 D7 says “Other replaceable kinds keep NIP-01 ordering”; its verification and forum BASELINE repeat that rule. ADR-2032 D4 requires the highest-`rev` 30312. ADR-2161 D2 requires highest-`rev` kind-30078 catalogues, including withdrawal replacements.  
   **Conflict:** Both 30312 and 30078 fall outside the storage exception. A timestamp-ordered relay can discard a higher-revision event, including a same-second withdrawal, while retaining a lower-revision one. Consumer-side revision checks cannot recover an event the relay discarded.  
   **Fix:** Decide and specify revision-aware storage for 30312 and the exact catalogue d-tag namespace, or a publication protocol that guarantees NIP-01 replacement preserves revision order. Do not change all 30078 namespaces indiscriminately.

10. **WORDING — The same-second snapshot explanation contradicts the emission limit.**  
    **Files and quotes:** ADR-2031 Consequences says “a same-second pair of snapshots is possible only across a removal slot”. D2 says “at most one snapshot per kind per group per second”; verification tests enforce that limit. AUDIT-DECISIONS accepts removal in the next one-second slot.  
    **Fix:** Delete the same-kind same-second scenario. Explain the need for revision ordering using admission order and clock behaviour without contradicting the emission limit.

11. **WORDING — The clock-skew explanation reverses the delayed-expiry direction.**  
    **Files and quotes:** ADR-2031 Consequences says skew “extends revocation by up to S (mirror ahead of the home)”. D4 defines age as `now − created_at`.  
    **Fix:** A fast mirror clock makes snapshots appear older and expire earlier; a slow mirror clock delays age-based expiry. Separately describe future-date rejection when the home appears too far ahead.

12. **WORDING — PLAN uses the obsolete claim d-tag.**  
    **Files and quotes:** PLAN §4 specifies `visionflow:claim:<group>:<spot>`. ADR-2032 D6, forum BASELINE’s d-tag table and AUDIT-DECISIONS C10 require `visionflow:claim:<home-key-hex>:<group>:<spot>`.  
    **Fix:** Add the canonical lowercase home key everywhere the claim namespace is specified.

13. **WORDING — Claims promise instantaneous remote removal despite leased membership.**  
    **Files and quotes:** ADR-2032 D6 says “A claimant removed from the group loses their claims at once.” ADR-2031 D4 permits remote consumers to retain an apparently fresh pre-removal membership snapshot until lease expiry.  
    **Fix:** State that claims disappear immediately when the consumer observes removal or loses fresh membership authority. Do not promise instantaneous propagation from the authoritative home.

14. **WORDING — PLAN rejects membership requests that the ADR routes through the cohort.**  
    **Files and quotes:** PLAN’s ADR-2030 summary says the relay “rejects manual 9000/9001”. ADR-2030 D3 routes the specified admin events through the cohort authority and never applies them directly. PLAN E5b also omits 9005, although AUDIT-DECISIONS B4 and ADR-2030 explicitly cover it.  
    **Fix:** Summarise the complete per-event routing/refusal policy, distinguishing rejection of a direct group mutation from acceptance through the cohort path. Include 9005 in the test inventory.

15. **WORDING — Governing documents flatten two different membership-authority cases.**  
    **Files and quotes:** Forum IDENTITY-keys-and-trust says “Group admission is derived from zone cohorts”. ADR-2030 D3 says standalone groups take membership from “their own NIP-29 event stream”. VisionClaw DATA-authority-erasure and IDENTITY-authority-chain name the home relay/member list as membership authority without the cohort distinction.  
    **Fix:** State both layers: zone cohorts author membership for zone-backed groups; standalone groups use the home’s admitted event stream. The home signs the authoritative projection consumed by VisionClaw.

16. **WORDING — VisionClaw categorically forbids bridge membership that the forum explicitly permits for posting.**  
    **Files and quotes:** ADR-2157 Foreclosed includes “treating the bridge key as a member or admin”. ADR-2030 D5 says the bridge’s kind-9 post requires it “to be a member of the target group”, at the forum’s discretion.  
    **Fix:** Foreclose **inferring membership or administration from the read grant**. Preserve separately granted membership for posting; the read grant itself adds no 39002 entry or write authority.

17. **WORDING — A relay-address change is ambiguously treated as a fork.**  
    **Files and quotes:** ADR-2157 D6 says “A change of home key or relay is a fork”. ADR-2031 D1 and PLAN §3 say “Renaming the host does not change the space”; identity is `(home key, group id)`.  
    **Fix:** Define a fork by a changed authoritative identity, not a hostname or endpoint change under the same pinned key.

18. **WORDING — ADR-2157 permits refusing the XR default instead of performing the binding migration.**  
    **Files and quotes:** ADR-2157 D7 allows P2 to “move it to a bound room or accept the refusal”. AUDIT-DECISIONS’ VisionClaw ruling says “Before `bound_only` becomes true, the XR default moves to a bound room”; PLAN P2 requires that migration.  
    **Fix:** Remove “or accept the refusal” unless the binding operator ruling is explicitly amended. Keep compatibility presence before P2, `bound_only = true` at activation, and the configurable default cap of 32.

19. **WORDING — PLAN’s source table contradicts its licence policy.**  
    **Files and quotes:** PLAN §1 restricts obelisk-relay to “read for edge cases only” and obelisk-agents to “Ideas only (grant unverified)”. PLAN §2 permits attributed obelisk-relay pieces and says obelisk-agents code “may be lifted with its MIT notice”; ADR-2030 D9 adopts that permission.  
    **Fix:** Update the source table to the adopted policy. Distinguish declining a wholesale engine port from permitting selected attributed code.

20. **WORDING — PLAN states the wrong volume-fit calculation.**  
    **Files and quotes:** PLAN’s ADR-2158 summary says “a volume scales by its smallest axis”. ADR-2158 D5, ADR-2032 D3, XR-client and AUDIT-DECISIONS C10 require `min_i(bounds_i / content_i)`, ignoring content extents below `1e-6`.  
    **Fix:** Use the exact ratio rule and degenerate-axis exception. The smallest bounds axis alone does not determine the scale.

21. **WORDING — PLAN misstates the layout review trigger and overstates settled placement units.**  
    **Files and quotes:** PLAN says “A fourth volume fires ADR-2135’s review trigger” and describes the arrangement as “in metres”. ADR-2158 D7 says the trigger is a **fourth body**, already reached by three volumes plus the memory cloud; its Consequences leave graph-unit conversion, vertex allocation and yaw unresolved. XR-client preserves those open items.  
    **Fix:** Say E3 itself triggers the fourth-body review. Describe metres as the target presentation unit, with conversion and arrangement unresolved before E3.

22. **WORDING — PLAN’s build order reverses the binding volume/backend dependency.**  
    **Files and quotes:** PLAN §5 gives P3a volume implementation “none (parallel)” and makes P3b concurrent backends depend on P3a. AUDIT-DECISIONS C9 says “2158 depends on 2159”; ADR-2159 D5 requires the two-client test “before any of this is built”; ADR-2158 allows volume work after that test passes.  
    **Fix:** Put the two-client preflight first and make the volume work depend on the required backend capability. Preserve the distinction between implementation sequencing and activation.

23. **WORDING — PLAN’s universal activation checklist contradicts the staged activation DAG.**  
    **Files and quotes:** PLAN §6 says “Nothing is advertised or switched on” until all listed tests, including E4, E5 and E10, pass, covering NIP-29, mirrors, presence and calls. AUDIT-DECISIONS C9 and the ADR activation sections activate forum 2030/2031 before presence and voice, while `public`/`dids` federation activates earlier still.  
    **Fix:** Replace the universal gate with feature-specific gates. Restoring NIP-29 requires the applicable forum tests and coherence check, not an already operating downstream voice service.

24. **WORDING — The E3 presence reference introduces an excluded dependency.**  
    **Files and quotes:** ADR-2159 D3 identifies the trial’s presence socket as “the VisionClaw room (ADR-2157)”. ADR-2158 activation says E3 “needs nothing from ADR-2157, 2160 or 2161”; PLAN says it runs on today’s backends without waiting for the forum.  
    **Fix:** Identify the trial’s existing authenticated presence room explicitly. Make bound-room admission conditional on the later ADR-2157 rollout.

25. **WORDING — Voice activation does not explicitly name its required 30312 producer.**  
    **Files and quotes:** ADR-2160 D4 requires a validated home-signed 30312 endpoint. ADR-2032 D4 creates that publisher. ADR-2160’s activation list names ADR-2157 and ADR-2161 `space:`, while the estate order repeatedly abbreviates the forum chain to “2030 then 2031”.  
    **Fix:** Include the ADR-2032 30312 publication capability as a voice activation prerequisite and show where it becomes live. It need not become a prerequisite for `public`/`dids` federation.

26. **WORDING — PLAN uses the wrong `space:` ACL value.**  
    **Files and quotes:** PLAN’s federation summary gives `space:<NIP-29 space>`. ADR-2161 D4 and VisionClaw PROTOCOL-registry require `space:<nip29-room-urn>`, resolved through ADR-2157’s registry.  
    **Fix:** Use the exact registered room-URN form. Do not leave implementers choosing between a group id, `host'id`, a key/group tuple and a room URN.

27. **WORDING — “Only id and ACL kind” excludes fields the catalogue contract also requires.**  
    **Files and quotes:** ADR-2161 D2 says a public catalogue entry “carries only the space id and the `acl` kind”, but the same decision requires `d`, `rev` and `endpoint`, or withdrawal `status`. PLAN, AUDIT-DECISIONS C8 and VisionClaw PROTOCOL-registry repeat the “only” restriction.  
    **Fix:** Apply the restriction to descriptive metadata. Explicitly retain the required identity, routing, revision, signature and withdrawal fields.

28. **WORDING — Agentbox’s endpoint rule omits the withdrawal exception.**  
    **Files and quotes:** ADR-2162 D3 says “A catalogue whose signed endpoint differs from it is rejected.” ADR-2161 D2 requires endpoint-less withdrawal replacements and explicitly accepts them on pinned signature and greater revision alone.  
    **Fix:** Require exact endpoint equality for active entries; require no endpoint for a valid withdrawal. Apply the distinction in agentbox’s validation and tests.

29. **WORDING — The control-envelope type names disagree.**  
    **Files and quotes:** ADR-2161 D6 defines exactly `dataspace_announce`, `link_notice` and `retraction`. ADR-2162 D7 instead lists “retraction, link notice, catalogue”. The catalogue itself is separately defined as a signed kind-30078 event.  
    **Fix:** Use the exact three envelope names in both contracts and define how `dataspace_announce` references or carries the separately signed catalogue. Align the signer allowlist with that distinction.

30. **DESIGN — Link assertions are simultaneously sole-store records and disposable projections.**  
    **Files and quotes:** ADR-2161 D5 says links are owned by the asserting node and “stored in the asserting node’s own store”. D7 calls every link record “local derived data, a disposable projection” that is “dropped without loss”. VisionClaw DATA-authority-erasure assigns the asserting node’s own stored link the same disposable classification.  
    **Conflict:** An original assertion stored only at its asserting node cannot be discarded without losing that assertion. A received reference or projection can be disposable, but it is a different data class.  
    **Fix:** Decide which records are authoritative assertions and which are derived references. Specify their respective stores, renewal authority, erasure and reconstruction rules. If original assertions themselves are intentionally leased and disposable, explicitly accept their loss and remove the claim that they are losslessly reconstructible projections.

31. **WORDING — The governing data rule forbids the in-memory render stores the ADRs require.**  
    **Files and quotes:** VisionClaw DATA-authority-erasure says no consumer copies remote “nodes, edges, owners or positions into any store, cache or backup”. ADR-2159 D1 requires a render store per connection; ADR-2161 D5 and D7 expressly use viewer render stores.  
    **Fix:** Prohibit durable storage, server-side import and backups of remote graph content. Explicitly permit disposable session-memory render stores needed for viewing.

32. **WORDING — The watermark changelog describes the obsolete tuple.**  
    **Files and quotes:** VisionClaw DATA-authority-erasure’s new changelog records a per-kind “`(rev, created_at, id)` watermark”. Its proposed section, ADR-2157 D5, ADR-2031 D3 and AUDIT-DECISIONS closure require `(rev, created_at, event id, content digest)`.  
    **Fix:** Include the digest in the changelog. Keep the distinct catalogue watermark `(rev, event id)` clearly separate from group-state watermarks.

33. **WORDING — Binding references point to nonexistent or wrong sections.**  
    **Files and quotes:** AUDIT-DECISIONS C9 places activation prerequisites in “PLAN §5a”, which is absent. ADR-2030 D8 points to “E1 to E6 of PLAN §5”, but the expectations are in §6. ADR-2161 D1, IDENTIFIER-taxonomy and AUDIT-DECISIONS identify the closed reference set as “ADR-2032 D1”; the actual closed-set rule is D2.  
    **Fix:** Correct the section references and give the staged activation gate a real, consistently referenced location.

VERDICT: design-open