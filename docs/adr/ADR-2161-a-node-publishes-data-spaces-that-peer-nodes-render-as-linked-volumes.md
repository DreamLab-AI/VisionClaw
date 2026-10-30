---
id: ADR-2161
title: A node publishes data spaces that peer nodes render as linked volumes
date: 2026-10-30
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: e74bcddc772d47a45b1971156a2bab540eaa027c
verified_paths: []
owner: jjohare
review_trigger: a second operator joining (cross-operator federation, which un-parks agentbox ADR-2025/2101); Codescape reaching its P6 auth; any change to the per-node visibility filter, the /wss upgrade auth or the ADR-2061 kind map; a request for cross-node writes or ontology alignment
repo: visionclaw
domain: IDENTITY-authority-chain
depends_on: [ADR-2159, ADR-2141, ADR-2021, ADR-2025]
amends: [ADR-2025]
relates: [ADR-2157, ADR-2158, ADR-2162, ADR-2013]
---

# ADR-2161 — A node publishes data spaces that peer nodes render as linked volumes

## Context
- **No federation exists yet.** VisionClaw nodes do not federate today. The only cross-boundary path is agentbox → VisionClaw agent events (`/wss/agent-events`, `src/main.rs:1113`), and nothing reads agentbox `[mesh]` (research 06).
- **Graph identity is node-local.** Wire node ids come from a process-global counter, are ephemeral (ADR-2024) and collide across sources. `Node` and `Edge` carry no origin, and an edge is two `u32` ids (research 07).
- **The read gate is coarse.** Release `/wss` refuses unauthenticated upgrades (`src/handlers/socket_flow_handler/http_handler.rs:296-323`). After that it filters per node by `owner_pubkey` or `public` only (`types.rs:269`), and it requires an Origin header (`:67-140`). The native XR client sends none.
- **Retraction is not wired.** `/v1/uri` documents 410 but never emits it. Pod, dataset and memory URNs are refused at the boundary by the ADR-2061 closed map.
- **Codescape is unauthenticated.** It accepts `authenticate` without verifying it until its P6.
- **What the operator wants:** a node's visualisation shows another node's data space beside its own, with links between them. Detail: `docs/proposals/spatial-rooms/research/06-*.md`, `07-*.md`.

## Decision
1. **A data space is a declared, read-only projection at its home node.** It is named `urn:visionclaw:dataspace:<node-key-hex>:<space-id>`, where `<node-key-hex>` is the home node federation key (64 lowercase hex, ADR-2162) and `<space-id>` matches `[a-z0-9-]{1,48}`. It is a closed `<content-ref>` form (forum ADR-2032 D2, the closed reference set, admits only this, `urn:visionclaw:codescape:<scene>`, or a path on the home's own node). The URN has a single typed mint and parse under ADR-2021. A selector evaluated at the home defines it: graph type, owner set, ontology class set and the `public` flag. It never copies data and is never a second store.
2. **Discovery is a signed, replaceable catalogue, with no new kind.**
   - **One key, no VisionClaw key.** The node federation key is held by the agentbox mesh service (ADR-2162 D4). VisionClaw holds no federation key. It signs catalogues, notices and retractions by calling the mesh service's local signing socket, which is uid-gated and signs only those payload types.
   - The catalogue is a NIP-78 kind 30078 replaceable event per space, with `d` = `visionclaw:dataspace:<space-id>`, carrying `["rev", <u64>]`. The latest `rev` wins and an older one is ignored. The `endpoint` tag is the home's `wss://` URL.
   - **Public metadata is minimal.** The restriction applies to *descriptive* metadata: a public catalogue entry carries, beyond the contract fields, only the space id and the `acl` kind (`public`, `dids` or `space`). The contract fields stay: `d`, `rev`, `endpoint` (or, for a withdrawal, `status`), plus the event's id, pubkey and signature. Counts, hints, names and descriptions of a non-public space are served only to a viewer the `acl` authorises, never in the public catalogue.
   - **Storage (final audit #9).** The catalogue d-tag namespace `visionclaw:dataspace:*` is stored revision-ordered by the forum relay and by any relay that carries it: the D1 `rev` migration (forum ADR-2031 D7) covers kind 30078 in exactly that d-tag namespace, so a same-second withdrawal at a greater `rev` is never discarded for a lower one. Other 30078 namespaces keep NIP-01 ordering. Consumers still check `rev` themselves. A pinned peer relay that does not apply revision-aware storage for this namespace is outside the guarantee, and the consumer's persisted watermark is then the only defence.
   - **Withdrawal is a replacement, not a removal (round 3).** The home publishes the same `d` at a greater `rev` carrying `["status", "withdrawn"]` and no endpoint, then issues the `retraction` (point 7). Consumers keep a persisted watermark per (K, space id) holding (`rev`, event id), so a relay replaying an older catalogue event after a restart cannot resurrect a withdrawn space. A NIP-09 deletion may follow as a courtesy; nothing relies on it.
   - It is published to the pinned peer relays and served verbatim at `/.well-known/visionclaw/dataspaces`.
   - A consumer accepts a catalogue entry only if a **pinned** peer key signed it (point 3) and either its `endpoint` exactly equals the endpoint configured for that key (a mismatch is rejected), or it is a withdrawal (`["status", "withdrawn"]`, no endpoint), which is accepted on signature, pinned key and greater `rev` alone and removes the space. Discovery outside the pinned set does not exist.
3. **Peers are pinned by the operator. The default is same-operator.**
   - The peer set is the list of node federation pubkeys, endpoints and origins in agentbox `[mesh]`. ADR-2162 projects it to VisionClaw. Configuration is authoritative for the key-to-endpoint binding.
   - **Rotation is a new node identity.** The URN embeds the node key, so a new key means new URNs. Before retirement the old key signs a retraction for every URN it named; links are not migrated in v1.
   - Keys are pinned out of band and never learned on first use.
   - Cross-operator federation is a separate decision. It un-parks agentbox ADR-2025 and ADR-2101, which this ADR does not change.
4. **The viewer reads the home directly, as themselves.**
   - The client opens one more connection to the home's endpoint, one per (source node, dataspace) (ADR-2159). It authenticates with the **viewer's own** NIP-98 key, never a node key or a node-issued token.
   - **Federated-read connection mode (server-side contract).** At upgrade the home binds the connection to (authenticated viewer DID, one dataspace). Every output and lookup on it (nodes, edges, counts, positions, search) is filtered server-side by **selector ∩ object visibility ∩ `acl`**. The server **refuses mutations** in this mode; `BackendCaps` is UX only and is never the control. The home's existing upgrade auth and visibility filter run first.
   - The `acl` is one of:
     - `public`: any authenticated DID;
     - `dids`: an explicit allowlist;
     - `space:<nip29-room-urn>`: membership through `RoomAdmission`, ADR-2157. The home looks the URN up in ADR-2157's room binding registry; an unregistered or unbound (`sha256-12`) URN denies.
   - **Live revocation.** Changes to the `acl` (for `space:`, a removal or a stale 39002; for `dids`, an allowlist edit), to the selector and to object visibility are re-evaluated when they happen and at least every C = 10 s. A connection that loses access is closed. The end-to-end bound is D = 90 s (ADR-2157).
   - **Origin is browser policy only.** The home's `/wss` upgrade requires an Origin header (`:67-140`), so the native XR client sends one, and the home derives its allowed Origins from the pinned peer set. An Origin is self-supplied: it is never evidence of a node's provenance and never admits anything.
   - A read is possible only against a **pinned peer**: the catalogue entry was signed by a pinned key and its endpoint matches the configured one. That is necessary, not sufficient; the viewer's own NIP-98 identity and the `acl` decide the read. A pinned peer key never reads on a viewer's behalf.
   - The remote source is read-only, server-side first and in `BackendCaps` second.
   - Positions arrive as the home's acknowledged stream (ADR-2141). The consumer never simulates remote nodes.
5. **Links are owned by the node that asserts them. Content is never copied.**
   - A cross-space link is a record `(subject URN, predicate, object URN, asserting node key)`. There are two classes (final audit #30). The asserting node's own record is **primary local data**: authored there, that node is its sole write-master, it is stored in the asserting node's own store, erasable by its author and **not disposable**; its lease T means "re-announce to peers", not "drop". A copy received by `link_notice` at the object's owner, and any viewer render state, is a **foreign reference** (point 7). **Ownership (round 3):** the subject is under the asserting node's key; the object is under the receiving node's key. A `link_notice` therefore goes from the asserting node to the object's owner.
   - A remote URN is an opaque reference. No server dereferences it into local storage. Remote nodes are render-only at the viewer: no consumer persists them durably, imports them server-side or backs them up. The viewer's per-connection render store (ADR-2159) is permitted as disposable session memory, needed for viewing, and is discarded on disconnect.
   - For linkable nodes, the initial graph load carries each node's `urn` (a PROTOCOL-registry change). The client resolves link endpoints against `id↔urn` tables keyed by (node key, dataspace, stream generation), so the same node in two overlapping spaces with different ACLs never shares a table.
   - A link is rendered only if the viewer is authorised for **both** ends. Otherwise nothing is drawn, with no stub and no boundary marker, so the link's existence is not disclosed.
6. **The node-to-node channel carries control, never content.**
   - It uses IS-Envelopes (`nostr-bbs-mesh`, AGPL-3.0-only, the same licence as both consumers) authenticated by the node federation key over the pinned peer relays. VisionClaw obtains the signature from the mesh service's socket (point 2); it holds no key.
   - There are exactly three envelope types: `dataspace_announce`, `link_notice` (optional courtesy to the home) and `retraction`. A retraction is a list of URNs that the home has erased or withdrawn. The catalogue is not an envelope type: it is the separately signed kind-30078 event of point 2. A `dataspace_announce` **references** it (the space URN, the catalogue event id and its `rev`) and never carries or substitutes for it; a consumer acts on the announce only after fetching and validating the catalogue event under point 2's pinned-key, endpoint and `rev` rules. The agentbox signer allowlist distinguishes the two: it signs catalogue events (kind 30078) and the three envelope types, as separate payload classes (ADR-2162 D4, D7).
   - **Authority checks** (agentbox ADR-2162 D7 enforces them in the mesh service). A `retraction` of `urn:visionclaw:dataspace:<K>:…` is accepted only from an envelope authenticated by `K`, since the URN names its own authority. A `link_notice` is accepted only if its asserting node equals the authenticated sender, the link's subject URN is under the sender's key and its object URN is under the receiving node's key. Anything else is dropped and logged.
   - **Replay.** Each sender carries a persisted per-sender sequence number, and a replayed or lower sequence is rejected. Accepted envelopes are written durably before they are acted on.
   - This ADR adds no new cryptography and publishes no new envelope format.
7. **Retraction is lease-bounded, in three deadlines.**
   - **Home erasure:** the home emits 410 at `/v1/uri` for erased subjects, live immediately; that is a prerequisite.
   - **Display removal:** on receipt of a `retraction`, or on the next (re)connect, whichever comes first, the consumer withdraws from its render stores anything the home no longer serves, and tombstones every link naming the URN.
   - **Reference purge:** within min(receipt + 24 h, lease expiry), the consumer purges those link records.
   - **Links are leases.** Every link record held as a foreign reference expires after lease **T = 7 days**, renewed by the home's catalogue refresh and the asserting node's re-announcement. An offline consumer drops that reference at lease expiry whether or not it ever received the retraction. (The asserting node's own record is not dropped at expiry; it re-announces. Point 5.) Tombstones persist for T + 1 day as anti-resurrection state, so a replayed link notice cannot revive a purged link.
   - **Two data classes (DATA-authority-erasure carries both rows).**
     - The **asserting node's own link record** is primary local data (point 5). Its renewal authority is the asserting node; the lease T means it re-announces to peers (a fresh `link_notice` and the catalogue refresh) every period. It is erased by its author, or tombstoned when the remote URN is retracted or 410s. It is not reconstructible from anything else, so it is never described as a disposable projection.
     - A **received copy** at the object's owner, and any viewer **render state**, is a **foreign reference**: a disposable projection, in memory or lease-bounded store, authoritative for nothing, expiring at T unless the asserting node re-announces, purged on retraction or lease expiry and reconstructible by that re-announcement. Dropping it loses nothing.
8. **The `dataspace` kind crosses.** agentbox adds it to `federation-kinds.json` first, as a crossing kind with fixture rows (ADR-2162 amends ADR-2061). VisionClaw then picks it up through the submodule pin, because `src/uri/mod.rs:662` is an `include_str!`. This amends ADR-2025's closed map by one arm.
9. **Out of scope for v1:**
   - Codescape as a federated source, until its P6.
   - Writes across nodes.
   - Ontology alignment. A link may carry IRIs for display, but Whelk does not reason over them.
   - Remote presence in a local room. A shared room is a NIP-29 space, ADR-2157.

## Planned evolution: cross-operator federation
Same-operator is the scope **for now** (operator, 2026-10-30); multi-operator federation is planned, not rejected. What changes when it lands, so v1 does not paint it into a corner:
- **Trust root.** Peers are pinned by the operator today. Cross-operator needs a peering agreement per remote operator: a pinned key exchanged out of band, plus a revocable per-peer policy (which spaces, which `acl` forms). The pin table already keys on node key, so this is an added policy column, not a schema change.
- **Parked ADRs.** It un-parks agentbox ADR-2025 (federation contract) and ADR-2101 (topology and key separation); the node key in ADR-2162 already follows ADR-2101's key-separation clause, so it carries over.
- **Visibility.** `public` and `dids` work unchanged across operators. `space:` (NIP-29-gated) needs the remote operator to trust the space's home relay key, which is the same pin mechanism.
- **Erasure.** Retraction and the T = 7 d lease already assume a peer we do not control; no change.
- **Not decided here.** Discovery of unknown operators, reputation, and any automatic peering. Each needs its own ADR.

## Consequences
- **One mechanism serves both cases.** A forum space can show another node's data space through its `volume` ref, a closed `<content-ref>` set. Whether the viewer may read it is still the home's decision, not the space's.
- **One key, one signer.** VisionClaw holds no federation key, so there is no second key to rotate or back up here. The cost is a runtime dependency: with the mesh service's socket down, VisionClaw cannot publish or retract, and fails closed.
- **Erasure at the home is immediate; remote copies are leased.** No node holds another node's graph content, so erasing at the home is complete once its 410 is live. Already-rendered remote content leaves the display on receipt or next connect, and references purge within min(receipt + 24 h, lease expiry), so an offline consumer is bounded by the 7-day lease.
- **The XR client must send an Origin header.** The home must derive allowed Origins from the peer set. Until both are done, a release home refuses native viewers. The Origin proves nothing about provenance.
- **Costs:**
  - one socket per (source node, dataspace) viewed, against the shared frame budget (ADR-2159);
  - a server-side connection mode and a re-evaluation loop every C on each federated connection;
  - persisted per-sender sequences, link leases and tombstones;
  - a new SECURITY-profiles entry for federation;
  - a new identifier form.
- **EDD expectations, which must hold before activation:**
  - **D1:** a catalogue entry signed by an unpinned key is ignored; an older `rev` or a mismatched endpoint is rejected; after a restart, a replayed pre-withdrawal catalogue event does not restore a withdrawn space.
  - **D2:** a viewer outside the `acl` receives no node of the space, including its count, and the public catalogue carries no count or description of a non-public space.
  - **D3:** no remote node content is written to the local store. A store diff across a viewing session is empty apart from link records.
  - **D4:** after a `retraction`, no link to the URN renders; a retraction from any key other than `K` is refused; a replayed sequence is dropped; an offline consumer drops a received foreign reference at lease expiry, while the asserting node's own record survives it and re-announces; no foreign reference persists beyond the three deadlines.
  - **D5:** a raw `/wss` client on a federated-read connection can neither mutate nor read outside selector ∩ visibility ∩ `acl`; a modified client gains nothing over `BackendCaps`.
  - **D6:** two sources with colliding wire ids, or one node in two overlapping spaces, render in separate volumes without cross-contamination, and a link with an unauthorised end draws nothing.
  - **D7:** an `acl` change, selector change or visibility change closes the connection within D = 90 s.

## Activation prerequisites
`depends_on` lists code dependencies only. Activation is a separate, feature-level gate (`AUDIT-DECISIONS.md` C9):
- **`public` and `dids`:** ADR-2159, agentbox ADR-2162 (step 1 of the cross-repo order) and the server-side stream contract of decision 4 (C7), plus 410 emission, the `urn` field on the wire and the `acl` gate. Neither the forum chain nor ADR-2157 is needed.
- **`space:<nip29-room-urn>`** additionally needs ADR-2157 live (its registry and `RoomAdmission`) and the forum chain (ADR-2030, then ADR-2031).
- Order: agentbox 2162; VisionClaw 2159, then 2161 (`public`, `dids`); forum 2030, then 2031; VisionClaw 2157, then 2161 (`space:`); VisionClaw 2160.
- Forum rendering of dataspace references consumes this ADR. It is never a prerequisite of it.

## Verification
`implementation_status: none`. Nothing here is implemented. Citations above were read at `e74bcddc772d47a45b1971156a2bab540eaa027c` (the draft's verification stamp) and are not re-checked in this revision. Cross-references: ADR-2157 (room binding registry, `RoomAdmission`), agentbox ADR-2162 (node key, peers, `dataspace` crossing; separate repository). Governing-doc entries for this ADR are in IDENTIFIER-taxonomy, PROTOCOL-registry, DATA-authority-erasure and IDENTITY-authority-chain, each marked proposed.
