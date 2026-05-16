# T2 — Auth-bypass gating mechanism: compile-time vs runtime

Status   : Resolution proposal
Date     : 2026-05-16
Auditor  : QE Security
ADRs     : ADR-02 D8, ADR-06 D1+D2, PRD-06 A1+A4
Baseline : radical-rollback @ 41979d33e

## The tension

ADR-02 D8 gates `--allow-skip-auth` on `build = debug || env(VISIONFLOW_DEV_MODE)`
— a runtime env-var check OR'd with `debug_assertions`.

ADR-06 D1+D2 gates the same surface on
`#[cfg(any(debug_assertions, feature = "dev-auth"))]` — purely compile-time.

PRD-06 contradicts itself: A4 matches ADR-02 D8 (env-var path); A1, three
paragraphs above it, matches ADR-06 D2 ("release builds physically do not
contain the bypass branch"). Three documents specify two mechanisms. The
strictest of them is the security-owner. The contradiction itself is the
failure mode the bypass is meant to prevent.

## Current state in code

**`src/settings/auth_extractor.rs:39-62`** — runtime bypass. The bypass code
is in the release binary. Trigger: `SETTINGS_AUTH_BYPASS=true` OR
(`DOCKER_ENV` set AND `NODE_ENV=development`). Production gate:
`APP_ENV=production` OR `RUST_ENV=production`. Six scalar vars jointly
determine safety. `APP_ENV=Production` (capital P) defeats the production
check — no `eq_ignore_ascii_case`. Same bypass repeats for the Bearer token
path at lines 153-172.

**`src/main.rs:98-100`** — startup refusal of `SETTINGS_AUTH_BYPASS=true`
when `APP_ENV=production`. Good in intent but case-sensitive and
single-var, so it does not cover the full ops-mistake surface.

**`src/handlers/socket_flow_handler/http_handler.rs:16-35`** — WebSocket
upgrade has a parallel anti-pattern: `ALLOW_INSECURE_DEFAULTS` gated by
`APP_ENV`/`RUST_ENV` (this one uses `eq_ignore_ascii_case`, inconsistent
with the settings path). When honoured, token validation failure is
logged-not-enforced — the moral equivalent of `--allow-skip-auth`, spelled
as an env var.

**`client/src/services/api/authInterceptor.ts:24-33`** — emits
`Authorization: Bearer dev-session-token` + `X-Nostr-Pubkey: dev-user` when
`nostrAuth.isDevMode()`. Symmetric to the server bypass.

**`Cargo.toml:163-176`** — no `dev-auth` feature exists yet. Release profile
sets `lto=true`, `strip=true`; `debug-assertions` unset (default false on
release). `#[cfg(debug_assertions)]` will compile-strip and `strip=true`
removes residual string literals. Compile-time gating works as advertised.

**`main` HEAD**: cosmetically different only — the `DOCKER_ENV+NODE_ENV`
extra arm was added on rollback. No security tightening landed on main.

## Threat model

Asset: admin/power-user access to settings, graph/ontology mutations, bot
spawn, MCP tool execution, LLM/RAG/Nano-Banana billing, Nostr publishing.

- **T-A. Ops misconfiguration** (most likely). `SETTINGS_AUTH_BYPASS=true` in
  staging compose, promoted to prod without grep. K8s configmap, systemd
  EnvironmentFile, `--env-file`, ops-repo `.env` carries the var. Or
  `APP_ENV=Production` (capital). Or `APP_ENV` dropped in a refactor. Six
  scalar vars; safe combinations are a narrow valley in a 6D space.
- **T-B. Supply-chain via ops repo.** Attacker who can write
  `docker-compose.yml`, k8s manifest, or `.env` flips the bypass without
  touching the app binary. App-repo and ops-repo review rigour are often
  asymmetric.
- **T-C. File-system access.** Init container, kubelet exec, or volume
  mount drops a `.env` into the process CWD before `dotenv().ok()`
  (`main.rs:135`). Privileged-attacker scenario; runtime gating gives a
  one-line escalation, compile-time does not.
- **T-D. Defence in depth.** When CSP (ADR-06 D5) or audit log (D6) fail,
  auth is the last wall. A release binary that *cannot execute* the bypass
  branch is one fewer thing that can fail under pressure.

T-A, T-B, T-C entirely defeated by compile-time gating with no runtime env
path. T-D materially improved.

## Industry idioms surveyed

When the difference between dev and prod is *attack surface presence* —
not configuration values — Rust idiom is unanimous: Cargo feature flag,
with `debug_assertions` as the default-on-debug convenience. Env vars
are for configuration values, not security toggles.

- **`rustls`**: `ClientConfig::dangerous().set_certificate_verifier(...)`
  is behind the `dangerous_configuration` Cargo feature. Docs explicit:
  "gated by a feature flag to make sure it doesn't accidentally get
  enabled in production builds." Identical reasoning to our case.
- **`tokio`**: uses `debug_assertions` for sanity checks that release
  elides. Production behaviour is a function of `#[cfg]`, not env.
- **`axum-extra`**: auth-bypass test fixtures live behind `#[cfg(test)]`,
  never env.
- **`cargo-audit`, `cargo-deny`**: developer modes are Cargo-feature-gated.

Cautionary cases at the service level:

- **HashiCorp `vault server -dev`** carries dev code into the production
  binary; HashiCorp compensates with boot-time refusals and ops
  discipline, and documents the resulting failure cases. This is the
  *anti-model*.
- **`etcd`** removed runtime-flag dev mode in v3.4+ citing ops-mistake
  exposure.
- **`kubelet --insecure-skip-tls-verify`** deprecated in k8s 1.22 and
  removed, explicit reason: ops-misconfiguration risk.
- **Django `DEBUG=True`** is the canonical case study against runtime
  bypasses. Python has no compile-time elimination; Rust does, so it
  should use it.

## Options evaluated

- **A. ADR-02 D8 defers to ADR-06 (compile-time only).** Release binaries
  cannot run the bypass branch. T-A,B,C fully defeated; T-D improved. Dev
  ergonomics: identical to ADR-06's scheme. Zero new env vars. Verifiable
  via V1.
- **B. ADR-06 relaxes to allow runtime gating in dev builds.** Reintroduces
  `VISIONFLOW_DEV_MODE`. T-A,B,C *reintroduced* wherever the dev binary
  runs. Saves one CLI flag at real cost. Rejected.
- **C. Hybrid: compile floor + env opt-in in dev builds.** Release =
  identical to A; dev requires *both* feature and env. Defensible but adds
  dev friction for no production-security gain (env var only matters if a
  dev build is exposed; env requirement does not prevent the exposure).
  Marginally worse than A.
- **D. Compile gate + boot-time hard-fail on suspect env vars in release.**
  Strict superset of A. Release binary, on startup, refuses to run if
  `SETTINGS_AUTH_BYPASS`, `VISIONFLOW_DEV_MODE`, `ALLOW_INSECURE_DEFAULTS`,
  or `NODE_ENV=development`+`DOCKER_ENV` are present. Catches "ops promoted
  dev compose to prod" at deploy time, not at first attack. ~30 lines in
  `main.rs`.

## Recommended resolution: A + D

Compile-time gating is the floor. Boot-time hard-fail on dev env vars is
defence-in-depth above it. ADR-02 defers to ADR-06. PRD-06 A4 corrected.

### Proposed ADR-02 D8 replacement

> ### D8. Auth model
>
> WebSocket upgrade requires a `?token=<nostr_jwt>` query param in production.
> In dev mode (`?skipAuth=true` to the HTML shell), the client emits no
> token; the server, if launched with `--allow-skip-auth`, accepts.
>
> The `--allow-skip-auth` flag is gated *exclusively* by the compile-time
> mechanism specified in ADR-06 D2:
> `#[cfg(any(debug_assertions, feature = "dev-auth"))]`. Release binaries
> built without the `dev-auth` feature physically cannot honour the flag —
> the flag-handling code is absent from the binary. There is no runtime
> env-var path. Section 6 owns this surface; this section defers.

### PRD-06 A4 correction

Replace `cfg(debug_assertions) || env(VISIONFLOW_DEV_MODE)` with
`cfg(any(debug_assertions, feature = "dev-auth"))`. Delete the
`VISIONFLOW_DEV_MODE` reference entirely (don't introduce a name that
implies a runtime path that doesn't exist).

### New ADR-06 D11 (Option D)

> ### D11. Startup refusal of dev-mode env vars in release
>
> The release binary, in `main.rs` after `dotenv().ok()` and before binding
> any socket, refuses to start if any of the following are present:
> `SETTINGS_AUTH_BYPASS`, `VISIONFLOW_DEV_MODE`, `ALLOW_INSECURE_DEFAULTS`,
> or `NODE_ENV=development` with `DOCKER_ENV` set. Logs each offending var
> to stderr, exits with status 2.
>
> Wrapped in `#[cfg(not(any(debug_assertions, feature = "dev-auth")))]` so
> dev builds skip it. The release binary cannot *honour* these vars (no code
> reads them) but their presence is signal of an ops promotion that brought
> dev settings forward. Refusing to start surfaces the error at deploy time.

## Verification

A reviewer proves a release binary cannot enable bypass via:

- **V1. Symbol absence.** After `cargo build --release` (no `--features
  dev-auth`):
  ```bash
  strings target/release/webxr | grep -E \
    'SETTINGS_AUTH_BYPASS|VISIONFLOW_DEV_MODE|dev-session-token|dev-user'
  ```
  Expected: no matches. Strings only appear inside `#[cfg(...)]` blocks
  that compile to nothing; `strip=true` removes residuals. Any hit is a
  regression.
- **V2. Argv refusal.** `./target/release/webxr --allow-skip-auth` →
  exit code 1, stderr `--allow-skip-auth is not available in release
  builds`. Argv parsing is allowed; honouring is not.
- **V3. Env-var no-op + D11 boot refusal.** Start release binary with
  `SETTINGS_AUTH_BYPASS=true VISIONFLOW_DEV_MODE=true
  NODE_ENV=development DOCKER_ENV=1`. With D11: exit code 2, each
  offending var named. Without D11: binary runs but `POST /api/settings`
  with no auth → 401. PRD-06's success-metric CI matrix is exactly V3.
- **V4. Source lint as a `cargo test`.** Recursively greps `src/` for
  `std::env::var\(.*BYPASS|DEV_MODE|INSECURE` and fails on matches
  outside `#[cfg(...)]` blocks. Regression contract.
- **V5. Disassembly sanity (one-time).** `objdump -d target/release/webxr
  | grep -c try_dev_bypass` → 0. Proves the function symbol is absent,
  not merely unreachable.

## Coordination notes

- ADR-02 D8: wording change only; no Section 2 implementation impact.
- PRD-06 A4: documentation fix.
- ADR-06 D11: ~30 lines in `main.rs`.
- `Cargo.toml`: add `dev-auth = []` feature (already in ADR-06 D1 listing).
- `scripts/launch.sh:348-356`: add `dev-auth` to the dev `FEATURES=` build
  arg; omit from prod. Currently passes `gpu,ontology` only.
- `is_insecure_defaults_allowed()` in `socket_flow_handler/http_handler.rs`
  must convert to the same compile-time gate — same anti-pattern.
- `authInterceptor.ts`: no client change. Server compile gate is sufficient.

## Decision

**Option A + Option D.** Compile-time gating is the floor; release-build
boot-time hard-fail on suspect env vars is defence-in-depth above it.
ADR-02 D8 wording updated to defer to ADR-06. PRD-06 A4 corrected. ADR-06
gains D11. Three documents now specify one mechanism.
