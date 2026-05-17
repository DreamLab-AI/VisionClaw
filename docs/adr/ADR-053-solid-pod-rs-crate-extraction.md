# ADR-053: solid-pod-rs Crate Extraction

## Status

Implemented 2026-04-20 (ratified 2026-04-19).

JSS was retired 2026-04-20. `solid-pod-rs` has been extracted to
`github.com/DreamLab-AI/solid-pod-rs` and is consumed by VisionClaw as
an external git dependency pinned to `v0.4.0-alpha.1`. The local
workspace crates (`crates/solid-pod-rs*`), the JSS proxy handler, and
the `SOLID_IMPL` shadow/dispatch scaffolding have all been removed.
See the `chore/solid-pod-rs-externalise-jss-cut` commit and the
completion banner at the top of `docs/ops/jss-native-cutover.md`.

## Date

2026-04-19

## Related Documents

- ADR-048: Dual-Tier Identity Model — Bridging KG Notes and Ontology Classes
- ADR-028-ext: Optional Authentication (Solid OIDC optionality)
- ADR-052: (predecessor ADR in Wave 4 sequence)
- `src/handlers/solid_proxy_handler.rs` — current JSS proxy
- community-forum-rs: `crates/pod-worker/` (Cloudflare Worker pod implementation)
- JavaScriptSolidServer (JSS): https://github.com/JavaScriptSolidServer/JavaScriptSolidServer
  — AGPL-3.0-only

## Context

VisionClaw currently proxies Pod requests through a Node.js JSS
(JavaScriptSolidServer) via `src/handlers/solid_proxy_handler.rs`.
The sibling community-forum-rs project (at
`/home/devuser/workspace/project2/community-forum-rs`) has a
Cloudflare-Worker-specific Rust Pod implementation in
`crates/pod-worker/` (44 KB, 13 modules, LDP + WAC + Solid
Notifications).

Both projects need a Rust Solid Pod server. Keeping two Pod
implementations (Node JSS + CF pod-worker) is long-term tech debt —
every spec change has to land twice, ops has two runtimes to operate,
and neither codebase gets community hardening.

We want to frame a Rust port as a contribution to the Solid ecosystem
rather than competition with JSS. solid-pod-rs is NOT a derivative
work of JSS's JavaScript source: the Rust implementation originates
from `community-forum-rs/crates/pod-worker` (written in Rust from
scratch), and we read JSS only as a reference-only resource to
understand Solid Protocol 0.11 + WAC + LDP + Solid-OIDC + Solid
Notifications behaviour. See `crates/solid-pod-rs/NOTICE` for the full
licensing relationship.

## Decision

Extract pod-worker into a standalone **`solid-pod-rs`** crate, initially housed
as a VisionClaw workspace member at `crates/solid-pod-rs/`, then extracted to
its own GitHub repo post-parity.

### Phase 1 (weeks 1-3) — Extract & Seed

- Convert VisionClaw root to a Cargo workspace; add new member
  `crates/solid-pod-rs/`.
- Lift pod-worker source; strip Cloudflare-isms: `worker` crate, R2 SDK, D1
  calls, KV calls, Durable Objects.
- Introduce a `Storage` trait abstraction with backends:
  - `MemoryBackend` — tests
  - `FsBackend` — VisionClaw default
  - `S3Backend` — feature-gated
  - `R2Backend` — feature-gated for community-forum consumers
- Vendor latest JSS at `references/javascript-solid-server/` as a git submodule,
  read-only reference for porting.
- `PARITY-CHECKLIST.md` catalogues every JSS feature →
  `present | partial | missing` status.

### Phase 2 (weeks 3-6) — Port Gaps to Parity

Expected gaps (from pod-worker vs JSS):

- **Solid-OIDC flow** — pod-worker bypasses via NIP-98; port behind
  `OIDC_ENABLED` feature flag for ecosystem clients.
- **LDP conformance edge cases** — `Link` headers, `Prefer` headers,
  `Accept-Post`, `PATCH` via SPARQL-Update.
- **Content negotiation matrix** — Turtle, JSON-LD, N-Triples, RDF/XML.
- **Full Solid Notifications** — WebSocketChannel2023 + WebhookChannel2023.
- **ACL inheritance edge cases** — port JSS test corpus (~N tests).
- **`.meta` resources**.
- **Server-managed triples** — `dateModified`, `size`, `contains`, etc.

**Parity gate**: port JSS's own test corpus to Rust; must pass to exit Phase 2.

### Phase 3 (weeks 6-9) — Integrate into VisionClaw

- Embed `solid-pod-rs` as an Actix service in VisionClaw at
  `src/handlers/solid_pod_handler.rs`.
- Feature flag `SOLID_IMPL=jss|native` — both paths coexist during shadow testing.
- Shadow-test in staging for one week: parallel requests to both
  implementations; compare bytes + headers.
- Cutover: flip default to `native`; retain `jss` as rollback for one release.
- Week 10: remove JSS proxy from VisionClaw.

### Post-Parity (week 10) — Extract to Own GitHub Repo

```bash
git subtree split --prefix=crates/solid-pod-rs -b solid-pod-rs-history
# Create dreamlab-ai/solid-pod-rs repo
git push new-origin solid-pod-rs-history:main
# Publish 0.1.0 to crates.io
# VisionClaw Cargo.toml: path = "crates/solid-pod-rs" → version = "0.1.0"
# Delete crates/solid-pod-rs/ from VisionClaw
# community-forum-rs updates to depend on the same published crate
```

### Licensing

**AGPL-3.0-only**, inherited from the JavaScriptSolidServer (JSS)
ecosystem covenant. JSS is AGPL-3.0-only and solid-pod-rs preserves
the same network-service copyleft protection rather than weakening
it with a permissive relicence. See `crates/solid-pod-rs/NOTICE` for
the full provenance chain (JSS → community-forum-rs pod-worker →
VisionClaw crates/solid-pod-rs → dreamlab-ai/solid-pod-rs) and the
rationale for inheritance.

Operational consequences:

- Consumers operating solid-pod-rs as a network-accessible service
  must make their corresponding source code available to their users
  per AGPL §13.
- VisionClaw itself is already AGPL-compatible; the previous MIT-OR-
  Apache-2.0 dual-licence stance was an under-claim that weakened the
  Solid ecosystem covenant. 0.3.0-alpha.3 (tracked in
  `crates/solid-pod-rs/CHANGELOG.md`) migrates to AGPL.
- Dependency-graph policy lives in `crates/solid-pod-rs/deny.toml`:
  AGPL-3.0 is on the allowlist; permissive ecosystem licences
  (MIT, Apache-2.0, BSD, ISC, MPL-2.0, etc.) remain on the allowlist
  for compatibility with the wider Rust dependency graph.

### JavaScriptSolidServer Engagement

At the `0.1.0` milestone, open an issue on
https://github.com/JavaScriptSolidServer/JavaScriptSolidServer
offering solid-pod-rs as a Rust-side reference implementation. Frame
as contribution to the Solid ecosystem, not competition. Invite the
JavaScriptSolidServer contributors to review the crate and file issues
against any parity gaps they care about.

## Consequences

### Positive

- Single Rust Pod server across VisionClaw + community-forum-rs (unified ops,
  one upstream).
- Eliminates Node.js runtime from the VisionClaw stack (single-language ops).
- OSS reference implementation for the Rust Solid ecosystem.
- community-forum-rs gets the R2 backend as a thin adapter over the shared crate.
- Feature flag cutover allows safe rollback.

### Negative

- ~1 month of extraction + porting + parity work (scoped to parallel Sprint B).
- We own Solid spec tracking forever (previously delegated to JSS).
- New crate has no fuzzing / production hardening; staging shadow testing is
  critical.

## Non-Goals (v0.1.0)

- Multi-backend orchestration (one backend per deployment is enough).
- IPFS-based storage (YAGNI; can add later as another backend).
- Solid-OIDC as primary auth for VisionClaw (we use NIP-98 per ADR-028-ext;
  OIDC is feature-flagged for ecosystem interop).
- ActivityPub bridge (future ADR, after 0.1.0).

## Compliance Criteria

- [ ] `crates/solid-pod-rs/` scaffolded as VisionClaw workspace member
- [ ] `Storage` trait with 2+ MVP backends (Memory, FS); Phase 1 exit gate
- [ ] `references/javascript-solid-server/` vendored; `PARITY-CHECKLIST.md` exists
- [ ] JSS-equivalent test corpus authored; all green; Phase 2 exit gate
- [ ] Shadow-testing harness in staging; Phase 3 operational
- [ ] `SOLID_IMPL=native` default on; JSS removed by week 10
- [ ] Crate extracted to own GitHub repo; 0.1.0 on crates.io
- [ ] community-forum-rs migrated to depend on published crate
- [ ] JavaScriptSolidServer contributors notified via GitHub issue

## Rollback

- `SOLID_IMPL=jss` preserves Node proxy behaviour across any phase up to week 10.
- Post-extraction: pin crate version in `Cargo.toml` for stability; breaking
  changes gated by 0.2.0.
