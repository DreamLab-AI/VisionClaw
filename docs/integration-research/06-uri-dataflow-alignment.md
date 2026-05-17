# URI & Data-Flow Alignment Audit

**Date:** 2026-05-07
**Author:** URI & Data-Flow Alignment Auditor
**Scope:** divergence analysis across `urn:visionclaw:*`, `urn:agentbox:*`,
`urn:solid:*`, and the `did:nostr:*` identity namespace, with translation
rules required to unblock unified mesh messaging across the
forum (community-forum-rs) / agentbox / VisionClaw triple.

**Sources mined:**
- `src/uri/{kinds,mint,parse,legacy,errors,mod}.rs`
- `src/handlers/uri_resolver_handler.rs`
- `agentbox/management-api/lib/uris.js`
- `agentbox/management-api/routes/uri-resolver.js`
- `agentbox/docs/reference/adr/ADR-013-canonical-uri-grammar.md`
- `agentbox/lib/linked-data-contexts.nix`
- `docs/PRD-006-visionclaw-agentbox-uri-federation.md`
- `docs/PRD-004-agentbox-visionclaw-integration.md`
- `docs/ddd-agentbox-integration-context.md`
- `docs/ddd-bead-provenance-context.md`
- `docs/prd-bead-provenance-upgrade.md`
- `docs/adr/ADR-053-solid-pod-rs-crate-extraction.md`
- `docs/adr/ADR-054-urn-solid-and-solid-apps-alignment.md`
- `docs/binary-protocol.md`
- `src/services/parsers/block_level_parser.rs`
- `src/services/wac_mutator.rs`
- `src/services/type_index_discovery.rs`
- `src/services/urn_solid_mapping.rs`

**Severity legend:** ALIGNED · DIVERGENT · MISSING · ANTIPATTERN.

---

## 1. Namespace inventory (visionclaw 6 / agentbox 18 / solid)

### 1.1 VisionClaw kinds (6, plus `did:nostr` identity URI)

Defined in `src/uri/kinds.rs:13-31` (`Kind` enum) and
`src/uri/kinds.rs:36-63` (`ParsedUri` enum). Re-exported via
`src/uri/mod.rs:34-41`.

| Kind | URN form | R-class | Mint fn |
|------|----------|---------|---------|
| `Concept` | `urn:visionclaw:concept:<domain>:<slug>` | R3 stable-on-identity | `mint_concept` (`mint.rs:14`) |
| `Group` | `urn:visionclaw:group:<team>#members` | R3 | `mint_group_members` (`mint.rs:21`) |
| `OwnedKg` | `urn:visionclaw:kg:<hex-pubkey>:<sha256-12-hex>` | R1 content + R2 owner | `mint_owned_kg` (`mint.rs:32`) |
| `Bead` | `urn:visionclaw:bead:<hex-pubkey>:<sha256-12-hex>` | R1 + R2 | `mint_bead` (`mint.rs:54`) |
| `AgentExecution` | `urn:visionclaw:execution:<sha256-12-hex>` | R1 | `mint_execution` (`mint.rs:70`) |
| `Did` | `did:nostr:<64-hex-pubkey>` | R3 | `mint_did_nostr` (`mint.rs:45`) |

CURIE alias: `vc:<domain>/<slug>` is the **substrate-internal** Concept form
on `:KGNode.iri` / `:OntologyClass.iri` (parse.rs:36-42, 78-80, 117-131,
ADR-048). The URN form is the API alias on `node.visionclaw_uri`. Round-trip
via `to_curie` / `from_curie` (`parse.rs:78-111`).

### 1.2 Agentbox kinds (18, plus `did:nostr`)

Defined in `agentbox/management-api/lib/uris.js:71-90` (`KINDS` table).
Grammar: `urn:agentbox:<kind>:[<scope>:]<local>` (URN_RE at line 92).

| Kind | ownerScope | contentAddressed | resolvableSurface |
|------|------------|------------------|-------------------|
| `pod` | yes | yes | pods |
| `envelope` | yes | yes | pods |
| `credential` | yes | yes | pods |
| `mandate` | yes | yes | pods |
| `receipt` | yes | yes | pods |
| `activity` | yes | yes | agent-events |
| `event` | yes | yes | agent-events |
| `mcp` | no | no | things |
| `memory` | no | no | memory |
| `skill` | no | no | skills |
| `adr` / `prd` / `ddd` | no | no | docs |
| `thing` | no | no | things |
| `dataset` | yes | no | memory |
| `bead` | yes | no | beads |
| `agent` | no | no | agents |
| `meta` | no | no | meta |

Note: ADR-013 §1 (lines 76-78) lists 17 kinds (`agent` missing); the
`KINDS` table at `uris.js:88` adds `agent`. The ADR is therefore stale by
one kind. Agentbox's own CLAUDE.md at line 35 of the agentbox CLAUDE
correctly lists 18.

**DIVERGENT** — ADR-013 grammar BNF and live `KINDS` table disagree on
kind count. Recommendation: update ADR-013 §1 to include `agent`.
File: `agentbox/docs/reference/adr/ADR-013-canonical-uri-grammar.md:76-78`
vs `agentbox/management-api/lib/uris.js:88`.

### 1.3 Solid namespaces (two distinct sources)

The `solid:` family in this codebase splits into two unrelated grammars:

1. **`urn:solid:<Name>`** — vocabulary registry from
   `https://urn-solid.github.io/`. Per ADR-054 (lines 16-29) and
   `docs/reference/urn-solid-mapping.md`. Used for ecosystem alignment via
   `owl:sameAs` triples on `:OntologyClass`. Codebase touchpoint:
   `src/services/urn_solid_mapping.rs:79`,
   `src/services/type_index_discovery.rs:43-45` (`AGENT_SKILL` =
   `urn:solid:AgentSkill`, `CONTRIBUTOR_PROFILE` =
   `urn:solid:ContributorProfile`).
2. **`visionclaw:owner:<npub>/kg/<sha256-64>`** — VisionClaw's own
   Solid-Pod-resident artefact identifier; the **non-URN** legacy IRI
   stored on `:KGNode.canonical_iri` (ADR-050 / ADR-054 §Context). Two
   parallel mints exist in production (npub-bech32 vs raw-hex pubkey),
   captured in `src/uri/legacy.rs:37-66`.

Critical distinction: there is **no** `urn:solid:*` namespace minted by
VisionClaw — `urn:solid:` values are imported from the upstream URN-Solid
registry as `owl:sameAs` aliases only (ADR-054 §1, lines 69-79), not as
substrate identifiers.

### 1.4 Cross-substrate identity URI

Shared across all three: `did:nostr:<64-hex-pubkey>`. Defined in:
- `src/uri/parse.rs:189-194` (parser)
- `src/uri/mint.rs:45-51` (minter)
- `agentbox/management-api/lib/uris.js:97` (regex `DID_NOSTR_RE`)
- ADR-013 §1 grammar (line 74)

**ALIGNED** — byte-for-byte identical regex (`^did:nostr:[0-9a-f]{64}$`)
across substrates. The forum's `did:nostr` resolver at
`src/auth/did-nostr.js:118` produces the same lowercase-hex form.

---

## 2. Identity binding (`did:nostr:<hex>`) parity across the three

| Property | VisionClaw | Agentbox | Forum / solid-pod-rs | Status |
|----------|------------|----------|----------------------|--------|
| Pubkey form on the wire | 64 lowercase hex | 64 lowercase hex (`PUBKEY_HEX_RE` line 96) | 64 lowercase hex (`did-nostr.js:118`) | ALIGNED |
| Bech32 npub at boundary | accepted by `normalise_pubkey` (parse.rs:244-261), re-emitted as hex | accepted by `_normalisePubkey` (uris.js:177-199), re-emitted as hex | accepted at relay edge only | ALIGNED |
| Canonical mint chokepoint | `mint_did_nostr` (mint.rs:45) | not minted; passed-through (ADR-013 §3 line 117) | `did:nostr:${pubkey.toLowerCase()}` (did-nostr.js:118) | DIVERGENT |
| Resolution endpoint | `GET /api/v1/identity/{hex}/did.json` (resolver L159-162) | `GET <podBase>/.well-known/did.json` (uri-resolver.js:68) | `solid-pod-rs` `/.well-known/did.json` | DIVERGENT path-shape |
| Underlying key | BIP-340 x-only secp256k1 | same | same | ALIGNED |

**DIVERGENT** — three different resolution paths for the same DID. Forum
and agentbox both go to `<pod>/.well-known/did.json`; VisionClaw uses a
substrate-internal path under `/api/v1/identity/<hex>/did.json`. Per
PRD-006 §7 open question 5, this is an unresolved design call.

**Recommendation:** introduce a single `did:nostr` resolver contract:
`GET /api/v1/uri/did:nostr:<hex>` returns `307` to whichever pod the
substrate considers authoritative for that pubkey, or `404` if no pod is
known. Agentbox's resolver already does this (uri-resolver.js:58-69);
VisionClaw's redirect at `uri_resolver_handler.rs:159-162` should be made
pluggable to optionally point at a Solid pod URL. (See §10 below for the
WebID-vs-DID alignment story.)

### 2.1 The mesh-needs-many-DIDs question

The user's critical question: forum + agentbox + VisionClaw all use
`did:nostr:<hex>`, but each holds a DIFFERENT keypair belonging to
DIFFERENT actors. There are at least three DID slots per session:

1. **Forum user DID** — the human posting in a thread; their pubkey
   lives at the forum relay.
2. **Agentbox agent DID** — a per-agent keypair generated by
   `scripts/sovereign-bootstrap.py` (agentbox CLAUDE line 14) inside the
   container, distinct from the human who launched it.
3. **VisionClaw operator DID** — the substrate's own service identity
   (used to sign substrate-emitted beads, e.g.
   `nostr_bead_publisher.rs`).

Today these are conflated: nothing in either grammar names the
relationship. There is no URN form that says "agent X owned by user U
running in agentbox container C on host H".

**MISSING** — relational URN for agent provenance.

**Recommendation:** mint a composite. Two viable shapes:

```
urn:agentbox:agent:<host-pubkey-hex>:<container-id>:<agent-pubkey-hex>
```

(reuses the agentbox `agent` kind, which is currently scope-less per
KINDS table). This binds agent identity to the host operator and a
container nonce. Resolution: 307 to `/v1/agents/<container-id>/<agent>`.

Or, treat the agent itself as a content-addressed receipt:

```
urn:agentbox:meta:<agent-pubkey-hex>:<sha256-12 of {host_pubkey + container_id + manifest_checksum}>
```

so the URN survives container restarts iff the manifest is unchanged.
This matches the `FederationSession.manifest_checksum` invariant in
`docs/ddd-agentbox-integration-context.md:74`.

The first is friendlier to humans browsing the viewer (S12); the second
gives provable session-binding. PRD-006 §5 leaves this open. **Decision
needed in BC20 design.**

---

## 3. Content addressing parity (`sha256-12-<12hex>`)

**ALIGNED.** Byte-identical implementations:

- VisionClaw: `src/uri/parse.rs:269-281` (`content_hash_12`). Computes
  SHA-256 over input bytes, takes the first 6 bytes (12 hex chars),
  lowercase, prepends `sha256-12-`.
- Agentbox: `agentbox/management-api/lib/uris.js:255-264`
  (`_contentAddress`). Same algorithm, same prefix, same width.
- Both validate via `validate_hash12` (parse.rs:202-220) — strict
  prefix `sha256-12-`, exactly 12 lowercase hex chars.

PRD-006 F10 (line 32) calls this out as a "free win — no negotiation
needed." The audit confirms: **both sides agree on the content-hash
format, the hash function, the truncation width, and the case.**

The legacy form `visionclaw:owner:<npub>/kg/<sha256-64>` (legacy.rs:37,
63) keeps **64 hex chars** of SHA-256, not 12. ADR-054 references it as
the existing on-Pod IRI. The 12-hex form is the API alias minted by the
new `mint_owned_kg`. Both must coexist on `:KGNode` rows during the
migration window described in PRD-006 §5.1.

**MISSING** — the legacy 64-hex form is not part of any of the three
canonical mints. It is preserved verbatim behind `#[deprecated]` in
`legacy.rs:31-66` so existing data isn't invalidated. The post-P2
backfill (legacy.rs:18-20 comment) will populate `visionclaw_uri` with
the new 12-hex form on every owner-scoped row, after which the resolver
prefers the new column.

### 3.1 Content-id width drift risk

ADR-013 §"Risk that was considered and rejected" (line 181) flags the
collision risk: 48 bits of entropy → 1% collision around 2.3M URIs of the
same kind+scope. The ADR says the regex permits any HEXDIGIT count, so
expansion to 24 hex is a follow-up. Audit confirms VisionClaw's
`validate_hash12` (parse.rs:202-220) **hard-codes** 12 — the regex is
strict about the width.

**ANTIPATTERN — latent.** If agentbox upgrades its 12-hex to 24-hex per
ADR-013's escape hatch but VisionClaw's `validate_hash12` still demands
12, every cross-substrate URN minted by agentbox post-upgrade will fail
VC's parser. **Translation rule:** the BC20 ACL `uris_acl.rs` should call
a width-flexible parser that accepts both 12 and 24 hex; only the mint
side is allowed to enforce the current width. Reuse `content_hash_12` for
the **mint** path, but the **parse** path needs `validate_hashN` with a
permissive accept-set. Track as a follow-up.

---

## 4. Mintability (where each URN is created; central mint vs ad-hoc)

### 4.1 VisionClaw

**ALIGNED — for canonical kinds.** Five mint functions live in
`src/uri/mint.rs`:

- `mint_concept` (line 14)
- `mint_group_members` (line 21)
- `mint_owned_kg` (line 32)
- `mint_did_nostr` (line 45)
- `mint_bead` (line 54)
- `mint_execution` (line 70)

Module doc at `src/uri/mint.rs:4-5` declares the rule: any `format!`
containing `"urn:visionclaw:..."` outside `src/uri/` is rejected by a
clippy-style grep gate in CI (PRD-006 §6, "Anti-Drift Gate").

**ANTIPATTERN — one breakage of the rule.**
`src/services/parsers/block_level_parser.rs:209` mints a non-canonical
form via raw `format!`:

```rust
format!("urn:visionclaw:concept:{}:page:{}", owner_prefix, slug)
```

This produces a **5-segment** Concept URN
(`urn:visionclaw:concept:<owner>:page:<slug>`) — but the canonical
grammar in `kinds.rs:16` and `mint.rs:14-18` is **4-segment**
(`urn:visionclaw:concept:<domain>:<slug>`). Calling
`parse("urn:visionclaw:concept:<owner>:page:<slug>")` will treat
`"<owner>"` as the domain and `"page:<slug>"` as the slug — possibly
working by accident, but the slug is now polluted with a `page:` prefix
and the domain is hijacked by an owner pubkey.

**File:line:** `src/services/parsers/block_level_parser.rs:198-210`.
**Translation rule:** route through `mint_concept(domain, slug)` after
deciding which segment is the domain (probably `"page"`) and which is
the slug. The owner scope belongs in a separate kind — likely
`urn:visionclaw:kg:<owner-hex>:<sha256-12>` since this is owner-scoped
content, not a concept.

The CI lint (PRD-006 §6) is currently inactive — the gate is described
but not yet enforced. Adding it would surface this regression
immediately.

**ANTIPATTERN — test-only.**
`src/actors/presence_actor.rs:568` calls
`Did::parse(format!("did:nostr:{}", ...))` inside `#[cfg(test)]`. Test
helper, not production. Acceptable but inconsistent with
`mint_did_nostr` — recommend swap-in for symmetry.

### 4.2 Agentbox

**ALIGNED.** Single mint chokepoint at
`agentbox/management-api/lib/uris.js:135` (`mint`). All 18 surfaces
(s01-pods through s11-http-meta) are required to call it per ADR-013
§3 line 109. The agentbox CLAUDE.md explicitly states: "Ad-hoc
`format!()` or template-literal URNs are prohibited" (line 37 of
agentbox/CLAUDE.md).

A scan for `urn:agentbox:` outside `lib/uris.js` returned only
references in tests and route handlers that **read** but never **mint**
URNs. No antipatterns surfaced.

### 4.3 Solid

**N/A** — `urn:solid:*` is imported from the upstream URN-Solid registry
(ADR-054), not minted locally. The substrate's role is registry consumer:
`src/services/urn_solid_mapping.rs` parses
`docs/reference/urn-solid-mapping.md` into `urn_solid` constants used
for `owl:sameAs` emissions on `:OntologyClass`.

`src/services/type_index_discovery.rs:43-45` defines two **constants**
(`AGENT_SKILL`, `CONTRIBUTOR_PROFILE`) — these are reads of upstream
names, not local mints, so they are not antipatterns.

---

## 5. Resolvability (HTTP routes; 307/404/410 semantics; cross-system resolution)

### 5.1 VisionClaw resolver (`/api/v1/uri/...`)

Implemented at `src/handlers/uri_resolver_handler.rs`. Routes registered
at lines 238-249:

- `GET /api/v1/uri` — self-describing index (lines 82-130).
- `GET /api/v1/uri/by-curie/{curie:.*}` — CURIE entry (lines 140-146).
- `GET /api/v1/uri/{urn:.*}` — main resolver (lines 133-136).

Status semantics (table at lines 13-21):

| kind | hit | miss | malformed |
|------|-----|------|-----------|
| `Concept` | 307 → `/api/v1/nodes/by-uri/<urn>/jsonld` | 404 | 400 |
| `OwnedKg` | 307 → same | 404 | 400 |
| `Did` | 307 → `/api/v1/identity/<hex>/did.json` | 404 | 400 |
| `Group` | 307 → `/api/v1/wac/groups/<team>` | 404 | 400 |
| `Bead` | 404 + federation-hop hint | 404 | 400 |
| `AgentExecution` | 404 + federation-hop hint | 404 | 400 |

`410 Gone` is documented in the contract (lines 12-21) but never emitted
by the current code; it is reserved.

**MISSING** — the actual Neo4j lookup is stubbed. Lines 27-32 admit:
"this handler currently only ANSWERS resolves; the actual lookup against
Neo4j … is wired here as a stub returning 404." Until P3 lands, every
hit returns 307 to a downstream handler that will 404 because P1 (v2
field plumbing per PRD-006 F1) is not done.

**MISSING** — the federation hop. Lines 167-169 and 193-205: `Bead` and
`AgentExecution` always return 404 with the
`federation_hop_required` hint. Once BC20 ACL ships, these become 307
to the agentbox sibling's `/v1/uri/<urn>` (PRD-006 §5.5).

### 5.2 Agentbox resolver (`/v1/uri/...`)

Implemented at `agentbox/management-api/routes/uri-resolver.js:42-151`.

| kind | resolution |
|------|------------|
| `did:nostr:*` | 307 → `<podBase>/.well-known/did.json` (line 68) |
| `pod`/`envelope`/`credential`/`mandate`/`receipt` | 307 → `<podBase>/agents/<pubkey>/<kind>/<local>` (line 93) |
| `activity`/`event` | 307 → `/v1/agent-events?id=<urn>` (line 101) |
| `mcp`/`thing` | 307 → `/v1/things/<local>` (line 106) |
| `memory`/`dataset` | 307 → `/v1/memory/<key>?namespace=<ns>` (line 117) |
| `skill` | 307 → `/v1/skills/<local>` (line 125) |
| `adr`/`prd`/`ddd` | 307 → `/docs/reference/<kind>/<local>.md` (line 131) |
| `meta` | 307 → `/v1/meta` (line 135) |
| `bead` | 307 → `/v1/beads/<local>` (line 139) |
| unknown kind | 404 (line 145) |
| 410 | reserved, never emitted (uri-resolver.js:14-18) |

**ALIGNED** at the contract level — both sides agree on 307/404/{400 on
malformed}/410 (reserved). Self-describing endpoint shape matches.

### 5.3 Cross-system resolution

**MISSING — there is no federation hop today.**

- VisionClaw's resolver returns 404 + `federation_hop_required` for
  `Bead` and `AgentExecution`
  (`uri_resolver_handler.rs:167-169`).
- Agentbox's resolver knows nothing about `urn:visionclaw:*` URNs at all
  — the regex `URN_RE` (`uris.js:92`) matches only `^urn:agentbox:` and
  no resolver case routes off-substrate.

**Translation rule needed (PRD-006 G4 line 47):** add a `thing` kind
extension on the agentbox side with `externalNamespaces` keyed
`{ visionclaw: 'urn:visionclaw:concept:' }`, and a federation-hop case
in `uri-resolver.js` that 307s to
`<vcBase>/api/v1/uri/urn:visionclaw:concept:<domain>:<slug>` for inputs
of the form `urn:agentbox:thing:visionclaw:<domain>:<slug>`. Symmetric
on the VisionClaw side: `Bead` and `AgentExecution` 307 to
`<agentbox-base>/v1/uri/urn:visionclaw:bead:<scope>:<hash>` (or to a
1:1 translated `urn:agentbox:bead:<scope>:<hash>`).

PRD-006 §7 open question 2 flags the "federation hop loops" risk: if
both sides 404, callers must short-circuit. **Recommendation:** 5-min
TTL negative cache, keyed by URN.

---

## 6. Cross-kind semantics

### 6.1 `bead` collision

Both substrates mint `bead`. Are they the same?

**DIVERGENT — same name, different shape.**

- VisionClaw `Bead`: `urn:visionclaw:bead:<hex-pubkey>:<sha256-12-hex>`
  (kinds.rs:24-26, mint.rs:54-64). Content-addressed over a serde JSON
  payload. Owner-scoped.
- Agentbox `bead`: `urn:agentbox:bead:<scope>:<local>`
  (uris.js:87 — `ownerScope: true, contentAddressed: false`). NOT
  content-addressed. `localId` is a slug.

Subtle but consequential: agentbox's `bead` is identified by a
caller-supplied id (e.g. an external `bd` CLI epic id), whereas
VisionClaw's `bead` is a SHA-256 fingerprint of the JSON payload itself.

This matches the `bd`-CLI domain semantics (`docs/ddd-bead-provenance-context.md`
§2 "Bead Aggregate"): `bead_id` is non-empty, valid as a Nostr `d` tag,
and immutable after creation. Bd CLI gives the bead a slug (e.g.
`bd-001`); the Nostr event itself is content-addressed by the relay.

**Translation rule:** `bc20::acl::uris_acl.rs` performs a 1:1 mapping:

```
urn:agentbox:bead:<scope>:<local>  ↔  urn:visionclaw:bead:<scope>:<local>
```

— but the local-segment width is unconstrained (slug on agentbox side,
sha256-12 on VC side). The ACL must inspect the local segment: if it
matches `sha256-12-[a-f0-9]{12}` it round-trips losslessly; otherwise
it is a slug and should be treated as the agentbox-native id with a
substrate marker so VC's resolver doesn't try to dereference a hash
that doesn't exist in `:Bead.content_hash`. Suggested wrap:

```
urn:visionclaw:bead:<scope>:<sha256-12 of {agentbox_bead_id}>
```

so VC's parser still sees the canonical 12-hex form, with the original
agentbox slug recoverable from a sidecar lookup.

### 6.2 `kg` (VC) vs `thing`/`dataset` (agentbox)

VisionClaw `OwnedKg` is the substrate's owner-scoped knowledge-graph
node URN. Closest agentbox equivalents:

- `urn:agentbox:thing:<local>` — owner-scope-less, slug-shaped, "any
  schema.org Thing" (uris.js:85). Maps to `things` resolver surface.
- `urn:agentbox:dataset:<scope>:<local>` — owner-scoped, slug-shaped,
  DCAT dataset.

**DIVERGENT.** Neither maps cleanly. `thing` lacks owner scope;
`dataset` has scope but uses a slug not a content-hash. ADR-013 §1 line
77 lists the kinds — there is no agentbox kind that is
`{ ownerScope: true, contentAddressed: true }` AND identifies a
graph-node-shaped resource.

**MISSING — the bridge concept.** PRD-006 §5.9 line 290-294 proposes
extending agentbox with:

```js
thing: { ownerScope: false, contentAddressed: false,
         resolvableSurface: 'thing-resolver',
         externalNamespaces: { visionclaw: 'urn:visionclaw:concept:' } }
```

This makes `urn:agentbox:thing:visionclaw:<domain>:<slug>` a federation
form for VC concepts. **It does not solve `OwnedKg`** — the
content-addressed, owner-scoped variant has no agentbox sibling.

**Recommendation:** add a 19th agentbox kind, `kg`, mirroring VC's
`OwnedKg`:

```js
kg: { ownerScope: true, contentAddressed: true, resolvableSurface: 'pods' }
```

Then `urn:agentbox:kg:<scope>:<sha256-12>` ↔
`urn:visionclaw:kg:<scope>:<sha256-12>` is identity-preserving.
Resolves through the pod (since on the VC side these live in
`./public/kg/<slug>` per ADR-052).

### 6.3 `concept` (VC) vs nothing (agentbox)

VisionClaw `Concept` (kinds.rs:16-19) has no agentbox equivalent. The
PRD-006 plan is to alias via `urn:agentbox:thing:visionclaw:<domain>:<slug>`
(§5.9). The `vc:<domain>/<slug>` CURIE
(`src/uri/parse.rs:36-42`) is the substrate-internal form on
`:KGNode.iri`.

**Translation rule:**

```
urn:visionclaw:concept:<domain>:<slug>  ↔
urn:agentbox:thing:visionclaw:<domain>:<slug>  ↔
vc:<domain>/<slug>  (database key only — never crosses the wire)
```

The CURIE form is **substrate-internal and must not appear in
agentbox-bound payloads**. Wrap all outbound emissions with
`from_curie` (parse.rs:97-111).

### 6.4 `execution` (VC) vs `activity`/`event` (agentbox)

VisionClaw `AgentExecution`:
`urn:visionclaw:execution:<sha256-12-hex>` — content-addressed over
`<action>|<slot>|<pubkey>|<unix_ts>` (mint.rs:70-83). NOT owner-scoped.

Agentbox `activity`/`event`: both owner-scoped + content-addressed,
both with `agent-events` resolver surface. ADR-013 §3 line 117
distinguishes: `activity` is "content-addressed on
action+slot+operation+input+output" (S5 provenance), `event` is
"content-addressed on action+slot+timestamp+payload" (S11 events).

**DIVERGENT — VC fingerprint omits owner scope, agentbox keeps it.**

VC's choice (no owner scope) means the same execution by different
operators collapses to the same URN. That is a feature for content
addressing (pure provenance) but breaks audit ("who ran this?"). The
mint hashes pubkey **into** the fingerprint at mint.rs:80
(`format!("{}|{}|{}|{}", action, slot, normalised, ts)`), so the URN
varies per operator — but the URN itself doesn't carry the pubkey. To
recover it, the BC20 ACL must store a sidecar `pubkey` field.

Agentbox's choice (scope-bearing) makes the URN self-describing.

**Translation rule:** when projecting a VC `execution` URN to agentbox,
the ACL must look up the original pubkey in
`:AgentExecution.requester_pubkey` (per
`docs/ddd-agentbox-integration-context.md:114-127`) and emit:

```
urn:agentbox:event:<requester-pubkey-hex>:<sha256-12>
```

The reverse direction (`event` → `execution`) drops the pubkey from the
URN body, accepting the loss because VC's grammar doesn't carry it.

### 6.5 `group` (VC) vs nothing (agentbox)

VisionClaw `Group`: `urn:visionclaw:group:<team>#members` —
ASCII-fragment-suffixed. No agentbox equivalent.

This is a fragment URI for WAC ACL groups — used by
`src/services/wac_mutator.rs:280` (per PRD-006 F5 line 27). Resolves to
`/api/v1/wac/groups/<team>` (uri_resolver_handler.rs:163-166).

**Recommendation:** leave VC-private. WAC groups are a substrate
concern; agentbox has its own ACL story (see §10 below). No federation
mapping needed.

---

## 7. JSON-LD context divergence

### 7.1 Agentbox catalogue (12 contexts)

Pinned at build time per `agentbox/lib/linked-data-contexts.nix`:

1. ActivityStreams 2.0 (S2 envelopes, S5 provenance)
2. W3C VC 2.0 (S3 credentials, S8 payments)
3. DID v1 (S4 DID Documents)
4. schema.org (S1, S6 things)
5. WoT-TD (S6 Things)
6. PROV-O (S5 provenance)
7. DCAT-3 (S9 datasets)
8. ODRL-2 (S8 mandates)
9. SKOS (S6 vocabularies)
10. dcterms (cross-cutting)
11. agentbox-v1 (in-tree at
    `agentbox/docs/reference/_vocab/agentbox-v1.context.jsonld`)
12. (extension slot)

ADR-012 invariant (DDD-004 §L09 cited at line 8 of linked-data-contexts.nix):
the encoder middleware loads the index at boot and **never performs
network I/O** thereafter.

### 7.2 VisionClaw context — does not exist yet

PRD-006 §5.8 (line 274) proposes adding `visionclaw-v2.context.jsonld`
to the agentbox catalogue:

```nix
visionclaw-v2 = {
  url = "https://visionclaw.dreamlab-ai.systems/ns/v2";
  hash = "sha256-AAAAAA...";  # FOD; resolved by prefetch
  destination = "/opt/agentbox/contexts/visionclaw-v2.context.jsonld";
};
```

Currently: VisionClaw publishes nothing under `/ns/v2`. The schema
document at `docs/schema/visionclaw-ontology-schema-v2.jsonld` is
referenced in PRD-006 §5.11 (line 328) as a deliverable but does not
yet exist in-tree.

**MISSING.** Per PRD-006 §7 open question 3: VC currently hosts no
public `/ns/v2`. Recommendation in the PRD: ship the schema JSON-LD
in-tree, serve via management API root, DNS only after sign-off.

### 7.3 Term conflicts

The `agbx:` vocabulary at
`agentbox/docs/reference/_vocab/agbx.md` (referenced from PRD-006 §5.8)
defines agentbox-specific properties. PRD-006 §5.8 line 286 mentions
"reciprocal aliases for VC-only terms with no upstream W3C equivalent
(`vc:bridges-to`, `vc:quality-score`, `vc:authority-score`)".

A grep for these in the agentbox tree returned zero matches today —
the alias entries are planned, not landed.

**MISSING — three named terms.** `vc:bridges-to`, `vc:quality-score`,
`vc:authority-score`. These need:
1. Definitions in `agentbox/docs/reference/_vocab/agbx.md`.
2. Pinned hashes in
   `agentbox/lib/linked-data-contexts.nix` if they are externalised.
3. Round-trip test fixtures in `tests/contract/linked-data/`.

No outright conflicts surfaced — just empty slots.

### 7.4 Content-hash convention shared term

Both substrates use `content_hash` as a JSON-LD property carrying the
`sha256-12-…` value (PRD-006 F10 line 32, kinds.rs:50-51 for `hash12`).
**ALIGNED** — both treat it as a string-literal property keyed to the
canonical hash form.

---

## 8. BC20 anti-corruption layer status (planned vs implemented)

### 8.1 Plan (DDD-agentbox-integration-context.md §5)

Six ACL modules:

| Module | Maps |
|--------|------|
| `beads_acl` | agentbox `bd`-CLI JSON ↔ `BeadProvenance` aggregate commands |
| `pods_acl` | Solid LDP containers ↔ VC pod artefact URIs |
| `memory_acl` | generic vector query/store ↔ VC namespace layout |
| `events_acl` | agentbox JSONL events ↔ VC Contributor Stratum bus |
| `orchestrator_acl` | agentbox stdio spawn ↔ VC actor spawn |
| `uris_acl` | `urn:agentbox:*` ↔ `urn:visionclaw:*` |

ACL rules (lines 171-176): translation must be **total**; unmapped
payloads raise `UnmappedAgentboxPayload`.

### 8.2 Reality

**MISSING — 0% implemented.**

- No `src/bc20/` directory exists. (Confirmed via `ls`.)
- PRD-006 F3 (line 25) admits: "Zero matches for `FederationSession`,
  `AgentExecution`, `*_acl` in `src/`."
- The stdio bridge writes to a stream nobody reads
  (PRD-006 F3 line 25, citing
  `agentbox/management-api/adapters/orchestrator/stdio-bridge.js:33-44`).
- The `/v1/meta` handshake (DDD §4.1a) is unimplemented.
- The `LocalFallbackProbe` Ed25519-signed origin probe (DDD §2 row,
  §4.1a step 4) is unimplemented.

**Effect on URI federation:** without `uris_acl.rs`, every
cross-substrate URN reference is a dead end. The bead provenance
domain (`docs/ddd-bead-provenance-context.md`) cannot reference
agentbox executions, and agentbox executions cannot cite VC
`OwnedKg` URNs in their receipts.

PRD-006's P3 (line 351) targets BC20 ACL by 2026-06-13. P2 (mint +
resolver) is in progress per the code already on disk but federation
hops are not implemented anywhere.

---

## 9. Message envelope contract (what shape do inter-system events take today, if any)

### 9.1 What exists

**Binary protocol (GPU position stream):** `docs/binary-protocol.md`,
`src/utils/binary_protocol.rs::encode_position_frame`. Format:

```
[u8  preamble = 0x42]
[u64 broadcast_sequence_LE]
[N × Node]   // each Node = u32 id + 6×f32 = 28 bytes
```

This is **client-substrate only** — not a federation envelope.
Forbidden patterns include "encoding session-static state in id flag
bits" (line 56). The frame carries no URN or DID.

**Bead provenance via Nostr:** beads are signed Nostr events (kind
30001 NIP-33 per `nostr_bead_publisher.rs`, ddd-bead §10). Wire format
is the standard Nostr event envelope (`id`, `pubkey`, `created_at`,
`kind`, `tags`, `content`, `sig`). The `bead_id` lives in the `d` tag
(NIP-33 addressable replaceable). The forum relay receives a re-signed
kind-9 NIP-29 mirror (`nostr_bridge.rs`, ddd-bead §5.4).

**Agentbox JSONL agent events:** one event per line on stdout from
`stdio-bridge.js` and persisted to disk by
`agentbox/management-api/adapters/events/local-jsonl.js` (PRD-006 F4
line 26). Schema is in
`agentbox/management-api/utils/agent-event-publisher.js` and
distributed via `agent-event-bridge.js`. Lifecycle kinds: `Spawned`,
`ToolUsed`, `Progress`, `Completed`, `Failed`
(PRD-006 §5.7 line 270 + DDD §4.2 line 134).

**Forum chat → Nostr relay:** kind-1 short-text-note, signed by user's
pubkey, posted to forum relay.

### 9.2 What's missing

**MISSING — cross-system envelope.** There is no canonical envelope
that:

1. Carries a URN identifying the resource,
2. Carries a DID identifying the actor,
3. Has a content-hash field for round-trip integrity,
4. Is the same shape over WebSocket (binary tick), HTTP REST, Nostr
   relay, and stdio JSONL.

The closest precedent is the **Nostr event itself**: every nostr event
has `pubkey` (≈ `did:nostr:<hex>` minus prefix), `id` (sha256 over the
serialised event = full content-hash), and `tags` (where `["d", <urn>]`
already supports URN-scoped addressable events per NIP-33). A Nostr
event already meets requirements 1-3 if you set:

- `pubkey = <hex of did:nostr:>`
- `tags = [["d", "<urn>"], ["e", "<sha256-12 of payload>"]]`
- `kind` per intent (1 chat, 30001 bead, 30078 agent-state, etc.)

A cross-substrate envelope can therefore be a Nostr-event-shaped
message regardless of transport. The forum already speaks this. The
bead provenance context already speaks this
(`nostr_bead_publisher.rs`, `nostr_bridge.rs`).

**Recommendation — minimal envelope contract:**

```jsonc
{
  "kind": 30050,                           // new kind: cross-mesh envelope
  "pubkey": "<64-hex>",                    // actor (forum user / agentbox agent / VC operator)
  "created_at": 1714003200,
  "tags": [
    ["d", "urn:visionclaw:bead:<scope>:<sha256-12>"],   // resource URN
    ["did", "did:nostr:<hex>"],                          // explicit actor DID
    ["host", "did:nostr:<hex>"],                         // host-DID for agent provenance
    ["session", "<federation-session-id>"],              // BC20 FederationSessionId
    ["content-hash", "sha256-12-<12hex>"]                // payload integrity
  ],
  "content": "<json-ld payload, possibly compact>",
  "id": "<sha256 of canonical event serialisation>",
  "sig": "<schnorr sig>"
}
```

This shape lets a forum chat message become an agentbox skill
invocation by:

1. Forum user posts a kind-1 message tagged `["t", "skill:<slug>"]`.
2. The agentbox sibling subscribes to forum relay with `#t skill`.
3. Agentbox extracts `["t", "skill:<slug>"]`, verifies sender via
   pubkey, mints
   `urn:agentbox:skill:<slug>` (lookup) +
   `urn:agentbox:event:<host-pubkey>:<sha256-12 of {sender, args, ts}>`
   (new agent execution).
4. Spawn skill with `userContext.pubkey = <forum-user-pubkey>`.
5. Emit a new kind-30050 envelope citing both URNs.

The forum sender's DID is preserved end to end, the agentbox skill URN
is reachable, the envelope content-hashes itself. **No new wire
protocol needed** — Nostr's existing event envelope suffices.

---

## 10. WAC ACL agent IRI compatibility (`did:nostr` vs WebID URL)

### 10.1 Solid WAC expectation

Solid Web Access Control (`acl:Authorization`) expects `acl:agent` to
be a WebID **URL** (per Solid Protocol 0.11). Examples:

```
acl:agent <https://alice.solidcommunity.net/profile/card#me>
```

### 10.2 VisionClaw / agentbox use `did:nostr`

VisionClaw and agentbox both identify actors via
`did:nostr:<64-hex>`. This is **not a URL**; it is a URN/DID. Solid
WAC parsers may reject it.

ADR-053 §2 (referenced from `docs/adr/ADR-053-solid-pod-rs-crate-extraction.md`)
notes that `solid-pod-rs` accepts `did:nostr:<hex>` and bech32 npub at
`/agents/*` (uri-resolver.js:90 comment), but mainstream Solid clients
won't.

`src/auth/did-nostr.js:37-92` resolves `did:nostr:<pubkey>` to a Solid
WebID via DID document — bidirectional link required. The DID document
carries:

```jsonld
{
  "id": "did:nostr:<hex>",
  "alsoKnownAs": ["<webid-url>"],
  "verificationMethod": [...],
  "service": [{"type": "SolidPodInbox", "serviceEndpoint": "<pod-url>"}]
}
```

The WebID profile must link back via `solid:nostrPubkey` or similar.

### 10.3 ADR-054 alignment

ADR-054 §1 line 199:

> WAC accepts `urn:solid:` scheme in agent identifiers

This is a **third** form (after `did:nostr:` and the WebID URL) used in
`urn:solid:AgentSkill` type-index entries
(`src/services/type_index_discovery.rs:43`). It addresses **vocabulary
classes**, not agents — so the conflict is semantic, not technical:
`urn:solid:` cannot appear as an `acl:agent` value.

### 10.4 Compatibility matrix

| Identifier form | WebID-compatible? | Solid Protocol clients? |
|-----------------|-------------------|--------------------------|
| `did:nostr:<hex>` | only via DID-document indirection | requires `did-nostr.js` resolver |
| `<webid-url>` | yes (canonical) | yes |
| `urn:solid:<Name>` | NO (vocabulary URN, not actor URI) | reject |
| `urn:visionclaw:kg:<scope>:<hash>` | NO (resource URN, not actor URI) | reject |

**DIVERGENT** — three substrates' actor identity story doesn't have one
canonical actor URI. The DID-document indirection (`did-nostr.js`) is
the reconciliation: every `did:nostr:` resolves to a WebID via
`alsoKnownAs`; WAC operates on the WebID; the `did:nostr:` is the
**substrate-portable** alias, the WebID is the **WAC-compatible**
canonical.

**Translation rule:** at every WAC boundary
(`solid_pod_handler.rs:160-162` `forbidden_response(acl, agent_uri,
path)`), the value passed as `agent_uri` MUST be a WebID URL, not a
DID. Wrap with `did_to_webid(did) -> Result<WebIdUrl, _>` that calls
the `did-nostr.js` resolver (or its Rust port).

---

## 11. Bead provenance graph cross-system links (PRD-008 / DDD-bead-provenance)

### 11.1 What today's bead aggregate carries

Per `docs/ddd-bead-provenance-context.md` §2 (lines 84-110):

```
Bead
├── bead_id: BeadId           // valid Nostr d tag
├── brief_id: BriefId
├── metadata: BeadMetadata
├── state: BeadState          // FSM
├── outcome: Option<BeadOutcome>
├── nostr_event: Option<NostrEventRef { id, pubkey }>
├── learnings: Vec<BeadLearning>
└── timestamps
```

The `nostr_event.pubkey` is a 64-hex pubkey. The `bead_id` is a
substrate-internal slug. There is **no** canonical URN field.

Cross-system reference today: zero. PRD-006 F9 (line 31) confirms:
"No bead/credential/event ever cites a VC URN; no VC node ever cites
an agentbox URN."

### 11.2 What's needed

Each bead should carry:

- `urn`: `urn:visionclaw:bead:<owner-hex>:<sha256-12>` — the canonical
  VC URN.
- `agentbox_urn`: optional `urn:agentbox:bead:<owner-hex>:<bead-id>` —
  the agentbox-side mirror, preserved when an `bd`-CLI bead is
  imported via the BC20 ACL.
- `subject_urns`: `Vec<Urn>` — URNs the bead is about (a brief, an
  agent execution, a KG node, a credential).
- `actor_did`: `did:nostr:<hex>` — the human or agent who caused the
  bead.
- `host_did`: optional `did:nostr:<hex>` — the agentbox container's
  service identity (relevant when the actor is an agent).

The DDD aggregate today has none of these explicitly. They live
implicitly in `metadata.brief_id`, `nostr_event.pubkey`, etc.

**Translation rule:** when `beads_acl::to_visionclaw(agentbox_payload)`
runs, the ACL extracts `actor_did` from the agentbox event's `pubkey`
field, mints the VC `urn:visionclaw:bead:<actor-hex>:<sha256-12 of
{agentbox_event_id}>`, and stores both in the BeadMetadata via a new
`canonical_urn` and `agentbox_urn_mirror` column on `:Bead`.

### 11.3 Graph cross-links

PRD-006 §5.4 (line 184) describes the `:KGNode-[:HAS_ONTOLOGY]->(
:OntologyClass)` MERGE as a single source of truth. Beads need an
analogous bridge:

```
(:Bead { urn })-[:CITES]->(:KGNode)
(:Bead { urn })-[:CITES]->(:AgentExecution)
(:Bead { urn })-[:ATTESTED_BY]->(:Identity { did_nostr })
```

The `prd-bead-provenance-upgrade.md` is an existing PRD document
covering bead provenance in general; PRD-008 (referenced in user
question 11) is named "PRD-008 / DDD-bead-provenance" but scanning the
repo: there is no `docs/PRD-008-*` file. **MISSING — PRD-008 is
unwritten.** The bead provenance upgrade lives in
`docs/prd-bead-provenance-upgrade.md` (no number prefix). Either
rename to PRD-008 or update the user question's reference.

The `:CITES` and `:ATTESTED_BY` edges are the substrate's way to
project the URN graph onto Neo4j without inflating per-frame binary
payload (per `docs/binary-protocol.md` line 56-58 forbidden patterns).

---

## 12. Concrete translation rules needed for each cross-system kind pair

| Source | Target | Rule | ACL home | Loss |
|--------|--------|------|----------|------|
| `urn:agentbox:bead:<scope>:<slug>` | `urn:visionclaw:bead:<scope>:<sha256-12 of slug>` | hash the slug into 12-hex | `uris_acl::to_visionclaw_bead` | original slug must be sidecar-stored |
| `urn:visionclaw:bead:<scope>:<sha256-12>` | `urn:agentbox:bead:<scope>:<sha256-12>` | identity (12-hex round-trips) | `uris_acl::to_agentbox_bead` | none |
| `urn:agentbox:thing:visionclaw:<domain>:<slug>` | `urn:visionclaw:concept:<domain>:<slug>` | strip federation prefix | `uris_acl::to_visionclaw_concept` | none |
| `urn:visionclaw:concept:<domain>:<slug>` | `urn:agentbox:thing:visionclaw:<domain>:<slug>` | prepend federation prefix | `uris_acl::to_agentbox_thing` | none |
| `urn:visionclaw:kg:<scope>:<sha256-12>` | (proposed) `urn:agentbox:kg:<scope>:<sha256-12>` | identity, requires new `kg` kind in agentbox | `uris_acl::to_agentbox_kg` | requires agentbox grammar extension |
| `urn:agentbox:event:<scope>:<sha256-12>` | `urn:visionclaw:execution:<sha256-12>` | drop scope; sidecar-store pubkey | `uris_acl::to_visionclaw_execution` | actor pubkey lost from URN body |
| `urn:visionclaw:execution:<sha256-12>` | `urn:agentbox:event:<requester-hex>:<sha256-12>` | look up requester via `:AgentExecution.requester_pubkey` | `uris_acl::to_agentbox_event` | requires DB hop |
| `urn:agentbox:credential:<scope>:<hash>` | (no VC equivalent today) | leave as opaque agentbox URN; reference from VC bead via `mentions` tag | n/a | — |
| `urn:agentbox:mandate:<scope>:<hash>` | (no VC equivalent today) | leave as opaque | n/a | — |
| `urn:agentbox:skill:<slug>` | (no VC equivalent today) | leave as opaque; VC's BC19 Skills aggregate stores agentbox URN as foreign key | n/a | — |
| `did:nostr:<hex>` | `did:nostr:<hex>` | identity (always) | passthrough | none |
| `urn:solid:<Name>` | `vc:<domain-of-name>/<name>` (substrate-internal) | resolve via `urn-solid-mapping.md` | `urn_solid_mapping.rs` (already exists) | mapping table required |
| `vc:<domain>/<slug>` | `urn:visionclaw:concept:<domain>:<slug>` | `from_curie` (parse.rs:97-111) | already implemented | none |
| `urn:visionclaw:group:<team>#members` | (no agentbox equivalent) | substrate-private; do not federate | n/a | — |

**Critical asymmetries:**

- VC `bead` is content-addressed; agentbox `bead` is slug-id'd. Round
  trip is **lossy** in agentbox→VC direction.
- VC `execution` drops the scope from the URN; agentbox `event` keeps
  it. Round trip is **lossy** in agentbox→VC direction (must reconstruct
  pubkey from sidecar).
- 5 agentbox kinds (`credential`, `mandate`, `receipt`, `mcp`, `skill`)
  have no VC counterpart. They survive as opaque URNs in VC's bead
  mentions but are never minted by VC.

---

## 13. Antipatterns

### 13.1 ANTIPATTERN: ad-hoc URN string formatting

**File:line:** `src/services/parsers/block_level_parser.rs:209`
**Code:**

```rust
format!("urn:visionclaw:concept:{}:page:{}", owner_prefix, slug)
```

**Problem:** mints a 5-segment Concept URN. The canonical Concept
grammar is 4-segment (`urn:visionclaw:concept:<domain>:<slug>`).
`parse()` will accept it (the parser splits on the FIRST `:` and treats
the entire tail as `<slug>` — see parse.rs:117-131), but the slug then
contains `page:<actual-slug>`, polluting downstream string matching.

**Severity:** medium — accepted by parser, but wrong shape and breaks
the assumption "Concept URNs are domain+slug only".

**Fix:** route through `mint_concept` after deciding the right kind.
If the owner scope is meaningful, use `mint_owned_kg` (since the URL
encodes content owned by a pubkey). If the scope is just a synthetic
prefix, drop it and use `mint_concept(domain="page", slug=slug)`.

### 13.2 ANTIPATTERN: parallel mints

**File:line:** `src/uri/legacy.rs:37` and `:63`
**Two divergent mints in production data:**

```rust
canonical_iri_npub:    visionclaw:owner:<npub>/kg/<sha256-64>
canonical_iri_raw_hex: visionclaw:owner:<raw-hex>/kg/<sha256-64>
```

**Problem:** The pubkey form differs (bech32 vs raw hex). Both forms
co-exist on the `canonical_iri` column (legacy.rs:11-17 comment).
`opaque_id.rs:166` derives bit29 binary-protocol opaque IDs from this
column — changing existing rows breaks GPU. Both are
`#[deprecated]` with notes pointing at the new `mint_owned_kg`.

**Severity:** P1 historical; P0 if a future caller picks the wrong
shim.

**Fix:** the post-P2 backfill (legacy.rs:18-20 comment) will populate
`visionclaw_uri` with the new 12-hex form on every row. The legacy
mints remain as inert shims for column-value preservation only.

### 13.3 ANTIPATTERN: content-id width drift (latent)

**File:** `src/uri/parse.rs:202-220` (`validate_hash12`)
**Problem:** width is hard-coded to 12. ADR-013 line 181 reserves the
right to expand to 24. If agentbox upgrades and VC doesn't, every
inbound 24-hex agentbox URN fails VC's parser.

**Severity:** latent / future. Currently no consumer uses 24.

**Fix:** the BC20 ACL `uris_acl::parse_permissive` accepts both widths;
mints continue to enforce the current width. Track in a follow-up.

### 13.4 ANTIPATTERN: hex pubkey leak

**File:line:** `src/models/node.rs:106` (per PRD-006 F7 line 29)
**Problem:** `owner_pubkey` is serialised at API boundaries as bare
64-hex, not wrapped in `did:nostr:`. Per PRD-006 G5 (line 47) and the
`did:nostr` invariant in §5.1 of PRD-006 (line 148):

> `did:nostr:<hex>` is the **only** form pubkeys take at API
> boundaries; the substrate-internal hex form is never serialised
> externally.

**Severity:** P1 — every external consumer must know to wrap, breaking
the "uniform identity URI" contract.

**Fix:** serde wrapper on the `owner_pubkey` field at the
serialisation boundary; `mint_did_nostr` already handles the
normalisation.

### 13.5 ANTIPATTERN: federation hop missing

**Files:** `src/handlers/uri_resolver_handler.rs:167-169`
(returns 404 federation-hop hint) and
`agentbox/management-api/routes/uri-resolver.js` (no `urn:visionclaw:*`
case at all).

**Problem:** every cross-substrate URN reference dies at the resolver.
Bead-provenance, agent-execution, and concept federation all rely on
this. PRD-006 P3 plans the BC20 implementation but it is not on disk.

**Severity:** P0 — blocks PRD-006 G3, G4, G6.

**Fix:** implement BC20 ACL as planned; add the federation case in
both resolvers per §5.3 of this audit.

### 13.6 ANTIPATTERN: stale ADR (kind count)

**File:line:** `agentbox/docs/reference/adr/ADR-013-canonical-uri-grammar.md:76-78`
**Problem:** BNF grammar lists 17 kinds; live `KINDS` table has 18
(missing `agent` from the ADR).

**Severity:** P3 — documentation drift, no functional impact.

**Fix:** add `agent` to the BNF grammar in ADR-013 §1.

### 13.7 ANTIPATTERN: PRD reference drift

**Reference:** the user's audit task references "PRD-008 /
DDD-bead-provenance" but the on-disk filename is
`docs/prd-bead-provenance-upgrade.md` (no number prefix). The
`docs/PRD-006-*.md` and `docs/PRD-007-*.md` files exist; PRD-008 is
absent.

**Severity:** P3 — documentation. PRDs are interleaved with no number
in some cases.

**Fix:** rename `docs/prd-bead-provenance-upgrade.md` to
`docs/PRD-008-bead-provenance-upgrade.md` for ordering parity, or
update audit references.

### 13.8 ANTIPATTERN: CI lint not enforced

**Reference:** `src/uri/mint.rs:4-5` documents a clippy-style grep gate
in CI: "rejects ad-hoc `format!('urn:visionclaw:...')` anywhere outside
`src/uri/`". Audit grep finds the gate has not caught
`block_level_parser.rs:209`. The gate exists in plan only.

**Severity:** P1 — without enforcement, antipattern §13.1 will recur.

**Fix:** add a script-level grep in pre-commit or
`.github/workflows/lint.yml`:

```bash
! grep -rn 'format!("urn:visionclaw' src/ | grep -v src/uri/
```

with non-zero exit on hits.

---

## 14. Cross-substrate readiness summary

| Concern | State | Blockers |
|---------|-------|----------|
| Identity DID parity | ALIGNED | DID-document indirection inconsistent across substrates (§10) |
| Content-hash format | ALIGNED | Latent 12-vs-24 width drift (§13.3) |
| Mint chokepoints | ALIGNED VC + agentbox sides; one VC antipattern (§13.1) |
| Resolver routes | ALIGNED at contract; STUBBED Neo4j lookups on VC side; NO federation hop |
| Cross-kind semantics | DIVERGENT — `bead` collision shape, `execution` vs `event` scope drop, no `kg` in agentbox |
| JSON-LD context catalogue | MISSING `visionclaw-v2` entry; three reciprocal `vc:*` aliases unpinned |
| BC20 ACL | MISSING entirely; 0% implemented |
| Cross-substrate references in code | MISSING — F9 confirms zero today |
| WAC actor URI | DIVERGENT — DID vs WebID requires `did-nostr.js` indirection |
| Bead cross-system links | MISSING — no canonical_urn / agentbox_urn fields on `:Bead` today |
| Mesh envelope contract | MISSING canonical shape; Nostr event already 80% of the answer |
| Multi-DID-per-actor URN | MISSING — no relational URN for "agent X owned by user U on host H" |

---

## 15. Recommended near-term actions (in order)

1. **Enforce the URN mint gate (P0 — half-day).** Add a
   `.github/workflows/lint-urn-format.yml` step: `grep -rn
   'format!("urn:visionclaw' src/ | grep -v src/uri/` exits non-zero.
   Catches §13.1 and prevents recurrence.
2. **Fix `block_level_parser.rs:209` (P0 — 1 hr).** Decide the right
   kind for owner-scoped page URNs and route through the canonical
   mint module.
3. **Wrap `owner_pubkey` in `did:nostr:` at the boundary (P1 — 2 hrs).**
   serde wrapper on `src/models/node.rs:106`. Per PRD-006 G5.
4. **Add `kg` kind to agentbox (P1 — half-day).** Mirror VC's
   `OwnedKg`. Avoids the `dataset`/`thing` impedance mismatch in §6.2.
5. **Implement `uris_acl.rs` (P0 — 1-2 days).** First BC20 ACL module;
   the URN translator. Needed before federation hops can return 307.
6. **Implement federation hops in both resolvers (P0 — 1 day).** VC
   `Bead` and `AgentExecution` 307 to agentbox; agentbox `thing` with
   `visionclaw:` prefix 307 to VC. Add 5-min negative cache (PRD-006
   §7 Q2).
7. **Define the cross-mesh envelope (kind-30050) (P1 — 1 day spec, 1-2
   days impl).** Document the Nostr-event-shaped envelope from §9.2 in
   a new ADR. Add a test fixture that round-trips a forum chat → agent
   execution → bead.
8. **Add `visionclaw-v2.context.jsonld` to agentbox catalogue (P1 — 1
   day).** Per PRD-006 §5.8. Also resolves the three missing `vc:*`
   aliases.
9. **Add `urn` and `agentbox_urn` columns to `:Bead` (P2 — half-day +
   migration).** Per §11.2.
10. **Extend ADR-013 grammar to include `agent` kind (P3 — 5 min).**
    Documentation parity.

---

## 16. References (verified file:line)

- `src/uri/kinds.rs:13-31` — `Kind` enum
- `src/uri/kinds.rs:36-63` — `ParsedUri` enum
- `src/uri/mint.rs:14` — `mint_concept`
- `src/uri/mint.rs:21` — `mint_group_members`
- `src/uri/mint.rs:32` — `mint_owned_kg`
- `src/uri/mint.rs:45` — `mint_did_nostr`
- `src/uri/mint.rs:54` — `mint_bead`
- `src/uri/mint.rs:70` — `mint_execution`
- `src/uri/parse.rs:202-220` — `validate_hash12`
- `src/uri/parse.rs:244-261` — `normalise_pubkey`
- `src/uri/parse.rs:269-281` — `content_hash_12`
- `src/uri/legacy.rs:37,63` — two legacy mint shims
- `src/handlers/uri_resolver_handler.rs:13-21` — resolver status table
- `src/handlers/uri_resolver_handler.rs:167-169` — federation hop hint
- `src/handlers/uri_resolver_handler.rs:238-249` — route registration
- `src/services/parsers/block_level_parser.rs:209` — antipattern §13.1
- `src/services/wac_mutator.rs:280` — only `urn:visionclaw:group:` mint
  call site (PRD-006 F5)
- `src/services/urn_solid_mapping.rs:79` — URN-Solid term consumer
- `src/services/type_index_discovery.rs:43-45` — solid type-index
  registration constants
- `agentbox/management-api/lib/uris.js:71-90` — KINDS table
- `agentbox/management-api/lib/uris.js:92` — `URN_RE` regex
- `agentbox/management-api/lib/uris.js:96-97` — `PUBKEY_HEX_RE`,
  `DID_NOSTR_RE`
- `agentbox/management-api/lib/uris.js:135` — `mint`
- `agentbox/management-api/lib/uris.js:255-264` — `_contentAddress`
- `agentbox/management-api/routes/uri-resolver.js:42-151` — resolver
- `agentbox/docs/reference/adr/ADR-013-canonical-uri-grammar.md:76-78`
  — grammar BNF (kind list stale)
- `agentbox/docs/reference/adr/ADR-013-canonical-uri-grammar.md:181` —
  width-drift escape hatch
- `agentbox/lib/linked-data-contexts.nix` — context catalogue
- `docs/PRD-006-visionclaw-agentbox-uri-federation.md` — federation
  spec
- `docs/PRD-004-agentbox-visionclaw-integration.md` — adapter pattern
- `docs/ddd-agentbox-integration-context.md` — BC20 design
- `docs/ddd-bead-provenance-context.md` — bead aggregate
- `docs/prd-bead-provenance-upgrade.md` — bead upgrade PRD
- `docs/adr/ADR-053-solid-pod-rs-crate-extraction.md` — solid-pod-rs
- `docs/adr/ADR-054-urn-solid-and-solid-apps-alignment.md` — URN-Solid
  alignment
- `docs/binary-protocol.md` — wire format reference

---

## 17. Open questions for a follow-up swarm

1. Per §2.1: should agent provenance use
   `urn:agentbox:agent:<host>:<container>:<agent-pubkey>` or
   `urn:agentbox:meta:<agent-pubkey>:<sha256-12>`? Decision is required
   before BC20 ACL `orchestrator_acl.rs` is implementable.
2. Per §6.4: VC `execution` drops scope from URN body. Is the lossy
   round-trip acceptable, or should the VC grammar be extended to
   `urn:visionclaw:execution:<scope>:<hash>` to match agentbox?
3. Per §9.2: ADR for kind-30050 cross-mesh envelope. Decide whether to
   adopt the Nostr-shape verbatim or define a new schema with a
   subsumption mapping.
4. Per §10: should `did:nostr:` resolution **always** indirect via a
   DID document, or can WAC consumers accept `did:nostr:` directly with
   a substrate-extension annotation? Affects every actor surface.
5. Per §11.3: does PRD-008 exist as a deferred file, or should
   `prd-bead-provenance-upgrade.md` be renumbered to PRD-008 for
   ordering?
6. Per §13.7: agree on a numbering convention for PRDs going forward.
