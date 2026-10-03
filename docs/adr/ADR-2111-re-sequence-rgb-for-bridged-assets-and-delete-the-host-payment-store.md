---
id: ADR-2111
title: Re-sequence ADR-124/128 for the bridged-asset case only, delete FsPaymentStore and /pay/* in favour of a proxy to agentbox /v1/wallet, implement AnchorConfirmer against sidestr-node, delete the empty extraction/solid-pod-rs mirror, and disambiguate visionclaw-contracts
date: 2026-09-21
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit:
verified_paths: []
owner: jjohare
review_trigger: AnchorConfirmer first passing against a live sidestr-node; any proposal to declare a web contract at L2 or above; any proposal to validate RGB contract state in-chain; the solid-pod-rs post-port version landing
repo: visionclaw
domain: BASELINE-architecture
---

# ADR-2111 — Re-sequence ADR-124/128 for the bridged-asset case only, delete FsPaymentStore and /pay/* in favour of a proxy to agentbox /v1/wallet, implement AnchorConfirmer against sidestr-node, delete the empty extraction/solid-pod-rs mirror, and disambiguate visionclaw-contracts

## Context

This repo runs a third `did:nostr`-keyed sats ledger, `FsPaymentStore`
(`src/handlers/pay_handler.rs:198-403`), behind an HTTP 402 route set whose own doc table
records `POST /pay/.deposit` as a "Deposit stub (returns 501)" (`:13`); the handler confirms it
(`pay_deposit_handler` `:480-493`, `HttpResponse::NotImplemented()` at `:487`), so no value has
ever entered it. ADR-124 §7 specified a P21 containment set, currency hard-pinned to testnet,
cash-out and `.withdraw`/`.swap`/`.pool` disabled, an owner-plus-legal build gate and the
flags "anchored on-seal" (`docs/archive/adr/ADR-124-smart-contract-features-web-contracts.md:202`);
none was built. `AnchorConfirmer` (`src/web_contract/ritual.rs:144-151`), the seam that would
make `prevout_spent_once` real for `TrustLevel` (`src/web_contract/ritual.rs:59`), has only test
doubles. ADR-124 §2.2 defers RGB to L3 as "a rewrite, not a tightening" conflicting with a
k256-only, zero-rust-bitcoin-dep posture (`:89`), and §9 rejects the RGB-first alternative
(`:227-228`). The owner decided on 2026-09-21 that our own sidestr chains are the sole value
instrument and that assets are bridged in (PRD-024 D0, D4).

## Decision

1. **`FsPaymentStore` and the `/pay/*` route set are deleted.** There is nothing to migrate:
   the deposit path is a 501 stub, so the store holds no value. What replaces them is a thin
   proxy to agentbox `/v1/wallet/*` and `/v1/chain/*` (agentbox ADR-2098 D5), carrying the
   caller's NIP-98 identity through. This repo stops owning a ledger; the chain is the ledger
   of record (agentbox ADR-2099 D2).
2. **`AnchorConfirmer` is implemented against `sidestr-node`.** On a chain DreamLab signs, an
   anchor's spent status is a UTXO-set membership test, which is why this is the cheapest path
   to an honest `prevout_spent_once`. Until that implementation is green, the phrase "single-use
   seal" stays out of code, documents and interfaces per ADR-124 §2.3
   (`docs/archive/adr/ADR-124-smart-contract-features-web-contracts.md:91`); the words are "anchor",
   "peg", "claim" and "marker" (agentbox ADR-2099 D6).
3. **ADR-124 P3 and ADR-128 §6
   (`docs/archive/adr/ADR-128-build-out-canonical-gitmark-blocktrails.md:236`) are re-sequenced
   for the bridged-asset case only.** An external
   RGB20 asset, including USD₮ on RGB, may enter as a **wrapped asset** claimed on our chain by
   an isolated bridge process holding the origin asset in reserve (agentbox ADR-2102 D1). That
   is the whole of the re-sequencing, and it is narrower than it first appears.
4. **In-chain RGB stays deferred and hard-refused, unchanged.** No AluVM, no
   consignment-as-State-layer, no `rgb-core` in this repo's dependency graph. ADR-124's L2
   hard-refusal pending an implemented and independently audited adaptor-signature CET engine
   stands verbatim. This record amends the sequencing of one path; it does not raise any trust
   level.
5. **P21 is now implemented, as an on-seal property of the chain document.** The testnet
   currency pin, cash-out disablement and owner-plus-legal gate are written into the chain
   document before `genesisHash` is computed, with the approving kind-31403 event id recorded
   as `p21Receipt` (agentbox ADR-2103 D3, D4). Changing any of them is a new chain with a new
   genesis, never a configuration edit. ADR-124 §7's "architecturally enforced, not
   aspirational" finally has a mechanism, and it is not a flag in this repo.
6. **The ADR-124 §7 regulatory matrix is unchanged.** No cell is exempted by RGB, by
   client-side validation, by a bridge or by a sidechain. A fiat-referenced stablecoin adds the
   FCA stablecoin regime on top. Nothing in this record may be cited as regulatory relief.
7. **The k256-only, zero-rust-bitcoin-dep posture named in ADR-124 §2.2 is retired**
   estate-wide (agentbox ADR-2096 D3). It must not be cited as live; the RGB objection it
   carried is answered by the process boundary in agentbox ADR-2102 D3, not by the posture.
8. **`extraction/solid-pod-rs` is deleted.** The directory holds only a `.git` entry and no
   working tree. This repo depends on the published crate
   (`Cargo.toml:218`, currently `0.4.0-alpha.15`) and moves to the post-port version in
   lockstep with the forum (solid-pod-rs ADR-2008 D8).
9. **`visionclaw-contracts` is disambiguated in place.** `crates/visionclaw-contracts` is the
   cross-boundary envelope-schema crate
   (`crates/visionclaw-contracts/Cargo.toml:14`) and has nothing to do with web contracts;
   `src/web_contract/` holds the ADR-124 trust ladder, gitmark and blocktrails. Each gains a
   header stating what it is not, and every ADR-124 or ADR-128 citation names the path rather
   than the word "contracts".

## Source qualification — 2026-10-01

The `sidestr-rs` parity release does not implement this host decision. It supplies reusable
consensus, wallet, EVM and Hitch libraries, but VisionClaw still owns each host-side change:

- `FsPaymentStore` and `/pay/*` remain in `src/handlers/pay_handler.rs`;
- `AnchorConfirmer` remains a trait with test doubles in `src/web_contract/ritual.rs`;
- no proxy carries the caller's NIP-98 identity to agentbox `/v1/wallet/*` or
  `/v1/chain/*`;
- publishing `sidestr-hitch` does not supply wallet funding, relay transport, chain watches,
  durable storage or broadcasting for VisionClaw.

ADR-2111 therefore remains `proposed / none / inactive`. Closing sidestr source parity in
the estate board's N-9 row does not close N-3 or any verification item below.

## Consequences

The host loses a payment surface it never settled with, which removes one of the three
unsynced ledgers outright rather than reconciling it. Anything that called `/pay/*` moves to
the proxy or stops; the order-book and AMM routes here retire, since the exchange surface is
solid-pod-rs's. Implementing `AnchorConfirmer` makes `states.len() == txo.len()` checkable and
earns the L1 rung honestly, at the cost of a runtime dependency on `sidestr-node`. Bridging
USD₮ makes the federation a custodian of a stablecoin, which is a question for counsel before
any mainnet chain document is sealed, not a technical one. Deleting the extraction mirror
removes a silent second source of pod code.

## Verification

Proposed; nothing built. Ratification evidence will be:

- `grep -rn "FsPaymentStore" src/` empty, and `/pay/.balance` served by the proxy returning the
  same figure as agentbox `/v1/wallet/balance` for the same authenticated DID.
- A test in which `AnchorConfirmer::prevout_spent_once` returns true for a spent outpoint and
  false for an unspent one against a running `sidestr-node`, with no test double in the path.
- `cargo tree | grep -c "rgb-core\|aluvm"` zero in this workspace.
- A fixture chain document whose `parent` is a mainnet variant and whose `p21Receipt` is absent
  fails the build gate; mutating `p21Receipt` after genesis makes the node refuse the chain.
- `test -d extraction/solid-pod-rs` false; `Cargo.toml` pinning the post-port solid-pod-rs
  version identical to the forum's pin.
- The vocabulary lint passing over `docs/` and `src/web_contract/` with no occurrence of
  "single-use seal" outside a historical quotation.

## Sidechain payments on the agent graph — 2026-10-02 (stream S5)

This record owns VisionClaw's side of the estate sidechain (`AnchorConfirmer`,
the `/v1/wallet` proxy), so the render of agent-to-agent payments is recorded
here rather than in a new record (ratchet). It does not change D1–D9. Owner
decisions 2026-10-02: **SC1**, the demo chain is `sidestr:dreamlab-txbt4`
(prefix `drt`, sealed beside BLAKE2b testnet4); **SC2**, each payment is a sidechain transaction (sessions
are a later layer); **SC5**, the chain is not anchored to its parent yet and the
screen must never imply that it is.

What is built (`src/services/chain_payments.rs` is the one home of the contract
and its rules):

- **Source.** `AgentMonitorActor` reads `GET /v1/chain/payments` (agentbox,
  stream S3) on its existing poll, beside `/v1/tasks` and `/v1/status`. The body
  is `schema: "agentbox.chain.payments/1"`: `chain`, `mirror_url`,
  `tip {height, hash}`, `checkpoint` (`null` or `{parent, txid, height,
  covers_height}`), `payments[] {txid, payer, payee, amount_sats, block_height,
  block_hash, settled, time}` newest first, and `balances[] {did, settled_sats,
  fold_height}`. The fixture `tests/fixtures/sidechain/chain-payments.v1.json`
  pins it with `chain: "sidestr:dreamlab-txbt4"`. The chain id always comes
  from the route and rides on every edge (`chain`) and node (`chain_id`); it is
  never assumed. There is no default mirror either. Two chains run side by side,
  so a response without `mirror_url` gets no link rather than the other
  chain's mirror. Another schema value is refused, not guessed at. A 404 means the route
  is not deployed and clears the edges quietly.
- **Nullable fields (S3's route).** A row with `txid: null` never reached the
  chain (pending approval, failed or denied), so it is skipped and counted as
  `skipped_off_chain`, not as an error. A row with `payee: null` (the payee's
  binding did not verify) is dropped and counted as unverified. With
  `tip: null` (producer unreachable), payments keep their stored inclusion and
  no balance is shown, because a fold tier needs a height. One such row never
  invalidates the rest of the response.
- **Keying.** An edge joins two agent nodes by `did:nostr`. A node counts as a
  verified agent only when it is an agent node whose `did_nostr` survives the
  `uri::did_nostr()` round-trip (ADR-2022). Task records may now carry that DID
  (`TaskInfo.did_nostr`, `didNostr` or `did_nostr` on the wire), gated the same
  way. A payment whose payer or payee is not a verified agent is dropped and
  counted (`dropped_unverified`; a malformed DID counts as `dropped_malformed`).
  The counts are served, never hidden.
- **Edge.** One `chain_payment` edge per transaction, payer → payee, carrying
  `amount_sats`, `txid`, `block_height` and `settled`. The label is its own. It is
  not `hierarchical` and not `domain_member`, so the ADR-2035 ranker, which ranks
  only `rdfs:subClassOf` provenance, never layers it. It maps to the Structural
  force category and gets its own registry spring and its own colour (`#E040FB`).
  Bots node ids are reassigned on every roster update, so `GraphStateActor`
  re-resolves the edges by DID each time.
- **Settled rule.** A payment shows as settled only when the endpoint says
  `settled` **and** it sits in a block at or below the reported tip. A mempool
  payment, or one claimed settled in a block past the tip, shows as unsettled.
- **Two-tier balance rule.** A node badge shows `settled (chain fold @ h)` from
  the chain fold. The second tier, `in session (signed state n)`, belongs to
  `GET /v1/chain/sessions` (`agentbox.chain.sessions/1`, empty until Hitch
  sessions land). It is a separate line and is never summed into, or shown as,
  the settled figure.
- **Anchor rule.** The badge and the panel read "anchored" only when the response
  carries a parent checkpoint confirmed on the parent (non-null `height`). A
  checkpoint without one reads `checkpoint unconfirmed on <parent> (not anchored)`. With `checkpoint: null`, which is today's state,
  they read `<chain>: not anchored (checkpoints off)`, for example
  `dreamlab-txbt4: not anchored (checkpoints off)`. The anchor line is always
  drawn, so a node never reads as anchored by omission.
- **Surfaces.** `/api/bots/data` gains `chain` (the projection plus the
  cumulative drop counters). The client draws the edge with a midpoint label
  (amount, short txid, "unsettled" when so) and a node badge. A payments panel
  in the Agent Ops surface lists the last ten payments, each linked to the Pages
  mirror's `blocks.json`.
- **Rehearsal.** `scripts/activation/sidechain-render-rehearsal.sh` starts
  nothing. It diffs the screen's settled figures, payments and anchor state
  against the S4b witness receipt's replay (`sidechain-witness/1`), and exits
  non-zero on any mismatch. Its `--self-test` checks one matching fixture pair
  and three mutated mismatches.

Tests: Rust `services::chain_payments` (14), `actors::agent_monitor_actor` S5
cases (3), `SemanticEdgeType` and registry `chain_payment` cases. The client has
vitest suites under `client/src/features/bots/chain/__tests__` (26). The served
view is pinned by `tests/fixtures/sidechain/bots-data-chain.v1.json`, which the
Rust test `served_view_matches_the_pinned_screen_fixture` and the client tests
both read.

Open: the route itself is S3's (agentbox). Until agentbox echoes a DID on
`/v1/tasks` or on the MCP roster, no agent node is verified and every payment is
dropped and counted. That is the intended failure mode. The live rehearsal
waits on a running stack, the S3 route and an S4b receipt. On 2 October
`visionclaw_container` was not running, so neither the live diff nor the
screenshot was taken.

## Blocktrails alignment and the solid-pod-rs pin — 2026-10-03 (stream S4)

This amends D2 and D8 and applies D9 to `src/web_contract`. It adds no
decision and leaves the record `proposed / none / inactive`: no confirmer
answers from `sidestr-node` yet.

- **D8, the pin, is met.** `Cargo.toml` pins every solid-pod-rs crate the host
  uses (`solid-pod-rs`, `-nostr`, `-idp`, `-server`) at `=0.5.0-alpha.12`,
  the forum's pin at nostr-rust-forum `b73ec8c`. `crates/vault` reaches
  solid-pod-rs through `nostr-bbs-core`, which pins it exactly, and every
  published `nostr-bbs-core` pins an older alpha. `nostr-bbs-core` is
  therefore patched to the forum at `b73ec8c` until a release carrying alpha.12
  is published. The `extraction/solid-pod-rs` half of D8 is unchanged.
- **D2, restated: the trail is walked, not its tip.** The earlier seam asked
  whether the tip was confirmed and, at L1, whether a prevout was spent once.
  A confirmed tip shows only that someone paid the tip's key. Under
  Blocktrails (blocktrails/spec `ef54a08`, "the rule as it is") every mark's
  output key `x(P_i)` is the base key tweaked by every state so far, so the
  check that matters is per link. `AnchorConfirmer` is now `is_confirmed`,
  `output_key(txid, vout)` and `spends(txid, prev_txid, prev_vout)`;
  `prevout_spent_once` is removed, since a mark spending its predecessor is
  what it was standing in for. `verify` recomputes every `x(P_i)` from
  `pubkeyBase` and the states with solid-pod-rs's walker
  (`solid_pod_rs::blocktrail::walk_blocktrail`). It reports
  blocktrails/verify `043e7af`'s verdict: `verified`, `confirmed` (marks
  on-chain, no `pubkeyBase` to recompute from) or `partial`. L0 accepts
  `verified` or `confirmed`; L1 accepts only `verified`.
- **The words.** `TrustLevel::L1SingleUseSeal` is `L1Anchored` (serialised
  `l1_anchored`; nothing serialised the old name). The "single-use seal"
  wording and the "C6 reconstruction" label on `blocktrails.json` are gone
  from `src/web_contract`. A unit test (`no_seal_vocabulary_in_the_module`)
  is the vocabulary lint the Verification list asks for, over that module.
  `docs/archive` keeps its historical wording.
- **The trail type.** `Blocktrails` wraps solid-pod-rs's shared `Blocktrail`,
  the gitmark profile §5.2 shape (`@type`, `version`, `profile`, `pubkeyBase`,
  `chain`, `states`, `txo` as TXO URIs
  `txo:<chain>:<txid>:<vout>?amount=&commit=`). Files in the host's earlier
  `{txid, vout, address}` form still load and are written back as TXO URIs.
- **Evidence.** `tests/fixtures/blocktrails/` vendors solid-pod-rs
  0.5.0-alpha.12's `verify-trail-vectors.json` (blocktrails/verify `043e7af`
  run as published) and `live-trail-txs.json` (blocktrails/git-mark `b852d7d`'s
  live trail, captured from mempool.space testnet4). The host walker gives the
  page's labels and verdict on all 13 cases except two documented ones. On a
  mark that does not spend its predecessor it says `broken link`, as
  solid-pod-rs does. It does not check recorded amounts, because the
  confirmer reports no output values. The live trail is `verified`. With two
  states swapped it is `partial` (`verified`, `wrong key`, `wrong key`) while
  its tip stays confirmed.
- **D9.** `src/web_contract`'s module header now states that it is not
  `crates/visionclaw-contracts`.

The Verification item on `prevout_spent_once` now reads: `verify` at L1 passes
a live trail and refuses a reordered one through a confirmer that answers from a
running `sidestr-node`, with no test double in the path.

## Disposition — 2026-10-02

- **Suitability:** fits, needs revision
- **Priority:** P3 — parked (review trigger: agentbox ADR-2099 reopens, or a `sidestr-node` exists for `AnchorConfirmer` to query)
- **Why:** The intent stands: one fewer unsynced ledger, and an honest `AnchorConfirmer`. Nothing is built. `FsPaymentStore` is still at `src/handlers/pay_handler.rs:198`, the deposit is still a 501 (`:487`), and `AnchorConfirmer` is still a trait (`src/web_contract/ritual.rs:144`), all at `0a9abd3f9` (TODO N-3). Three statements are overtaken. D5 says P21 "is now implemented"; agentbox ADR-2103 records its CI receipt check and `pin:` as unbuilt. D8 says `extraction/solid-pod-rs` holds only a `.git` entry; it is now a populated, gitignored checkout at solid-pod-rs `045c24e` (`.gitignore:255`). D3's RGB re-sequencing follows agentbox ADR-2102, whose RGB route has given way to an origin-neutral reserve (sidestr-rs ADR-0003). D1's proxy target, agentbox `/v1/wallet/*`, does not exist (agentbox ADR-2098 D5 unbuilt).
- **Next:** On reopening, split the record. D1's deletion of the never-funded store and D9's naming fix need no chain and can land on their own. D2 and D3 wait for `sidestr-node` and a successor to agentbox ADR-2102. D5 and D8 are restated against current fact.
