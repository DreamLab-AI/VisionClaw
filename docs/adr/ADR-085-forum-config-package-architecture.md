# ADR-085 — `forum-config/` Package Architecture & Branding Extension Points

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-012 G3, G4, G5, F1, F5; PRD-011 G3 (downstream consumer pattern) |
| Companion ADRs | ADR-073, ADR-080 D7, ADR-082, ADR-083, ADR-084 |
| Companion PRDs | PRD-011, PRD-012 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` BC-MESH-DREAMLAB-CONSUMER |
| Affected repos | `dreamlab-ai-website` (consumer), `nostr-rust-forum` (kit upstream — defines extension API) |

## Context

PRD-012 §5.1 specifies that DreamLab's website becomes a thin downstream consumer of the VisionFlow forum kit (`nostr-bbs-rs`). The mechanism is a new `forum-config/` Cargo package inside `dreamlab-ai-website` that:
- Depends on kit crates as Cargo deps
- Supplies `dreamlab.toml` instance configuration
- Wires DreamLab branding into the kit's extension hooks
- Re-exports kit workers for wrangler to deploy

The kit (PRD-011 §5.1) ships ~11 crates: `nostr-bbs-{core,config,mesh,forum-client,auth-worker,pod-worker,preview-worker,relay-worker,search-worker,admin-cli,setup-skill}`. The kit's design (per PRD-011 G4 + ADR-080 D7) treats the consumer as a configuration-only package — *no consumer code should reimplement protocol or business logic*. But there are legitimate consumer concerns:

- **Branding**: theme colours, logos, copy strings, hero text — these are deployment-specific
- **Custom React/HTML in forum-client**: DreamLab's existing forum-client has custom mounting code, sw.js registration, webgpu-particles.js etc. Kit's `nostr-bbs-forum-client` exposes mount points; consumer's branding shim populates them.
- **Build-time asset paths**: kit's Trunk build needs to know where the consumer's `dreamlab.toml`, custom CSS, custom assets live
- **Wrangler manifest generation**: each of the 5 worker bundles needs a per-deployment wrangler.toml with the resource IDs from ADR-084

This ADR specifies the architecture of `forum-config/` — what shape, what files, what extension API the kit exposes, what the consumer is and is NOT allowed to do.

## Decision

### D1 — Cargo workspace shape

`forum-config/` is its **own Cargo workspace** (not a path-dep within an outer workspace). Rationale:
- Isolates consumer build from the rest of `dreamlab-ai-website` (which is React/TypeScript, no Cargo workspace at root)
- Matches the precedent of `community-forum-rs/` (also its own workspace inside the website monorepo)
- Cargo dep graph stays small + auditable
- `cargo build` invocations from CI target only `forum-config/`

```
dreamlab-ai-website/
├── (React app — no workspace)
└── forum-config/                    ← OWN workspace
    ├── Cargo.toml                   ← workspace + members + deps
    ├── Cargo.lock
    ├── src/
    │   ├── main.rs
    │   ├── dreamlab_branding.rs
    │   └── dreamlab_workers.rs      ← per-worker entry shims
    ├── dreamlab.toml
    ├── deploy/
    │   ├── auth-worker.wrangler.toml
    │   ├── pod-worker.wrangler.toml
    │   ├── relay-worker.wrangler.toml
    │   ├── search-worker.wrangler.toml
    │   └── preview-worker.wrangler.toml
    ├── assets/                       ← optional; static assets bundled in worker WASM
    │   ├── dreamlab-logo.svg         ← (mirrored from website public/static)
    │   └── dreamlab-theme.css
    └── README.md
```

### D2 — Workspace `Cargo.toml`

```toml
# forum-config/Cargo.toml
[workspace]
members = []  # all members are external kit crates; this workspace is just a deps container

[package]
name    = "dreamlab-forum-config"
version = "0.1.0"
edition = "2021"
publish = false   # not published to crates.io

[lib]
crate-type = ["cdylib", "rlib"]   # cdylib for WASM workers; rlib for tests

[dependencies]
# Kit core + config
nostr-bbs-core           = { version = "3.0", default-features = false, features = ["wasm"] }
nostr-bbs-config         = { version = "3.0" }

# Kit workers (each as a separately-buildable cdylib target via feature flag)
nostr-bbs-auth-worker    = { version = "3.0", optional = true }
nostr-bbs-pod-worker     = { version = "3.0", optional = true }
nostr-bbs-relay-worker   = { version = "3.0", optional = true }
nostr-bbs-search-worker  = { version = "3.0", optional = true }
nostr-bbs-preview-worker = { version = "3.0", optional = true }

# Forum-client (Leptos WASM)
nostr-bbs-forum-client   = { version = "3.0", optional = true }

# Mesh federation
nostr-bbs-mesh           = { version = "3.0", optional = true }

# Admin CLI
nostr-bbs-admin-cli      = { version = "3.0", optional = true }

# Setup skill
nostr-bbs-setup-skill    = { version = "3.0", optional = true }

[features]
default          = []   # explicit per build target
auth-worker      = ["nostr-bbs-auth-worker", "nostr-bbs-mesh"]
pod-worker       = ["nostr-bbs-pod-worker", "nostr-bbs-mesh"]
relay-worker     = ["nostr-bbs-relay-worker", "nostr-bbs-mesh"]
search-worker    = ["nostr-bbs-search-worker"]
preview-worker   = ["nostr-bbs-preview-worker"]
forum-client     = ["nostr-bbs-forum-client"]
admin-cli        = ["nostr-bbs-admin-cli"]
```

Each worker is built with `cargo build --target wasm32-unknown-unknown --release --features <worker>` and wrangler deploys the resulting WASM blob.

### D3 — Source files

#### `src/main.rs` — empty stub

```rust
//! dreamlab-forum-config — DreamLab Community Forum kit consumer package.
//!
//! This package depends on `nostr-bbs-*` kit crates and supplies DreamLab-specific
//! configuration via `dreamlab.toml` + branding via `dreamlab_branding.rs`.
//!
//! Per ADR-085: this file is intentionally minimal — all heavy lifting lives in
//! the kit. This consumer package is configuration + branding + CF deployment
//! glue only.

#![cfg_attr(target_arch = "wasm32", no_main)]

mod dreamlab_branding;
mod dreamlab_workers;

#[cfg(target_arch = "wasm32")]
pub use dreamlab_workers::*;
```

#### `src/dreamlab_branding.rs` — branding extension hooks

```rust
//! DreamLab-specific branding overrides for the kit's extension points.
//!
//! Per ADR-085 D4: this module is the ONLY place in `forum-config/` allowed to
//! override kit-default behaviour. Anti-drift lint enforces this.

use nostr_bbs_core::branding::{BrandingConfig, ThemeColours, CopyStrings};

/// Build the DreamLab branding configuration consumed by kit components.
pub fn dreamlab_branding() -> BrandingConfig {
    BrandingConfig {
        theme: ThemeColours {
            primary:    "#FFC857",  // DreamLab amber-400
            secondary:  "#A77B00",  // DreamLab amber-700
            background: "#0F0F0F",  // DreamLab dark
            foreground: "#FFFFFF",
            font_primary:   "Inter",
            font_monospace: "JetBrains Mono",
        },
        copy: CopyStrings {
            site_name:    "DreamLab Community Forum",
            site_tagline: "Conversations at the boundary of human and AI",
            footer_text:  "© 2026 DreamLab AI Ltd.",
            hero_text:    "Welcome to the DreamLab Community Forum",
        },
        logo_url:        Some("https://dreamlab-ai.com/static/dreamlab-logo.svg".into()),
        favicon_url:     Some("https://dreamlab-ai.com/static/favicon.svg".into()),
        custom_css_url:  Some("/community/dreamlab-overrides.css".into()),
        og_image_url:    Some("https://dreamlab-ai.com/static/og.png".into()),
    }
}

/// Optional per-zone display name overrides (DreamLab uses cohort names).
pub fn dreamlab_zone_display() -> Vec<(&'static str, &'static str)> {
    vec![
        ("public",  "Lobby"),       // kit zone "public" rendered as "Lobby"
        ("members", "Members"),
        ("private", "Trusted"),
    ]
}
```

#### `src/dreamlab_workers.rs` — per-worker entry shims

Each shim is a `cdylib` entry that:
1. Loads `dreamlab.toml` (baked via `include_str!`)
2. Loads branding config (D3 above)
3. Calls into the kit's worker entry with the configuration

```rust
//! Per-worker entry shims. Each is gated by a Cargo feature so wrangler
//! builds only the worker it deploys.

use nostr_bbs_config::Config;
use crate::dreamlab_branding::dreamlab_branding;

const DREAMLAB_TOML: &str = include_str!("../dreamlab.toml");

#[cfg(feature = "auth-worker")]
#[allow(non_snake_case)]
pub mod auth_worker {
    use super::*;
    use nostr_bbs_auth_worker as kit;
    
    #[event(fetch)]
    pub async fn main(req: Request, env: Env, ctx: Context) -> Result<Response> {
        let cfg = Config::from_toml(DREAMLAB_TOML)?;
        let branding = dreamlab_branding();
        kit::dispatch(req, env, ctx, &cfg, &branding).await
    }
}

#[cfg(feature = "pod-worker")]
#[allow(non_snake_case)]
pub mod pod_worker {
    use super::*;
    use nostr_bbs_pod_worker as kit;
    
    #[event(fetch)]
    pub async fn main(req: Request, env: Env, ctx: Context) -> Result<Response> {
        let cfg = Config::from_toml(DREAMLAB_TOML)?;
        let branding = dreamlab_branding();
        kit::dispatch(req, env, ctx, &cfg, &branding).await
    }
}

#[cfg(feature = "relay-worker")]
pub use nostr_bbs_relay_worker::NostrRelayDO;

#[cfg(feature = "relay-worker")]
pub mod relay_worker {
    use super::*;
    use nostr_bbs_relay_worker as kit;
    
    #[event(fetch)]
    pub async fn main(req: Request, env: Env, ctx: Context) -> Result<Response> {
        let cfg = Config::from_toml(DREAMLAB_TOML)?;
        let branding = dreamlab_branding();
        kit::dispatch(req, env, ctx, &cfg, &branding).await
    }
}

// (similar for search-worker, preview-worker)

#[cfg(feature = "forum-client")]
pub mod forum_client {
    use super::*;
    use nostr_bbs_forum_client as kit;
    
    #[wasm_bindgen(start)]
    pub fn main() {
        let cfg = Config::from_toml(DREAMLAB_TOML).expect("dreamlab.toml parses");
        let branding = dreamlab_branding();
        kit::mount_with_config(cfg, branding);
    }
}
```

The kit's `dispatch(req, env, ctx, &cfg, &branding)` is the canonical extension API (defined in ADR-085 D4 below).

### D4 — Kit extension API contract

The kit's worker crates each export a `dispatch` function with this signature:

```rust
// In every kit worker crate (auth-worker, pod-worker, relay-worker, search-worker, preview-worker)
pub async fn dispatch<C, B>(
    req: Request,
    env: Env,
    ctx: Context,
    config: &C,           // typed Config from nostr-bbs-config
    branding: &B,         // typed BrandingConfig
) -> Result<Response>
where
    C: AsRef<Config>,
    B: AsRef<BrandingConfig>;
```

The kit's forum-client exports:

```rust
// nostr-bbs-forum-client/src/lib.rs
pub fn mount_with_config(config: Config, branding: BrandingConfig);
```

Branding extension points (the slots `BrandingConfig` populates) inside the kit:
- `<title>{copy.site_name}</title>`
- `<meta name="description" content="{copy.site_tagline}">`
- `<link rel="icon" href="{favicon_url}">`
- `<link rel="stylesheet" href="{custom_css_url}">` (when set)
- Footer text uses `copy.footer_text`
- Hero block uses `copy.hero_text`
- Theme tokens applied via Tailwind config-time (compile-time generated CSS) OR runtime CSS variables
- Logo `<img>` uses `logo_url`

Anti-drift lint (per ADR-077 P3) enforces: kit code DOES NOT hardcode any DreamLab string; all strings come from `BrandingConfig`. Kit ships with `default_branding()` returning generic `nostr-bbs-rs` branding.

### D5 — `dreamlab.toml` location & format

The complete configuration. Per PRD-011 §5.2 schema. Lives at `forum-config/dreamlab.toml`. Baked into worker WASM bundles via `include_str!` (per ADR-084 D8α). Not separately deployed.

Validates against `nostr-bbs-config`'s schema (kit publishes a JSON Schema at `nostr-bbs-config/schemas/config.schema.json` per ADR-082 protocol; consumer-side CI validates).

### D6 — Wrangler manifests

Per ADR-084 D6. Each `forum-config/deploy/<worker>.wrangler.toml` declares:
- `name` = preserved legacy worker name (per ADR-084 D3)
- `main` = `target/wasm32-unknown-unknown/release/dreamlab_forum_config_<worker>.wasm` (build-output path)
- `compatibility_date` = current
- All resource bindings (D1, KV, R2, DO) with preserved IDs (per ADR-084 D1)
- All routes preserved
- All vars preserved + `DREAMLAB_TOML_PATH` reference for documentation

The build target name `dreamlab_forum_config_<worker>` is generated from the workspace + feature combination:

```bash
cargo build --target wasm32-unknown-unknown --release --features auth-worker
# Produces target/wasm32-unknown-unknown/release/dreamlab_forum_config.wasm
# Renamed in CI to dreamlab_forum_config_auth_worker.wasm

cargo build --target wasm32-unknown-unknown --release --features pod-worker
# Produces same path; rename to dreamlab_forum_config_pod_worker.wasm
```

Each worker is a separate `cargo build` with a different feature flag. CI orchestrates the 5 separate builds + renames.

### D7 — Forum-client Trunk build pipeline

The Leptos WASM forum-client is built with Trunk. Pre-transition: `cd community-forum-rs/crates/forum-client && trunk build --release`. Post-transition:

Option α — Kit publishes the forum-client as a published-asset crate:

```toml
# forum-config/Cargo.toml (additional)
[package.metadata.trunk]
forum-client-crate = "nostr-bbs-forum-client"
```

The website's `deploy.yml` runs:
```bash
trunk build --release \
  --public-url /community/ \
  --features forum-client \
  forum-config/dreamlab-forum-client.html
```

Where `forum-config/dreamlab-forum-client.html` is a Trunk index file that imports from the kit:

```html
<!-- forum-config/dreamlab-forum-client.html -->
<!DOCTYPE html>
<html>
<head>
  <link data-trunk rel="rust" href="src/dreamlab_workers/forum-client.rs" data-bin="dreamlab-forum-client" />
  <link data-trunk rel="copy-file" href="../assets/dreamlab-overrides.css" />
  <link data-trunk rel="copy-file" href="sw.js" />
  <title>DreamLab Community Forum</title>
</head>
<body>
  <div id="forum-root"></div>
</body>
</html>
```

Option β — Kit ships pre-built bundle; consumer just inserts branding strings.

*Selected Option α* — gives consumer flexibility to override Trunk hooks (e.g. DreamLab's existing webgpu-particles.js post_build hook).

### D8 — Build pipeline integration

The website's existing `.github/workflows/deploy.yml` updated:

```yaml
# .github/workflows/deploy.yml — relevant excerpt
- name: Build forum-client WASM
  run: |
    cd forum-config
    cargo install trunk
    trunk build --release --public-url /community/ \
      --features forum-client \
      dreamlab-forum-client.html
    
- name: Build CF Worker WASMs
  run: |
    cd forum-config
    for worker in auth-worker pod-worker relay-worker search-worker preview-worker; do
      cargo build --target wasm32-unknown-unknown --release --features $worker
      mv target/wasm32-unknown-unknown/release/dreamlab_forum_config.wasm \
         target/wasm32-unknown-unknown/release/dreamlab_forum_config_${worker//-/_}.wasm
    done
    
- name: Deploy CF Workers
  run: |
    cd forum-config
    for worker in auth-worker pod-worker relay-worker search-worker preview-worker; do
      wrangler deploy -c deploy/${worker}.wrangler.toml
    done
```

### D9 — Anti-drift lint scope for `forum-config/`

Per ADR-077 P3 + PRD-012 F7:

```bash
# scripts/anti-drift-lint.sh additions for forum-config/

# Reject hardcoded URLs other than what's in dreamlab.toml or dreamlab_branding.rs
grep -rE 'https?://(api|pods|search)\.dreamlab-ai\.com' forum-config/src \
  --exclude-dir=target \
  | grep -v 'dreamlab_branding.rs' \
  && echo "FAIL: hardcoded DreamLab URL outside branding shim" && exit 1

# Reject DreamLab-specific strings outside branding shim
grep -rE 'DreamLab|Dreamlab' forum-config/src \
  --exclude-dir=target \
  | grep -v 'dreamlab_branding.rs' \
  | grep -v '^.*// .*DreamLab' \
  && echo "FAIL: DreamLab-specific string outside branding shim" && exit 1

# Reject ad-hoc URN minting
grep -rE 'format!.*urn:visionclaw:|format!.*urn:agentbox:' forum-config/src \
  --exclude-dir=target \
  && echo "FAIL: ad-hoc URN minting in forum-config/" && exit 1

echo "PASS: forum-config anti-drift lint clean"
```

### D10 — README + operator-facing docs

`forum-config/README.md` is operator-facing:

```markdown
# DreamLab Community Forum Configuration

This package configures the DreamLab Community Forum atop the `nostr-bbs-rs` kit.

## What this is

A thin downstream consumer of [`nostr-rust-forum`](https://github.com/DreamLab-AI/nostr-rust-forum) (the kit upstream).

## Files

- `dreamlab.toml` — THE forum configuration; edit this to change admin keys, zones, branding, mesh peers
- `src/dreamlab_branding.rs` — DreamLab visual identity (logo, theme, copy)
- `deploy/*.wrangler.toml` — CF Worker deployment manifests; preserved resource IDs (do not edit unless re-binding to new CF resources)

## Operator workflow

### Add a new admin pubkey

```bash
# Edit dreamlab.toml [admin].static_pubkeys; add hex pubkey
$ vim dreamlab.toml

# Validate
$ cargo install nostr-bbs-config-cli
$ nostr-bbs-config validate dreamlab.toml

# Open PR with the change
$ git checkout -b ops/add-admin-${USER}
$ git commit -am "ops: add admin pubkey for ${USER}"
$ gh pr create
```

### Update branding (logo, copy, theme)

Edit `src/dreamlab_branding.rs`. Same PR workflow.

### Update mesh peer roster

Edit `dreamlab.toml [mesh].peer_relays`. Same PR workflow.

### Upgrade kit version

Edit `Cargo.toml`'s `nostr-bbs-* = "3.X"` line. Run `cargo update`. Run parity test (per PRD-012 X2). Open PR.

## CI gates

PRs to this package run:
- `cargo check` (build)
- `cargo test` (tests)
- `cargo clippy -- -D warnings` (lint)
- `nostr-bbs-config validate dreamlab.toml` (schema)
- `scripts/anti-drift-lint.sh forum-config/` (drift)
- `scripts/audit-cf-resource-mapping.sh` (resource preservation per ADR-084 D9)

## See also

- PRD-012 — DreamLab Website Kit Adoption (this package's spec)
- ADR-085 — `forum-config/` Package Architecture (this ADR)
- ADR-084 — Cloud Infrastructure Mapping for Kit Consumers
- ADR-083 — Cutover migration pattern
- https://github.com/DreamLab-AI/nostr-rust-forum (kit upstream)
```

## Consequences

### Positive

- **Cleanly bounded consumer surface**: ~1,500 LOC max in `forum-config/` (per PRD-012 M7) versus ~30,000 LOC in legacy `community-forum-rs/`. Massive maintenance reduction.
- **Operator-friendly**: changes are TOML edits, not code changes. Operator workflow documented per D10.
- **Branding is a first-class extension point**: kit's `BrandingConfig` API + this consumer's `dreamlab_branding.rs` cleanly separates branding from protocol.
- **Independent Cargo workspace**: doesn't affect rest of website repo's build. CI scopes cleanly.
- **Anti-drift enforced**: hardcoded DreamLab values can only land in `dreamlab_branding.rs` + `dreamlab.toml`; everything else is kit-driven.
- **Re-export pattern (D6 DO class)**: existing CF Durable Object IDs continue to work because the kit exposes `NostrRelayDO` with the same class name as legacy.

### Negative

- **Kit extension API is a binding contract**: D4's `dispatch(req, env, ctx, config, branding)` shape is now part of kit's public API; breaking changes require coordinated migration.
- **5 separate `cargo build` invocations per CI** (one per worker feature): adds ~3-5 min CI time. Acceptable.
- **Branding hot-reload not supported**: changing `dreamlab_branding.rs` requires recompile + redeploy. No runtime CSS-variable swap. (Workaround: consumer-side CSS at `dreamlab-overrides.css` URL can hot-reload.)
- **Trunk build path is bespoke** (`forum-config/dreamlab-forum-client.html`): operators learning Trunk-from-scratch may find this confusing. Mitigation: thorough README + runbook.

### Neutral

- **`include_str!("../dreamlab.toml")` makes config immutable per build**: matches CF Workers immutability model; operator changes via redeploy not surprising.
- **Per-worker feature flag explosion** (5 features): manageable with naming convention.

## Alternatives Considered

### Alt-A — `forum-config/` is part of an outer Cargo workspace
Add a root `Cargo.toml` to `dreamlab-ai-website` that lists `forum-config/` as a workspace member.

*Rejected*: dreamlab-ai-website is fundamentally a React/TypeScript monorepo; adding a root Cargo workspace conflates languages. Independent Cargo workspace (D1) keeps boundaries clean.

### Alt-B — `forum-config/` as a published crates.io package
Publish `dreamlab-forum-config` as a crate.

*Rejected*: makes no sense — DreamLab's branding is not generally useful. Internal-only package.

### Alt-C — Branding via TOML (no Rust shim file)
Express ALL branding (logo, theme, copy) entirely in `dreamlab.toml`, no `dreamlab_branding.rs`.

*Rejected*: some branding behaviour is computational (e.g. theme-token-to-Tailwind-config compilation); pure TOML insufficient. Hybrid (TOML for strings + URLs; Rust shim for computed structures) is right.

### Alt-D — Single mega-WASM bundle for all 5 workers
One `cargo build` produces a single WASM blob; CF Worker entry routes by URL.

*Rejected*: mega-WASM exceeds CF Worker 1 MiB limit; 5-worker split is the existing sharding from sprint v9.

### Alt-E — `forum-config/` is a Cargo workspace with sub-crates
`forum-config/Cargo.toml` declares sub-members like `forum-config/auth-worker-shim/`, `forum-config/pod-worker-shim/`, etc.

*Considered*. Cleaner than feature flags. Trade-off: more files, more Cargo.toml files, more orchestration. Feature flags (D2) are simpler for the small consumer-shim use case.

### Alt-F — Skip `forum-config/` entirely; just write wrangler.toml
Don't write any Rust code in `forum-config/`; directly invoke kit binaries.

*Rejected*: branding extension requires Rust code (D3); forum-client custom HTML requires Trunk shim (D7); the package shape exists for legitimate reasons.

## Implementation notes

### Initial `forum-config/` scaffolding (X1 deliverable)

```bash
# X1 day 1: scaffold the package
mkdir -p dreamlab-ai-website/forum-config/{src,deploy,assets}
cd dreamlab-ai-website/forum-config
cargo init --lib --name dreamlab-forum-config
# Edit Cargo.toml per D2

# Day 2: copy current dreamlab values
# Read community-forum-rs/crates/*/wrangler.toml; extract resource IDs into deploy/*.wrangler.toml
# Read existing admin pubkey config; insert into dreamlab.toml [admin].static_pubkeys
# Read existing Tailwind config; encode amber palette into src/dreamlab_branding.rs

# Day 3: validate
cargo check --features auth-worker
cargo check --features pod-worker
cargo check --features relay-worker
cargo check --features search-worker
cargo check --features preview-worker
cargo check --features forum-client
nostr-bbs-config validate dreamlab.toml

# Day 4: anti-drift + tests
scripts/anti-drift-lint.sh forum-config/
cargo test
```

### Per-worker target binary naming

Cargo by default produces `target/wasm32-unknown-unknown/release/<crate-name>.wasm` (where crate name is `dreamlab_forum_config`). All 5 worker builds collide on the same output path. Solutions:

**Solution α** — Each `cargo build` with a feature flag, then rename + cache:
```bash
cargo build --features auth-worker
mv target/.../dreamlab_forum_config.wasm target/.../auth_worker.wasm

cargo clean -p dreamlab-forum-config
cargo build --features pod-worker
mv target/.../dreamlab_forum_config.wasm target/.../pod_worker.wasm
```

**Solution β** — Multiple `[[bin]]` targets in Cargo.toml, each gated on feature:
```toml
[[bin]]
name = "dreamlab_auth_worker"
path = "src/bin/auth_worker.rs"
required-features = ["auth-worker"]

[[bin]]
name = "dreamlab_pod_worker"
path = "src/bin/pod_worker.rs"
required-features = ["pod-worker"]
```

*Selected Solution β* — cleaner; cargo handles the naming + feature gating natively.

### Sprint Carry-Over Fixture Suite execution

Per PRD-011 G6 + PRD-012 F8: the suite runs against the deployed `forum-config/` (in staging). Each test asserts a Sprint v9-v11 capability is preserved:

- NIP-98 replay: spam-test the same NIP-98 token twice; second rejected
- WAC Control coercion: PUT to `*.acl` requires `acl:Control` mode
- SSRF redirect: preview-worker rejects RFC1918-redirected URL
- KV namespace split: auth-worker writes ADMIN_KV; pod-worker reads from ADMIN_KV_RO; verify the same namespace ID
- Profile backfill: verify endpoint returns existing kind-0 events
- Username reservation: claim-check-release flow
- NIP-26 delegation verifier: post a delegated event; check it's accepted
- Mesh service-list scaffolding: kind-30033 emit at boot
- Signer-trait NIP-98: sign via NIP-07 mock, verify accepted
- Tailwind build-time CDN replacement: `community/tailwind.dist.css` ≤ 100KB
- forum-client sw.js: GET `/community/sw.js` returns 200
- Profiles batch + search: API contract test

All 12 tests live in `nostr-rust-forum/tests/cutover/sprint-carry-over.rs` (per ADR-082 D6 protocol); consumer's CI invokes them against the staging deployment.

## References

- PRD-012 — DreamLab Website Kit Adoption (G3, G4, G5, F1, F5)
- PRD-011 — VisionFlow Forum Kit Extraction (G3 downstream-consumer pattern, §5.2 TOML schema)
- ADR-073 — Mesh topology (relay-worker DO continuity)
- ADR-080 D7 — Forum Kit Deployment Topology (downstream-consumer pattern exemplar)
- ADR-082 — Cross-substrate test fixture sharing (consumer's CI)
- ADR-083 — Cutover migration (gating)
- ADR-084 — Cloud infrastructure mapping (resource ID preservation)
- `docs/integration-research/02-forum-surfaces.md` (current forum architecture)
- Cloudflare Workers WASM bundle limits: https://developers.cloudflare.com/workers/platform/limits/
- Trunk Cargo build tool: https://trunkrs.dev/
- GitHub repos:
  - https://github.com/DreamLab-AI/dreamlab-ai-website (the consumer host)
  - https://github.com/DreamLab-AI/nostr-rust-forum (the kit upstream — defines extension API)
