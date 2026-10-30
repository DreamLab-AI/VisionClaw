# 01 — obelisk-sky: spatial model, presence, voice, games (worker `sky-spatial`)

Source: shallow clone `obelisk-src/obelisk-sky` at `3d89866`, vendored obelisk-dex snapshot `eb1a6ee5` (`vendor/obelisk/COMMIT`). Read only; nothing executed. All paths are repo-relative to `obelisk-sky/` unless prefixed with an estate repo. Behaviour is described, not quoted (see Licence).

## 0. Licence findings

| Item | Finding |
|---|---|
| obelisk-sky itself | **No LICENSE file, no `license` field** (`package.json` has `"private": true` only). No licence header in any `client/src` or `server` file found. Treat as all-rights-reserved: idea-level and wire-protocol interop only. |
| Vendored obelisk-dex (`vendor/obelisk/src`, 2,218 files) | No LICENSE file in the snapshot. It only links upstream: `vendor/obelisk/src/constants/legal/contact.ts:18,24` → `github.com/obelisk-app/obelisk/blob/main/LICENSE`. Upstream terms unverified from here. |
| `vendor/nostr-wot-ui-0.8.3-obelisk.2.tgz` | `package.json` inside declares `"license": "MIT"` (read via `tar -xzO`, not extracted). |
| Kenney models (`client/public/models/LICENSE-kenney*.txt`) | CC0. |
| Quaternius characters (`client/public/models/people/README.txt`, `LICENSE-quaternius.txt`) | CC0 1.0 (free Standard kit). |

Protocol facts below (kinds, tags, JSON field names, rates) are interop data. Implementations should be clean-room.

## 1. Spatial model: how a NIP-29 group becomes a place

**One relay = one island.** Sky browses one group relay at a time (`client/src/nostr/hub.ts:18-28`, "single-relay rule"; suggested relays `hub.ts:16`). Group metadata is a plain `kinds:[39000]` REQ to `activeRelay()` (`client/src/nostr/feed.ts:94-100`). It is parsed from the `d`, `name`, `about`, `parent`, `public`/`private`, `hidden`, `open`/`closed` and `t` tags. `t` takes `voice`, `voice-sfu` or `forum`, giving channel kind `text|forum|voice|voice-sfu` (`client/src/nostr/groups.ts:9-60`). Private/hidden groups are not shown (`groups.ts:1-8`).

**The island layout is fixed, not on the wire.**
- `planIsland(seed = 7)` builds an island of radius 18.5 with a plaza at the origin, a river, a pond and mountains (`client/src/world/layout.ts:73-90`). It is called with the default seed (`client/src/world/world.ts:290`), so every relay gets the same terrain.
- Building slots are a greedy fill outward from the plaza over a 0.5 m candidate grid with seeded jitter (`layout.ts:118-142`). The first five slots are 3×3 and the rest 2×2. Every slot's door faces the plaza and has a `yard` in front of it (`layout.ts:44-53,139-142`).
- Landmarks (La Crypta, café, theater, LABITCONF…) reserve slots first (AGENTS.md:136).

**Channel → slot (client-side).**
- Each client runs `World.setGroups(groups, activity)` (`world.ts:339-353`) → `assignByDistrict` (`layout.ts:544-581`):
  - Themed channels go to the free slot nearest their district's point on an 11.5 m ring, busiest first (`DISTRICT_ANGLE`, `layout.ts:425-435`).
  - Plaza channels then fill the lowest free slot index, busiest first.
- `activity` is the count of kind 9 messages per group in the loaded events (`client/src/main.ts:978-982`). The window is 30 days (`client/src/nostr/state.ts:20`), with `limit: 4000` per filter (`feed.ts:150-158`).
- House height follows activity: tiers at 5/20/60 messages, up to 3 floors (`client/src/world/buildings.ts:42-47,67`).
- Existing channels never move once assigned. The map `slotOf` is held in memory only and cleared on relay switch (`world.ts:161,345-353,416,456`). The first assignment comes from the snapshot (`main.ts:885-899`).

**Operator overlay (custom).**
- Kind 30078, `d = obelisk-sky:themes:<relay>`. Tags `["channel", groupId, theme]` / `["category", catId, theme]`, theme ∈ `plaza|sports|gaming|library|stage`.
- Trusted only from the relay's NIP-11 operator pubkey (`client/src/nostr/themes.ts:1-58`).
- It reuses dex's operator-signed `obelisk:layout:<relay>` categories (`themes.ts:37,60-66`).
- Branding is `obelisk:branding:<relay>`, also operator-only (`client/src/nostr/branding.ts:2,39`).
- Special rooms by heuristic:
  - Voice channels become a tree with a campfire instead of a house (AGENTS.md:137).
  - A gym/fitness-named channel becomes the gym (AGENTS.md:140).
  - The "general" chat is picked by name regex (AGENTS.md:130).

**"Being in a room" = standing in its yard.**
- `placeOfPos` resolves to the theater hall's channel, else the nearest yard (`main.ts:1494-1496`; `client/src/ui/livePlaces.ts:1-37`).
- Chat bubbles show only to people at the same place as the speaker. The general chat is the exception (`livePlaces.ts:31-37`; AGENTS.md:130).
- Posting in a channel makes the poster's character walk to that house (README; AGENTS.md:3).

**No event carries a house's position.**
- Positions in motion and activity events are raw island world coordinates (x, z in metres; height derived locally, AGENTS.md:134).
- Agreement between clients therefore depends on identical inputs (the same 30-day snapshot) and an identical build. AGENTS.md:165 admits spot ids change between builds.
- Two clients that ranked channels differently would place the same coordinates at different houses. This is inferred from the code; no reconciliation was found.

**Every kind number used** (sky code plus the vendored dex it calls):

| Kind | Role | Where |
|---|---|---|
| 0, 1, 3 | profile, notes (guest/landing), follows | `feed.ts:213`, `main.ts:2401`, `nostr/follows.ts:2` |
| 5, 7, 9 | NIP-09 delete, reaction / ⚡ zap marker, NIP-29 chat (`h`) | `nostr/publish.ts:26-38`, `feed.ts:154-156` |
| 9021 | NIP-29 join request | `publish.ts:46` |
| 39000/39001/39002 | NIP-29 metadata / admins / members | `feed.ts:96,136`, `nostr/members.ts:62` |
| 2390 | games: one stored log per table (`h` channel, `e` game) | `client/src/games/book.ts:19`; `vendor/obelisk/src/constants/nostr/nip-kinds.ts:184-198` |
| 9735 | NIP-57 zap receipts | `feed.ts:199` |
| 14 / 13 / 1059, 10050, 10002 | NIP-17 DMs, inbox relays | `client/src/nostr/dm.ts:26,89,185-191` |
| 25055 | 1:1 call control rumours (gift-wrapped) | `dm.ts:27` |
| 20078 | voice presence beacon (`e` channel, `t obelisk-voice-presence`, `["v","screen"]`, SFU `["sfu","1"]`) | `client/src/nostr/liveCalls.ts:1-21`; `nip-kinds.ts:130-135` |
| 25050 / 25051 / 25052 | voice signalling / mod action / SFU control (vendored dex only) | `nip-kinds.ts:137-162`; `vendor/obelisk/src/services/voice/*` |
| 31313 / 31314 | SFU advert (`url`, `relay`, `trusted_relay`, `region`…) / active-call (`d` channel, `status`, `expiration`) | `nip-kinds.ts:164-181`; `services/voice/sfu-discovery.ts:37-80`; `liveCalls.ts:20` |
| 30078 `obelisk:avatar3d:v1` | **sky-only**: your 3D look and up to three `haunts` | `client/src/nostr/avatars.ts:4,22,87` |
| 30078 `obelisk-sky:activity:v1` | **sky-only**: live seat/station claim | `client/src/nostr/activity.ts:88-117` |
| 30078 `obelisk-sky:themes:<relay>` | **sky-only**: operator districts | `themes.ts:36` |
| 30078 dex d-tags | `obelisk:readstate:v1`, `obelisk:roles:`, `obelisk:layout:`, `obelisk:branding:`, `obelisk:bot-commands:` | `readstate.ts:158`, `members.ts:83-89`, AGENTS.md:96,101 |
| 30030 | operator emoji set | AGENTS.md:96 |
| **20390** | **sky-only ephemeral motion** | `client/src/nostr/motion.ts:20` |
| 28934 | NIP-43 invite claim `["claim", code]` | AGENTS.md:120 |
| 24242 | Blossom upload grant (profile pictures) | `client/src/nostr/guest.ts:44-48` |
| 27235 (NIP-98) | `GET /api/invites/me` on the relay | AGENTS.md:120 |

`32390` was **not found** (`grep -rn 32390 client/src vendor/obelisk/src` gave no hits).

## 2. Presence

There is no presence server. All presence is Nostr events signed by the user's own key and sent to the group relay. The relay must have AUTH'd and whitelisted the key (AGENTS.md:121).

**Motion (ephemeral kind 20390)** (`motion.ts`)
- **Content:** JSON `{v:1, t, p:[x,z], mv?, run?, d?:[x,z] | h?, j?, e?, es?, y?, lp?, rd?}`. Coordinates are rounded to 1 cm.
- **Tags:** `["t","obelisk-sky-motion"]` and NIP-40 `["expiration", created_at+60]` (`motion.ts:69-88`).
- **What it carries:** intent (destination or heading), not a pose stream. Every client replays the walk with the same speeds and pathfinding, and extrapolates key-walking from the heading (`motion.ts:1-13,114-119`).
- **Rates:**
  - at most 1 event/s;
  - a correction every 2 s when walking by keys, every 4 s on a clicked path;
  - a 30 s heartbeat while standing;
  - stale after 75 s;
  - exponential back-off up to 8× on relay refusal (`motion.ts:23-32,190-250`).
- **Subscription:** `kinds:[20390]` with `since: now-10` on the active relay only. There is **no `#t`, `#h` or `authors` filter** (`motion.ts:158-165`). Motion is relay-wide, one village per relay, and is not scoped per group.
- **Validation:** the strict parse checks only that numbers are finite with |v| ≤ 1000 and that `t` is monotonic per pubkey (`motion.ts:64-67,91-112,134-139`). There is no velocity, teleport or bounds gate. Receivers fade someone across instead when they are more than 4 m off (AGENTS.md:164).
- **Opt-outs:** disabled for NIP-46 remote signers, guests, and when the `shareMotion` pref is off (`main.ts:2052`).

**Activity claims (kind 30078 `obelisk-sky:activity:v1`)** (`activity.ts`)
- **Tags:** `op` start/stop, `spot`, `t`, and NIP-40 expiry at 90 s (30 s for a stop).
- **Content:** `{v, op, spot, kind, since}`.
- **Spot grammar:** `station:N | seat:X:Z | lane:N | here:X:Z`, in decimetres (`activity.ts:48-117`).
- **Conflicts:** the earliest `since` wins, ties go to the lower event id. This is a deterministic lock-free arbitration (`activity.ts:120-122,157-175`).
- **Timing:** sent 0.3 s after sitting, then re-sent every 45 s (AGENTS.md:116).
- **NIP-46 users:** their emotes and riding go out as `here:` claims (AGENTS.md:117).

**Online status** (`client/src/nostr/presence.ts:10,26`)
- Online means live in the village and active within 5 min, or posted within 15 min.
- AFK means live but idle.

**Avatar** (`avatars.ts`): replaceable 30078 holding look plus haunts. It is cached locally, re-published to relays that lack it, and followed live.

**Trust model**
- Identity is the Nostr key; access is the relay whitelist plus NIP-42 AUTH.
- Spatial claims are unauthenticated beyond the signature: any admitted key can claim any coordinate.
- Private groups: content is only REQ'd once the client holds that group's 39002 (AGENTS.md:119).

**What `server/` does** (`server/index.ts:59-62,172-198`; AGENTS.md:12)
- It serves static files, `/healthz`, an OpenGraph unfurler, a CORS avatar proxy with SSRF guards (`server/avatarProxy.ts`), and a fixed-upstream ticket proxy.
- It holds no keys and has no relay access, by design.
- Its CSP `permissions-policy` keeps microphone, camera and display-capture at `(self)`.

## 3. Voice and calls

- **Voice is scoped to the channel, not to proximity.** You join a voice channel's call from its tree/campfire or the theater (`client/src/voice/callRoom.ts:1-14`). It uses dex's vendored `VoiceClient`, so village and app users share one call (`callRoom.ts:154-171`).
- **Mesh vs SFU.** The mesh holds up to 4 people. `voice-sfu` channels go via the SFU (`expectSfu`, `callRoom.ts:48,158`). The SFU is discovered on kind 31313 (re-advertised every 5 min, 24 h `since`; `sfu-discovery.ts:67-80`). Clients address it with 25052 control; signalling and RPC run on 25050 with `p`-tag routing (`sfu-rpc-relay.ts:1-52`; `nip-kinds.ts:137-162`).
- **Discovery of live calls.** The SFU advertises a live call on 31314 and members beacon on 20078 (`liveCalls.ts`). Beacons repeat about every 15 s with 30 s expiry (`nip-kinds.ts:130-135`).
- **Spatialisation is purely client-side.**
  - Each remote audio track goes through a Web Audio `PannerNode`: HRTF, inverse distance, `refDistance` 2, `maxDistance` 40, rolloff 0.8 (`callRoom.ts:333-345`).
  - `updatePositions` sets the listener to your position and camera yaw, and each panner to that person's world position (`callRoom.ts:280-299`; called each frame at `main.ts:2904`).
  - Those positions come from motion events (§2). People not on the island are heard "beside you".
  - Screen and film audio stay unpanned.
- **Leaving.** Walking out of the call's yard leaves after 25 s unless you press Stay (`callRoom.ts:59-64`).
- **1:1 calls.** Kind 25055 rumours in gift wraps (`dm.ts:27`; AGENTS.md:105).

## 4. Apps and games in-world

- **Games.** Kind 2390 stored event log per table (`h` channel, `e` game id). It is replayed through dex's pure game engines, so a table in sky equals a table in the app (`client/src/games/book.ts:1-19`; `nip-kinds.ts:184-198`). Chain Reaction is playable. Matches start from `/play` in a channel composer. Walk-up game tables and arcade cabinets are still on the roadmap (`docs/ROADMAP.md:64-81`).
- **Kind 32390 apps and Blossom sandbox: not found** in sky or the vendored dex. `obelisk-apps` appears only as a README link (`README.md:225`). Blossom appears only as kind 24242 upload grants via a shim (`guest.ts:44`; AGENTS.md:96).
- **Embedded media.**
  - YouTube and the "totemcillo" page are DOM iframes laid over 3D screens with `CSS3DRenderer`, shown only when an occlusion ray-probe finds the screen clear (AGENTS.md:143-145).
  - The theater shows a call's shared screen as a video texture (`client/src/world/theaterScreen.ts`; AGENTS.md:152).
  - The iframe sandbox is `allow-scripts allow-same-origin` (`client/src/ui/theaterPanel.ts:30`).
- **Mate, emotes, zaps.**
  - Mate rounds on shared benches (`world/mateRound.ts`).
  - Emotes are synced by wall clock via motion `e`/`es` (AGENTS.md:155).
  - Zaps fly sender → recipient as a 3D effect (`world/zapFlight.ts`).
  - These are all presentation layered on existing kinds, with no new wire format.

## 5. Multi-relay / multi-node

- **Single relay at a time.** A relay switch tears down and rebuilds the island (`hub.ts:20-28`; `world.ts:416,456`). Sky does not span relays or federate islands. "Each community gets its own island" means one island per relay URL (README "Pick a relay").
- **Group identity.** A group is `(activeRelay, d)`, where the relay URL is normalised (`wss://host[:port][/path]`, `hub.ts:30-43`). Operator config d-tags embed the relay URL (`obelisk-sky:themes:<relay>`). The world location of a group is local and derived (§1), with no global coordinate for `(relay, group)`.
- **Other relays.** Profile and look reads also use public profile relays, without AUTH (`hub.ts:45-58`). Relay invites can switch relay only for known relays; otherwise a confirmation card asks first (AGENTS.md:120).

## 6. Rendering stack and XR

- **Stack.** three.js 0.180, Vite 7, TypeScript, plain-DOM UI, and React only for re-hosted dex islands (AGENTS.md:9; `package.json` deps `three ^0.180.0`).
- **Rendering features.**
  - GTAO, tilt-shift and bloom post-processing, budgeted point lights, merged static meshes.
  - Quaternius rigged humanoids with runtime-built exercise and emote clips.
  - First-person view with procedural arms.
  - `CSS3DRenderer` overlays for iframes.
  - (AGENTS.md:131-181.)
- **XR: none found.** `grep -rniE "webxr|xr\.enabled|VRButton|ARButton|immersive-vr|navigator\.xr|OpenXR"` over `client/src` and `vendor/obelisk/src` gave no relevant hits. It targets desktop and landscape phones (AGENTS.md:125).

## 7. Adoption table

Estate paths checked:
- VisionClaw `crates/visionclaw-xr-presence/src/{wire.rs:9,39,92; room.rs:23-116; validate.rs:10,77}`, `docs/XR-client.md:824-828`.
- nostr-rust-forum `docs/adr/ADR-2029-room-profile-deployment-type.md` and `crates/nostr-bbs-relay-worker/src/relay_do/broadcast.rs:349-421`.

| Idea | obelisk-sky mechanism | Estate analogue | Verdict | Licence route |
|---|---|---|---|---|
| A group is a place | NIP-29 39000 → house in a yard; "in the room" = nearest yard (`layout.ts:544-581`, `main.ts:1494`) | `PresenceRoom` keyed by `RoomId` (`room.rs:23`, `types.rs:39`); forum ADR-2029's "room" profile is **Anagram 34550 + kind 9 / NIP-171, not NIP-29**, and the forum relay only advertises NIP-29 (its "NIP-29 TODO" is cited in that ADR) | **Adopt the thesis**. Note that nostr-rust-forum has no NIP-29 room support to build on yet | idea-only |
| Canonical placement | none: derived per client from a fixed seed plus local 30-day activity ranking; not on the wire | none | **Adapt**: make placement canonical, either a tag on room metadata (e.g. operator-signed position/scene ref) or a pure function of `(relay, group id)` alone, so all clients agree | idea-only |
| Operator-signed spatial config | 30078 `obelisk-sky:themes:<relay>`, trusted from the NIP-11 operator pubkey (`themes.ts:1-58`) | forum zone config (`zone_config.rs`), NIP-11 `nostr_bbs` block | **Adapt** (own d-tag namespace) | idea-only (do not reuse their d-tag) |
| Intent-based motion over Nostr ephemeral events | kind 20390, ≤1/s, dest/heading replayed locally, back-off on refusal (`motion.ts`) | 0x43 binary pose over WebSocket, server-validated (`wire.rs:9`, `validate.rs`) | **Adapt for a low-rate tier** (2D/web/forum presence, "who's in which room"). Keep 0x43 for XR head/hand poses, which intent-replay cannot convey. The forum relay already treats 20000–29999 as ephemeral (`broadcast.rs:349-421`) | protocol-interop possible (kind 20390 shape is a fact); better to mint our own kind |
| Room scoping of presence | absent: motion is relay-wide, no `h`/`t` filter (`motion.ts:158-165`) | `PresenceRoom` join/leave | **Adapt**: add `h <group>` to presence events so relays can gate by membership | idea-only |
| Spatial claim validation | finite and ≤1000 only; no velocity or teleport gate | `velocity_gate`, `world_bounds`, `monotonic_timestamp` (`validate.rs:10,77,96`) | **Skip theirs; keep ours** | n/a |
| Deterministic seat/spot locks | 30078 claim, NIP-40 90 s expiry, earliest `since` then lowest id wins (`activity.ts:120-175`) | none (agent/seat occupancy) | **Adopt** the arbitration rule (lock-free, relay-agnostic) | idea-only |
| Avatar look as a replaceable event | 30078 `obelisk:avatar3d:v1` `{look, haunts}`, re-published to relays that lack it | `AvatarMetadata` in `room.rs:42` (server-side) | **Adapt**: a signed replaceable avatar profile under our own d-tag, referenced from did:nostr | idea-only |
| Channel-scoped call plus client-side HRTF panning | join by channel; `PannerNode` HRTF from motion positions (`callRoom.ts:280-345`) | `SpatialVoiceRouter` (routing maths, transport absent; `XR-client.md:824-828`) | **Adopt the model**: call membership = room, spatialisation local from presence. Fits LiveKit (room per group, positions from 0x43) | idea-only |
| SFU discovery and live-call adverts over Nostr | 31313 advert, 31314 active call, 20078 beacons, 25050/25052 signalling | LiveKit (unbuilt) | **Adapt** the discovery pattern (an SFU advertises on relays; a "LIVE" room badge comes from a replaceable `d=<group>` event). Interop with Obelisk's SFU only if we want Obelisk users in our calls | protocol-interop (kinds are documented facts in the vendored dex) |
| Leave the call after leaving the room | 25 s away timer with Stay (`callRoom.ts:59`) | none | **Adopt** | idea-only |
| Event-sourced games | kind 2390 stored log replayed by pure engines (`book.ts`) | `nostr-bbs-poker` crate | **Adopt the pattern**. Interop on 2390 only if we want cross-play with obelisk.ar | protocol-interop |
| In-world apps (32390 / Blossom sandbox) | **not found** in sky | — | n/a | — |
| Chat audible only at your place | bubbles shown only to listeners at the same place; general chat global (`livePlaces.ts:31-37`) | none | **Adopt** as a room-locality UX rule | idea-only |
| Activity-sized buildings | tiers by 30-day message count (`buildings.ts:42-47`) | VisionClaw graph node sizing | **Adapt** as visual salience | idea-only |
| Guest island (local scripted demo for unadmitted keys) | `guest/`, unsigned local events | forum whitelist UX | **Skip** (product choice) | n/a |
| Rendering | three.js, no XR | Godot 4.6 + gdext OpenXR | **Skip** | n/a |
| Relay invites | NIP-43 28934 `claim` plus NIP-98 `/api/invites/me` | forum whitelist and invite flow | Out of scope here; a candidate for another worker | protocol-interop (NIPs) |

**Bottom line.** Sky shows that "NIP-29 group = place" works as a UX: rooms become houses, being in a room means standing in its yard, and the call and chat are scoped to that place. Its wire layer is thin and loosely trusted:
- placement is not canonical;
- motion is relay-wide and unvalidated;
- positions are raw local coordinates.

What we should take is the protocol pattern, not their kinds:
- operator-signed spatial config;
- an ephemeral, low-rate, intent-based presence tier carrying an `h` tag;
- deterministic claim arbitration;
- SFU discovery over Nostr;
- channel-scoped calls with local spatialisation.

Keep 0x43 and `PresenceRoom` as the authoritative XR tier.
