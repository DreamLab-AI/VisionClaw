# CI Alignment Report -- ADR-077 Ecosystem QE Policy

| Field | Value |
|-------|-------|
| Date | 2026-05-09 |
| Scope | All 5 DreamLab ecosystem substrates |
| Reference | ADR-077 (Ecosystem Quality Engineering Policy) |
| Author | QE Worker 4 (CI alignment specialist) |

## 1. Substrate CI Matrix

Legend: Y = present and blocking, A = present but advisory (continue-on-error), N = missing, n/a = not applicable.

| CI Job | VisionClaw | nostr-rust-forum | solid-pod-rs | dreamlab-ai-website | agentbox |
|--------|-----------|-----------------|-------------|--------------------|---------| 
| **Rust fmt** | Y | Y | Y | Y (forum-config) | n/a (Nix) |
| **Rust clippy** | A (backlog) | Y (-D warnings) | Y (-D warnings) | Y (forum-config) | n/a (Nix) |
| **Rust test** | Y | Y | Y (matrix: 2 OS x 2 toolchains x 4 feature sets) | Y (forum-config) | n/a |
| **Rust audit** | Y | Y (weekly + PR) | Y | N | n/a |
| **Rust deny** | Y | Y | Y | N | n/a |
| **MSRV check** | N | N | Y (1.75) | N | n/a |
| **WASM check** | N | Y | Y | N | n/a |
| **Doc check** | N | Y | Y | N | n/a |
| **JS/TS lint** | Y (new: client-lint) | n/a | n/a | Y (ESLint) | n/a |
| **JS/TS test** | Y (new: client-test vitest) | n/a | n/a | Y (vitest, advisory) | n/a |
| **JS/TS build** | N | n/a | n/a | Y | n/a |
| **npm audit** | N | n/a | n/a | Y (advisory) | n/a |
| **Nix flake check** | n/a | n/a | n/a | n/a | Y (x86_64 + aarch64) |
| **Manifest validate** | n/a | n/a | n/a | n/a | Y |
| **Contract tests** | n/a | n/a | n/a | n/a | Y |
| **TUI tests** | n/a | n/a | n/a | n/a | Y (pytest) |
| **Runtime contract** | n/a | n/a | n/a | n/a | Y (RC-*.sh) |
| **ShellCheck** | N | N | N | N | Y |
| **Secret scan** | N | N | N | N | Y (gitleaks) |
| **Image scan** | N | N | N | N | Y (Trivy + SBOM) |
| **Coverage** | N | N | Y (tarpaulin + Codecov) | N | N |
| **Fixtures validity** | Y (ADR-082) | N | N | N | N |
| **Docs CI** | N | N | N | N | Y |
| **Concurrency group** | Y | Y | Y | Y (partial) | Y |
| **Path filtering** | N (intentional) | Y | Y | Y | Y |
| **ci-pass aggregator** | Y (new) | Y | Y | Y (new) | Y |

## 2. Changes Made

### 2.1 VisionClaw (`/home/devuser/workspace/project/.github/workflows/rust-ci.yml`)

**Added 3 new jobs:**

1. **`client-test`** -- Runs `npx vitest run --reporter=verbose` in the `client/` directory. Node 20, npm ci, jsdom environment. This gates the 8+ test files (websocketStore, quality-gates, filter-tab, visualization, etc.) that previously had zero CI enforcement.

2. **`client-lint`** -- Runs `npm run lint` (ESLint) on the client TypeScript/React codebase. Catches type-unsafe patterns and import issues before merge.

3. **`ci-pass`** -- Aggregator job (`CI required`) that depends on all 8 jobs: fmt, clippy, test, audit, deny, client-test, client-lint, fixtures-validity. Uses `if: always()` with per-job result verification. Branch protection should point at this single job name.

### 2.2 dreamlab-ai-website (`/home/devuser/workspace/dreamlab-ai-website/.github/workflows/ci.yml`)

**Created new unified CI workflow** with 8 jobs:

1. **`node-lint`** -- ESLint on React/TS source
2. **`node-test`** -- vitest (advisory/continue-on-error while test suite matures)
3. **`node-build`** -- `npm run build` with stub env vars to verify type/build correctness
4. **`node-audit`** -- `npm audit --audit-level=high` (advisory)
5. **`rust-fmt`** -- `cargo fmt --check` on `forum-config/`
6. **`rust-clippy`** -- `cargo clippy -D warnings` on `forum-config/`
7. **`rust-test`** -- `cargo test` on `forum-config/`
8. **`ci-pass`** -- Aggregator requiring all 7 upstream jobs to succeed

This complements the existing `test-and-lint.yml` (reusable, called by deploy.yml) by providing a standalone CI pipeline that runs on push/PR with path filtering and concurrency groups.

### 2.3 No Changes Needed

- **nostr-rust-forum**: Already has a complete 7-job CI (fmt, clippy, test, wasm, doc, deny, ci-pass) plus separate audit.yml. Gold standard in the ecosystem.
- **solid-pod-rs**: Already has the most comprehensive CI (build matrix, MSRV, WASM, cargo-deny, cargo-audit, coverage, ci-required aggregator). Reference implementation.
- **agentbox**: Already has 16 workflow files covering Nix flake check, manifest validation, contract tests, TUI tests, runtime contract, ShellCheck, secret scan, image scan, docs CI, multi-arch build, release, and a CI aggregator. The most extensive CI in the ecosystem, appropriate for its complexity.

## 3. Remaining Gaps (Prioritised)

### P0 -- Blocking for ADR-077 compliance

| Gap | Substrate | Effort | Impact |
|-----|-----------|--------|--------|
| VisionClaw clippy not -D warnings | VisionClaw | Medium (backlog clearance) | Lint regressions slip through |
| No cargo-deny for dreamlab-ai-website forum-config | dreamlab-ai-website | Low | Licence policy unenforced on consumer overlay |
| No cargo-audit for dreamlab-ai-website forum-config | dreamlab-ai-website | Low | Advisory DB not checked on consumer overlay |

### P1 -- High value, medium effort

| Gap | Substrate | Effort | Impact |
|-----|-----------|--------|--------|
| No MSRV check in VisionClaw or nostr-rust-forum | Both | Low | MSRV drift undetected |
| No WASM check in VisionClaw | VisionClaw | Medium | scene-effects WASM crate not gated |
| No doc check in VisionClaw | VisionClaw | Low | Broken intra-doc links undetected |
| No ShellCheck in VisionClaw | VisionClaw | Low | scripts/ has unchecked bash |
| No secret scan in VisionClaw, nostr-rust-forum, solid-pod-rs, dreamlab-ai-website | All except agentbox | Low | Credential leak risk |
| No npm audit in VisionClaw client | VisionClaw | Low | JS dependency advisories unchecked |

### P2 -- Desirable, lower priority

| Gap | Substrate | Effort | Impact |
|-----|-----------|--------|--------|
| No coverage reporting in VisionClaw, nostr-rust-forum, dreamlab-ai-website | 3 substrates | Medium | Coverage regression invisible |
| No mutation testing anywhere | All | High | ADR-077 P4 unenforced |
| No cross-substrate contract tests (L2) | All | High | ADR-077 P2 L2 unenforced |
| No bundle size budget CI gate | nostr-rust-forum, dreamlab-ai-website | Medium | ADR-076 D5 unenforced |

## 4. Ecosystem CI Maturity Scores

Scoring: each ADR-077-required capability scores 1 point (fmt, lint, test, audit, ci-pass = 5 baseline). Extra capabilities add 0.5 each. Advisory-only jobs score 0.5.

| Substrate | Baseline (0-5) | Extras | Total | Grade |
|-----------|----------------|--------|-------|-------|
| solid-pod-rs | 5.0 | +3.0 (MSRV, WASM, coverage, doc, matrix) | 8.0 | A |
| nostr-rust-forum | 5.0 | +2.0 (WASM, doc, path-filter) | 7.0 | A- |
| agentbox | 4.5 (no Rust -- Nix-native) | +5.0 (contract, TUI, RC, shellcheck, secret-scan, image-scan, docs-ci) | 9.5 | A+ |
| VisionClaw | 4.5 (clippy advisory) | +1.5 (client-test, client-lint, fixtures) | 6.0 | B+ |
| dreamlab-ai-website | 4.5 (audit advisory, test advisory) | +1.0 (build check, dual-stack) | 5.5 | B |

## 5. Recommended Next Steps

1. **Immediate**: Enable branch protection on all 5 repos pointing at the `CI required` / `CI passed` aggregator job.
2. **Sprint N+1**: Add gitleaks secret scanning to VisionClaw, nostr-rust-forum, solid-pod-rs, dreamlab-ai-website (copy agentbox pattern).
3. **Sprint N+1**: Add cargo-deny + cargo-audit jobs to dreamlab-ai-website for the forum-config overlay.
4. **Sprint N+2**: Clear VisionClaw clippy backlog and promote to `-D warnings`.
5. **Sprint N+2**: Add coverage reporting (tarpaulin) to VisionClaw and nostr-rust-forum.
6. **Sprint N+3**: Implement ADR-077 P4 mutation testing (weekly cron, cargo-mutants/stryker).
7. **Sprint N+4**: Implement ADR-077 P2 Level-2 cross-substrate contract tests (nightly cron).
