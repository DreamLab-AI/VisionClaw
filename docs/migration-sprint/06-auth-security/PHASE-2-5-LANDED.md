# Phase 2.5 — Auth Compile-Time Gates Landed

Status   : Landed (pending Queen merge)
Date     : 2026-05-16
Branch   : `impl/phase-2-5-auth-gates` (off `radical-rollback @ d260a6158`)
Owner    : security-specialist
ADRs     : ADR-06 §D1 + §D2 + §D11, ADR-02 §D8 (deferred to ADR-06)
PRD      : PRD-06 §A1 + §A4 (corrected per resolution T2)
Resolves : T2 (auth-bypass gating mechanism)

## What landed

The auth-bypass surface is now compile-time gated by a `dev-auth` Cargo feature.
Release binaries built without `--features dev-auth` physically cannot honour
any of the historic bypass paths.

### Cargo feature

`Cargo.toml` declares:
```toml
[features]
default = ["gpu", "ontology"]
dev-auth = []
```

### Code changes

| File | Decision | Lines changed |
|------|----------|---------------|
| `Cargo.toml` | D1 — feature declaration | +13 |
| `src/settings/auth_extractor.rs` | D1 — `try_dev_bypass` compile-gated | ~50 (net: -28 runtime guards, +35 compile-gate) |
| `src/handlers/socket_flow_handler/http_handler.rs` | D1 — `is_insecure_defaults_allowed` compile-gated; all `insecure_allowed` branches paired with release-only branches | ~70 |
| `src/main.rs` | D11 — `enforce_release_env_hygiene()` boot hook; removed case-sensitive `APP_ENV=production` runtime guard (T2 audit bug at lines 98-100); CORS fallback gated | ~70 |
| `scripts/launch.sh` | Pass `dev-auth` in `FEATURES=` for dev environment only | +12 |
| `client/src/services/api/authInterceptor.ts` | Informational `console.warn` when server returns 401 on dev token (release-mode detection) | +38 |
| `tests/auth_bypass_release.rs` | V1–V4 enforcement (symbol absence, argv refusal, D11 boot refusal, source-grep contract) | +290 (new) |
| `docs/migration-sprint/06-auth-security/PHASE-2-5-LANDED.md` | This file | +160 (new) |

### Bypass sites compile-stripped

1. **`SETTINGS_AUTH_BYPASS`** (formerly read at `src/settings/auth_extractor.rs:44`
   and `:155`): both runtime reads removed. Bypass path now lives in
   `try_dev_bypass()` which is `#[cfg(any(debug_assertions, feature = "dev-auth"))]`.

2. **`ALLOW_INSECURE_DEFAULTS`** (formerly read at
   `src/handlers/socket_flow_handler/http_handler.rs:17`): the function
   `is_insecure_defaults_allowed()` now has paired definitions — a dev variant
   that reads the env var, and a release variant that returns `false`
   unconditionally. The two are mutually exclusive via `#[cfg]`.

3. **`--allow-skip-auth`** (ADR-02 §D8 + ADR-06 §D2): handled in
   `enforce_release_env_hygiene()` which exits with status 1 if the flag is
   passed in a release build.

4. **`Bearer dev-session-token`** (formerly accepted at
   `src/settings/auth_extractor.rs:155-172`): the token-acceptance branch is
   now inside a `#[cfg(any(debug_assertions, feature = "dev-auth"))]` block.

5. **CORS broadening on `ALLOW_INSECURE_DEFAULTS`** at `src/main.rs:670`
   (formerly): the localhost-allowlist fallback is now compile-gated. Release
   builds use the restrictive single-origin default unless
   `CORS_ALLOWED_ORIGINS` is explicitly configured.

### D11 boot refusal

`enforce_release_env_hygiene()` (release-only, called from `main()` immediately
after `dotenv().ok()`):

- **Argv check** — refuses `--allow-skip-auth` with `exit(1)`.
- **Env-var check** — exits with `exit(2)` if any of `SETTINGS_AUTH_BYPASS`,
  `ALLOW_INSECURE_DEFAULTS`, `VISIONFLOW_DEV_MODE` are present, OR if
  `NODE_ENV=development` AND `DOCKER_ENV` are both set.

Each offending var is logged to stderr by name.

### Anti-pattern eliminated

The case-sensitive `APP_ENV=production` runtime guard at the original
`src/main.rs:98-100` (T2 audit bug — `APP_ENV=Production` defeated it) has been
removed. It is no longer needed because dev-bypass codepaths are absent from
the release binary by `#[cfg]`, not by runtime guard. The single remaining
read of `APP_ENV` in `validate_required_env_vars()` is now purely a
"missing-required-vars" decision (warn-and-default in dev, hard-fail in prod)
with NO security semantics.

## Verification

### V1 — Symbol absence

```bash
cd /home/devuser/workspace/visionflow-worktrees/phase-2-5-auth-gates
cargo build --release  # builds without dev-auth feature
strings target/release/webxr | grep -E \
  'SETTINGS_AUTH_BYPASS|VISIONFLOW_DEV_MODE|dev-session-token|dev-user|--allow-skip-auth'
```

Expected matches in release: ONLY the D11 SUSPECT_ENVS const array literals
(`"SETTINGS_AUTH_BYPASS"`, `"ALLOW_INSECURE_DEFAULTS"`, `"VISIONFLOW_DEV_MODE"`)
inside `enforce_release_env_hygiene()`, and the FATAL error messages naming
the same vars. NO matches for `dev-session-token`, `dev-user`, or any of those
strings inside auth-acceptance code paths.

**Status: VERIFICATION DEFERRED — `cargo build --release` could not be run in
this agent's environment due to two pre-existing infrastructure constraints:**

1. **Empty `whelk-rs` path dependency** in the worktree. The `Cargo.toml`
   declares `whelk = { path = "./whelk-rs" }` but the directory is empty.
   Sibling worktree `visionflow-worktrees/phase-1-persistence/whelk-rs` has
   the content; a permanent fix would be a git submodule or `whelk-rs` checked
   into all worktrees.
2. **CUDA toolchain not at the path `cust_raw` build-script expects**
   (`/opt/cuda` or `/usr/local/cuda`, with `lib64/libcuda.so` and
   `include/cuda.h`). The container has a Nix CUDA at
   `/nix/store/xp7xq1b0qcv3r4vqdv6qwb5xw1bskzry-cuda-merged-12.9` but `/opt`
   is read-only.

Both are container-environment issues, not introduced by this change. The
Queen should run V1 in the tmux tab 6 host environment where Docker handles
the toolchain layout.

### V2 — Argv refusal

```bash
./target/release/webxr --allow-skip-auth
# Expected: exit code 1, stderr: "FATAL: --allow-skip-auth is not available in release builds"
```

### V3 — D11 boot refusal

```bash
SETTINGS_AUTH_BYPASS=true ALLOW_INSECURE_DEFAULTS=1 \
  ./target/release/webxr
# Expected: exit code 2
# Expected stderr: each var named with "FATAL: dev env var '<NAME>' set in release build"
```

### V4 — Source-grep contract

```bash
cargo test --release --test auth_bypass_release
```

Or, manual grep:
```bash
grep -rn "std::env::var.*\(SETTINGS_AUTH_BYPASS\|ALLOW_INSECURE_DEFAULTS\|VISIONFLOW_DEV_MODE\)" \
  src/main.rs src/middleware/ src/settings/ src/handlers/socket_flow_handler/
```

Expected: every match is either inside a `#[cfg(any(debug_assertions, feature = "dev-auth"))]`
block, or inside `enforce_release_env_hygiene()` (which is itself
`#[cfg(not(any(debug_assertions, feature = "dev-auth")))]`).

## Additional bypass surface discovered (NOT addressed in Phase 2.5)

Five additional sites read `ALLOW_INSECURE_DEFAULTS` as a **Neo4j password
default-credentials fallback** (not auth bypass — DB cred bypass). These are
out of scope for ADR-06 (Section 6, Auth) and belong to a future phase:

| File | Line | Purpose |
|------|------|---------|
| `src/bin/sync_local.rs` | 29 | Allows `NEO4J_PASSWORD` to be missing |
| `src/bin/sync_github.rs` | 27 | Same |
| `src/adapters/neo4j_adapter.rs` | 57, 74 | Same |
| `src/adapters/neo4j_settings_repository.rs` | 94 | Same |
| `src/adapters/neo4j_ontology_repository.rs` | 44, 73 (doc) | Same |

Recommendation: address in **Section 11 (Persistence Migration)** when Neo4j
is replaced by Oxigraph/SQLite. The new persistence adapters should not have
any `ALLOW_INSECURE_DEFAULTS` codepath at all — empty/default credentials
should fail-hard everywhere.

## Trait surface unchanged

Per task constraint, no auth trait or struct signature was modified:

- `AuthenticatedUser { pubkey: String, is_power_user: bool }` — unchanged
- `OptionalAuth(pub Option<AuthenticatedUser>)` — unchanged
- `RequireAuth::*` constructors — unchanged
- `FromRequest for AuthenticatedUser` — unchanged signature
- `get_authenticated_user()` — unchanged

The behaviour change is purely *inside* `from_request()` and the helper
`try_dev_bypass()`, both of which are private.

## Coordination notes for Phase 7 (D3-D11 remainder)

Phase 7 of ADR-06 still owes:

- D3 + D4: three-state route declaration + endpoint audit table (22+ handlers)
- D5: CSP tightening end-to-end
- D6: audit log SQLite table + middleware
- D7: docker socket scoping
- D8: trusted-proxy CIDR boundary
- D9: handler-layer `unwrap()` audit
- D10: NIP-98 URL canonicalisation cross-validation
- D12: WebSocket endpoint enumeration CI check

Phase 2.5 lands D1 + D2 + D11 only, per README §"Implementation phasing" item
2.5 and ADR-06 §Phasing.

## Client behaviour change

`client/src/services/api/authInterceptor.ts` now emits a one-shot
`console.warn` when:
- the request carried `Authorization: Bearer dev-session-token`, AND
- the response status is 401.

This is informational — the client cannot grant bypass. It surfaces the case
where a dev-mode browser is talking to a release-mode server, so users
immediately understand why `?skipAuth=true` is being ignored.

## Boot order in `main()`

```rust
async fn main() -> std::io::Result<()> {
    std::panic::set_hook(...);
    dotenv().ok();
    enforce_release_env_hygiene();  // <-- D11 boot refusal, BEFORE everything else
    tracing_subscriber::registry()...;
    validate_required_env_vars()?;
    // ... rest of startup ...
}
```

`enforce_release_env_hygiene()` runs before logging init so the `eprintln!`
fatal messages reach stderr unconditionally.

## launch.sh wiring

```bash
# scripts/launch.sh build_containers()
if [[ "$ENVIRONMENT" == "dev" ]]; then
    feature_list="${feature_list},dev-auth"
fi
build_args+=("--build-arg" "FEATURES=${feature_list}")
```

`up dev` → `FEATURES=gpu,ontology,dev-auth` (or `ontology,dev-auth` if no GPU).
`up prod` → `FEATURES=gpu,ontology` (NO `dev-auth`).
