# PRD-011 — VisionFlow Forum Kit Extraction

| Field | Value |
|-------|-------|
| Status | Phase 2 complete; kit at nostr-rust-forum v3.0.0-rc4 on crates.io |
| Authors | J. O'Hare + multi-agent synthesis |
| Predecessors | PRD-010, ADR-073, ADR-074, ADR-075, ADR-076, ADR-077, ADR-078 |
| Companion ADRs | ADR-079 (forum-setup skill), ADR-080 (kit deployment topology) |
| Affected repos | `nostr-rust-forum` (canonical kit), `dreamlab-ai-website` (downstream consumer), this repo (mesh integration), `agentbox` (skill provider), `solid-pod-rs` (foundation library) |
| GitHub URLs | https://github.com/DreamLab-AI/nostr-rust-forum, https://github.com/DreamLab-AI/dreamlab-ai-website, https://github.com/DreamLab-AI/VisionClaw, https://github.com/DreamLab-AI/agentbox, https://github.com/DreamLab-AI/solid-pod-rs |

---

## 1. Executive Summary

Extract the community forum currently embedded in `DreamLab-AI/dreamlab-ai-website` (subdirectory `community-forum-rs/`) into a generic, reusable Rust-crate kit hosted at `DreamLab-AI/nostr-rust-forum`. The kit becomes a federated mesh substrate (per ADR-073/074/075) that any operator can deploy with their own configuration and branding. DreamLab's website re-imports the kit as Cargo dependencies plus a `dreamlab.toml` configuration package — closing the fork-and-modify pattern and turning the forum into a first-party generic substrate alongside `solid-pod-rs` and `agentbox`.

### Branding & naming strategy
- **Internal ecosystem brand** (DreamLab PRDs/ADRs/DDD): "VisionFlow forum" / "VisionFlow"
- **GitHub repo** (canonical home): `DreamLab-AI/nostr-rust-forum` (preserves existing 3 stars, 2 forks, redirect-chain integrity; avoids the `visionflow` GitHub collision with the renamed `VisionClaw` repo)
- **Public product name** (README, crates.io, public docs): `nostr-bbs-rs`
- **Crate prefix**: `nostr-bbs-*` (e.g. `nostr-bbs-core`, `nostr-bbs-relay-worker`, `nostr-bbs-pod-worker`)
- **Local workspace path**: `/home/devuser/workspace/nostr-rust-forum/`

The dual-brand approach lets DreamLab refer to "VisionFlow" internally while public-facing documentation, crate names, and the GitHub repo stay unbranded — operators of the kit are not pulled into DreamLab's name space.

---

## 2. Goals

### G1 — Reusable forum substrate
Any operator with Cloudflare Workers + a Solid pod target + a Nostr relay can stand up a forum by writing a single TOML configuration file. Default behaviour mirrors a generic Nostr-BBS deployment; overrides specialise per-deployment.

### G2 — Federation-native
The kit ships ADR-073/074/075 mesh federation built in. `[mesh].mode` flag toggles standalone/federated/client; default standalone. Federation is the kit's core value-add — every deployment is potentially mesh-capable.

### G3 — DreamLab as one consumer among many
DreamLab's website becomes a downstream consumer of the kit, not the canonical implementation. `dreamlab-ai-website` adds a `forum-config/` package that consumes `nostr-bbs-*` crates as Cargo deps and supplies a `dreamlab.toml` configuration that recreates the current forum's behaviour (cohorts, zones, admin pubkeys, branding).

### G4 — TOML-driven configuration
Every operator-customisable surface is configurable via a single `<deployment>.toml` file:
- Branding (name, copy, logos, theme colours)
- Zone access model (public/members/private + custom zones)
- Trust progression (auto-promotion thresholds)
- Custom moderation kinds (default: 30910–30916, overridable)
- WebAuthn relying party (RP ID, expected origin)
- Solid pod base URL
- Relay endpoint(s)
- Federation mesh peer list
- Welcome bot configuration
- Invite/WoT thresholds

### G5 — AI-assisted configurator
A `forum-setup` skill (per ADR-079) helps operators author the per-deployment TOML. Provider-abstracted: works with Codex, Claude Code, the agentbox sovereign agent, or direct API keys (Anthropic / OpenAI). Operator runs `forum-setup wizard` (or invokes via Claude Code), answers ~15 questions, gets a complete validated TOML.

### G6 — Sprint v9-v11 work captured
The forum's existing maturity (Sprint v8 NIPs, Sprint v9 security hardening, Sprint v10 nicknames, Sprint v11 mesh service-list / Tailwind / forum stall fixes) carries forward into the kit as v3.0. No regression from current `dreamlab-ai-website/community-forum-rs/` capabilities.

**Measurable via** the **Sprint Carry-Over Fixture Suite** at `nostr-rust-forum/tests/cutover/sprint-carry-over.rs`. The suite MUST assert each of the following capabilities passes against kit v3.0.0-rc1:

| Capability | Sprint origin | Test |
|------------|---------------|------|
| NIP-98 replay-store trait + KvReplayStore | v9 STREAM-B B1 | unit test on Nip98ReplayStore TTL behaviour |
| WAC Control coercion on `*.acl` writes | v9 STREAM-B B3 | property test: PUT/PATCH on .acl path requires `Control` mode |
| SSRF redirect-bypass fix | v9 STREAM-B B4 | preview-worker ssrf_fetch_with_redirects rejects RFC1918 hop |
| KV namespace split (ADMIN_KV / ADMIN_KV_RO) | v9 STREAM-B B5 | integration test: pod-worker reads ADMIN_KV_RO; auth-worker writes ADMIN_KV |
| NIP-98 admin endpoint `/api/admin/profiles/backfill` | v10 + v11 F4a | integration test: POST returns BackfillResult JSON |
| Username reservation system | v10 | integration test: claim/check/release flow |
| NIP-26 delegation verifier endpoint | v8 W6 | unit test: valid + invalid delegation tokens |
| Mesh service-list scaffolding (kind-30033) | v11 | event-shape test |
| Signer-trait NIP-98 (NIP-07/hardware) | v11 F4b | integration test: extension signer mints valid NIP-98 |
| Tailwind build-time CDN replacement | v11 F1 | bundle-size assertion: tailwind.dist.css ≤ 100KB |
| forum-client sw.js shipped | v11 F1 phase A | curl /sw.js returns 200 |
| Profiles batch + search endpoints | v10 | API contract test |

Suite runs as part of PRD-011 Phase X4 (kit v3.0.0-rc1 → GA gate). Failure on any capability blocks kit GA.

### G7 — De-branding completeness
Zero DreamLab-specific strings, URLs, pubkeys, or copy in `nostr-rust-forum/`. Anti-drift CI gate prevents reintroduction.

### G8 — Re-import path
After kit v3.0 ships, `dreamlab-ai-website` PR replaces `community-forum-rs/` subdirectory contents with a thin `forum-config/` package. The website's deployment pipeline produces the same artefacts as today (Cloudflare Workers + Pages site).

### G9 — Quality engineering parity
Kit inherits ADR-077 ecosystem QE policy. Coverage targets, mutation kill-rates, reference vectors, contract tests apply per ADR-077 P1-P10.

### G10 — Library convergence
Kit adopts ADR-076 (forum nostr-core absorption into upstream `nostr` crate) from inception. No hand-rolled NIP-04/19/26/44/59/98/etc. — everything delegates to `nostr` crate. Kit's `nostr-bbs-core` is a thin shim (~700 LOC target).

---

## 3. Non-Goals

- **NG1**: Replacing `dreamlab-ai-website` as a website. The website is a website; the kit is a substrate. They consume each other.
- **NG2**: Migrating other DreamLab forum instances. The kit is for new deployments; existing instances migrate when convenient (or not).
- **NG3**: Proprietary licensing. Kit is open-source under MIT or Apache-2.0 (TBD per operator preference; existing nostr-rust-forum repo licence currently TBD).
- **NG4**: Multi-tenant single-deployment hosting (one kit deployment serving N forum instances). Each forum instance is its own deployment.
- **NG5**: Replacing `solid-pod-rs` or `agentbox`. The kit consumes `solid-pod-rs` (post 0.5 absorption) and integrates with `agentbox` via mesh federation.

---

## 4. Current State

### Source: `dreamlab-ai-website/community-forum-rs/` (subdirectory of website repo)
8 crates (`auth-worker`, `forum-client`, `nostr-core`, `pod-worker`, `preview-worker`, `relay-worker`, `search-worker`, `admin-cli`), Sprint v9-v11 work landed, ~XX,000 LOC. Branded with DreamLab cohorts, admin pubkeys, copy.

### Target: `DreamLab-AI/nostr-rust-forum` (existing GitHub repo)
Created 2025-12-11; last pushed 2026-04-06; 7-crate workspace (no admin-cli); v2.0 "complete Rust rewrite" tagged. README markets as `nostr-bbs-rs` with 3-zone configurable access model and "first-user-is-admin" approach. **Does not have Sprint v9-v11 work.** 3 stars, 2 forks, public.

### Diff
Sprint v9-v11 capabilities present in `community-forum-rs/` but not (yet) in `nostr-rust-forum`:
- NIP-98 replay-store trait + KvReplayStore impl across 4 workers
- WAC Control coercion for `.acl` writes
- SSRF redirect bypass fix in preview-worker
- KV namespace splits (ADMIN_KV/ADMIN_KV_RO)
- Profiles backfill admin endpoint
- NIP-26 delegation verifier endpoint
- Username reservation system
- Admin-CLI binary
- Mesh service-list scaffolding (kind-30033 per ADR-074)
- Tailwind build-time CDN replacement
- Sprint v11 forum stall + sw.js fixes

### Other related repos
- `dreamlab-ai-website` — https://github.com/DreamLab-AI/dreamlab-ai-website (consumer post-extraction)
- `agentbox` — https://github.com/DreamLab-AI/agentbox (mesh peer + skill provider)
- `solid-pod-rs` — https://github.com/DreamLab-AI/solid-pod-rs (foundation library, post 0.5 absorption)
- `VisionClaw` — https://github.com/DreamLab-AI/VisionClaw (this repo, mesh integration substrate)

---

## 5. Architecture

### 5.1 Target workspace layout (`DreamLab-AI/nostr-rust-forum` v3.0)

```
nostr-rust-forum/
├── Cargo.toml                    # workspace
├── README.md                     # public-facing, no DreamLab branding
├── CLAUDE.md                     # contributor guide
├── LICENSE-MIT (or LICENSE-APACHE)
├── docs/
│   ├── adr/                      # kit's own ADR set, ADR-001 onward (per Q11:a)
│   ├── prd/                      # kit's own PRD set, PRD-001 onward
│   ├── deployment/
│   │   ├── quickstart.md
│   │   ├── toml-reference.md     # canonical TOML schema docs
│   │   ├── mesh-federation.md    # how to join the mesh (ADR-073/074/075)
│   │   └── examples/
│   │       ├── minimal.toml      # smallest viable deployment
│   │       ├── private-team.toml # private-only deployment
│   │       └── public-walled.toml# walled-garden public deployment
│   └── ecosystem/                # cross-references to other DreamLab projects
├── tests/
│   ├── fixtures/                 # cross-substrate reference vectors (ADR-077 P1)
│   ├── conformance/              # IS-Envelope conformance, federation contract tests
│   └── smoke/                    # end-to-end deployment smoke tests
├── crates/
│   ├── nostr-bbs-core/           # thin shim over `nostr` crate; project-specific kinds + Signer
│   ├── nostr-bbs-config/         # TOML schema + loader + validator (NEW)
│   ├── nostr-bbs-mesh/           # ADR-073 federation worker + mesh service-list (NEW)
│   ├── nostr-bbs-forum-client/   # Leptos WASM browser client
│   ├── nostr-bbs-auth-worker/    # CF Worker: WebAuthn + NIP-98 + invites
│   ├── nostr-bbs-pod-worker/     # CF Worker: Solid pod LDP + WAC
│   ├── nostr-bbs-preview-worker/ # CF Worker: link previews + SSRF
│   ├── nostr-bbs-relay-worker/   # CF Worker: NIP-01 relay + NIP-42 AUTH + DO
│   ├── nostr-bbs-search-worker/  # CF Worker: vector search
│   ├── nostr-bbs-admin-cli/      # operator CLI (rename to `bbs-admin`)
│   └── nostr-bbs-setup-skill/    # forum-setup skill implementation (per ADR-079, NEW)
├── examples/
│   ├── deployment-dreamlab/      # reference deployment showing dreamlab.toml shape
│   └── deployment-private-team/  # private-team example
└── .github/
    └── workflows/
        ├── ci.yml                # ADR-077 P1-P9 gates
        ├── mutation.yml          # ADR-077 P4 weekly mutation
        └── deploy-example.yml    # smoke-deploy a dev instance
```

### 5.2 TOML configuration schema (G4)

Single source of truth at `nostr-bbs-config/src/schema.rs`. Per-deployment `<name>.toml`:

```toml
# Required: deployment identity
[deployment]
name           = "Dreamlab Community"        # display name (G7-allowed; not internal branding)
hostname       = "https://forum.example.com"
contact        = "ops@example.com"
license_text   = "© 2026 Example Co. — CC BY-SA 4.0"

# Required: WebAuthn relying party
[webauthn]
rp_id          = "example.com"
expected_origin = "https://forum.example.com"

# Required: Solid pod base
[pod]
base_url       = "https://pod.example.com"
storage_backend = "cf-r2" | "fs" | "s3"

# Required: at least one relay endpoint
[relay]
url            = "wss://relay.example.com"
ingress_policy = "allowlist" | "signed-only" | "open"

# Required: at least one initial admin pubkey OR enable first-user-is-admin
[admin]
mode           = "first-user" | "static"
static_pubkeys = []                          # only when mode="static"

# Optional: branding
[branding]
theme_colour   = "#FFC857"
logo_url       = "https://forum.example.com/static/logo.svg"
hero_text      = "Welcome to our forum"
footer_text    = "Powered by nostr-bbs-rs"

# Optional: zones (default 3-zone model)
[[zones]]
id             = "public"
display_name   = "Public"
read_access    = "anyone"
write_access   = "tl1"

[[zones]]
id             = "members"
display_name   = "Members"
read_access    = "tl1"
write_access   = "tl2"

[[zones]]
id             = "private"
display_name   = "Private"
read_access    = "tl3"
write_access   = "tl3"

# Optional: trust progression (kit-level defaults applied if absent — Q5:a)
[trust]
tl0_to_tl1_threshold = { posts = 1, days = 0 }                    # auto-promote on first post
tl1_to_tl2_threshold = { posts = 5, days = 7 }
tl2_to_tl3_threshold = { admin_invitation = true }

# Optional: invites + welcome bot (Q6:a — on by default)
[invites]
enabled              = true
referente_kind3_required = true
welcome_bot_pubkey   = "<hex>"
default_credit       = 5

# Optional: moderation kinds (Q4:a — kit standard 30910-30916)
[moderation]
kinds_range          = [30910, 30911, 30912, 30913, 30914, 30915, 30916]
nip56_kind_1984      = true

# Optional: federation (default standalone — ADR-073)
[mesh]
mode                 = "standalone" | "federated" | "client"
peer_relays          = []
federated_kinds      = [14, 1059, 30033, 30910, 30911, 30912, 30913, 30914, 30915, 30916]
allowed_remote_dids  = []
honor_remote_moderation = []
delegation_required  = true

# Optional: rate limits (kit defaults applied)
[ratelimit]
relay_events_per_sec = 10
auth_per_60s         = 20
preview_per_60s      = 30
search_per_60s       = 100

# Optional: feature flags
[features]
dvm_marketplace      = false
nip90_handlers       = false
calendar_events      = true
deletion_kind5       = true
```

Validator runs on deployment startup; rejects malformed/missing config with operator-facing error messages.

### 5.3 Federation (G2 + ADR-073/074/075)

Kit deployments natively implement the mesh per ADR-073:
- Federation worker as part of `nostr-bbs-mesh` crate
- Operator flips `[mesh].mode` to enable
- Federation key generated at first boot (separate from operator pubkey)
- Health endpoint `/health/mesh` exposes peer status

Within the DreamLab ecosystem, a kit deployment + agentbox container + VisionClaw substrate form the mesh; cross-deployment, two kit deployments can federate without DreamLab involvement.

### 5.4 DreamLab consumer package (G3 + G8)

`dreamlab-ai-website/forum-config/` (new package post-extraction):

```
dreamlab-ai-website/
├── ... (existing website code)
├── forum-config/
│   ├── Cargo.toml              # depends on nostr-bbs-* crates
│   ├── src/
│   │   ├── main.rs             # boots all 5 CF Workers + relays config
│   │   └── dreamlab_branding.rs # DreamLab-specific copy/logos baked in
│   ├── dreamlab.toml           # the kit configuration that recreates current forum
│   └── deploy.sh
└── community-forum-rs/         # DELETED post-extraction (G8 follow-up sprint)
```

The `dreamlab.toml` carries the existing forum's exact configuration:
- Cohorts: `lobby`, `members`, `trusted` (renamed from kit-default zones)
- Admin pubkey: existing operator key
- Hostname: `dreamlab-ai.com`
- Relay URL: `wss://dreamlab-nostr-relay.solitary-paper-764d.workers.dev`
- Pod URL: `https://pods.dreamlab-ai.com`
- Branding: DreamLab logos, copy, theme

### 5.5 AI-assisted configurator (G5 + ADR-079)

`forum-setup` skill (per ADR-079) is provider-abstracted:

```
nostr-bbs-setup-skill/
├── src/
│   ├── lib.rs                  # skill entry point
│   ├── conversation.rs         # ~15-question conversation flow
│   ├── validators.rs           # TOML validators per question
│   ├── providers/
│   │   ├── claude_code.rs      # Claude Code (default)
│   │   ├── codex.rs            # OpenAI Codex
│   │   ├── agentbox_nostr.rs   # via agentbox sovereign agent over Nostr
│   │   ├── api_key_anthropic.rs# direct Anthropic API
│   │   └── api_key_openai.rs   # direct OpenAI API
│   └── output.rs               # writes <deployment>.toml + sanity-checks
└── README.md
```

CLI usage:

```bash
nostr-bbs-setup-skill wizard \
  --provider claude-code \
  --output ./my-deployment.toml

# alternative: invoked from inside Claude Code
/forum-setup
```

Conversation flow (~15 questions):
1. Deployment name + hostname
2. WebAuthn RP ID
3. Solid pod target (CF R2 / FS / S3)
4. Relay endpoint
5. Admin model (first-user-is-admin / static keys)
6. Zone model (default 3-zone / custom)
7. Trust progression (default thresholds / custom)
8. Invites (enabled / disabled)
9. Welcome bot
10. Moderation (default kind range / custom)
11. Federation (standalone / federated / client + peers)
12. Branding (theme, logo, copy)
13. Rate limits (defaults / custom)
14. Feature flags
15. Sanity check + output path

Skill validates each answer; on completion writes a working TOML and recommends next steps (`wrangler deploy`, `nostr-bbs-admin init-admin`, etc.).

---

## 6. Functional Requirements

### F1 — Repository setup
- F1.1: Local clone of `DreamLab-AI/nostr-rust-forum` at `/home/devuser/workspace/nostr-rust-forum/`.
- F1.2: New branch `import/v3-from-dreamlab-ai-website`.
- F1.3: Single import commit replaces `crates/` content with de-branded extraction.
- F1.4: PR + merge + v3.0 tag.
- F1.5: README rewritten for kit positioning (no DreamLab references; "VisionFlow" appears nowhere public).

### F2 — De-branding pass
- F2.1: All hardcoded URLs (`dreamlab-ai.com`, `dreamlab-nostr-relay.solitary-paper-764d.workers.dev`, etc.) move to `[deployment]` / `[relay]` / `[pod]` config.
- F2.2: Cohort names (`lobby`) become operator-defined `[[zones]]`; kit ships default 3-zone (`public`, `members`, `private`).
- F2.3: Admin pubkeys move to `[admin].static_pubkeys` or first-user-is-admin mode.
- F2.4: Copy/tagline strings move to `[branding]`.
- F2.5: Logos/theme assets move to operator-supplied config-bundle.
- F2.6: Anti-drift CI lint rejects any remaining `dreamlab` substring across `nostr-rust-forum/` (with explicit allowlist for crate names if needed).

### F3 — TOML schema + validator
- F3.1: `nostr-bbs-config` crate with full schema (per §5.2).
- F3.2: Loader supports merging defaults + per-deployment override.
- F3.3: Validator emits structured errors on missing required / invalid types / referenced unknown zones / etc.
- F3.4: TOML reference docs at `docs/deployment/toml-reference.md` auto-generated from schema.

### F4 — Mesh federation native (ADR-073/074/075)
- F4.1: `nostr-bbs-mesh` crate implements ADR-073 federation worker.
- F4.2: Kit emits kind-30033 mesh service-list at boot (per ADR-074 D9).
- F4.3: IS-Envelope encode/decode (per ADR-075) at the messaging boundary.
- F4.4: NIP-26 delegation verifier wired in `relay-worker` event ingest.

### F5 — Library convergence (per ADR-076 + ADR-078)
- F5.1: `nostr-bbs-core` is a thin shim over upstream `nostr` crate (default-features = false, NIP feature set per ADR-076 D3).
- F5.2: Sprint v9-v11 hand-rolled NIPs (`nip04.rs`, `nip19.rs`, `nip26.rs`, `nip44.rs`, `nip90.rs`, `gift_wrap.rs`) NOT carried over from `community-forum-rs/`. Kit uses upstream from inception.
- F5.3: WebAuthn delegated to `webauthn-rs = "0.5"` + `passkey-types = "0.3"` (per ADR-078 B3).
- F5.4: NIP-98 verifier delegates to upstream `nostr::nips::nip98`; kit retains `Nip98ReplayStore` trait + `KvReplayStore` impl (CF KV-specific).

### F6 — QE policy compliance (per ADR-077)
- F6.1: Reference test vectors at `tests/fixtures/` (paulmillr/nip44, NIP-19, BIP-340, RFC 8785 JCS, etc.).
- F6.2: Within-substrate Level 1 contract tests (ADR-077 P2 L1).
- F6.3: Cross-substrate Level 2 contract tests run by VisionClaw (kit participates).
- F6.4: Federation Level 3 smoke tests (kit deploys to staging, runs T7 from Q5).
- F6.5: Mutation testing baseline ≥80% kill-rate on `nostr-bbs-core`, `mesh`, `auth-worker`, `pod-worker`, `relay-worker` ACL paths.
- F6.6: Coverage thresholds per ADR-077 P6.
- F6.7: Anti-drift CI on `nostr-bbs-core/src/` rejects new `nip\d+|bech32|webauthn|jcs` modules unless thin shims.

### F7 — Forum-setup skill (per ADR-079)
- F7.1: `nostr-bbs-setup-skill` crate with conversation flow + 5 provider implementations.
- F7.2: Skill registered in `agentbox/skills/` + standalone CLI binary `nostr-bbs-setup-skill`.
- F7.3: Skill produces validated TOML; sanity-check pass before write.
- F7.4: Provider abstraction (Q8:b) — single skill, swappable provider via `--provider` flag or `[provider]` config.

### F8 — DreamLab consumer package (G3, G8)
- F8.1: New package `dreamlab-ai-website/forum-config/` (created post-kit-v3.0).
- F8.2: `dreamlab.toml` recreates current forum behaviour (cohorts, admin, branding).
- F8.3: `community-forum-rs/` subdirectory deleted post-cutover.
- F8.4: Existing CI / deployment workflows in `dreamlab-ai-website` updated to use kit crates.

### F9 — Documentation
- F9.1: Kit's own `README.md` — public-facing, no DreamLab references, "what is nostr-bbs-rs", quickstart.
- F9.2: Kit's own `CLAUDE.md` — contributor guide for the kit's repo.
- F9.3: Kit's own `docs/adr/` set starting `ADR-001` (per Q11:a). Initial ADRs: ADR-001 (workspace structure), ADR-002 (TOML schema design), ADR-003 (provider-abstracted setup skill — references this PRD), ADR-004 (federation behaviour).
- F9.4: Cross-reference from kit docs to mesh ADRs (`see https://github.com/DreamLab-AI/VisionClaw/blob/main/docs/adr/ADR-073-private-nostr-relay-mesh-topology.md`).
- F9.5: Migration guide for operators of existing forum instances.

### F10 — Release versioning
- F10.1: Kit `v3.0.0` ships with extraction + de-branding + mesh federation + ADR-076 absorption.
- F10.2: Subsequent kit releases follow semver; minor updates carry forward to dreamlab-ai-website via `cargo update`.
- F10.3: Kit publishes to crates.io as `nostr-bbs-*` crates (Cargo registry; not git deps long-term).

### F11 — Per-project ecosystem cross-referencing
Each of the 5 ecosystem repos gains an `Ecosystem & Federation` section in its CLAUDE.md cross-referencing the others. Per project (per Q10:c — add new artefacts now, in-place sweep later):
- VisionClaw CLAUDE.md → references kit + agentbox + solid-pod-rs + dreamlab-ai-website + forum-config
- nostr-rust-forum CLAUDE.md (NEW) → references VisionClaw + agentbox + solid-pod-rs + dreamlab-ai-website
- agentbox CLAUDE.md → references VisionClaw + kit + solid-pod-rs + skill provider
- solid-pod-rs CLAUDE.md → references VisionClaw + kit + agentbox as consumers
- dreamlab-ai-website CLAUDE.md → references kit (its upstream) + VisionClaw + agentbox

---

## 7. Phasing

### Phase X0 — Repository setup (~3 days)
- Clone nostr-rust-forum locally.
- Create `import/v3-from-dreamlab-ai-website` branch.
- Run de-branding extraction script over `community-forum-rs/`.
- Single import commit + PR + merge + v3.0-rc1 tag.

### Phase X1 — Workspace restructure (~1 sprint)
- Add `nostr-bbs-config` crate with TOML schema.
- Add `nostr-bbs-mesh` crate scaffolding (ADR-073/074/075 stubs).
- Refactor existing crates to consume `nostr-bbs-config` for all hardcoded values.
- Anti-drift CI added.

### Phase X2 — Library convergence (~1.5 sprints)
- Kit-side ADR-076 absorption: `nostr-bbs-core` becomes thin shim over `nostr` crate.
- WebAuthn delegated to `webauthn-rs` (kit-side ADR-078 B3).
- NIP-98 verifier delegates to upstream; replay store trait kept.
- F26-style WASM/CF Workers compatibility spike validates for kit's specific build matrix.

### Phase X3 — QE policy compliance (~1 sprint)
- Reference vectors landed.
- Mutation testing baseline.
- Coverage gates active.
- Anti-drift CI active.
- Cross-substrate contract tests run by VisionClaw integration repo.

### Phase X4 — Forum-setup skill (~1 sprint, parallel)
- Per ADR-079 implementation.
- Provider abstraction with 5 backends.
- Standalone CLI + agentbox skill registration.

### Phase X5 — DreamLab consumer cutover (~1 sprint)
- `dreamlab-ai-website/forum-config/` created.
- `dreamlab.toml` reproduces current forum behaviour.
- `community-forum-rs/` deleted.
- Deployment pipeline updated.

### Phase X6 — v3.0.0 GA (~0.5 sprints)

**Exit criteria (lifted from ADR-083 D11 cutover pre-flight; v3.0.0 GA tag MUST satisfy all):**

```
[ ] All Sprint Carry-Over Fixture Suite tests pass (G6 above; nostr-rust-forum/tests/cutover/sprint-carry-over.rs)
[ ] Cross-substrate fixture sync verified per ADR-082 D5 (sync-fixtures.sh --verify clean across all 4 substrates)
[ ] L2 contract tests passing for ≥7 consecutive nights (ADR-077 P2 L2)
[ ] Federation smoke tests passing for ≥7 consecutive nights (ADR-077 P2 L3)
[ ] Anti-drift CI lint passes on nostr-rust-forum (ADR-077 P3; zero `dreamlab` substring matches)
[ ] WASM/CF Workers compatibility spike (F26) outcome recorded — all 5 worker targets compile, bundle deltas within +200 KiB
[ ] Coverage ≥80% line / ≥70% branch on all kit crates (ADR-077 P6)
[ ] Mutation kill-rate ≥80% on protocol-implementing modules (ADR-077 P4)
[ ] DID Document type-string CI assertion passes (verificationMethod.type == SchnorrSecp256k1VerificationKey2019)
[ ] crates.io org/namespace registered (per §9 Q2) and `nostr-bbs-*` crates publishable
[ ] Migration guide for operators of existing forum instances at docs/deployment/migration.md
[ ] forum-setup skill (ADR-079) end-to-end smoke passes for all 5 providers
[ ] License decision (MIT/Apache-2.0 dual per §9 Q1) committed in LICENSE files
[ ] Forum kit v3.0.0-rc3 stable for ≥72 hours in staging
[ ] Operator runbook for D6 federation key custody (ADR-081 D9 tooling) published
```

Activities (operational deliverables):
- crates.io publish of `nostr-bbs-core`, `nostr-bbs-config`, ..., `nostr-bbs-setup-skill`
- v3.0.0 git tag + GitHub release with changelog from v2.0
- Migration guide for operators
- Federation smoke testing in production-equivalent env per ADR-077 P2 L3

**X6 MUST NOT proceed if any exit criterion fails.** Acceptable to slip date; not acceptable to ship with failing gates.

**Total: ~5-6 sprints (~10-12 weeks) at 1 engineer FTE.** Phase X4 (skill) and Phase X3 (QE) can run parallel to X1/X2 if 2 engineers; total compresses to ~3-4 sprints.

---

## 8. Risks

### R1 — De-branding completeness (HIGH)
DreamLab-specific strings hide in unexpected places (test fixtures, error messages, log lines, TOML examples).
*Mitigation*: Anti-drift CI lint with allowlist; manual sweep at v3.0-rc1; bug bounty for any DreamLab string found post-GA.

### R2 — TOML schema divergence (MEDIUM)
As deployments customise, the schema accumulates feature flags. Without governance, the TOML grows to a config-soup.
*Mitigation*: Schema versioning (`schema_version = "1"` field); RFC process for new fields; deprecation policy.

### R3 — Sprint v9-v11 work fidelity (MEDIUM)
Importing v9-v11 work via single commit risks losing context. Reviewers can't easily diff.
*Mitigation*: PR description includes per-sprint summary linked to `dreamlab-ai-website/community-forum-rs/` commit ranges; v3.0-rc1 → v3.0-rc2 → v3.0 staged; existing sprint-v9-status memory keys carry forward.

### R4 — Skill provider proliferation (LOW)
Five provider backends adds maintenance burden.
*Mitigation*: ADR-079 specifies a clear `Provider` trait; new providers ship in their own feature flag; community contributions encouraged.

### R5 — DreamLab forum regression during cutover (HIGH)
Replacing `community-forum-rs/` with `forum-config/` could break existing users' sessions, lose data, etc.
*Mitigation*: Cutover happens behind a feature flag; old-and-new paths run in parallel for 1-2 weeks; data migration scripts validated; rollback plan documented.

### R6 — GitHub repo brand confusion (LOW)
Repo named `nostr-rust-forum` while public product is `nostr-bbs-rs` — operators may be confused.
*Mitigation*: README opens with explicit alias note ("nostr-rust-forum is the GitHub home of nostr-bbs-rs"); crates.io and docs use product name consistently.

### R7 — Mesh federation regression (MEDIUM)
Kit absorbs PRD-010 mesh requirements; if those introduce bugs, all kit deployments inherit them.
*Mitigation*: ADR-077 P2 Level 3 federation smoke runs on every kit release pre-GA.

### R8 — License decision deferred (LOW)
Kit's licence (MIT vs Apache-2.0) currently TBD. Operators may delay adoption pending choice.
*Mitigation*: Choose at v3.0-rc1; document in PRD-001 of kit's own ADR set.

---

## 9. Open Questions

### Q1 — License: MIT vs Apache-2.0 vs MIT/Apache dual?
Kit licensing affects downstream commercial deployments. solid-pod-rs is AGPL-3.0; agentbox is MIT/Apache-2.0; nostr crate is MIT. **Default recommendation**: MIT/Apache-2.0 dual (matches Rust ecosystem convention and `nostr` crate). Decision deferred to kit ADR-001.

### Q2 — Crates.io organisation
Publish under `dreamlab-ai` org or generic `nostr-bbs` namespace? Generic namespace requires registration. **Recommendation**: `nostr-bbs` namespace if available; fall back to `dreamlab-ai-` prefixed.

### Q3 — Skill discovery
How do operators find the `forum-setup` skill? Via `agentbox/skills/` directory; via `cargo install nostr-bbs-setup-skill` for CLI; via Claude Code's skill registry. Multi-channel discovery — needs marketing pass at GA.

### Q4 — Existing `community-forum-rs` retirement
Hard-cut at GA, or keep both deployment paths for a quarter? **Recommendation**: keep both for 1 month post-GA; cutover when DreamLab forum operator confirms stable.

### Q5 — Backwards-compatible TOML schema migrations
When schema v2 ships with breaking changes, how do existing v1 TOMLs migrate? **Recommendation**: `nostr-bbs-config migrate` CLI command; auto-migration on deploy with warning.

### Q6 — Per-deployment localisation
Forum UI strings live in TOML or in separate `i18n/<lang>/` JSON files? **Recommendation**: TOML for high-level strings; JSON for full UI translations. Out-of-scope for v3.0; v3.1+ feature.

### Q7 — Federation key custody for kit deployments
Each kit deployment generates a federation key on first boot. Operator key custody is out-of-band. **Recommendation**: kit ships `nostr-bbs-admin federation-key generate` command + docs; operators choose KMS or filesystem.

### Q8 — Cross-deployment user identity
A user with passkey on Forum A — can they log in to Forum B with the same passkey? Yes if both deployments share `[webauthn].rp_id`; otherwise different keys per Q7:a. **Recommendation**: document the trade-off in `docs/deployment/identity-portability.md`.

---

## 10. Success Metrics

### M1 — De-branding completeness
Zero `dreamlab` substring matches in `nostr-rust-forum/` (excluding allowlisted contributor names in commit history). Verified by anti-drift CI.

### M2 — TOML round-trip
A deployment booted from `examples/deployment-dreamlab/dreamlab.toml` reproduces the exact behaviour of the current `dreamlab-ai-website/community-forum-rs/` deployment. Verified by integration smoke.

### M3 — Mesh federation participation
A kit deployment in `[mesh].mode = "federated"` successfully participates in a 3-substrate mesh (kit + VisionClaw + agentbox). Verified by ADR-077 P2 L3.

### M4 — Setup skill completion rate
A new operator using `forum-setup wizard` produces a valid TOML in <15 minutes for their first deployment. Verified by user testing.

### M5 — DreamLab cutover safety
DreamLab forum cutover completes with zero data loss, zero session loss, ≤1% user-visible regression rate. Verified by post-cutover audit.

### M6 — Quality engineering targets
Coverage ≥80% line / ≥70% branch on all kit crates. Mutation kill-rate ≥80% on protocol-implementing modules. Reference vectors pass on every PR. Anti-drift CI active.

---

## 11. References

- ADR-073 — Private Nostr Relay Mesh Topology (mesh kit inherits)
- ADR-074 — Cross-System DID:Nostr Canonicalisation (kit emits canonical DID Documents)
- ADR-075 — IS-Envelope v1 Contract (kit's messaging boundary)
- ADR-076 — Forum nostr-core absorption into upstream `nostr` crate (kit applies from inception)
- ADR-077 — Ecosystem QE Policy (kit complies)
- ADR-078 — Cross-Substrate Library Convergence (kit's shim layer)
- ADR-079 (companion) — Forum-setup Skill Provider Abstraction
- ADR-080 (companion, planned) — Kit Deployment Topology Patterns
- PRD-010 — DID:Nostr Mesh Federation (kit becomes 5th substrate)
- `docs/integration-research/qe-fleet/Q1..Q5-*.md` — QE fleet audit
- GitHub repos:
  - https://github.com/DreamLab-AI/nostr-rust-forum (canonical kit, public product `nostr-bbs-rs`)
  - https://github.com/DreamLab-AI/VisionClaw (this monorepo, mesh integration)
  - https://github.com/DreamLab-AI/agentbox (mesh peer + skill provider)
  - https://github.com/DreamLab-AI/solid-pod-rs (foundation library)
  - https://github.com/DreamLab-AI/dreamlab-ai-website (downstream consumer post-cutover)
