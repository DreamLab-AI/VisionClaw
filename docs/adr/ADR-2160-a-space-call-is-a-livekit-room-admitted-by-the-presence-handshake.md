---
id: ADR-2160
title: A space call is a LiveKit room admitted by the presence handshake
date: 2026-10-30
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: e74bcddc772d47a45b1971156a2bab540eaa027c
verified_paths: []
owner: jjohare
review_trigger: the operator lifting the voice deferral; the first code that mints a LiveKit token; ADR-2157 not being accepted; the forum relay publishing 30312 events; the LiveKit media transport landing on any XR target; a Rust SFU reaching production use in the estate
repo: visionclaw
domain: IDENTITY-authority-chain
depends_on: [ADR-2157, ADR-2019]
amends: []
relates: [ADR-2158, ADR-2159]
---

# ADR-2160 — A space call is a LiveKit room admitted by the presence handshake

Plan: `docs/proposals/spatial-rooms/PLAN.md` §4 (VisionClaw, ADR-2160 entry), §6 E10; evidence in `research/03-sfu-apps-agents.md`. The forum's NIP-53 events are produced by the nostr-rust-forum repository.

## Context
- `SpatialVoiceRouter` holds routing maths and a per-avatar position map; the media transport that would consume it is not wired on any built target (`xr-client/rust/src/webrtc_audio.rs:1-5`; `docs/XR-client.md`, "LiveKit / spatial voice incomplete").
- The server has LiveKit settings including an API secret (`crates/visionclaw-domain/src/config/services.rs:159-168`) and a per-user LiveKit participant id in the audio router (`src/services/audio_router.rs:39-40`, `:420-424`). No code mints a LiveKit token.
- Poses already flow on `0x43` per room, so the server knows who is where (`docs/PROTOCOL-registry.md`).
- Obelisk's SFU authorises with a Nostr challenge, sends SDP and ICE as cleartext relay events and left NIP-29 membership gating disabled (research 03, §1.4). The code is unlicensed, so only the observations are used.
- NIP-53 kind 30312 ("Interactive Room") carries a `service` URL and Host/Moderator/Speaker roles; the forum publishes one per space (plan §4, forum ADR-2032). This ADR allocates no event kind.
- Spatialising a voice needs the speaker's pose; the client already receives it.

## Decision

**Deferred (operator, 2026-10-30): voice is not allowed.** No LiveKit token is minted for any room, bound or unbound; the token mint, the LiveKit server-API client and the reconciler are not built; the `LiveKitSettings` config stays inert. The decisions below are the agreed design for when the operator lifts the deferral, which is a recorded operator decision, not a side effect of activating any other ADR. Until then ADR-2157, ADR-2161 and the forum ADRs activate without voice.

A space's call is a LiveKit room, and the presence handshake is the only way into it.

1. **Room name.** The LiveKit room name is derived from the room URN (ADR-2157) by a pure, fixed mapping pinned by a test vector. The same space is the same call on every client.
2. **One credential path.** VisionClaw mints a LiveKit token **only after** the presence handshake and `RoomAdmission` have passed for that DID and room. There is no other route to a token. A token is **never** minted for an unbound (`sha256-12`) room or for a `nip29-` URN absent from ADR-2157's room binding registry.
3. **Token shape and SFU binding.** LiveKit does **not** enforce an `aud` claim, so no audience is relied on. The SFU binding is custody: each SFU has its own API key and secret, held only by that SFU and VisionClaw, and the key is the token's `iss`. A token minted for one SFU fails signature verification at any other. Grants are room-scoped only: join that one room, with publish and subscribe as the member's role allows, and no `roomAdmin`, `roomCreate` or `roomList`. Token TTL is **at most L = 60 s** (ADR-2157). A client that stays in the room obtains a fresh token only while `RoomAdmission` still passes for its DID; each refresh re-runs it. A removed member is never issued a refreshed token once the removal is observed by `RoomAdmission` or their snapshot is stale; before then a cached, still-admissible pre-removal snapshot can authorise a refresh, which D6 bounds. A token is minted only on a **fresh** `RoomAdmission` (a snapshot within L + S). LiveKit cannot revoke an issued token, so the revocation bound for voice is weaker than for presence (decision 6, final audit #7).
4. **Discovery and SFU authority.** Server configuration is authoritative for the SFU and is what minting uses. The client takes the endpoint from the `service` tag of the home relay's NIP-53 30312 event for the space (`d` = group id), but only if the tag equals the configured URL **and** the event passes validation: signature, pinned home key, group, and latest `rev`. Otherwise the client refuses it.
5. **Spatialisation.** HRTF runs on the client, from `0x43` poses. This completes `SpatialVoiceRouter`. The SFU forwards streams and does no spatial processing.
6. **Leaving and removal.**
   - A **voluntary** disconnect from the presence room starts a 25 s grace timer, after which VisionClaw calls `RemoveParticipant` (the call does not drop by itself). The leave deadline is **persisted**, so a restart honours it; if it cannot be recovered, VisionClaw removes the participant conservatively after restart (at once, if the member is no longer admitted or present). Token expiry is never the deadline.
   - A **removal, ban or kick** (including a 39002 gone stale under ADR-2157 decision 4) calls the LiveKit server API `RemoveParticipant` for that identity in that room, at once. The 25 s grace never applies to it and never delays it.
   - **Token expiry is not a revocation mechanism.** LiveKit checks a token at join and does not eject a connected participant when it expires. So VisionClaw runs a **voice reconciler**: every C = 10 s it lists the participants of each bound room through the LiveKit server API and calls `RemoveParticipant` for anyone not currently admitted. It retries until the participant is gone.
   - **Voice bound (final audit #7, operator ruling): D_voice = 2L + 2S + C = 150 s** from removal at the home. The latest token a removed member can hold is minted while a pre-removal snapshot is still admissible: measured age at most L + S, which with a consumer clock up to S behind the home is at most L + 2S = 80 s of real time after removal. It expires L = 60 s later. From the moment the reconciler observes the removal (a fresh post-removal 39002) or the snapshot's staleness, every rejoin with that token is removed within C; before that point the reconciler has no grounds to remove it, which is why the bound is D_voice and not D. After D_voice, rejoin is impossible. Presence and dataspace streams keep D = 90 s with rejoin refused outright; the E5 expectation for voice reads "removed within C until D_voice", not "rejoin is refused". The partition exception below applies on top.
   - **Serialised mint.** Token minting and revocation take the same per-room lock, so a mint racing a kick cannot escape it.
   - **Partition.** If the LiveKit API is unreachable for longer than D = 90 s (ADR-2157), minting stops for every room and an alarm fires. Participants already connected stay unenforced until the API returns. That is an **accepted residual risk that needs operator sign-off at activation**.
7. **Signalling.** SDP and ICE never travel as cleartext relay events. They go over the LiveKit signalling channel, which the token authorises.
8. **Server.** LiveKit stays the SFU. Rust SFUs (`str0m`, MIT OR Apache-2.0) are noted for later and not chosen.
9. **Scope.** This covers the VisionClaw node. Codescape serves its own voice route (`docs/XR-client.md`, "Codescape as a second server") and is not changed by this ADR.

Rejected alternatives:
- **Nostr challenge straight to the SFU, no server-minted token** (obelisk-sfu's approach). It would need a second membership check at the SFU and a second credential path; the handshake already proves the key.
- **Build an SFU on `str0m`.** We would write rooms, forwarding, congestion control and recording ourselves. LiveKit already ships client SDKs for Android, Rust and Unity.
- **Signalling through relays** (obelisk's 25050). Cleartext SDP and ICE leak addresses to relay operators; wrapping them (NIP-44 or NIP-59) adds latency and a second state machine.
- **Instant drop on a voluntary leave.** A flaky headset link would cut the call on every reconnect, so voluntary leaves keep 25 s. Kicks get no grace.
- **Rely on token expiry alone for removal.** LiveKit does not eject on expiry, so it leaves a removed member in the call indefinitely. Lost to `RemoveParticipant`, the reconciler and a short TTL.
- **Bind the token to the SFU with an `aud` claim.** LiveKit does not enforce one. Lost to per-SFU key and secret custody.

## Consequences
- **Easy:** membership removal ends both presence and voice through one gate; no second user database; the same URN names the room, the call and the cache entry.
- **Hard:** the token mint becomes a security-critical new component holding the LiveKit API secret, and now also lists participants and calls the LiveKit server API on a 10 s cycle. No LiveKit API client exists today. It is new server code and goes through the secrets and fail-closed review in `docs/SECURITY-profiles.md`. The 25 s grace (voluntary leave only) needs a persisted leave deadline, or conservative removal after a restart, and an actual `RemoveParticipant` call; token TTL is not an enforcement deadline.
- **Revocation bound.** A kicked member is out of the call when `RemoveParticipant` succeeds. For voice the guarantee is the weaker **D_voice = 2L + 2S + C = 150 s** from authoritative removal (ADR-2157), not D = 90 s, because an unexpired token can still rejoin and the reconciler removes the participant again within C, repeatedly, until that token has expired. A removed member is never issued a new token. Expiry ends nothing already connected. The LiveKit-partition exception applies on top.
- **Foreclosed:** a space call without a presence room; unauthenticated listen-only access (a later ADR if wanted).
- **Dependency:** none of this is live before ADR-2157 is accepted and implemented, because the mint calls `RoomAdmission`.
- **Counter-examples that block activation (E10):** a token minted without a passing `RoomAdmission`; a token minted for an unbound or unregistered room; a token with a TTL above L, a `iss` key other than the configured SFU's, or a grant beyond one room; a removed member still in the call after D_voice = 150 s while the LiveKit API is reachable, or a rejoin before then that is not removed within C (during a partition the accepted exception of D6 applies, and the member must be removed within C of the API returning); a kick delayed by the 25 s grace; a refreshed token issued to a removed member; a mint racing a kick that escapes it; a 30312 `service` that differs from the configured URL, or fails validation, being used. The activation test covers LiveKit API partition (including minting stopping after D), reconnect, VisionClaw restart and reuse of an unexpired token. The handler test is scoped to the VisionClaw node.
- **Revisit** if LiveKit's licence or hosting model changes, if a call needs to outlive the presence room (broadcast rooms), or if an end-to-end encrypted call becomes a requirement (LiveKit E2EE is not covered here).
- **Follow-on:** `docs/XR-client.md` links its LiveKit gap to this ADR; `docs/IDENTITY-authority-chain.md` records the proposed mint.

## Activation prerequisites
`depends_on` lists code dependencies only. This is the last step of the cross-repo order (`AUDIT-DECISIONS.md` C9):
- ADR-2157 and ADR-2161 `space:` are active, with their tests passing.
- The forum's **ADR-2032 30312 publisher** is live (decision 4 requires a validated home-signed 30312 endpoint). It goes live after the forum chain of step 3 (ADR-2030, then ADR-2031) and before this ADR (step 5): the forum order for voice is 2030, 2031, then 2032 D4. It is a prerequisite of voice only, not of `public`/`dids` data-space federation.
- The end-to-end SFU revocation tests pass: kick, reconciler, partition, reconnect, restart and token reuse.
- The operator signs off the accepted residual risk of an unenforced reconciler during a LiveKit API partition.

## Verification
`implementation_status: none`. At `e74bcddc772d47a45b1971156a2bab540eaa027c`:
- Transport absent, routing maths present: `xr-client/rust/src/webrtc_audio.rs:1-5` (`SpatialVoiceRouter` at `:141`, core at `:43`).
- LiveKit configuration present, no mint: `crates/visionclaw-domain/src/config/services.rs:159-168`; a search of `src/` and `crates/` for `AccessToken`, `livekit_api` and `api_secret` finds only configuration fields.
- Audio router references a participant id only: `src/services/audio_router.rs:39-40`.
- Crate licence (`str0m` MIT OR Apache-2.0) is from `research/03-sfu-apps-agents.md` §2, read 2026-10-30; not re-checked.
