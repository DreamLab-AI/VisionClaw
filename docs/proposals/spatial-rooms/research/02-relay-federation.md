# 02 — Relay federation: obelisk / obelisk-relay / menhir as port sources

Worker: relay-federation. Research only. Paths: `OR` = scratchpad `obelisk-src/obelisk-relay`, `OB` = `.../obelisk`, `MH` = `.../obelisk-menhir`, `FORUM` = `~/workspace/nostr-rust-forum`. Nothing was built or run.

## Headline

1. **obelisk-relay has NO federation.** It is a single-node NIP-29 relay. The only federation item is an unticked roadmap line: "Relay federation — Sync groups between multiple relay instances via negentropy" (`OR/ROADMAP.md:148`), plus "Whitelist sync" (`:115`). Nothing in `src/` replicates groups between relays. The multi-node design therefore has to come from our own `nostr-bbs-mesh` plus NIP-29's own text, not from this source.
2. **Its NIP-29 engine is not self-contained.** The protocol layer and storage are verse-pbc's `relay_builder` + `nostr-lmdb` (git dependencies, `OR/Cargo.toml:11-17,62`). Only the group state machine (`group.rs`, `groups.rs`) is portable, and it is written against `nostr_sdk`/`nostr_lmdb::Scope`/`relay_builder::Error` types. A Cloudflare Workers port would be a rewrite of the I/O around it.
3. **Recommendation: clean-room from the NIP-29 spec for the state machine; port nothing verbatim.** The one thing worth reading closely (not copying) is the `can_see_event`/admin-gating semantics. Our gap is federation design, not NIP-29 parsing.
4. **No crate to depend on.** `groups_relay`, `relay_builder` and `websocket_builder` are not on crates.io (API returned no crate for any of them). `nostr-relay-builder` on crates.io is rust-nostr's (MIT, 0.45.0-alpha.3, repo `rust-nostr/nostr`), a different project from verse-pbc's `relay_builder`, and tokio-based, so no use on CF Workers.

---

## 1. obelisk-relay architecture

### Storage and framework
- Binary name `groups_relay`, version 0.1.0, authors `Daniel Cadenas` (the upstream author, left unchanged), `license = "AGPL-3.0-only"`, `repository = obelisk-app/obelisk-relay` (`OR/Cargo.toml:1-7`).
- Framework: `relay_builder` from `github.com/verse-pbc/relay_builder` (unpinned git; `Cargo.lock` is the pin; comment at `:11-23` explains why `rev=` breaks the build). Websocket layer `websocket_builder`. Nostr types from verse-pbc's fork of rust-nostr (`nostr`, `nostr-sdk`, `nostr-lmdb`, `:62`).
- Storage: **LMDB** via `nostr-lmdb` with `heed` 0.20 opened directly for compaction (`:75-86`, `src/compaction.rs`). Events live in LMDB; group state is held in memory in a `Groups` struct (`src/groups.rs:219`) and **rebuilt at startup by replaying stored events** (`load_groups`, `src/groups.rs:234-450`), i.e. event-sourced, no separate group table.
- **Multi-tenancy via "scopes"** (`nostr_lmdb::Scope`): scope is derived from the request subdomain (`.with_subdomains_from_url(&settings.relay_url)`, `src/server.rs:322`; `/api/subdomains`, `:897`). Groups are keyed `(Scope, group_id)` (`src/groups.rs:456-508`). Note: this means a group is namespaced by relay host+subdomain inside one process, which is a hosting convenience, not federation.
- Extras beyond NIP-29, all single-node: HTTP admin console (`src/admin.rs`, `frontend/`), Prometheus metrics, pruner/retention (`src/pruner.rs`), reports/moderation queue for kind 1984 (`src/reports.rs`), storage stats, an "Obelisk bootstrap/messages" HTTP index authenticated with NIP-98 (`src/obelisk_api.rs:11-12`, `src/obelisk_index.rs`), web-push registration kinds 3079/3080, Cashu/NIP-60 wallet kinds passthrough, kind 32390 app manifests (`src/group.rs:38-68`).

### NIP-29 coverage (`OR/src/group.rs:71-129`)
| Kind | Status |
|---|---|
| 9000 put-user, 9001 remove-user, 9002 edit-metadata, 9005 delete-event, 9007 create-group, 9008 delete-group, 9009 create-invite | implemented (constants `:71-78`; handlers `src/groups.rs:588-900`) |
| 9006 set-roles | implemented, annotated "This was removed [from the NIP] but at least 0xchat uses it" (`:77`) |
| 9021 join-request, 9022 leave-request | implemented (`:80-81`; `group.rs:1145,1476`) |
| 39000/39001/39002/39003 | relay-signed addressable state, regenerated from in-memory group (`ADDRESSABLE_EVENT_KINDS` `:88`; `generate_*_event` `group.rs:1770-1970`) |
| h-tag enforcement | `Group::extract_group_h_tag`/`verify_member_access` (`group.rs:1689-1704`); events with no `h` refused unless kind is in `NON_GROUP_ALLOWED_KINDS` (`:95`; `validation_middleware.rs` test `a_group_event_without_an_h_tag_is_refused`) |
| Invite codes (group-level) | kind 9009 creates, 9021 with `code` redeems; `Invite` struct with `can_use`/`mark_used`/expiry (`group.rs:457-540`) |
| `previous` timeline references | **Not implemented.** `grep -rn "'previous'\|\"previous\"" src tests` over obelisk-relay returns no hits. The relay does not enforce or emit `previous`. |
| Visibility | `private` (content reads) and `hidden` (metadata discovery) flags; `can_see_event` (`group.rs:2059-2130`): public → anyone, private → NIP-42 authed member/admin/relay key/author, `auth_required` error otherwise |
| Roles/permissions | `GroupRole` enum (`group.rs:334-350`); `can_edit_members`, `can_edit_metadata`, `can_create_invites`, `can_delete_group`, `can_delete_event` (`:1972-2058`) |
| NIP-42 | on: `relay_config.enable_auth = true` (`src/server.rs:327-328`) |

### Obelisk-specific, relay-level (not NIP-29)
- **Relay admission tiers**: blacklist, manual whitelist, follow-derived, WoT, relay invite codes (`OR/CLAUDE.md:258-262`; `src/whitelist.rs` 734 lines, `src/relay_invites.rs` 1221 lines, `docs/relay-invites.md`). Invite codes are 20 base62 chars, stored in `config/relay_invites.json`, not in the event store (`docs/relay-invites.md` §2). Kind 28934 (`KIND_CLAIM_28934`, `group.rs:52`) is a NIP-43-style claim.
- **WoT admission**: local BFS over kind-3 follow graph (`src/wot_graph.rs:119-155`, `distance()` with `max_hops`), inputs = contact lists on-relay plus fetched from damus/nos.lol/purplepag.es (`wot_graph.rs:34-38`) for reference accounts, budget `DEFAULT_MAX_REMOTE_FETCHES = 25_000`. Optional external oracle `nostr-wot-oracle` (`src/wot.rs:1-45`); an unreachable oracle never admits (`wot.rs:20-25`). Incomplete graph fails closed.

### Change history / provenance
- Git history: 61 commits, **single author Fabricio Acosta**, earliest 2026-09-15 (`git log`). History appears squashed/rebased at that date; the verse-pbc/Cadenas history is not present, so the diff against upstream is not derivable from this tree. The fork is declared in `README.md:266-267` ("A fork of verse-pbc/groups_relay; the copyleft is inherited, not chosen") and `CLAUDE.md:3,258-262`.
- Divergence markers visible in code: Obelisk additions to `NON_GROUP_ALLOWED_KINDS` (wallet, push, claim, manifest kinds; `group.rs:95-117`); `follow_sync.rs`, `wot*.rs`, `relay_invites.rs`, `reports.rs`, `obelisk_*.rs`, `pruner.rs`, `compaction.rs`, `storage_history.rs`, `admin.rs` look entirely Obelisk-side. `group.rs`/`groups.rs` carry both (the 9006 comment and `generate_roles_event` are examples of compat patches).

### Licence text and headers
- `OR/LICENSE`: stock GNU AGPL v3, 19 Nov 2007, unmodified FSF text with the template placeholder `<year> <name of author>` left unfilled (`LICENSE:633`). No per-file copyright or SPDX headers: `grep SPDX` over `src/` returns nothing, and `grep Copyright` finds only the FSF boilerplate. No `NOTICE` file.
- The only copyright-holder trace in the tree is `authors = ["Daniel Cadenas"]` in `Cargo.toml:5`. Whether that line plus the `license = "AGPL-3.0-only"` equals the upstream notice cannot be verified offline; upstream `verse-pbc/groups_relay` was not fetched (sources are read-only local copies). Treat as: upstream copyright holder = verse-pbc / Daniel Cadenas, Obelisk contributors on top. **To confirm before any port: fetch upstream LICENSE and file headers.**
- `OR/README.md:261` and `Cargo.toml` agree: AGPL-3.0-only. Obelisk client `OB/package.json` also `AGPL-3.0-only`; `OB/LICENSE` AGPL v3.
- Dependencies `relay_builder`/`websocket_builder` licences are not visible locally (git deps); check their repos before reusing.

---

## 2. obelisk client (TypeScript, AGPL-3.0-only)

- **Group addressing = relay + id.** NIP-29's `host'id` is carried as a URL/query pair: `channelInviteLink(href, groupId, relay)` builds `?c=<groupId>&relay=<shortHost(relay)>` via `pageLink` (`OB/src/utils/chat/channel/channel-link.ts:20-35`); `relayHostLabel` takes the host as display label (`OB/src/utils/relay-url/relay-host.ts`). It is a per-relay client: each relay URL is an independent server with its own groups; there is no cross-relay group identity or sync.
- **Federation:** none. The only "mesh" in the client is WebRTC voice signalling (`src/services/voice/mesh-*.ts`), unrelated.
- **WoT** (`OB/src/services/wot/`): client-side filter, not admission. `engine.ts:1-40` documents the contract: synchronous `isAllowed(pubkey, kind?)` on the ingest hot path; if unresolved → **fail-open and enqueue** for batch resolution via a browser WoT extension (`extension.ts`, `wotBatch`); later denials notify UI. Policy (`policy.ts`): `allow` iff `distance <= maxHops` AND (`paths == null` OR `paths >= minPaths`). `minPaths` defaults to 1; higher = corroborating follows required so one rogue follower cannot vouch for spam. Mutes (NIP-51 + local) override allow; blocks are a hard denylist; always-allow kinds include own events, 39000/39001/39002 and 9007. The graph itself is computed by an external extension/oracle, the client only caches verdicts (`verdict-cache.ts`).
- **Key management:** `src/lib/crypto/` has `session-vault.ts`, `vault-idb.ts`, `record-cipher.ts`, `webcrypto.ts` (encrypted-at-rest vault in IndexedDB); NIP-07/NIP-46 touchpoints exist in `services/wallet` and `services/social`. Not studied in depth: our `nostr-bbs-core` already covers keys, and AGPL client code is irrelevant to our Rust core.

---

## 3. obelisk-menhir (MIT, Rust)

- `MH/LICENSE`: MIT, "Copyright (c) 2026 Obelisk"; workspace `license = "MIT"` (`MH/Cargo.toml:15`). Crates: `menhir-core` (events/filter/keys/client, ~900 lines), `menhir-relay` (axum + `rusqlite` bundled, ~2000 lines), `menhir-cli`.
- **Self-described as a rebuild, not a copy:** "Concepts borrowed from obelisk-relay, rebuilt small" (`MH/README.md:19`). It is a "NIP-29-lite" text-only relay: kinds 0, 9, 9000-9002, 9007, 9008, 9021, 9022, 22242 and custom 20284 invite redeem (`MH/crates/menhir-core/src/kinds.rs:1-48`; `MH/docs/PROTOCOL.md:11-27`). No 9005/9006/9009/39003, no `previous`, no federation.
- **Reusable without AGPL obligations (MIT: keep copyright + licence text only):**
  - `kinds.rs` kind-class helpers (`is_ephemeral/replaceable/addressable`, `:44-61`) — trivial, we have equivalents.
  - `server.rs` tag-count caps ("each indexed tag becomes a row", `:24-26,220-222`) and cheap-reject-before-signature-check ordering — idea only; our relay-worker already bounds events.
  - `db.rs` (669 lines) group/membership SQLite schema — useful as a reference for relational group state, but we store in D1/DO; port is a schema read, S effort.
  - `tor.rs` — irrelevant to CF Workers.
  - Anything copied keeps MIT notice; because MIT is compatible with AGPL-3.0-only the result may live in our AGPL crates. It should **not** go into `nostr-bbs-core` unless that crate carries the MIT notice file per copied snippet (core is MIT OR Apache-2.0; MIT-origin code is fine there with attribution, but menhir offers nothing core lacks).
- Caveat: Menhir's relay logic is described as derived in concept from the AGPL obelisk-relay. Because it was "rebuilt small" its MIT licence is the authors' to grant, but if any file is a close translation of AGPL code the MIT label would not be a safe harbour. Spot-check before copying anything beyond trivial helpers; I did not diff them.

---

## 4. Our forum today

### Channel/zone model
- Public channels are **NIP-28** (kinds 40/41/42), channel id = kind-40 event id, with the `root`-marked `e` tag identifying the channel (`FORUM/crates/nostr-bbs-relay-worker/src/relay_do/filter.rs:316-337`: `channel_tag`, `zone_channel_id`). Zones are config-driven access tiers declared in `forum.toml [[zones]]` and exposed as `ZONE_CONFIG` (`zone_config.rs:1-55`); zone gates decide read/write of channel events. Kinds 40/41/42 are hard-coded in several places (ADR-2029 context lists `nip_handlers.rs:69,1414,1556,2354,2549`, `profile_stats.rs:11`).
- ADR-2029 (`FORUM/docs/adr/ADR-2029-room-profile-deployment-type.md`, status *proposed, not implemented*) adds a second `room` profile (Anagram kind 34550 rooms + NIP-171 private groups + NIP-17 DMs). It is the natural home for "groups as virtual spaces" but does **not** federate: it mentions only publishing room traffic to the definition's `relay` tags (like nostr.chat) and a `successor`/`predecessor` tag.

### NIP-29: advertised vs real
- **Advertised:** NIP-11 `supported_nips` includes 29 (`nip11.rs:207`: `[1, 9, 11, 16, 29, 33, 40, 42, 45, 50, 56, 59, 65, 98]`).
- **Real:** `nip_handlers.rs:51-53` defines `is_nip29_admin_kind` = kinds 9000-9020 and 39000-39002. At `:1298-1311` the handler requires an `h` tag and an admin author, then **falls through to normal storage**; the code literally carries `// NIP-29 TODO: ... full group metadata should be relay-key-generated rather than accepted from arbitrary clients`. There is no group registry, no membership check on kind 9, no 9021 join handling, no relay-signed 39000-39002 generation, no 39003, no invite 9009, no private-group read gate keyed on `h`.
- **Core builders without callers:** `nostr-bbs-core/src/groups.rs` (718 lines, `pub mod groups` at `lib.rs:33`) builds kind 9, 9000, 9001, 9005, 9021, 9024 (registration), and `build_group_metadata/admins/members` for 39000-39002 (`groups.rs:124-306`). Repo-wide grep finds **no caller** outside the file itself (ADR-2029 context confirms: "with no callers"). Tag convention `["h", id]` ok. So NIP-29 is advertised on a thin admin-only gate; strictly the NIP-11 claim overstates capability (compare the careful NIP-17/NIP-90 non-claims at `nip11.rs:198-205`).
- Related prior art in our own ADRs: ADR-2028 "zones adopt NIP-171 key plane", ADR-2016 e2e zones. Zone ≈ a NIP-29 group with `private` + `closed`, minus the protocol.

`docs/deployment-profiles.md` (forum vs room table) has no mesh/federation content; it confirms the `room` profile is public-read and not federated.

### How nostr-bbs-mesh federates today
- **Envelope:** IS-Envelope v1, JCS-canonicalised (`mesh/src/envelope.rs`, `jcs.rs`), gift-wrapped into kind 1059 and published as `["EVENT", ...]` to peer relays (`lib.rs:14-27`; `transport.rs` `send_envelope`/`recv_envelope`/`decode_incoming_wrap`). `via` chain limited to `MAX_VIA_HOPS = 4` (`envelope.rs:59,266`), default TTL 7 days (`:67`), dedup key = outer kind-1059 event id (`transport.rs:294`, ADR-075 §D12). It carries *messages between systems* (chat/service envelopes with LDN/AS2 mapping), not NIP-29 state.
- **Fan-out and dedup:** `PeerManager` + `SeenIds` capacity-bounded LRU for loop prevention (`transport.rs:579-663`). `MeshConfig` (`config.rs:53-78`): `mode` standalone/federated/client, `peer_relays`, `federated_kinds` allowlist, `federated_pubkeys`, `honor_remote_moderation`, `allowed_remote_dids`, `delegation_required`, `client_relay`. Relay-worker glue: `FederationManager::plan_fanout(kind, author, event_id)` applies kind ∩ pubkey allowlists + not-yet-seen (`relay-worker/src/mesh.rs:180-192`); inbound events from recognised mesh peers are filtered to `federated_kinds` (`nip_handlers.rs:1172-1182`, `is_mesh_peer :3575`). Mirror anchors: kind 30033 `d`=source relay (`mesh/lib.rs:70-73`).
- **Honest status:** the crate's own docs call the relay-worker module wiring "compiles"; the worker-side `MeshSocket` implementation over `worker::WebSocket` is described as the integration seam (`relay-worker/src/mesh.rs:37-42`). I did not verify end-to-end operation.

### Gap to "federated NIP-29 groups across nodes"
| Gap | Detail |
|---|---|
| G1 No group state machine | No registry, roles or membership; needs the NIP-29 relay behaviours above first. |
| G2 No canonical-home concept | NIP-29 says a group belongs to one relay, but "the community may move the group or fork it" with the same id across relays (`OR/docs/nip29spec.md:19,147`). Need an explicit `home` per group (relay URL + pubkey that signs 39000-39003) vs `mirror` (read replica or active fork). |
| G3 Moderation authority | 9000-9022 only valid at the home relay; mirrors must accept them only when signed by an admin listed in the home's current 39001 (or by the home relay key). `honor_remote_moderation` in `MeshConfig` is the hook but has no semantics for groups. |
| G4 Member-list replication | 39002 is relay-signed state; a mirror cannot re-sign it. Replicate the home relay's signed 39000-39003 events verbatim (addressable `d` = id, `created_at` newest-wins). Verify signer = home relay pubkey pinned in config. Our `storage.rs:119-133` replacement logic has no late-older/same-second guard (ADR-2029 context), which would corrupt mirror ordering. |
| G5 Content replication | Kind 9 content events carry author signatures and can be mirrored verbatim; mirrors must re-check `h` membership against the *replicated* member list. Mesh dedup (`SeenIds`) is in-memory and lost on DO eviction; need persisted id index. |
| G6 Wire transport | Mesh sends gift-wrapped envelopes. Group sync wants plain signed events (REQ/EVENT between relays, or NIP-77 negentropy), not wrapped envelopes; the mesh `PeerManager` fan-out is reusable, the envelope pipeline is not. |
| G7 Identity | Group id must become `host'id` (NIP-29 canonical) or `(home_relay, id)`; ADR-2029 currently keys rooms by `a` tag address. |
| G8 Advertised capability | NIP-11 should only list 29 once G1 exists; add `nostr_bbs.federation` block (home/mirror, peers). |

---

## 5. Port assessment

All obelisk-relay code is **AGPL-3.0-only**; menhir is **MIT**. Port targets must be AGPL crates (`nostr-bbs-relay-worker`, `nostr-bbs-mesh`), never `nostr-bbs-core` (MIT OR Apache-2.0, published).

| Module / feature | Source (repo:path, licence) | Value for multi-node forum | Port target crate | Effort | Clean-room route (from NIP-29 spec) | Recommendation |
|---|---|---|---|---|---|---|
| Group state machine: members, roles, metadata, apply 9000-9022 | `OR/src/group.rs` (4045 lines), `groups.rs` (3766), AGPL | High (the missing G1) but tied to `nostr_sdk`, `Scope`, `relay_builder::Error` and in-memory `DashMap` | `nostr-bbs-relay-worker` (new `groups/` module) with pure logic in a new AGPL crate `nostr-bbs-groups` | L if ported, M if clean-room | NIP-29 text is short; state = fold over kinds 9000-9022 ordered by `created_at`/id. Our `core/groups.rs` already holds the builders. | **Clean-room.** Port would drag verse-pbc types and AGPL notices for little gain. |
| 39000-39003 generation (relay-signed) | `OR/src/group.rs:1770-1970`, AGPL | High: replicable state for mirrors | relay-worker | S | Straight from spec; builders exist in `core/groups.rs:254-306` | Clean-room; reuse core builders (add 39003 and roles). |
| Visibility rules `can_see_event` (public/private/hidden) | `OR/src/group.rs:2059`, AGPL | Medium: maps to our zone visibility | relay-worker `zone_config`/`nip42` | S | Spec semantics: `private`, `hidden`, `closed`, `restricted` flags | Clean-room; read for edge cases only. |
| Group invite codes (9009 + 9021 `code`) | `OR/src/group.rs:457-540,1385`, AGPL | Medium for virtual-space onboarding | relay-worker | S-M | Spec defines 9009; policy (expiry/uses) is ours | Clean-room. |
| Relay-level invites with referral tree | `OR/src/relay_invites.rs`, `docs/relay-invites.md`, AGPL | Low-medium: our whitelist/trust ladder (ADR-2014/2006) already covers admission | relay-worker `whitelist.rs` | M | Own design; doc is a good spec | Skip / borrow idea. |
| Local WoT graph BFS + fail-closed rules | `OR/src/wot_graph.rs`, `wot.rs`, AGPL | Medium: cross-node trust signal (is a remote author trusted by us?) | `nostr-bbs-relay-worker` `trust.rs` | M | BFS over kind-3 is textbook; `minPaths` idea is from client | Clean-room; adopt "incomplete graph fails closed" and `minPaths` ideas. |
| Client WoT policy (`maxHops`, `minPaths`, always-allow kinds) | `OB/src/services/wot/policy.ts`, AGPL (TS) | Low direct; design reference | relay-worker/forum-client | S | Trivial rule | Clean-room. |
| Federation / negentropy sync | none (roadmap only, `OR/ROADMAP.md:148`) | Highest | `nostr-bbs-mesh` | L | NIP-77 + NIP-29 mirror semantics | **Design ourselves**; nothing to port. |
| LMDB storage, compaction, pruner, metrics | `OR/src/compaction.rs`, `pruner.rs`, AGPL | None on CF Workers (D1/DO) | n/a | n/a | n/a | Do not port. |
| Group-state SQLite schema | `MH/crates/menhir-relay/src/db.rs`, MIT | Low (reference for relational schema) | relay-worker D1 migration | S | n/a | Read only; copy nothing needed. |
| Tag-count/size caps before sig check | `MH/.../server.rs:24-26,220`, MIT | Low (we have bounds) | relay-worker | S | Trivial | Idea only. |
| Obelisk HTTP bootstrap/messages index (NIP-98) | `OR/src/obelisk_index.rs`, `obelisk_api.rs`, AGPL | Low-medium: bulk fetch for mirror bootstrap | relay-worker | M | Own REST + NIP-98 (already in use) | Skip. |
| `previous` timeline refs | not implemented in OR or MH | Medium (anti-fork / ordering across mirrors) | core builder + relay-worker check | S-M | Spec only | Implement from spec; no source to port. |

### AGPL obligations if anything is ported verbatim
- **Attribution:** AGPL §5(a) requires prominent notices that you modified the work and the date; §4 requires keeping all copyright/licence notices intact. obelisk-relay has no per-file headers and no NOTICE, so the practical content is: keep the licence text, preserve any headers present in the copied files at copy time, add an in-file `// Portions derived from obelisk-app/obelisk-relay (AGPL-3.0-only), itself derived from verse-pbc/groups_relay (Daniel Cadenas et al.)` comment, record it in a repo `NOTICE`/`THIRD_PARTY.md`, and record the decision in the ADR. Verify upstream copyright holders first (gap above).
- **Licence compatibility:** AGPL-3.0-only into our AGPL-3.0-only crates is fine. It is **one-way**: the resulting crate cannot be relicensed MIT/Apache, and it must never be a dependency of `nostr-bbs-core`. A new `nostr-bbs-groups` crate must be marked `AGPL-3.0-only` and should be kept out of core's dependency tree (check with `cargo tree -p nostr-bbs-core`).
- **§13 (network use) on Cloudflare Workers:** AGPL §13 applies to modified versions that users interact with remotely: operators must offer those users the Corresponding Source of the running version. For a Workers deployment: (1) our forum crates are already AGPL so a source-offer is presumably already given (check the forum UI/NIP-11 for a source link; `nostr_bbs` NIP-11 block could carry `source_url`, `licence`, and the deployed commit); (2) there's nothing extra for the Workers runtime itself, since the obligation attaches to the code that was modified and the users interacting with it, but the offer must cover the ported files and a build recipe (wasm artefacts built from the published commit). Forks of the deployment must do the same. Treat as: publish exact deployed commit, ideally in NIP-11 and the client footer. This is standard reading, not legal advice.
- **Clean-room route needs no licence** beyond not copying expression. To keep that defensible: author from the NIP-29 text and our own tests, don't paste or closely transliterate from `group.rs`, and record in the ADR that the obelisk-relay source was read only for behavioural edge cases.

---

## 6. crates.io check (queried 2026-10-30 via `curl https://crates.io/api/v1/crates/<name>`)

| Name | Result |
|---|---|
| `groups_relay`, `groups-relay` | not on crates.io |
| `relay_builder` (verse-pbc) | not on crates.io (git dep only) |
| `websocket_builder` | not on crates.io |
| `nostr-relay-builder` | exists: 0.45.0-alpha.3, MIT, repo rust-nostr/nostr, updated 2026-06-27. rust-nostr's relay builder, tokio-based, **a different project** from verse-pbc/relay_builder; no NIP-29 group logic (no evidence otherwise found; not inspected in depth); not CF-Workers-compatible. |
| `nostr-lmdb` | exists: 0.45.3, MIT, repo nostrdevkit/nostr (LMDB; native-only, no use on Workers) |

So there is nothing on crates.io to depend on for NIP-29 group logic. A clean-room `nostr-bbs-groups` (AGPL) is the only route, and if it proves clean and reusable, a candidate for later publication under the global crate policy (rustdoc, README, licence). Under the "never publish bespoke formats" rule, a pure NIP-29 state machine is protocol-standard, so that rule does not block it. Licence for publication would need a deliberate choice: clean-room (no obelisk code) could be MIT/Apache; any ported code would force AGPL.

---

## Suggested direction for the lead's plan/ADRs

1. **ADR: NIP-29 group engine, clean-room** in a new AGPL crate `nostr-bbs-groups` (pure, no I/O, `?Send`-friendly, wasm32), consumed by relay-worker. Wire the existing `core/groups.rs` builders; add 9006?/9009/9022/39003; relay-signed 3900x with the relay key; fix `nip_handlers.rs:1298` TODO; until done, drop `29` from `supported_nips` or qualify it.
2. **ADR: group home/mirror model.** Group identity `host'id`; `home` relay signs 39000-39003 (pin pubkey in config); mirrors replicate those events verbatim and enforce membership from them; moderation honoured only from home-listed admins (`honor_remote_moderation` extended to groups); kind-9 content mirrored verbatim, dedup persisted; optional `previous` tags for ordering; forking = new home, same id, explicit `successor`/`predecessor` (reuse ADR-2029 tags).
3. **Transport:** reuse `PeerManager`/`SeenIds`/allowlists from `nostr-bbs-mesh`, but add plain-event replication (REQ/EVENT or NIP-77), not gift-wrap envelopes, for group sync.
4. **Virtual spaces:** a group = space with metadata (name/about/picture, `private`/`closed`), zone mapping = group flags; map `[[zones]]` to groups so one config drives both dialects (ties into ADR-2029 `room` profile).
5. **Licence hygiene:** keep `nostr-bbs-core` free of any obelisk-derived code; add a CI check (`cargo tree`/`cargo deny`) that no AGPL crate is a dependency of core.

## Open items / not verified
- Upstream `verse-pbc/groups_relay` LICENSE and file headers (not fetched; Cargo `authors` is the only trace).
- Whether menhir files were derived closely from obelisk-relay (not diffed).
- End-to-end functioning of our mesh in production; `relay_do` mesh socket was not executed.
- `obelisk` client key-management details (only directory-level survey).
