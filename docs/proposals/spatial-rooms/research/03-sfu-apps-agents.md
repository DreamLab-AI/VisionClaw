# 03 — obelisk-sfu / obelisk-apps / obelisk-agents / obelisk-design

Worker: sfu-apps-agents. Research only; nothing run, nothing edited elsewhere.
Paths below are relative to `scratchpad/obelisk-src/` (e.g. `sfu:` = `obelisk-sfu/`). Line numbers are from `cat -n` of the file named. Where I cite a doc by section only, the section is unique in the file.

## 0. Licence audit (exact)

| Repo | Finding | Route |
|---|---|---|
| obelisk-sfu | No LICENSE/COPYING/NOTICE file; `package.json` has no `license` field; README has no licence line (grep of README, package.json, tree: nothing). No vendored dirs. | All rights reserved by default → **ideas + wire-protocol facts only**. |
| obelisk-apps | No LICENSE file; no `license` field in root, `apps/*`, `packages/*` package.json. `apps/stacker/assets/music/*-ncc.mp3` are third-party "NCC" (non-commercial?) music — do not reuse. | Same: ideas/protocol facts only. |
| obelisk-agents | `package.json:7` says `"license": "MIT"`; **no LICENSE file anywhere** (find returned nothing). MIT without the licence text/copyright holder is not a clean grant: MIT's one obligation is to carry the notice, and there is no notice to carry. | Treat as **"declared MIT, unverified"**. Safe: ideas + protocol. If we ever port code, ask upstream to add LICENSE first, or clean-room. |
| obelisk-design | No LICENSE. Docs/SVG only. | Ideas only. Do not copy La Crypta marks/logos. |
| (not in scope but in tree) obelisk, obelisk-relay, obelisk-menhir have LICENSE; obelisk-sky has asset licences (Kenney, Quaternius). Not read. |

No code is quoted below beyond a protocol literal. Everything here is a description of wire facts.

## 1. obelisk-sfu — Nostr-RPC signalling

Stack: Node 22 + **mediasoup** + nostr-tools (`sfu/docs/sfu-system.md:110-122`). Media never touches relays (`sfu-system.md:3-7`). Important: the **current** design is *not* "RPC over Nostr events". It is a direct authenticated WebSocket (`/rpc`) whose request envelope is the same one used by the older kind-25050 relay path, which is retained as a fallback (`sfu-system.md:9-21`, README table `sfu/README.md:47-54`). HANDOFF.md is stale (dated 2026-05-04, werift era, "mediasoup migration future work", `sfu/HANDOFF.md:115-119`) — don't trust it over `sfu-system.md`/code.

### 1.1 Event kinds (single source: `sfu/src/nip-kinds.ts`)

| Kind | Class | Role | Evidence |
|---|---|---|---|
| **20078** | ephemeral | Presence beacon, same wire as the mesh voice. Tags: `["e",channelId]`, `["t","obelisk-voice-presence"]`, `["expiration", now+30]`, **`["sfu","1"]`** (topology marker), `["p",pk]` per connected peer; content `""`. Published every ~15 s (room.ts header). | `sfu/src/signaling.ts:20-55`, `nip-kinds.ts:22-23` |
| **25050** | ephemeral | Directed signalling, `p`=target, `e`=channelId, `t`=`obelisk-voice-signal` (+`t`=`obelisk-sfu-rpc` on SFU notifications). Content is **cleartext JSON** (see 1.4). Two payload dialects: mesh `VoiceSignalPayload` {type: offer/answer/ice/bye/trackinfo/qualityhint/requestReset, sdp, candidates, trackInfo{trackId, kind: audio/camera/screen/screen-audio, originPubkey}, sessionId, seq} and the RPC envelope. | `signaling.ts:63-85`, `sfu/src/types.ts:12-58`, `room-mediasoup.ts:900-935` (sendNotification; retry delays L105-106) |
| **25052** | ephemeral | SFU **control**: user-signed, `p`=SFU pubkey, `e`=channelId, `t`=`obelisk-sfu-control`, NIP-40 `expiration`. Content `{"action":"start"|"end"|"kick"|"update","params":{video,screen,allow[],deny[],maxParticipants,endsAt,target,reason}}`. Silent drops on failure; no reply event — caller infers success from a 31314 update. | `call-listener.ts:1-14,180-235`, `types.ts:60-93`, example publish `HANDOFF.md:36-47` |
| **31313** | param-replaceable, `d="obelisk-sfu"` | **SFU advertisement**. Tags: `t=obelisk-sfu-advertisement`, `cap` (max participants/room), `max_rooms`, `version=1`, `session` (random per-boot id → clients detect restarts), `booted_at`, `url`, `relay`×N, `trusted_relay`×N, `codec`=opus/vp8, `allow`×N (the pubkey allow-list is *published in the clear*), `operator`, `region`. Content `""`. Re-published at boot, SIGHUP, and **every 5 min** (was 6 h; relays forgot replaceable events). A kind 0 profile with `bot:true` is published first so clients render a name. | `sfu/src/advertise.ts:29` (5-min refresh), `:62-72` (start/timer), `:98-110` (kind 0, `bot:true`), `:136-198` (tags; `session` L148, `allow` L164) |
| **31314** | param-replaceable, `d=<channelId>` | **Active-call state**: `e`, `host`, `status` (starting/active/ending/closed), `count` (distinct pubkeys), NIP-40 `expiration` = now+TTL (design says 60 s refresh / 90 s expiry), `t=obelisk-sfu-active-call`. | `room-mediasoup.ts:1456-1485`, `nip-kinds.ts:37-43` |
| 22242 | (NIP-42 style) | Used **twice**: (a) SFU↔relay NIP-42 AUTH (`relay.ts:9-12,88-130`); (b) client↔SFU `/rpc` auth challenge (see 1.3). | `direct-rpc.ts:24,226-243` |
| 30078 | NIP-78 | Per-channel *pin* of one SFU URL (+ verified snapshot `{pubkey,url,trustedRelays,relays}`). Written by channel admin in the dex client, not by the SFU. | `sfu-system.md:23-60` |
| 39000/39001/39002 | NIP-29 | SFU *reads* group metadata/admins/members. | `nip-kinds.ts:15-20` |
| 27235 | NIP-98 | Operator admin HTTP auth. | `design: security-workflows/operator-admin.md` |

### 1.2 RPC envelope + method set

Envelope (JSON): `request {type,requestId,method,data?,clientId?}`, `response {type,requestId,ok,data|error{message,code?}}`, `notification {type,method,data?}` (`sfu/src/nostr-rpc.ts:1-61`). `clientId` (regex `^[a-zA-Z0-9._:-]{1,80}$`, `direct-rpc.ts:155`) distinguishes two devices of one pubkey; peers keyed `pubkey|clientId`; cap counts distinct pubkeys (ROADMAP "Multi-device support" `ROADMAP.md:23-27`).

Methods = the **mediasoup protocol verbatim**: `getRouterRtpCapabilities`, `createWebRtcTransport{direction}`, `connectWebRtcTransport`, `produce`, `consume`, `resumeConsumer`, `closeProducer`, `leave` (`room-mediasoup.ts:988-1345`). I.e. the SFU is a thin Nostr-authenticated shell around an off-the-shelf mediasoup router; signalling is not custom beyond identity/rooms.

### 1.3 Auth / allow-listing

Direct `/rpc?channelId=<hex>` flow (`sfu/src/direct-rpc.ts`): server sends `{type:"auth", kind:22242, challenge(32 B random), relay:<wss url>, channelId}` (L116-122) → client returns `{type:"auth", clientId, event}` where event is kind 22242 with tags `challenge`, `e`(channel), `relay` (L226-243). Verified: signature, `|now-created_at| ≤ 300 s`, challenge match, channel match, relay path+query match (replay/phishing binding to *this* SFU URL). 15 s auth timeout → close 4401; not allow-listed → **4403** (shown to user, no silent fallback) (L113,165-168). Then room is *auto-started* if absent and caller is allowed (L170-181).

`isAllowedToStart` (`sfu/src/auth.ts:21-27`) admits: `allowAll` flag, an operator bypass window (`whitelistBypassUntil`), operator pubkey, `allow.json` (SIGHUP hot-reload), or **trusted-referent follows** (pubkeys followed by a configured reference account — a WoT allow-list; `follow-whitelist.ts`). Control events (25052) additionally: a *trusted-author relay* (a relay with its own write-whitelist) is treated as proof of authorisation; `requireAllowedPubkey=1` forces both (`call-listener.ts:236-266`). Manage/kick/end = host who started the room or operator only (`auth.ts:94-101`).

### 1.4 Encryption

- **Signalling is cleartext**. `nostr-rpc.ts:6-8`: "cleartext for v1; NIP-44 wrapping is a Phase-2 polish item"; HANDOFF §7 plans NIP-59 gift-wrap for 25050/25052 (`HANDOFF.md:121-123`). SDP/ICE (IPs, fingerprints) on a relay is visible to relay operators.
- Media: DTLS-SRTP browser↔SFU, i.e. **hop-by-hop, SFU sees plaintext** (`sfu-system.md:132-133`). No E2EE/insertable-streams. Trust model says so explicitly: run an SFU you trust, ideally operator == group owner (`README.md:69-70`, `docs/RUNBOOK.md:348-354`).
- Responses/notifications are **re-published several times** on a retry schedule (`RPC_RESPONSE_RETRY_DELAYS_MS`, `room-mediasoup.ts:875-898` sendResponse) because relay delivery of ephemeral kinds is lossy — a good reason the project moved to direct WebSocket.

### 1.5 Room ↔ NIP-29 group binding

- A "channel" is a NIP-29 group; its id is the `d` of kind 39000 and the `e` tag everywhere. Channel type is a `["t","voice-sfu"]` tag on the group's 39000 metadata (priority voice-sfu > voice > forum > text) (`channel-registry.ts:157-200`). Start is refused if registry says channel isn't `voice-sfu`; unknown channels are accepted with a warning (`call-listener.ts:279-291`).
- Membership tracker watches 39001/39002 per channel (`membership.ts`) — **but the strict member gate is deliberately disabled** ("SOFTENED in v0": `void ctx.members`) because open channels don't materialise in 39002 (`auth.ts:60-76`). So today the only gates are allow-list + per-call allow/deny. Real NIP-29 membership enforcement is a TODO — don't copy as-is.

### 1.6 Discovery of an SFU

Resolution order in the client: (1) per-channel 30078 URL pin, (2) build-time fallback, (3) signed 31313 discovery (`sfu-system.md:53-57`). Pin flow: admin enters only a URL; client fetches CORS-enabled `GET /info` and requires `service=="obelisk-sfu"`, 64-hex `pubkey`, same-origin advertised URL, public `wss://` relays (`sfu-system.md:31-36`). Client filter for browsing: `{kinds:[31313], '#t':['obelisk-sfu-advertisement']}` (`HANDOFF.md:72`). Open questions never solved: tie-break when two SFUs advertise one channel; auto vs manual pick (`HANDOFF.md:129-130`).

### 1.7 Scaling notes (ROADMAP/HANDOFF/DEPLOY)

Single process, mediasoup 4-worker pool, per-room cap (`cap`) and `max_rooms`; **no clustering/federation** ("Recording and federation stay out until there's a concrete product need", `README.md:134`). Gaps: worker death exits the process (PM2 restarts; calls drop) (`ROADMAP.md:46-49,78-80`), no per-room bandwidth cap (`:64-66`), no consumer-score reporting (`:50-53`), keyframe heartbeat every 8 s costs 5-10 % (`:28-31,84-86`). SIGUSR1 = drain (keep rooms, refuse new). Ops: UDP 40000-40099, public IP or `SFU_PUBLIC_IP`, optional TURN force-relay (`README.md:110`, `ROADMAP.md:61-63`).

### 1.8 Anything spatial?

**No.** `grep -i spatial|positional|panner|HRTF|pose` over src/docs/README/ROADMAP/HANDOFF finds nothing relevant. Audio is plain Opus forwarding; mixing/panning is client-side. The only quasi-spatial neighbour is obelisk-sky (3D village client, "join a call from a voice channel's pavilion", `sfu/README.md:141`) — not read, out of scope; I'd expect it to pan client-side with WebAudio. Spatial = our problem regardless.

## 2. Compare with planned LiveKit spatial voice

Our state: `SpatialVoiceRouter` does routing maths only; LiveKit media backend not built (`docs/XR-client.md:824-826`: the livekit-android AAR transport "would consume it"). `visionclaw-xr-presence` carries pose.

**Could Nostr-signalled SFU replace LiveKit tokens?** Yes for *authorisation + discovery*; no for *the SFU itself* — someone still has to implement a media plane. The obelisk idea decomposes into three separable pieces:

1. **Identity-as-credential**: replace "server mints a LiveKit JWT" with "client proves key possession (NIP-42-shaped kind 22242 challenge bound to SFU URL + room) to the SFU". Cheap, and it is exactly the same signing capability our agents/forum already have. The SFU verifies against *its own* policy (allow-list, group membership read from the relay). LiveKit equivalent: a tiny token-minting sidecar that does the identical Nostr challenge and then issues a short-lived LiveKit JWT — gets the same property **without** replacing LiveKit. This is the lowest-risk route and keeps LiveKit's client SDKs (important for Godot/Android: livekit-android AAR already planned).
2. **Advertised SFU (31313-style)**: SFU publishes its own URL/capacity/region/codecs/operator as a replaceable event on the relay(s) it serves; a room/group may pin one (30078-style). For a federated estate (VisionClaw nodes + forum relay + agentbox) this gives: no central LiveKit control-plane, each node/relay operator runs an SFU and advertises it; clients discover by relay; a group admin pins. Failure mode: stale advertisement (they went to 5-min refresh) and no tiebreak for multiple SFUs.
3. **Room state as events (31314)**: presence/“LIVE n” without polling the SFU; fits our relay-native UI.

What it does *not* buy: positional audio, E2EE, SFU load-balancing, or recording. For **spatial voice** the relevant design is orthogonal: SFU forwards N Opus streams (maybe with interest management: only forward streams within audible radius — requires the SFU to know positions, which obelisk's SFU doesn't). Practical VisionClaw design: client does HRTF/panning with positions from `visionclaw-xr-presence`; SFU only needs a cheap "subscribe set" API (consume only speakers within R). LiveKit has track subscription control and selective forwarding natively; with mediasoup you'd implement it via `consume`/`pause`. With str0m you'd write it.

Recommendation: **Adapt, don't replace.** Keep LiveKit (Apache-2.0, Go server, mature client SDKs incl. Android/Rust/Unity) as the media plane; add (a) Nostr-challenge token mint, (b) 31313/31314-style advertisement and pin so a federated estate discovers SFUs via relays. Revisit a Rust SFU only if LiveKit's Go operational footprint or its JWT trust root is rejected.

### Rust SFU / media options (crate licences checked against the crates.io API, 2026-10-30)

| Option | What | Licence | Notes |
|---|---|---|---|
| `mediasoup` 0.33.0 (Rust crate, bindings to C++ worker) | Same engine obelisk drives from Node | **ISC** | Worker is C++; battle-tested; fits "Nostr shell around mediasoup" literally. Not pure Rust. |
| `str0m` 0.24.1 | Sans-IO WebRTC, SFU building block | **MIT OR Apache-2.0** | You write rooms/forwarding. Pure Rust, embeddable in a relay process. |
| `webrtc` 0.21.1 (webrtc-rs) | Pion-port WebRTC stack | **MIT/Apache-2.0** | Heavier; async-runtime coupled. |
| `livekit` 0.10.0 / `livekit-api` 0.10.0 | Rust client SDK / server API + token minting | **Apache-2.0** | `livekit-api` can mint JWTs, i.e. the "Nostr-challenge then token" bridge is writable in Rust. |
| LiveKit server (Go) | Full SFU | Apache-2.0 (GitHub, **not checked here**) | Track-subscription control; no Nostr awareness. |
| werift (TS) | What obelisk abandoned for production: it transcodes on forward (`sfu/HANDOFF.md:115-119`) | MIT (npm, **not checked here**) | Cautionary tale. |

## 3. obelisk-apps — kind 32390, Blossom, sandbox

### 3.1 Wire

(`apps/docs/app-format.md`, "draft v1", self-declared unimplemented at the time: L3.)
- **Manifest kind 32390** (addressable, `d`=slug `^[a-z0-9-]{1,32}$`): tags `title`, `description`, `api` (host-API major, required), `t` (game|widget|tool), `version` (label only), `players min max`, `realtime`, `path /abs ⟨sha256⟩` (≤64; `/index.js` required, one ES module ≤2 MiB; total ≤32 MiB), `icon`, **`x ⟨aggregate⟩ aggregate`** (NIP-5A aggregate = sha256 of sorted `"<sha> <path>\n"` lines), `server` (Blossom hints), `source`, `alt` (NIP-31). Mismatched `x` ⇒ ignored. Identity = `32390:<author>:<d>` (app-format.md L19-72). Reuses NIP-5A (nsite) file vocabulary but adds a runtime contract.
- **Session kind 2390** (regular, `h`-scoped, 7-day relay retention): `h`, `t=obelisk-app`, `op`, `e`(root session). `create` is published **by the host, not the app**, and *copies the manifest's path tags + aggregate* — the version pin; hosts recompute the aggregate and refuse mismatch (L106-132). App ops `^[a-z][a-z0-9-]{0,31}$` (reserved: create/join/leave/cancel/status), content ≤64 KiB, only extra tag `n` (L145-152). Ordering `(created_at,id)` (L154-156). Chat marker `[[app:<sessionId>]]` in a kind 9 (L158-171).
- **Discovery**: `{kinds:[32390]}` on the *active* relay only; per-relay catalogue; whoever can write to the relay can publish an app (L74-84).
- Quirk: relay must allow 32390 *without* an `h` tag, else NIP-29 relays reject it (`known-issues.md` "The relay must allow kind 32390…").

### 3.2 Blossom

Bundles/assets are sha256 blobs on Blossom servers listed in `server` hints; host verifies each blob against the pinned hash before handing to the frame (`host-api.md` `asset`). Their own server = fork of hzrd149 blossom-server with `pubkeysFile` rules and `sandboxBlobs` (every blob served with `CSP: default-src 'none'; sandbox`) and a **WoT upload allow-list sidecar** (`packages/blossom-wot/README.md`: tier1 = reference accounts + follows (1-yr retention), tier2 = 2+ hops (3 months), others 401; disk guard closes uploads at 3 GiB). Known: single disk no mirror, no per-uploader quota (`known-issues.md`).

### 3.3 Sandbox model (the valuable part)

(`apps/docs/security.md`, `host-api.md`, `packages/frame/Caddyfile.snippet`)
- App code is **untrusted**; host is the only holder of signer/sockets/storage. `<iframe sandbox="allow-scripts" allow="" referrerpolicy="no-referrer">` — **no `allow-same-origin`** ⇒ opaque origin.
- Frame loader on a **dedicated origin** (`frame.obelisk.ar/v1/`), no login session, CSP as *header*: `default-src 'none'; script-src 'self' blob: 'wasm-unsafe-eval'; connect-src 'none'; img/media/font-src data: blob:; form-action 'none'; base-uri 'none'; frame-ancestors <allow-list>`; no `unsafe-eval`; `Permissions-Policy` denies camera/mic/geo/etc.
- Handshake: loader `hello` → host checks `event.source === iframe.contentWindow` → transfers a `MessageChannel` port with `boot{entry: Blob}`; thereafter only the port. Entry imported from a `blob:` URL; single self-contained ES module; `main({port, root})`.
- Host API v1 (message bridge over the port): pushes `init, events, participants, visibility, env, connection`; requests `publish{op,content,n}` (host builds the event; kind/`h`/`e`/`t` fixed; token bucket 20 deep @ 5/s; 64 KiB cap), `asset(path)` (only pinned paths, hash-verified, cached by hash), `storage.get/set` (per app+user, 256 KiB, host localStorage), `profiles(pubkeys≤32)` (only session authors), `ui.resize/toast/close`. **Not exposed:** arbitrary signing, NIP-04/44, zaps, reading chat/members/relays, network, popups, camera/mic.
- Replay-trust: all clients replay the same sorted log through the same pinned code; no referee (`security.md` "Replay trust").
- Honest gaps: self-navigation exfiltration (host kills frame on second `load` but request already sent), **WebRTC bypasses CSP** `connect-src`, CPU abuse, in-frame phishing, no hidden information (cards need commit-reveal/NIP-44 not designed), cheats detected not prevented, same-second reordering (`known-issues.md`).

### 3.4 Relevance to us

- **Poker** (`nostr-bbs-poker`): Obelisk's replay model explicitly **cannot host hidden-information games**; poker needs either a trusted dealer (our relay-side crate) or commit-reveal/mental-poker. So obelisk-apps is the wrong *game* model for poker; it's a good *packaging/sandbox* model for untrusted widgets (in-world panels, mini-games, tools) in the forum web client or an XR WebView. Our poker should stay server-authoritative (kinds 31400-31405 control surface already exists on our side).
- **Godot XR client**: a browser iframe sandbox doesn't port to a native gdext client. Equivalent there = run untrusted logic as **WASM (wasmtime/wasmi) or a sandboxed scripting subset** with a fixed host API of the same shape (`publish` of a templated event, pinned-hash `asset`, per-app storage, no net) — the *contract* transfers, the *mechanism* doesn't. In a WebXR/web path, the iframe model transfers directly.
- Reusable pieces as **interop-free ideas**: (a) manifest = addressable event whose `x` aggregate pins code (NIP-5A-compatible: we can read nsites for free); (b) *session create event copies the pin* so replaced manifests don't orphan live sessions; (c) host-built templated events so apps never see a signer; (d) separate frame origin; (e) Blossom WoT upload gate.
- Caveat: kind numbers 2390/32390 are **unregistered** (known-issues). Collision risk with our kinds (our 31400-31405 are distinct, fine). If we interoperate, use their numbers verbatim; if not, pick our own — don't half-adopt.

## 4. obelisk-agents — bot model

- A bot = **one Node script + one nsec in `.env.local` + one PM2 stanza** (`agents/docs/building-bots.md` §intro). Shared libs: `lib/pool.mjs` (SimplePool with `automaticallyAuth` NIP-42, `pool.mjs:9-13`), `lib/group-watcher.mjs` (kind 39000 scan → onAdd/onRemove, default every 10 min), `lib/state.mjs` (json state), `lib/secret.mjs`.
- Group participation: subscribe kind 9 with `#h:[groupId]`, **one filter per REQ** (relay29-derived relays reject multi-filter REQs), `since` to avoid replay, dedupe by id, ignore self; publish kind 9 with `["h", gid]` (+`["e", parent]` for reply), `Promise.any` over relays; mentions as `nostr:npub…`; command bots with `!cmd` and anti-ping-pong throttle. Join: a human admin adds the bot via kind 9000 `put-user` (`tools/add-member.mjs`); bots "only publish kinds they're documented to publish" — admin kinds 9000-9007 need an explicit human grant (`building-bots.md` §6; `admin-bot.md` kind table).
- **LLM agents** (`lib/agent-bot.mjs`): hears **only whitelisted pubkeys** (`ALLOWED_PUBKEYS`; empty ⇒ deaf) — non-whitelisted users never enter the LLM context ⇒ closed prompt-injection surface (`agent-bot.mjs:123-129`, `docs/agents.md:3-8`); rolling context per group (default 30), mention/`all` trigger, per-group reply gap 4 s, replies capped 2000 chars, publishes kind 9 only. Provider-pluggable (Anthropic SDK / OpenAI-compatible).
- CLI toolkit (`tools/`): `new-bot`, `whoami`, `set-profile` (kind 0), `grant-admin`, `add-member`, `list-groups`, `inspect-events`, `probe-relays/write/zaps`, `simulate-zap`. Manager server+UI: Nostr-challenge login (kind 22242, admin pubkeys) (`server/auth.mjs`), per-bot settings scraped from `process.env.X` in source, Blossom avatar upload signed with the bot key (kind 24242) (`building-bots.md` §5 table).
- **Relevance**: agentbox agents already speak Nostr; this repo confirms the *minimum viable membership recipe* (NIP-42 auto-AUTH → be a group member via 9000 → kind 9 with `h`). For "first-class members of rooms/spaces" the gaps are: (a) agent identity in presence — their SFU publishes a kind 0 `bot:true` (`advertise.ts:98-110`); adopt a `bot` flag on profile so UIs label agents; (b) **agents as voice participants**: obelisk has no voice-bot at all (no media code in agents). A voice agent would be a mediasoup/LiveKit client producing TTS audio and consuming a mixed or per-speaker stream — new work, nothing to copy; (c) whitelist-only context is the right default for LLM agents in any shared space and maps to our permission model; (d) their admin-bot pattern conflates moderation with agent status — keep ours separate (our control surface kinds 31400-31405).
- Quality: bots are single-file scripts with no tests directory seen; treat as reference patterns, not a library. MIT is declared but unverified (§0).

## 5. obelisk-design — what is worth reading

Mostly branding (La Crypta palette `#b4f953`, `lc-*` classes, admin shell, social). **Skip** design-system/, development-style/. Worth reading:
- `security-workflows/README.md` (defaults: no parallel auth store; NIP-07 → NIP-46 → imported nsec; NIP-42 relay, NIP-98 HTTP, challenge events only where established; "treat every media or storage boundary as a threat-model decision").
- `security-workflows/admin-console-auth.md` (137 lines; describes relay/SFU/agents consoles: NIP-98 for SFU (kind 27235, no challenge; tags `u`,`method`), challenge-based kind 22242 for relay/agents, session handling, **logout clearing every layer**, `awaitingChoice` pattern because NIP-07 always re-derives, known gaps) — directly useful for our admin surfaces.
- `security-workflows/app-sandbox.md` (cross-project policy summary of §3.3; 8 rules + 5 known gaps incl. "no signing-prompt budget" and hand-maintained Tauri CSP drift).
- `operator-admin.md`: SFU is a separate trust boundary (terminates media, own Nostr identity); "relays, SFUs and app frontends should be independently replaceable and discoverable" — a design principle matching federation goal.
- `development-style/nostr-links.md` (68 lines) — probably nostr: URI linking conventions; not read in detail.

## 6. Idea table

| Idea | obelisk mechanism | Estate analogue | Verdict | Licence route |
|---|---|---|---|---|
| Key-based room auth | Server-sent challenge + client kind-22242 event bound to `e`=room and `relay`=SFU URL, ±5 min, then allow-list check (`direct-rpc.ts:226-243`) | Mint LiveKit JWT after Nostr challenge (forum relay / VisionClaw API) | **Adopt** (protocol shape is NIP-42-like, public) | protocol interop / idea-only |
| SFU advertisement | Kind 31313 param-replaceable `d=obelisk-sfu`, tags url/cap/region/codec/operator/session, 5-min refresh | Same event for federated VisionClaw nodes / forum relays; **choose our own kind** (e.g. in our 314xx range) unless we want to interoperate with obelisk clients | **Adapt** | idea-only (tag vocabulary is a protocol fact, but don't clone verbatim without interop need) |
| Active-room state | Kind 31314 `d=<channel>`, `count`, `status`, NIP-40 expiry | XR "who's talking where" badge from relay | **Adapt** | idea-only |
| Channel pin | NIP-78 30078 per-channel SFU URL, verified via `/info` | Group admin pins SFU for a space | **Adopt** | idea-only (NIP-78 itself is public) |
| Session-restart detector | Random per-boot `session` tag in advertisement | Any advertised service | **Adopt** | idea-only |
| Multi-device peers | `clientId` in envelope, key `pubkey|clientId` | Agent + human same key; XR headset + phone | **Adopt** | idea-only |
| Allow-list = WoT follows | Trusted-referent follows (`follow-whitelist.ts`) | Our relay already has WoT/allowlists | **Adopt** | idea-only |
| Trusted-author relay | Relay write-whitelist proves authorisation of control events | Our forum relay already is that | **Adopt** | idea-only |
| Mediasoup RPC over Nostr | 25050 envelope retried N× because ephemeral delivery lossy | — | **Skip** (they themselves moved to direct WS) | — |
| Cleartext signalling | NIP-44/59 wrapping "later" | Must wrap (NIP-44/59) since SDP leaks IPs | **Adapt** (fix their gap) | idea-only |
| Strict NIP-29 member gate | Disabled in v0 (`auth.ts:60-76`) | We should enforce group membership at SFU/token mint | **Adapt** (do what they didn't) | idea-only |
| Positional audio | None; client-side | `SpatialVoiceRouter` + `visionclaw-xr-presence` + subscribe-set API | **Skip** (nothing to take) | — |
| Rust SFU | None (Node + mediasoup) | LiveKit now; `str0m`/`mediasoup` crate later | **Skip** their code; evaluate crates independently | crate licences (verify) |
| App manifest event | 32390 + NIP-5A path/x/server tags | Web client widgets, in-world apps | **Adapt** (optionally interop via same kind) | protocol interop if we reuse numbers; else idea-only |
| Pinned session create | Session event copies path+aggregate | Any event-sourced game/session | **Adopt** | idea-only |
| Host-built events | App never sees signer; fixed template; token bucket | Any plug-in surface | **Adopt** | idea-only |
| Iframe sandbox + frame origin | `sandbox="allow-scripts"`, own origin, CSP header, MessagePort | Web path only; XR native needs WASM equivalent | **Adapt** | idea-only |
| Blossom + WoT upload gate | Hourly WoT tiers → pubkey files → blossom-server fork | Asset/plugin storage on forum relay | **Adapt** | idea-only (blossom-server is hzrd149's; check its licence separately) |
| Replay-trust games | Deterministic log replay, no referee | Poker needs hidden info → **doesn't fit**; use for perfect-info games | **Skip for poker** | — |
| Whitelist-only LLM context | `ALLOWED_PUBKEYS`, empty ⇒ deaf | agentbox agents in shared rooms | **Adopt** | idea-only (MIT unverified) |
| Bot recipe | NIP-42 auto-auth, one-filter REQ, kind 9 + `h`, 9000 put-user | agentbox agent group membership | **Adopt** (confirm relay quirks apply to nostr-rust-forum) | protocol facts / idea-only |
| `bot:true` profile flag | kind 0 JSON `bot` | Label agents in XR/forum UI | **Adopt** | idea-only (NIP-24 defines `bot`) |
| Admin logout discipline | Clear server session + signer storage + SDK + `awaitingChoice` | Our admin UIs | **Adopt** | idea-only |

## 7. Gaps / uncertainties

- I did not read `room-mediasoup.ts` in full (1485 lines), `peer.ts`, nor `follow-whitelist.ts`; claims about them come from headers/greps.
- 31314 expiry is 90 s (`room-mediasoup.ts:72` `ACTIVE_CALL_TTL_SECONDS = 90`); the 60 s refresh figure is from the doc comment at `nip-kinds.ts:39-41`.
- Crate licences in §2 verified via crates.io API; LiveKit server and werift licences not checked.
- obelisk-sky (3D client with voice pavilions) and obelisk-relay's WoT ladder are potentially more relevant to spatial voice and federation than anything here; recommend another worker if not already assigned.
