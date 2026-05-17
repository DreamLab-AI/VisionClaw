# ADR-056: JSS Parity Migration Architecture

## Status

Ratified 2026-04-20 (Sprint 4 kickoff).

## Context

Sprint 3 landed `solid-pod-rs` as an extracted standalone crate (ADR-053) and
paid down the sovereign-mesh debt that blocked production (ADR-055). During
Sprint 3 extraction, upstream was mis-attributed to Community Solid Server
(Melvin Pirera, MIT). That attribution was wrong. The real upstream is
**JavaScriptSolidServer (JSS)**, maintained by the JS Solid Server
organisation under **AGPL-3.0**. The crate inherited AGPL-3.0 at
`v0.3.0-alpha.3` and the NOTICE + LICENSE corpus was corrected on main.

A fresh gap analysis landed in the same cycle:

- `crates/solid-pod-rs/GAP-ANALYSIS.md` — 97 features enumerated, 6,000 words
- `crates/solid-pod-rs/PARITY-CHECKLIST.md` — row-by-row tracking, 4,400 words
- `crates/solid-pod-rs/docs/reference/jss-feature-inventory.md` — the
  authoritative feature inventory derived from the JSS source tree

Current strict parity: **74%**. The remaining 26% splits cleanly into two
categories — spec-required core gaps (small, bounded) and JSS-specific extras
(large, optional). The architectural question for Sprint 4 is how to
structure the closing push without bloating the core crate, diverging from
the JSS ecosystem, or smearing AGPL conformance across ambiguous boundaries.

This ADR pins the answer for the v0.4.0 and v0.5.0+ trains.

## Decision

### D1 — v0.4.0 scope: core parity (6 F-tickets + 1 refactor)

The v0.4.0 scope stays narrow. Only features that are **Solid Protocol
spec-required** or **security-ship-blockers** land in the core crate. JSS
niceties defer to v0.5.0+ sibling crates. Tickets (full requirements in the
PRD):

| Ticket | Scope | Driver |
|--------|-------|--------|
| F1 | SSRF guard on outbound HTTP | Security ship-blocker |
| F2 | Dotfile allowlist at pod root | Security ship-blocker |
| F3 | `solid-0.1` notifications adapter | JSS compat |
| F4 | `acl:origin` enforcement | WAC spec gap |
| F5 | DPoP `jti` replay cache | OIDC spec hardening |
| F6 | Config-loader parity (env + file merge) | Ops compat |
| F7 | Library-vs-server refactor | Architectural hygiene |

F7 is prerequisite to D3 below.

### D2 — v0.5.0+ scope: JSS extras as sibling crates

Each major JSS-specific subsystem becomes a **sibling crate** under the
`dreamlab-ai/solid-pod-rs-{name}` namespace rather than a feature flag on
the core crate:

| Crate | Scope | Rough LOC |
|-------|-------|-----------|
| `solid-pod-rs-activitypub` | ActivityPub federation | ~1,200 |
| `solid-pod-rs-git` | Git HTTP backend | ~600 |
| `solid-pod-rs-idp` | OAuth/OIDC identity provider | ~900 |
| `solid-pod-rs-nostr` | Embedded Nostr relay + `did:nostr` auth | ~700 |
| `solid-pod-rs-webid-tls` | Legacy WebID-TLS | ~300 (low priority) |

Rationale for per-crate separation rather than Cargo features:

1. **Core stays small.** JSS positions itself as "~432 KB minimal". Sibling
   crates preserve that positioning; opaque `--features ap,git,idp` would
   defeat it via transitive dependency bloat even when the flag is off.
2. **Independent semver.** Federation, Git, and IDP evolve on separate
   timelines. A breaking change in `activitypub` should not force a major
   bump of `solid-pod-rs` proper.
3. **Opt-in surface.** Consumers that embed the library (VisionClaw,
   downstream Solid apps) pull only what they use. Minimal deployments
   stay minimal.
4. **Mirror, don't copy, JSS philosophy.** JSS gates features behind CLI
   flags on a monolith. We encode the same "opt in to the heavy stuff"
   intent as crate boundaries — more honest for Rust consumers, same
   outcome for operators.

### D3 — Library-vs-server separation (F7)

Today's `solid-pod-rs` bundles the HTTP server directly into the library
crate. `actix-web::HttpServer::new` lives alongside the trait-based storage
API. That conflates two audiences: embedders (VisionClaw, third-party
Rust services) and JSS-style operators (cargo install, run the binary).

The v0.4.0 refactor splits these:

- **Library crate `solid-pod-rs`** — Trait-based, embeddable. No
  `#[tokio::main]`. No direct `HttpServer` construction. Consumers wire
  their own server using the traits. Zero Actix-specific surface in the
  public API.
- **Binary crate `solid-pod-rs-server`** — The standalone CLI + server that
  JSS users expect. Depends on `solid-pod-rs` + chosen extension crates.
  Ships the default Actix wiring, config loader, and `main.rs`.

Both crates ship from the same Cargo workspace. `cargo install
solid-pod-rs-server` gives operators a drop-in binary; library embedders
depend on `solid-pod-rs` only.

### D4 — AGPL-3.0 conformance

All new dependencies in the workspace must be AGPL-3.0-compatible.
`cargo deny check` runs in CI against an updated allowlist. The §13
network-service copyleft obligation — any hosted instance exposes
corresponding source — is documented for operators in
`docs/ops/agpl-compliance.md` (pointer from each sibling crate's README).

Downstream consumers that object to AGPL are directed to evaluate whether
they need the library (MIT/Apache candidates exist) or the full server
(they do not; AGPL applies). This ADR does not relicense.

### D5 — Parity measurement discipline

Every future PR that touches parity surface:

- Updates the relevant row status in `PARITY-CHECKLIST.md`
- Lists the affected rows in the PR description
- Links to the JSS `file:line` for the matched feature
- Is QE-verified by a `parity_vNN.rs` test (new file per train)

**Ratchet rule.** The parity row count can only go up. Regressions are
blocker bugs and must be fixed on the same branch that introduced them.

## Consequences

### Positive

- Core crate stays small; performance and embedding appeal preserved
- Sibling-crate model enables opt-in federation, Git, IDP without bloat
- Library-server split matches Rust ecosystem conventions (hyper, tower,
  axum all do this)
- AGPL scope becomes unambiguous: core + binary + siblings all AGPL-3.0,
  documented, `cargo deny`-enforced
- Parity ratchet mechanism prevents silent drift across trains

### Negative

- Consumers targeting JSS's "full features via flags" mental model must
  discover and compose several crates. Docs burden is on us to make that
  trivial — sibling READMEs + a landing-page matrix are mandatory.
- Each sibling crate adds CI, release cadence, and coordination cost.
  Estimate: ~0.5 engineer-day per release per crate.
- Ownership question for sibling crates. Near-term: DreamLab AI
  (`dreamlab-ai` org). Long-term: community-fork-friendly; contributor
  licence agreement is the copyleft inbound-equals-outbound default.

### Neutral

- JSS v0.0.x velocity remains high. Our semver discipline is deliberately
  slower — not a bug, a positioning choice for embedders.
- Our `acl:origin` + `acl:agentGroup` enforcement (F4 + existing work)
  exceeds JSS's own WAC coverage. This is spec-correct, not
  parity-breaking; documented in the checklist as "spec-equal, impl-ahead".

## Non-goals (this ADR)

- v1.0.0 long-term roadmap — separate future ADR after v0.5.0 lands
- ActivityPub protocol choices inside `solid-pod-rs-activitypub` —
  separate ADR when that crate is scoped
- Hosting infrastructure, TLS termination, deployment manifests — ops
  concern, not architecture
- Funding and governance of the sibling crates — separate governance doc

## Compliance criteria

- [ ] v0.4.0 ships with F1–F6 landed + F7 refactor merged
- [ ] `PARITY-CHECKLIST.md` row coverage ≥ 95%
- [ ] Each extension crate registered under `dreamlab-ai` GitHub org
      (placeholders acceptable until v0.5.0 scope)
- [ ] Library crate `solid-pod-rs` contains zero direct
      `actix_web::HttpServer` references
- [ ] Binary crate `solid-pod-rs-server` compiles, runs, and serves
      `/.well-known/solid` against a smoke-test storage backend
- [ ] `cargo deny check` passes across the whole workspace
- [ ] `docs/ops/agpl-compliance.md` exists and is linked from every
      crate's README

## Rollback

- Each F-ticket lands behind its own feature flag (gate list defined in
  the PRD). Disable the flag to revert behaviour without code revert.
- Library-server split is reversible: re-merge `solid-pod-rs-server` as a
  `[[bin]]` target inside `solid-pod-rs`'s `Cargo.toml`. Single-file
  revert, no API break for library consumers (they never depended on the
  binary).
- Extension crates do not land on `main` until their scope is green-lit
  per-crate. If a sibling crate proves unviable, it is abandoned in
  place; the core crate carries no rollback cost.

## Related documents

- PRD: `docs/prd/jss-parity-migration.md` (Sprint 4, sibling-agent scope)
- DDD: `docs/design/jss-parity/` (Sprint 4, sibling-agent scope)
- Gap analysis: `crates/solid-pod-rs/GAP-ANALYSIS.md`
- Parity checklist: `crates/solid-pod-rs/PARITY-CHECKLIST.md`
- Feature inventory: `crates/solid-pod-rs/docs/reference/jss-feature-inventory.md`
- ADR-053: `solid-pod-rs` crate extraction (precursor)
- ADR-055: Sovereign debt payoff + Phase 2 sprint (precursor)
