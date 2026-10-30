# Audit decisions: Codex audit round 1 (2026-10-30)

These are the lead's decisions on the cross-vendor audit findings (`codex exec -m gpt-6-astra`, round 1). The audit text is in `research/codex-audit-round1.md`. Every decision here is binding on the ADR revisions. Each ADR must state each relevant decision as enforceable Decision text, and must not merely list it under open items.

## Blockers (activation-blocking until encoded)

**B1. Ordering and authority.** The home relay is the only writer of group state. It applies moderation events (9000–9022) **in its own admission order**, and authorises each one against the state current at admission. A moderation event whose `created_at` is older than the group's current state revision is refused, so backdating cannot rewrite history. The home emits each 3900x snapshot with `created_at` strictly greater than the previous snapshot of that kind, bumping by one second on a tie. Mirrors and VisionClaw replace a snapshot only on a strictly greater `created_at`, and keep a persisted per-group watermark that rejects replays and late-older events. The `storage.rs:97-139` guard is the forum prerequisite.

**B2. Freshness and revocation bound.** The target revocation bound is **R = 60 s**. The home re-signs 39002 at least every R/2 (30 s), even when nothing has changed. Mirrors and VisionClaw `RoomAdmission` treat a 39002 older than R as stale, and stale means deny, including for **already-joined** sessions, which are closed. Admission is checked again after registration, to close the join-versus-removal race. A removal (9001, ban) closes affected sessions within R everywhere. Because the snapshot carries a timestamp, a mirror cannot withhold a removal for longer than R.

**B3. Voice revocation.** A removal or kick calls the LiveKit server API `RemoveParticipant` for that identity in that room. Token TTL is at most R. A removed member never receives a refreshed token. The 25 s presence grace applies only to voluntary disconnects; it never delays a kick.

**B4. One membership authority.** For a zone-backed group, the cohort/zone is the authority and the group is a projection. Every membership-changing event on such a group (9000, 9001, 9009, 9021, 9022) is either refused, or routed through the cohort authority and re-projected. It is never applied to the group directly. If the projection lags, access is denied. Admin Access-tab calls (9000 with `cohort`, 9005) go through the cohort path.

## Majors

**M5. Move deferred.** `moved_to` is removed from v1. A move needs an authenticated two-home handover: a frozen old revision, a destination snapshot, re-admission at the destination, and revocation at cutover. That is a future ADR. A fork is a new identity with fresh membership. Nothing carries over.

**M6. Room binding registry.** VisionClaw keeps a server-side registry mapping each `nip29-` room URN to (pinned home key, group id). Only `room_from_group()` fills it. An unknown or unregistered URN is rejected. The hash input is length-prefixed, and the ADR must give a golden test vector. Voice is offered only for bound rooms; unbound `sha256-12` rooms get presence only.

**M7. Bridge read grant.** The VisionClaw bridge key's read access to private or hidden groups is a **scoped read grant**, separate from membership: it never confers write, never appears in 39002, and the forum issues and revokes it.

**M8. Key pinning and token audience.**
- The operator pins the home key out of band, in config. It is never trusted on first use from NIP-11 `self`, which is advertised for discovery only.
- The relay signing key is a separate key from the admin contact (`pubkey`). It is held in Worker secrets and rotated only by a deliberate re-home (a fork).
- LiveKit tokens bind their audience to the configured SFU URL and carry room-scoped grants only.

**M9. Licence wording.** AGPL-3.0-only is the outbound licence, not proof of provenance. See PLAN §2 (revised 2026-10-30): the operator owns our estate; obelisk-relay AGPL code *may* be ported with attribution, files are marked own or derived, and the unlicensed repos are ideas-only.

**M10. §13.** The source offer is a prominent user-facing link (footer and about page) to Corresponding Source for the exact deployed commit. NIP-11 supplements it.

## Cross-set
- **ADR-2032 D1 `<content-ref>`** is a closed set: `urn:visionclaw:dataspace:<node-key-hex>:<space-id>` (ADR-2161), `urn:visionclaw:codescape:<scene>`, or a relative path on the home's own node. Anything else is ignored by clients.
- ADR-2161 depends on ADR-2157's registry and on ADR-2162 (agentbox) for peers, the node key and the `dataspace` crossing. It must **not** depend on forum ADRs 2030–2032 for activation.

## Rulings on reviser conflicts (round 1)

- **Mirror outage.** Confirmed. After R without a fresh 39002, a mirror denies membership-dependent actions. Reads of groups whose flags do not gate reads are unaffected. Availability yields to revocation, by design.
- **Clock skew.** The skew tolerance is **S = 10 s**. A 39002 is stale when `now − created_at > R + S` (70 s). One with `created_at > now + S` is refused as future-dated. Both the forum mirror and VisionClaw `RoomAdmission` use the same R and S constants. Nodes run NTP; that is an operational prerequisite.
- **30 s re-sign.** Driven by the Durable Object alarm, not the cron. Accepted.
- **Closed ref set extended to `scene`.** Accepted.
- **Peer read vs. viewer read (ADR-2161 D4).** Confirmed. A pinned peer (a signed catalogue, with endpoint and Origin from the pinned set) is necessary but not sufficient. The viewer's own NIP-98 identity plus `acl` decides. A node key never reads on a viewer's behalf.
- **LiveKit binding (ADR-2160 D3).** LiveKit does not enforce an `aud` claim. The SFU binding is the per-SFU API key (`iss`) and secret, which only that SFU and VisionClaw hold. Tokens carry room-scoped video grants only.
- **Unbound `sha256-12` rooms (ADR-2157).** Default: still allowed, for compatibility. They are presence only, never voice, need an authenticated DID, and are subject to the existing cap. `presence.bound_only` defaults to false and is set true when P2 activates. **Confirmed by the operator 2026-10-30.**

# Audit decisions — Codex round 2 (2026-10-30)

Audit: `research/codex-audit-round2.md`. These decisions are binding and **supersede** any conflicting round-1 wording. Finding numbers follow the audit.

**C1. One revocation clock (findings 5, 7, B2).**
- **Admission sequence.** The home keeps an admission sequence. Every 3900x carries `["rev", <u64>]`, the home's monotonically increasing group-state revision. Ordering, replacement, watermarks and backdating checks use `rev`, never `created_at`.
- **`created_at` is wall-clock time and is never bumped.** The home emits at most one snapshot per kind per second, coalescing bursts.
- **A removal is published immediately**, not on the next 30 s tick.
- **Constants:**
  - Snapshot lease L = 60 s: a 39002 is fresh while `now − created_at ≤ L + S`.
  - Relative clock-error budget S = 10 s.
  - Live-session re-check interval C = 10 s.
- **The single end-to-end guarantee is D = 90 s** from authoritative removal at the home to closure of every presence session, dataspace stream and voice participant everywhere. That covers a withheld removal (L + S = 70 s), plus C, plus margin. Every "within R" or "60 s guarantee" statement becomes D = 90 s. Verification tests assert D.

**C2. Cohort coherence (finding 9, B4).** The home signs 39002 only when the group projection is coherent with the cohort authority, recording `["proj", <cohort-rev>]`. If the projection lags or fails, the home **stops re-signing**, so consumers go stale within L + S and deny. A test covers projection failure while the signer and network stay healthy.

**C3. Watermarks (finding 8).** Each consumer keeps a persisted watermark per room *and per kind*, holding (`rev`, event id). It may rehydrate the exact accepted event (same id) after a restart, but never treats it as newer.

**C4. Voice revocation (findings 6, 14, B3).**
- **What LiveKit does and doesn't do.** LiveKit checks tokens at join and does not eject a connected participant when the token expires.
- **Reconciler.** VisionClaw runs a voice reconciler: every C it lists the participants in each bound room through the LiveKit server API, and calls `RemoveParticipant` for anyone not currently admitted.
- **Serialised mint.** Token minting and revocation take the same per-room lock, so a mint racing a kick cannot escape it.
- **Tokens.** TTL ≤ L. A removed member is never re-minted a token.
- **Partition.** If the LiveKit API is unreachable for more than D, minting stops for every room and an alarm fires. The voice reconciler running unenforced during such a partition is an **accepted residual risk that needs operator sign-off at activation**. An activation test covers the partition, reconnect, restart and token-reuse cases.
- **SFU authority.** Server configuration is authoritative for the SFU. The 30312 `service` tag must equal the configured URL and pass validation (signature, pinned home, group, latest `rev`), or the client refuses it. The SFU binding is the per-SFU API key (`iss`) and its secret.

**C5. One node key (finding 1).**
- **One key, held by agentbox.** Each node has exactly one canonical **node federation key**. The agentbox mesh service holds it and exposes a local signing interface (an identity-port-style socket; no new crypto).
- **No VisionClaw key.** VisionClaw holds no federation key of its own. It signs catalogues, notices and retractions through that interface.
- **The key's roles.** It names dataspaces (`urn:visionclaw:dataspace:<node-key-hex>:…`), signs catalogues, authenticates envelopes, and is the `pubkey` in peer entries and `allowed_remote_dids`.
- **Rotation creates a new node identity.** The old key signs retractions for all its URNs before retirement. Links are not migrated in v1.

**C6. Retraction and link authority (findings 2, 4).**
- **Who may retract a URN.** A retraction of `urn:visionclaw:dataspace:<K>:…` is accepted only from an envelope authenticated by K, since the URN names its own authority. A link notice's asserting node must equal the authenticated sender, and the local end of the link must belong to the receiving node.
- **Replay.** Each sender carries a persisted per-sender sequence, and replays are rejected.
- **Links are leases.** Every link record expires after lease T = 7 days, renewed by the home's catalogue refresh. An offline consumer drops a link at lease expiry, whether or not it ever received the retraction. Tombstones persist for T + 1 day as anti-resurrection state.
- **Three deadlines:**
  - home erasure: the 410 is live immediately;
  - display removal: on receipt, or on the next (re)connect, whichever comes first, dropping anything the home no longer serves;
  - reference purge: within min(receipt + 24 h, lease expiry).

**C7. Server-side stream contract (findings 3, 12).**
- **Federated-read connection mode.** At upgrade, the server binds each connection to (authenticated viewer DID, one dataspace). Every output and lookup — nodes, edges, counts, positions, search — is filtered server-side by **selector ∩ object visibility ∩ acl**.
- **Mutations are refused by the server** in this mode. `BackendCaps` is UX only.
- **Live revocation.** ACL, selector and visibility changes are re-evaluated on change and at least every C, and a connection that loses access is closed.
- **Connection identity.** There is one connection per (source node, dataspace). `id↔urn` tables are keyed by (node key, dataspace, stream generation).
- **Link display.** A link is rendered only if the viewer is authorised for **both** ends. Otherwise nothing is drawn, with no stub, so its existence is not disclosed.

**C8. Catalogue (findings 10, 11).**
- **Replacement.** The catalogue is a replaceable signed event carrying `rev`; the latest `rev` wins.
- **Withdrawal.** An entry is withdrawn by removing it from the catalogue and issuing a retraction.
- **Public metadata.** Public catalogue entries carry only the id and the acl kind (`public` / `dids` / `space`). Counts, hints and descriptions for non-public spaces are served only to authorised viewers.
- **Endpoint authority.** Configuration is authoritative: the endpoint must exactly match the configured peer endpoint for that key, and a mismatch is rejected.
- **Origin is browser policy only**, never node provenance.

**C9. Activation DAG (finding 13).**
- **Code dependencies vs activation.** `depends_on` lists code dependencies only. Activation prerequisites are listed separately in each ADR and in PLAN §5a.
- **ADR-2158 and ADR-2159.** 2158 depends on 2159, and 2159 does not depend on 2158.
- **ADR-2161.** Activation for `public` and `dids` needs 2159, agentbox 2162 and C7. `space:` additionally needs 2157 and the forum chain.
- **ADR-2030.** Restoring the `29` advertisement requires the whole applicable gate (E1–E6 and D3 coherence), not E1 alone.
- **Activation order:**
  1. agentbox 2162;
  2. VisionClaw 2159 → 2161 (`public` and `dids`);
  3. forum 2030 → 2031;
  4. VisionClaw 2157 → 2161 (`space:`);
  5. VisionClaw 2160.

  The volume work (2158) can proceed after 2159.

**C10. Minors (finding 15).**
- **Claims.** Claims are namespaced by full space identity (home key + group id).
- **Placement frames.** The descriptor's frame is the home's own frame. Each consuming node applies its own placement transform, and no cross-node frame is shared in v1.
- **Volume fit.** scale = min over axes of (bounds_i / content_i), ignoring any axis whose content extent is below 1e-6.
- **ADR-2032.** Correct its "one signed event for both" statement.

## Rulings on forum round-2 points

- **`rev` storage.** Accepted as a D1 schema migration (an indexed `rev` column); ADR-2031 D7 now says so.
- **One DO alarm.** Accepted. A single alarm scheduler multiplexes idle eviction, the 30 s re-sign and the coherence re-check. A homing DO never idles out; the cost is measured in P1, not a blocker.
- **Removal in the next one-second slot.** Accepted. The D bound covers only `rev`-aware consumers; NIP-01-only clients are outside it, and the estate's own consumers are all `rev`-aware.
- **Interpretations.** Accepted: "D3 coherence" means the C2 coherence test, and 30312 carries the `rev` of the 3900x it derives from.

## Rulings on VisionClaw round-2 points

- **Member cap.** No cap exists in code (`presence_actor.rs:166-171`, `room.rs:42-49`), so "the existing cap" was wrong. Default 32 members per room, configurable, applied to every room, bound and unbound. It is new P2 work. **Operator may override the number.**
- **XR default room.** Before `bound_only` becomes true, the XR default moves to a bound room. Recorded in P2.
- **LiveKit client and `/wss` federated-read mode.** Both are new code, now explicit in phases P5 and P7b.
- **PLAN §5.** The heading was lost in an earlier splice; it is restored, and phase rows now carry the round-2 work.

## Closure (rounds 3-8)

Rounds 3 to 8 were narrow closure checks (`research/codex-audit-round3.md` … `round8.md`). They fixed heartbeat renewal (an equal-`rev` re-sign with identical content and later `created_at`; watermark `(rev, created_at, event id, content digest)`, the digest being SHA-256 over canonical JSON of (`tags`, `content`)), qualified D by the LiveKit-partition exception everywhere, named link subject/object ownership, and made catalogue withdrawal an endpoint-less greater-`rev` replacement. Round 8 verdict: **proposals-ready**. This means the design text is internally consistent; nothing is implemented and every ADR is proposed / none / inactive.

# Final whole-set audit (`research/codex-audit-final.md`): rulings

The audit found 31 items: 28 are wording and are fixed as the audit proposes; three are design decisions, ruled on here.

- **#7 Voice rejoin (DESIGN).** LiveKit cannot revoke an issued token. Ruling: accept the weaker voice bound, stated explicitly.
  - A token is minted only on a fresh `RoomAdmission`, with TTL ≤ L.
  - The latest token a removed member can hold is minted before their snapshot goes stale (≤ L + S after removal) and expires L later.
  - Between those points, every rejoin is removed by the reconciler within C.
  - **Voice bound:** D_voice = 2L + S + C = 140 s from removal at the home. During that window the member may reconnect for at most C at a time. After it, rejoin is impossible.
  - Presence and dataspace streams keep D = 90 s, and rejoin there is refused outright.
  - E5 says "rejoin is refused" for presence; for voice it says "removed within C until D_voice".
  - The LiveKit-partition exception still applies on top.
- **#9 Revision-ordered storage (DESIGN).** Ruling: the D1 `rev` migration (ADR-2031 D7) covers 39000-39003, 30312, and kind 30078 only in the `visionclaw:dataspace:*` d-tag namespace. Other kinds keep NIP-01 ordering. VisionClaw consumers still check `rev`; the relay rule stops a higher-`rev` event being discarded.
- **#30 Link records (DESIGN).** Ruling: two classes.
  - The asserting node's own link record is **primary local data**: it is authored there, sole write-master, erasable by its author and not disposable. Its lease T means "re-announce to peers", not "drop".
  - A received copy (from `link_notice`) at the object's owner, and any viewer render state, is a **foreign reference**: disposable, purged on retraction or lease expiry.
  - DATA-authority-erasure carries both rows.

## Confirmation round (2026-10-30)
- **N1 (arithmetic):** a consumer clock up to S behind the home stretches the admissible window to L + 2S of real time, so the voice bound becomes D_voice = 2L + 2S + C = 150 s. It is corrected everywhere.
- **#7:** "never refreshed" and "every rejoin removed within C" now apply from the moment `RoomAdmission` or the reconciler observes the removal or the staleness. Before that point a cached pre-removal snapshot can still authorise voice. D_voice bounds this.
- **#2, #25, #29:** wording only, fixed (voice taken out of D in ADR-2157; the 30312 publisher goes live at step 3; the signer list includes `dataspace_announce`).

## Operator decisions (2026-10-30, after confirmation round)
- **Cross-volume links:** optional and switchable. `xr.cross_volume_links` defaults off and toggles at runtime; it covers KG↔ontology and federated data-space links (ADR-2158 D6).
- **Federation scope:** same-operator for now; multi-operator is planned. "Planned evolution" notes are added to ADR-2161 and ADR-2162.
- **Member cap:** 32 per room confirmed (ADR-2157).
- **Voice deferred (2026-10-30):** voice is not allowed. ADR-2160 stays proposed as the design for later, but none of it is built: no token mint, no LiveKit API client, no reconciler, no 30312 publisher. The D_voice bound and the partition sign-off only matter once the operator lifts the deferral.
- **Forum encryption deferred:** how encrypted zones relate to NIP-29 spaces is decided at first use of group federation. Until then encrypted zones are not projected, and the in-flight encrypted-zone work is unaffected (ADR-2030 D7).
- **Dark launch:** NIP-29 groups and federation are built in parallel and ship off by default: `NIP29_GROUPS_ENABLED` and `NIP29_FEDERATION_ENABLED`, both `"false"` (ADR-2030 D12, ADR-2031 D12).
