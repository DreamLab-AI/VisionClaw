# PRD-012 — DreamLab Website Kit Adoption & Cloud Infrastructure Transition

| Field | Value |
|-------|-------|
| Status | Phase 3 forum-config/ created; cutover pending (ADR-083) |
| Authors | Mega-sprint synthesis (J. O'Hare) |
| Predecessors | PRD-010, PRD-011, ADR-073-083 |
| Companion ADRs | ADR-080 D7, ADR-083 (tactical cutover), ADR-084 (NEW; cloud infra mapping), ADR-085 (NEW; forum-config/ architecture) |
| Companion DDD | `docs/ddd-mesh-federation-context.md` BC-MESH-DREAMLAB-CONSUMER (V6 + V13 extensions) |
| Affected repos | `dreamlab-ai-website` (primary subject), `nostr-rust-forum` (kit upstream — version pin only) |
| GitHub URLs | https://github.com/DreamLab-AI/dreamlab-ai-website, https://github.com/DreamLab-AI/nostr-rust-forum |

---

## 1. Executive Summary

Specify the engineering work that transforms `dreamlab-ai-website` from a **monorepo containing the forum** (the current shape — `community-forum-rs/` subtree with 8 crates + 5 CF Workers + branding mixed with protocol code) into a **thin downstream consumer of the VisionFlow forum kit** (`nostr-bbs-rs` published from `DreamLab-AI/nostr-rust-forum`). Post-transition the website's `forum-config/` Cargo package depends on `nostr-bbs-*` crates, supplies a `dreamlab.toml` instance configuration, retains existing Cloudflare Workers infrastructure (D1 / KV / R2 / DO bindings preserved unchanged), and the `community-forum-rs/` subtree is deleted.

This PRD differs from PRD-011 (which extracts the kit upstream) by focusing on **the consumer side** — the infrastructure migration, the branding preservation, the operator journey, and the engineering deliverables the website team owns. ADR-083 supplies the tactical cutover mechanics (feature flag, dual-deploy, rollback). PRD-012 fills the gap between "the kit exists" and "the website successfully consumes it."

**Branding & naming clarity**:
- The kit (upstream): public name `nostr-bbs-rs`, GitHub `DreamLab-AI/nostr-rust-forum`, internal brand "VisionFlow forum"
- The consumer (this PRD's subject): GitHub `DreamLab-AI/dreamlab-ai-website`, internal product name "DreamLab Community Forum"
- `dreamlab.toml`: the operator-side TOML configuration that recreates DreamLab's current forum behaviour atop the kit

---

## 2. Goals

### G1 — Zero-regression behavioural parity
Post-cutover, the production forum at `https://dreamlab-ai.com/community/` delivers byte-equal behaviour to pre-cutover for the 100% of user-visible flows: registration, login, posting, threading, DM, mod actions, profile management, invite redemption, welcome bot interactions. Verified via the **Sprint Carry-Over Fixture Suite** (per PRD-011 G6 + ADR-083 D6 parity reports).

### G2 — Cloud infrastructure preserved
All existing Cloudflare resources keep their identity (per ADR-084):
- D1 databases: `dreamlab-relay`, `dreamlab-auth` — kept; schema unchanged (per ADR-083 D4 invariant)
- KV namespaces: `SESSIONS`, `POD_META`, `ADMIN_KV`, `ADMIN_KV_RO`, `NIP98_REPLAY`, `RATE_LIMIT`, `SEARCH_CONFIG` — all kept
- R2 buckets: `dreamlab-pods`, `dreamlab-vectors` — both kept
- Durable Objects: `NostrRelayDO` class — kept (re-exported from kit)
- Custom domains: `dreamlab-ai.com`, `api.dreamlab-ai.com`, `pods.dreamlab-ai.com`, `search.dreamlab-ai.com` — kept
- Existing wrangler routes, bindings, secrets — kept (re-bound by `forum-config/` package)

No data migration. No bucket rename. No schema migration during cutover.

### G3 — Branded experience preserved
- Theme: amber palette, dark mode default, Inter font
- Logos, footer copy, hero text, page metadata — all preserved
- Cohort names `lobby`/`members`/`trusted` (DreamLab-specific) supplied via `dreamlab.toml [[zones]]` overrides on top of kit's default `public`/`members`/`private` 3-zone naming
- DreamLab admin pubkeys preserved exactly
- Welcome bot pubkey preserved exactly
- All page content (workshop pages, team profiles, custom routes) untouched (these live OUTSIDE community-forum-rs/ in the website's React app — unaffected by this transition)

### G4 — `forum-config/` is the SOLE Rust artefact in the website repo
After cutover, the website's Rust footprint shrinks from 8 forum crates + 1 CF Worker compilation pipeline to **a single `forum-config/` package** that depends on kit crates as Cargo deps. The website's React/TypeScript surface (the actual website) is untouched.

### G5 — TOML-driven configuration as the operator interface
Every DreamLab-specific value lives in `forum-config/dreamlab.toml`. Operator changes — new admin keys, branding tweaks, new zones, threshold adjustments — are TOML edits + redeploy, not code changes. The kit's `nostr-bbs-config` validator gates every change.

### G6 — `community-forum-rs/` subtree deleted
After ADR-083 cutover settles (T₇ + 7 days), the legacy forum subtree is git-rm'd. `forum-config/` becomes the only forum-related path in the website repo. PR-3 (per ADR-083 D12) executes the deletion.

### G7 — CI/CD pipeline parity
The existing GitHub Actions workflows (`deploy.yml`, `workers-deploy.yml`, `rust-ci.yml`) are adapted (not replaced) to:
- Build `forum-config/` instead of `community-forum-rs/crates/*`
- Deploy via the same wrangler steps to the same CF resources
- Keep all secrets, all environments, all branch protections

### G8 — Rollback safety until T₆+7
ADR-083's dual-deploy + traffic-split mechanism stays in place until 7 days after T₆ (full cutover). At any moment in that window, `wrangler secret put ROUTING_MODE old-only` reverts to the legacy stack. The website team retains both deployment paths active.

### G9 — Operator-friendly authoring
The forum-setup skill (per ADR-079) can produce `dreamlab.toml` automatically by interviewing an operator. For this transition specifically, the website team will:
- Hand-author the initial `dreamlab.toml v1` in PR (transparency + reviewability)
- Use the skill in **regenerate mode** (per ADR-079 D8 replay) for future updates if desired

### G10 — Documentation
Each transition phase has an explicit runbook at `docs/operations/runbooks/<phase>.md` (in the website repo). Per ADR-083 D11 pre-flight checklist + per the cutover-log discipline established in the website repo's existing `sprint-vN-complete` memory entries.

---

## 3. Non-Goals

- **NG1**: New forum capabilities (NIPs, mod kinds, etc.). Capability changes happen upstream in the kit, not in this transition.
- **NG2**: Cloud provider migration. Cloudflare → AWS / GCP / Vercel is out-of-scope. Same provider, same resources.
- **NG3**: Domain renaming or DNS changes.
- **NG4**: Data export / import. No migration of D1 rows, R2 objects, or KV entries — both old and new stacks share storage during cutover (per ADR-083 D3).
- **NG5**: Schema evolution. Kit v3.0 must preserve existing schemas exactly (per ADR-083 D4 invariant). Kit v3.1+ schema additions land in a future sprint.
- **NG6**: User communication / marketing campaign. The transition is transparent (per ADR-083 D10).
- **NG7**: Re-architecting the website's React surface. The community forum URL `/community/` still mounts the kit's WASM forum-client. The rest of the React app is untouched.
- **NG8**: Multi-tenant kit hosting. The website runs ONE forum instance (the DreamLab Community Forum). Multi-tenancy is a future kit capability, out-of-scope for the website's transition.

---

## 4. Current State

### 4.1 Pre-transition repository structure

```
dreamlab-ai-website/
├── (React app: src/, public/, vite.config.ts, etc.)
├── community-forum-rs/                ← TO BE DELETED at T₇
│   ├── Cargo.toml (workspace)
│   ├── crates/
│   │   ├── auth-worker/
│   │   ├── forum-client/
│   │   ├── nostr-core/
│   │   ├── pod-worker/
│   │   ├── preview-worker/
│   │   ├── relay-worker/
│   │   ├── search-worker/
│   │   └── admin-cli/
│   └── scripts/
└── (other website dirs)
```

The `community-forum-rs/` subtree carries Sprint v9-v11 production-grade work (per `02-forum-surfaces.md`): NIP-98 replay store, WAC Control coercion, SSRF redirect-bypass fix, KV namespace splits, profiles backfill, NIP-26 delegation verifier endpoint, kind-30033 mesh service-list scaffolding, Tailwind CDN replacement, etc.

### 4.2 Pre-transition cloud stack

5 deployed Cloudflare Workers (per `02-forum-surfaces.md` §9):

| Worker | Domain | D1 | KV | R2 | DO |
|--------|--------|----|----|----|-----|
| auth-worker | `api.dreamlab-ai.com` | `dreamlab-auth` | SESSIONS, POD_META, ADMIN_KV, NIP98_REPLAY | `dreamlab-pods` | — |
| pod-worker | `pods.dreamlab-ai.com` | — | POD_META, ADMIN_KV_RO, NIP98_REPLAY | `dreamlab-pods` | — |
| relay-worker | `dreamlab-nostr-relay.solitary-paper-764d.workers.dev` | `dreamlab-relay` | NIP98_REPLAY | — | `RELAY` (NostrRelayDO) |
| search-worker | `search.dreamlab-ai.com` | — | SEARCH_CONFIG, NIP98_REPLAY | `dreamlab-vectors` | — |
| preview-worker | `dreamlab-link-preview.solitary-paper-764d.workers.dev` | — | RATE_LIMIT | — | — |

CORS allowed origins (drift documented in `02-forum-surfaces.md` §10 #10): some workers single-origin (`https://dreamlab-ai.com`), some multi-origin. Preserved post-transition.

GitHub Actions:
- `deploy.yml` — Pages deploy (React + forum-client WASM)
- `workers-deploy.yml` — 5 worker deploys
- `rust-ci.yml` — fmt + clippy + test for `community-forum-rs/`

### 4.3 Post-transition repository structure (target)

```
dreamlab-ai-website/
├── (React app: src/, public/, vite.config.ts, etc. — UNCHANGED)
├── forum-config/                      ← NEW; the SOLE Rust footprint
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs                    ← thin shim that boots kit-supplied workers
│   │   └── dreamlab_branding.rs       ← branding extension hooks
│   ├── dreamlab.toml                  ← THE configuration; supplied to all workers
│   └── deploy/
│       ├── auth-worker.wrangler.toml
│       ├── pod-worker.wrangler.toml
│       ├── relay-worker.wrangler.toml
│       ├── search-worker.wrangler.toml
│       └── preview-worker.wrangler.toml
└── (other website dirs)
```

The `forum-config/` package depends on kit crates:

```toml
# forum-config/Cargo.toml
[package]
name = "dreamlab-forum-config"
version = "0.1.0"
edition = "2021"

[dependencies]
nostr-bbs-core         = { version = "3.0", default-features = false }
nostr-bbs-config       = { version = "3.0" }
nostr-bbs-mesh         = { version = "3.0" }
nostr-bbs-relay-worker = { version = "3.0", default-features = false, features = ["cdylib"] }
nostr-bbs-pod-worker   = { version = "3.0", default-features = false, features = ["cdylib"] }
nostr-bbs-auth-worker  = { version = "3.0", default-features = false, features = ["cdylib"] }
nostr-bbs-search-worker  = { version = "3.0" }
nostr-bbs-preview-worker = { version = "3.0" }
nostr-bbs-forum-client = { version = "3.0", default-features = false }
nostr-bbs-admin-cli    = { version = "3.0" }
```

---

## 5. Architecture

### 5.1 Consumer pattern (per ADR-080 D7 + ADR-085)

The kit is the upstream library; `forum-config/` is the downstream consumer that supplies branding + configuration + cloud-binding wiring.

```
┌──────────────────────────────────────────────────────────┐
│ DreamLab-AI/nostr-rust-forum (upstream)                   │
│  - nostr-bbs-* crates published to crates.io              │
│  - federation-native (per ADR-073)                        │
│  - configurable via TOML (per PRD-011 §5.2)              │
└──────────────────────┬────────────────────────────────────┘
                       │ Cargo dep
                       ▼
┌──────────────────────────────────────────────────────────┐
│ DreamLab-AI/dreamlab-ai-website                            │
│ ├── forum-config/                                          │
│ │   ├── dreamlab.toml          ← branding/zones/admin keys│
│ │   ├── src/dreamlab_branding.rs ← logo/copy/theme slots  │
│ │   └── deploy/*.wrangler.toml  ← CF resource bindings    │
│ └── (React website surface, untouched)                     │
└──────────────────────┬────────────────────────────────────┘
                       │ wrangler deploy
                       ▼
┌──────────────────────────────────────────────────────────┐
│ Cloudflare Workers (existing infra; identifiers preserved)│
│  - dreamlab-relay D1                                      │
│  - dreamlab-auth D1                                       │
│  - dreamlab-pods R2                                       │
│  - dreamlab-vectors R2                                    │
│  - SESSIONS / POD_META / ADMIN_KV / etc. KV               │
│  - NostrRelayDO class                                     │
│  - api.dreamlab-ai.com / pods.dreamlab-ai.com / etc.      │
└──────────────────────────────────────────────────────────┘
```

### 5.2 `dreamlab.toml` shape

The complete instance configuration. Authored and committed at PR-time (transparency); validates against `nostr-bbs-config` schema. Per PRD-011 §5.2 + ADR-085 D5:

```toml
# forum-config/dreamlab.toml — THE DreamLab forum configuration

[deployment]
name           = "DreamLab Community Forum"
hostname       = "https://dreamlab-ai.com"
contact        = "ops@dreamlab-ai.com"
license_text   = "© 2026 DreamLab AI Ltd."

[webauthn]
rp_id          = "dreamlab-ai.com"
expected_origin = "https://dreamlab-ai.com"

[pod]
base_url       = "https://pods.dreamlab-ai.com"
storage_backend = "cf-r2"
r2_bucket      = "dreamlab-pods"

[relay]
url            = "wss://dreamlab-nostr-relay.solitary-paper-764d.workers.dev"
ingress_policy = "allowlist"

[admin]
mode           = "static"
static_pubkeys = [
  "<dreamlab-admin-pubkey-1>",
  "<dreamlab-admin-pubkey-2>",
  # ... preserved from current production
]

[branding]
theme_colour   = "#FFC857"  # DreamLab amber-400
logo_url       = "https://dreamlab-ai.com/static/dreamlab-logo.svg"
hero_text      = "DreamLab Community Forum"
footer_text    = "Powered by DreamLab AI"
font_primary   = "Inter"

[[zones]]
id             = "lobby"
display_name   = "Lobby"
read_access    = "anyone"
write_access   = "tl0"

[[zones]]
id             = "members"
display_name   = "Members"
read_access    = "tl1"
write_access   = "tl2"

[[zones]]
id             = "trusted"
display_name   = "Trusted"
read_access    = "tl2"
write_access   = "tl3"

[trust]
tl0_to_tl1_threshold = { posts = 1, days = 0 }
tl1_to_tl2_threshold = { posts = 5, days = 7 }
tl2_to_tl3_threshold = { admin_invitation = true }

[invites]
enabled              = true
referente_kind3_required = true
welcome_bot_pubkey   = "<dreamlab-welcome-bot-pubkey>"
default_credit       = 5

[moderation]
kinds_range          = [30910, 30911, 30912, 30913, 30914, 30915, 30916]
nip56_kind_1984      = true

[mesh]
mode                 = "federated"  # DreamLab joins the mesh
peer_relays          = [
  # other DreamLab-side mesh participants
]
federated_kinds      = [14, 1059, 30033, 30910, 30911, 30912, 30913, 30914, 30915, 30916]
allowed_remote_dids  = [
  # operator-managed list of trusted federation-key DIDs
]
honor_remote_moderation = []
delegation_required  = true

[ratelimit]
relay_events_per_sec = 10
auth_per_60s         = 20
preview_per_60s      = 30
search_per_60s       = 100

[features]
dvm_marketplace      = false
calendar_events      = true
deletion_kind5       = true

[custody]
operator         = "tier-2"   # CF Workers Secrets
federation       = "tier-2"
welcome_bot      = "tier-1"

[custody.tier-2]
provider         = "cf-workers-secret"
secret_name      = "dreamlab-mesh/{role}/v{version}"
```

The current existing branding extension assets (logos at `dreamlab-ai.com/static/...`) live in the existing React `public/` directory, untouched. The TOML simply references the URLs.

### 5.3 Cloud infrastructure mapping (per ADR-084)

Every existing resource is preserved with **no rename** and **no migration**. Wrangler bindings move from `community-forum-rs/crates/*/wrangler.toml` (per-crate) to `forum-config/deploy/*.wrangler.toml` (per-worker):

```toml
# forum-config/deploy/auth-worker.wrangler.toml (post-transition)
name           = "dreamlab-auth-api"
main           = "../../target/wasm32-unknown-unknown/release/nostr_bbs_auth_worker.wasm"
compatibility_date = "2025-09-01"

[vars]
EXPECTED_ORIGIN = "https://dreamlab-ai.com"
RP_ID          = "dreamlab-ai.com"
DREAMLAB_TOML_PATH = "../dreamlab.toml"  # consumed at boot

[[d1_databases]]
binding        = "DB"
database_name  = "dreamlab-auth"
database_id    = "<existing-id>"            # IDENTITY PRESERVED

[[kv_namespaces]]
binding        = "SESSIONS"
id             = "<existing-id>"            # IDENTITY PRESERVED

[[kv_namespaces]]
binding        = "ADMIN_KV"
id             = "<existing-id>"            # IDENTITY PRESERVED

[[kv_namespaces]]
binding        = "NIP98_REPLAY"
id             = "<existing-id>"            # IDENTITY PRESERVED

[[r2_buckets]]
binding        = "PODS"
bucket_name    = "dreamlab-pods"            # IDENTITY PRESERVED

# Same routes:
[[routes]]
pattern        = "api.dreamlab-ai.com/*"
zone_name      = "dreamlab-ai.com"
```

ADR-084 specifies the complete mapping per worker.

### 5.4 Operator journey

The operator (DreamLab ops team) flow:

1. **Pre-T₀**: Read this PRD + PRD-011 + ADR-083. Confirm understanding.
2. **T₀ pre-flight**: Confirm kit v3.0.0 GA tagged on `nostr-rust-forum`. Confirm L2 contract tests passing for ≥7 nights.
3. **T₁ staging**: PR-1 in `dreamlab-ai-website` adds `forum-config/` package + `dreamlab.toml` v1; deploys to staging. Smoke tests against existing staging resources.
4. **T₂ parity**: Run cutover/parity-check.sh between staging-old (community-forum-rs subtree) and staging-new (forum-config) for 48h. ≤1% deviation rate required.
5. **T₃-T₆**: Production cutover per ADR-083 D2 traffic split.
6. **T₇**: Delete `community-forum-rs/` subtree (PR-3 per ADR-083 D12).
7. **T₈**: Post-cutover audit.

The website team's specific deliverables per phase are in §7 phasing.

---

## 6. Functional Requirements

### F1 — `forum-config/` Cargo package
NEW package at `dreamlab-ai-website/forum-config/` with:
- `Cargo.toml` declaring deps on kit crates (per §4.3)
- `src/main.rs` thin shim binding kit workers to dreamlab.toml at boot
- `src/dreamlab_branding.rs` extension hooks invoking kit's branding slots
- `dreamlab.toml` configuration file
- `deploy/*.wrangler.toml` per-worker deployment manifests (5 files)
- `README.md` operator-facing documentation

### F2 — `dreamlab.toml` v1 authored
Hand-authored from current production values:
- All admin pubkeys preserved exactly (verified against existing `community-forum-rs` admin-cli configs)
- Zone names preserve `lobby`/`members`/`trusted` cohort taxonomy
- Trust thresholds match current production (research from existing whitelist promotion logic)
- Welcome bot pubkey preserved
- Branding values match current Tailwind theme + Inter font + DreamLab amber palette
- Hostname / WebAuthn rp_id / expected_origin all preserved

### F3 — TOML schema validator integrated
Pre-deploy CI step runs `nostr-bbs-config validate forum-config/dreamlab.toml`. Failures block deploy. Per ADR-082 + PRD-011 F3.

### F4 — Cloud resource ID preservation
Every resource binding in `forum-config/deploy/*.wrangler.toml` uses the existing production `database_id` / KV `id` / R2 `bucket_name` from the current production `community-forum-rs/crates/*/wrangler.toml` files. **No new resources provisioned.** Tracked at ADR-084 D2.

### F5 — Branding extension hooks
The kit's branding extension points (per ADR-085 D4) are populated by `forum-config/src/dreamlab_branding.rs`:
- Logo URL slot → existing `https://dreamlab-ai.com/static/dreamlab-logo.svg`
- Theme tokens slot → DreamLab amber palette (50-950)
- Footer copy slot → "Powered by DreamLab AI"
- Hero copy slot → "DreamLab Community Forum"
- Custom CSS hook → existing `community-forum-rs/crates/forum-client/index.html` `<style>` block migrated

### F6 — GitHub Actions adapted
Existing workflows updated:
- `rust-ci.yml`: build target changes from `community-forum-rs/Cargo.toml` to `forum-config/Cargo.toml`. Same tests, same fmt/clippy gates.
- `workers-deploy.yml`: `wrangler deploy` invocations point at `forum-config/deploy/*.wrangler.toml`. Same secrets, same environments.
- `deploy.yml` (Pages): forum-client WASM bundle build moves from `community-forum-rs/crates/forum-client/` to consuming kit's `nostr-bbs-forum-client` crate via `forum-config/`. Trunk build paths updated.

### F7 — Anti-drift CI gate
Per ADR-077 P3, anti-drift lint runs on `forum-config/`:
- Reject `format!("urn:visionclaw:..")` style ad-hoc URN minting
- Reject hardcoded URLs other than what's in `dreamlab.toml`
- Reject DreamLab-specific strings outside `dreamlab_branding.rs` and `dreamlab.toml`

### F8 — Sprint Carry-Over Fixture Suite executed
Per PRD-011 G6, `nostr-rust-forum/tests/cutover/sprint-carry-over.rs` runs against `forum-config/` deployment in staging. All 12 capabilities (NIP-98 replay, WAC Control coercion, SSRF redirect-bypass, KV split, profile backfill, username reservation, NIP-26 delegation verifier, mesh service-list scaffolding, Signer-trait NIP-98, Tailwind build-time, sw.js, profiles batch+search) MUST pass before T₃.

### F9 — Parity test harness
Per ADR-083 D6 + new `tests/cutover/api-parity.sh`:
- 5-minute cadence in T₃-T₆ window
- ≥30 sample pubkeys × ≥10 endpoints
- Divergence threshold ≤1% per phase before stepping up

### F10 — Operator runbook published
`docs/operations/runbooks/dreamlab-cutover-T0-to-T8.md` with:
- Pre-flight checklist (lifted from ADR-083 D11)
- Per-phase checklist
- Rollback paths (lifted from ADR-083 D9)
- Stakeholder communication template (lifted from ADR-083)
- On-call scheduling

### F11 — `community-forum-rs/` deletion PR
At T₇+7, PR-3 deletes the subtree. PR description references this PRD + ADR-083 D12. Includes:
- Confirmation that no symlinks/scripts/external refs remain
- Confirmation that `forum-config/` covers all the deleted functionality

### F12 — Memory continuity
Sprint memory entries (`sprint-vN-*` keys in `project-state` namespace) referencing `community-forum-rs/` paths are flagged for review post-cutover. Selected entries get `community-forum-rs/` → `<consumed-via-forum-config>` annotations.

### F13 — Documentation updates
- `dreamlab-ai-website/README.md` — add "Forum Configuration" section pointing at `forum-config/`
- `dreamlab-ai-website/CLAUDE.md` — update "Tech Stack" table; remove Leptos forum entry from "this repo's responsibilities"; add cross-reference to nostr-rust-forum
- `dreamlab-ai-website/community-forum-rs/CLAUDE.md` — annotated as historical at T₇

### F14 — Forum-setup skill regenerate-mode validation
Per ADR-079 D8 + G9 above: confirm that running `nostr-bbs-setup-skill replay <run-id>` produces a `dreamlab.toml` byte-equal to the hand-authored v1 (or document divergences as acceptable). Provides a CI replay-ability guarantee for future regenerations.

### F15 — Federation key custody decision
Per ADR-081 D2 + `dreamlab.toml [custody]` section: DreamLab's choice is Tier-2 CF Workers Secrets (not Tier-1 filesystem because CF Workers don't have persistent filesystem; not Tier-3 HSM because cost/operational overhead doesn't fit DreamLab's posture). Decision committed in dreamlab.toml.

### F16 — Mesh federation enabled
Per `dreamlab.toml [mesh]` section: `mode = "federated"` (per G6 from PRD-010). DreamLab is one of the founding mesh participants. `peer_relays` list populated with other operator endpoints as the mesh comes online.

### F17 — Kit version pinning
`forum-config/Cargo.toml` pins `nostr-bbs-* = "3.0"` (caret range). Operator-controlled cargo update cadence: monthly review of upstream patch releases; full version bump (3.0 → 3.1) requires explicit PR + parity test re-run.

### F18 — Secrets handling
CF Workers Secrets:
- All existing secrets (`MANAGEMENT_API_KEY`, `RP_ID`, etc.) preserved.
- New federation secrets (`MESH_FEDERATION_PRIVKEY`, `WELCOME_BOT_PRIVKEY`) populated via `wrangler secret put` per ADR-081 D2 Tier-2 custody.
- Operator pubkey (admin static identity) NOT a secret — published in `dreamlab.toml [admin].static_pubkeys`.

### F19 — DNS untouched
Existing zones, A/AAAA records, MX, custom CF Worker routes — all preserved. `dreamlab-ai.com`, `api.dreamlab-ai.com`, `pods.dreamlab-ai.com`, `search.dreamlab-ai.com` resolve to the same routes; only the binding-side wrangler manifest changes.

### F20 — Custom domain re-binding
`forum-config/deploy/*.wrangler.toml` declares the same `[[routes]]` blocks. Wrangler picks them up; CF moves the binding to the new worker name automatically (CF supports zero-downtime route migration).

### F21 — Service worker (sw.js) preserved
The forum-client's service worker shipped in Sprint v11 (per memory key `sprint-v11-stream-f1-status`). Kit v3.0 must include it; `forum-config/` confirms via parity check.

### F22 — Forum-client public URL preserved
`https://dreamlab-ai.com/community/` continues to be the forum URL. Trunk build target adapts; URL stays.

### F23 — Operator-pubkey-as-admin preserved
Per memory key `sprint-v9-wave1-complete` (admin auto-bootstrap on first kind-0): kit v3.0's `[admin] mode = "static"` honors the pre-existing admin pubkeys (no first-user-is-admin override that would steal admin status from the operator).

### F24 — Mod history preserved
Existing kind-30910 (ban) / 30911 (mute) / 30914 (mod-action) events in the relay's D1 events table remain valid. Kit v3.0's relay-worker reads them. ADR-083 D4 schema parity invariant covers this.

### F25 — Username reservations preserved
Per Sprint v10: existing `username_reservations` D1 table content preserved. Kit v3.0's auth-worker reads it. Schema parity per ADR-083 D4.

### F26 — Profile backfill state preserved
Per Sprint v11 F4a: existing `profiles` D1 table content preserved. Kit v3.0's relay-worker reads it.

### F27 — Cron triggers preserved
Existing `*/5 * * * *` keep-warm cron on relay-worker — preserved. Wrangler manifest carries `[triggers] crons = ["*/5 * * * *"]`.

### F28 — Branch protection rules preserved
DreamLab-AI/dreamlab-ai-website main branch protections — preserved. PR-1 (forum-config addition) goes through the same review flow.

### F29 — Backup discipline
Pre-T₃ snapshot all D1 / KV / R2 to a backup bucket. Post-T₈ audit confirms: snapshot integrity + no data loss vs snapshot. Standard CF backup tooling.

### F30 — Cost monitoring
CF dashboard alerts during T₃-T₆ dual-deploy window: cost spike >2× baseline triggers ops investigation. Document expected ~2× cost during dual-deploy per ADR-083 §Negative.

---

## 7. Phasing

### Phase X0 — Pre-flight (~3 days)
**Owner**: DreamLab ops team (this PRD's primary subject)
- Confirm kit v3.0.0 GA tagged on `nostr-rust-forum`
- Read PRD-012 + PRD-011 + ADR-083 + ADR-084 + ADR-085 (this stack)
- Run `nostr-bbs-config validate` on a draft `dreamlab.toml v0`
- Schedule on-call coverage for T₃-T₆ window
- Brief stakeholders (DreamLab leadership, forum admins)

### Phase X1 — `forum-config/` package authored (~1 sprint)
**Owner**: website team Rust engineer
- Create `forum-config/` directory + Cargo.toml + deps
- `src/main.rs` thin shim
- `src/dreamlab_branding.rs` extension hooks
- `dreamlab.toml v1` hand-authored from current production values
- `deploy/*.wrangler.toml` for 5 workers with preserved IDs
- Anti-drift lint passes
- Local cargo build clean

PR-1 lands at end of phase. Branch `mega-sprint/2026-05-08-forum-config`.

### Phase X2 — Staging parity (~1 sprint)
**Owner**: website ops + Rust engineer
- Deploy `forum-config/` to staging via existing GHA `workers-deploy.yml`
- Sprint Carry-Over Fixture Suite runs in staging — all 12 capabilities PASS
- `tests/cutover/api-parity.sh` runs in staging (old vs new) for 48h continuous; ≤1% deviation
- Schema parity verified (D1 / KV / R2 IDs match production exactly)
- Forum-setup skill replay produces byte-equal TOML (F14)
- L2 contract tests passing for ≥7 nights

Gate: T₃ traffic split cannot proceed until X2 sign-off.

### Phase X3 — Production cutover (~14 days, per ADR-083 D1 timeline)
**Owner**: website ops
- Per ADR-083 D2: ROUTING_MODE transitions canary → 50 → 95 → only over 14 days
- Parity monitoring 5-min cadence
- Rollback ready at every phase
- Pre-flight checklist (per ADR-083 D11) executed at T₃

### Phase X4 — `community-forum-rs/` deletion (~1 sprint window)
**Owner**: website Rust engineer
- T₆+7 days: PR-3 deletes `community-forum-rs/` subtree
- Documentation updates (F13)
- Memory entries flagged for review (F12)
- forum-config/Cargo.toml verified as the only Rust footprint

### Phase X5 — Post-cutover audit (~1 sprint)
**Owner**: website ops
- All ADR-083 D9 rollback triggers confirmed silent for 30 days
- Monthly cost report shows return to single-stack baseline
- User-reported regression count: 0 critical, ≤5 minor
- Closeout entry in `docs/operations/cutover-log.md`

**Total elapsed time**: ~45-55 days from X0 start to X5 completion. Estimate matches ADR-083 timeline.

---

## 8. Risks

### R1 — Subtle DreamLab-specific behaviour hidden in community-forum-rs (HIGH)
A custom mod kind, an edge cohort logic, a specific NIP-XX use that lives in code but not in `dreamlab.toml`. Cutover discovers it as a parity divergence.

*Mitigation*: ADR-083 D11 pre-flight + F8 Sprint Carry-Over Fixture Suite + F9 parity test harness. Worst case: rollback during cutover, fix the missing TOML override in kit, retry.

### R2 — Kit GA slips (HIGH)
PRD-011 Phase X6 GA gate failures push back this PRD's X1 start.

*Mitigation*: Track upstream kit progress weekly. If kit GA slips >2 weeks, evaluate whether the website can ship a `forum-config/` against kit v3.0.0-rc3 with explicit "RC release" caveat.

### R3 — wrangler.toml binding migration breaks resource access (HIGH)
A typo in `forum-config/deploy/*.wrangler.toml` resource IDs causes a worker to bind to an empty/wrong KV namespace at runtime — cutover would silently corrupt user data.

*Mitigation*: pre-deploy validation step compares each resource ID in `forum-config/deploy/` against the live `wrangler kv namespace list` / `wrangler d1 list` output. Mismatch fails CI. Plus integration test that writes a known sentinel value to each resource and reads it back via the new worker stack pre-cutover.

### R4 — Branding extension slot mismatch (MEDIUM)
Kit v3.0 may not expose every extension hook DreamLab's current customisation needs. E.g. a custom rate-limit override that was hard-coded in `community-forum-rs/crates/relay-worker/`.

*Mitigation*: F8 Sprint Carry-Over Fixture Suite catches behaviour regressions. If something is missing, escalate to kit upstream as a v3.1 feature request. Defer the dependent customisation if non-critical.

### R5 — TOML schema evolution between authoring and GA (MEDIUM)
Schema version drift between when DreamLab authors `dreamlab.toml v1` and when kit v3.0 GA ships.

*Mitigation*: F3 schema validator pinned to kit v3.0.0 exact version. Schema migration tooling per ADR-082 D2 covers minor changes.

### R6 — Cost spike during dual-deploy beyond expected 2× (MEDIUM)
Both stacks running simultaneously cost more than projected.

*Mitigation*: F30 cost monitoring + alert at 2.5× baseline; ops investigation; option to compress T₃-T₆ duration if cost is unsustainable.

### R7 — Operator authoring `dreamlab.toml v1` misses values (MEDIUM)
Hand-authored TOML omits a setting that's currently a DreamLab-specific override; cutover regression.

*Mitigation*: F14 forum-setup skill replay-mode generates an alternate version; diff against hand-authored; resolve discrepancies.

### R8 — `community-forum-rs/` deletion breaks unrelated build artefacts (LOW)
Other parts of the website repo unexpectedly depend on `community-forum-rs/` paths.

*Mitigation*: pre-deletion `grep -r 'community-forum-rs' .` audit; T₇+7 buffer means deletion is well after stable production cutover.

### R9 — sw.js / forum-client URL changes (LOW)
The kit's forum-client may render at a different sub-path; users with bookmarks lose them.

*Mitigation*: F22 explicitly preserves `/community/` URL. Kit's Trunk build supports `--public-url /community/` per existing pattern.

### R10 — Documentation drift across Sprint vN memory entries (LOW)
Many memory keys reference `community-forum-rs/` paths; finding/updating them post-deletion is tedious.

*Mitigation*: F12 selective annotation, not bulk rewrite. Old entries remain searchable as historical context.

---

## 9. Open Questions

### Q1 — Cargo workspace shape: monorepo or path-dep?
Should `forum-config/` be a sibling Cargo workspace inside the website monorepo (independent `Cargo.toml`), or a path dep from a root workspace? **Recommendation**: independent workspace; simpler isolation; matches `community-forum-rs/` precedent. Decision deferred to ADR-085 D1.

### Q2 — `dreamlab.toml` schema-version pinning policy
Pin to kit `3.0.x` semver caret, or to exact 3.0.0? **Recommendation**: `^3.0` (caret), with monthly review of patch releases. Major bump (3 → 4) requires explicit operator decision.

### Q3 — Branding asset bundling
Logos and theme tokens in `dreamlab.toml` reference URLs. Should they be bundled in `forum-config/assets/` and served from CF Pages, or referenced from existing `dreamlab-ai.com/static/...`? **Recommendation**: continue using existing public/static URLs; no asset migration. Reduces risk surface.

### Q4 — Mesh peer roster bootstrapping
At launch, what goes in `dreamlab.toml [mesh].peer_relays`? Initially empty? With agentbox + VisionClaw substrate URLs once they go federated? **Recommendation**: empty initially; populated in a follow-up PR once peer relays are operational.

### Q5 — Federation key generation timing
Does the federation key get generated pre-cutover (as part of X1 forum-config authoring) or post-cutover (as part of mesh activation)? **Recommendation**: pre-cutover; stored in CF Workers Secret immediately; advertised in kind-30033 only when `mesh.mode = "federated"` (see Q4).

### Q6 — sprint-v9 admin-cli: kit-side or website-side?
admin-cli was a DreamLab-specific tool in the legacy `community-forum-rs/`; the kit's `nostr-bbs-admin-cli` aims to replace it. Does DreamLab continue using its custom admin-cli (forked into `forum-config/`) or migrate to kit's? **Recommendation**: migrate to kit's; bring DreamLab-specific commands as kit-upstream PRs if needed.

### Q7 — dreamlab-ai-website CLAUDE.md ownership post-transition
Currently CLAUDE.md describes the forum extensively. Post-transition, the forum is not "in" this repo. Should the website's CLAUDE.md drop forum sections entirely, or keep a "consumes nostr-bbs-rs at version X" pointer? **Recommendation**: keep a brief "Forum Configuration" section pointing at `forum-config/dreamlab.toml` and the kit's GitHub URL; drop deep forum-internal documentation.

### Q8 — Forum-client WASM bundle build pipeline
Currently Trunk builds inside `community-forum-rs/crates/forum-client/`. Post-transition, kit publishes `nostr-bbs-forum-client` and `forum-config/` consumes it. Where does the Trunk build run — kit-upstream or `forum-config/`? **Recommendation**: kit publishes a build artefact (or trunk-buildable crate); `forum-config/` runs Trunk against the kit crate. Detailed in ADR-085 D7.

---

## 10. Success Metrics

### M1 — Behavioural parity
Sprint Carry-Over Fixture Suite (PRD-011 G6) passes 100% on `forum-config/` deployment. Verified during X2.

### M2 — Zero-downtime cutover
Production downtime during T₃ → T₆ ≤ 60 seconds (only the brief moment of `wrangler secret put` propagation).

### M3 — Zero data loss
Pre-T₃ snapshot equality with post-T₈ snapshot for all D1 / KV / R2 resources (modulo expected normal user activity).

### M4 — Zero session loss
`session_drop_total{stack="new"} / session_total{stack="new"}` ≤ 0.1% in any 1-min window during T₃-T₆.

### M5 — User-visible regression rate ≤1%
ADR-083 D6 deviation report < 1% per phase before stepping up. Final report at T₈ < 0.1%.

### M6 — Cost stability
Post-T₈ monthly CF cost ≤ pre-T₀ baseline + 5% (no compounding cost from kit's federation features when mesh is enabled).

### M7 — `community-forum-rs/` LOC delta
Pre-T₆: ~30,000 LOC in `community-forum-rs/` subtree. Post-T₇+7: 0 LOC. `forum-config/` LOC ≤ 1,500 (just config + branding shim).

### M8 — Operator self-sufficiency
Operator can author a new `dreamlab.toml` change (e.g. add a new admin pubkey) + redeploy in ≤10 minutes. Verified via runbook execution.

---

## 11. Affected Files

### `dreamlab-ai-website/` (primary subject)

NEW:
- `forum-config/Cargo.toml`
- `forum-config/src/main.rs`
- `forum-config/src/dreamlab_branding.rs`
- `forum-config/dreamlab.toml`
- `forum-config/deploy/{auth,pod,relay,search,preview}-worker.wrangler.toml`
- `forum-config/README.md`
- `docs/operations/runbooks/dreamlab-cutover-T0-to-T8.md`

MODIFIED:
- `.github/workflows/rust-ci.yml` — build target `forum-config/Cargo.toml`
- `.github/workflows/workers-deploy.yml` — wrangler manifest paths
- `.github/workflows/deploy.yml` — Trunk build path for forum-client
- `README.md` — Forum Configuration section
- `CLAUDE.md` — Tech Stack table

DELETED at T₇+7:
- `community-forum-rs/` (entire subtree, ~8 crates + scripts + workflows)

### `nostr-rust-forum/` (kit upstream)
- No changes per this PRD. Kit v3.0 GA is a hard prerequisite (R2).

### Other repos
- VisionClaw monorepo: no changes per this PRD (already has the cross-substrate fixtures + ADR set)
- agentbox: no changes per this PRD
- solid-pod-rs: no changes per this PRD

---

## 12. References

- PRD-010 — DID:Nostr Mesh Federation (mesh participation)
- PRD-011 — VisionFlow Forum Kit Extraction (kit upstream)
- ADR-073 — Mesh topology (DreamLab participates as a federation peer)
- ADR-074 — DID:Nostr canonicalisation (DreamLab DID Document conformance)
- ADR-075 — IS-Envelope (DreamLab consumes kit's envelope)
- ADR-076 — Forum nostr-core absorption (kit ships absorbed; DreamLab inherits)
- ADR-077 — Ecosystem QE Policy (DreamLab CI complies)
- ADR-078 — Cross-substrate library convergence (DreamLab consumes upstream libs via kit)
- ADR-079 — Forum-Setup Skill Provider Abstraction (DreamLab uses skill in regenerate mode for `dreamlab.toml`)
- ADR-080 — Forum Kit Deployment Topology Patterns (DreamLab is the D7 downstream-consumer exemplar)
- ADR-081 — Federation key custody & rotation (DreamLab uses Tier-2 CF Workers Secrets)
- ADR-082 — Cross-substrate test fixture sharing (DreamLab consumes)
- ADR-083 — Cutover migration pattern (the tactical mechanics for X3)
- ADR-084 (companion, NEW) — Cloud Infrastructure Mapping for Kit Consumers
- ADR-085 (companion, NEW) — `forum-config/` Package Architecture & Branding Extension Points
- `docs/ddd-mesh-federation-context.md` — BC-MESH-DREAMLAB-CONSUMER (V6 + V13)
- `docs/integration-research/02-forum-surfaces.md` — current forum infrastructure details
- `docs/integration-research/qe-fleet/V1-V3-*.md` — quality validators
- GitHub repos:
  - https://github.com/DreamLab-AI/dreamlab-ai-website (primary subject)
  - https://github.com/DreamLab-AI/nostr-rust-forum (kit upstream)
  - https://github.com/DreamLab-AI/VisionClaw (mesh integration)
