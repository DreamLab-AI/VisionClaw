# ADR-083 — `dreamlab-ai-website` Cutover Migration Pattern

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-011 G3 + G8 + R5 (cutover safety), Phase X5 |
| Companion ADRs | ADR-073, ADR-074, ADR-075, ADR-076, ADR-077, ADR-078, ADR-079, ADR-080, ADR-081, ADR-082 |
| Companion PRDs | PRD-010, PRD-011 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` |
| Affected repos | `dreamlab-ai-website`, `nostr-rust-forum` |

## Context

PRD-011 specifies the extraction of the VisionFlow forum kit (`nostr-bbs-rs`, hosted at `DreamLab-AI/nostr-rust-forum`) from the `dreamlab-ai-website` monorepo. The clean-architecture endpoint (PRD-011 §5.4 + Q3:a) is:
- Forum kit at `DreamLab-AI/nostr-rust-forum` is the canonical upstream.
- DreamLab's website at `DreamLab-AI/dreamlab-ai-website` becomes a downstream consumer with a new `forum-config/` package.
- `dreamlab-ai-website/community-forum-rs/` is **deleted** at cutover.

The risk surface (PRD-011 R5: HIGH) is that this is a **production cutover** — the live `https://dreamlab-ai.com/community/` forum carries:
- Real users with passkey-PRF nsecs (irreplaceable; lost on bad migration)
- Real Nostr events on the relay (D1 events table; backfill-recoverable but disruptive)
- Real Solid pods (R2 buckets; recoverable but downtime-sensitive)
- Real mod actions (D1 moderation_actions; visible loss = trust regression)
- Real kind-0 profiles (D1 profiles table; backfill-recoverable per Sprint v10)

This ADR specifies the **cutover pattern** that ensures PRD-011 M5 (zero data loss, zero session loss, ≤1% user-visible regression). It locks in the feature-flag traffic split, dual-deploy parallel window, data-migration scripts, session-handling strategy, rollback plan, and reconciliation tooling.

## Decision

### D1 — Cutover phases

```
T₀: Pre-cutover baseline established
T₁: Forum kit v3.0.0-rc1 deployed to staging (dreamlab-staging.com)
T₂: forum-config/ package built; smoke tested against staging
T₃: Production traffic split: 5% to new stack (canary)
T₄: 50% split (parity validation)
T₅: 95% split + reconciliation
T₆: 100% on new stack; old stack quiesced
T₇: community-forum-rs/ subdirectory deleted; PR merged
T₈: Post-cutover audit + Sprint v12-Z (cleanup)
```

Total elapsed time T₀ → T₆: **~14 days**. T₆ → T₇: **+7 days observation**. T₇ → T₈: **+7 days audit**.

### D2 — Feature-flag traffic split

A Cloudflare Worker (`router-worker`) sits in front of forum traffic and routes per-request:

```
Request → router-worker
  ↓
  Read CF Worker var ROUTING_MODE: "old-only" | "new-canary" | "new-50" | "new-95" | "new-only"
  Read pubkey from Cookie / NIP-98 header
  ↓
  match (ROUTING_MODE, pubkey_in_canary_set):
    (old-only, _)               → forward to old worker
    (new-canary, true)          → forward to new worker  (canary cohort only)
    (new-canary, false)         → forward to old worker
    (new-50, hash(pk) % 100 < 50) → forward to new
    (new-50, otherwise)         → forward to old
    (new-95, hash(pk) % 100 < 95) → forward to new
    (new-95, otherwise)         → forward to old
    (new-only, _)               → forward to new worker
  ↓
  Append response header X-Forum-Stack: <old|new>
  Forward to client
```

Operator runs `wrangler secret put ROUTING_MODE` at each transition. CF Workers Durable Object propagation is ~30s; cuts can be near-instant.

`hash(pk) % 100` is the consistent-hash cohort assignment — same pubkey always lands on same stack within a phase, eliminating per-request thrashing.

### D3 — Dual-deploy parallel architecture

During T₃-T₆ (the parallel window), **both** old and new forum stacks run simultaneously. They share:
- **R2 bucket `dreamlab-pods`**: both write to same Solid pods. Concurrent-write detection via ETag. Most pod writes are user-scoped + low-velocity; conflicts are rare.
- **D1 database `dreamlab-relay`**: both read/write same events table. Schema is identical between old and new (kit v3.0 inherits Sprint v9-v11 schema). Both honour same id-uniqueness constraints.
- **D1 database `dreamlab-auth`**: both read/write same passkey/username tables.
- **KV namespaces**: shared between old and new. NIP98_REPLAY in particular MUST be shared so a token validated by old worker is rejected if replayed against new.

Both stacks emit Prometheus metrics with `stack=old|new` label so operators can monitor parity in real-time.

### D4 — Schema compatibility invariant

The forum kit v3.0 SHALL preserve the existing D1 schema unchanged. Specifically:
- Table `events` schema unchanged (all columns same; same indexes).
- Table `whitelist` unchanged.
- Table `moderation_actions` unchanged.
- Table `reports` unchanged.
- Table `hidden_events` unchanged.
- Table `profiles` unchanged.
- Table `channel_zones` unchanged.
- KV `NIP98_REPLAY` keying scheme unchanged (canonical event id).
- KV `SESSIONS` schema unchanged.

If the kit's evolution requires schema changes, those land in **forum kit v3.1.x** AFTER the cutover settles — not before. PRD-011 G6 + this ADR's M2 success metric jointly require pre-cutover schema parity.

### D5 — Pubkey-bound session continuity

Forum sessions are `(pubkey, expires_at)` tuples in KV `SESSIONS`. A user signing in via passkey-PRF on the old stack is identified by `did:nostr:<hex>` regardless of stack — the new stack reads the same SESSIONS KV and accepts the existing session.

WebAuthn rp_id and expected_origin are **identical** between old and new stacks (`dreamlab-ai.com`) so the browser presents the same passkey. PRF derivation info string is **identical** (`"nostr-secp256k1-v1"`) so the same passkey + PRF salt produces the same nsec.

Cookie / Authorization header parsing is identical (NIP-98 + Bearer hybrid).

Net effect: **zero session loss** during cutover. A user mid-thread on old stack moves to new stack with no re-login.

### D6 — Deviation classification + rollback triggers

Every cutover phase emits a deviation report — comparison of old vs new stack on the same input:

```bash
# tests/cutover/parity-check.sh
for endpoint in /api/profile /api/profiles/batch /api/setup-status /community/; do
  curl -s "https://old-router.dreamlab-ai.com$endpoint" > old-${endpoint//\//-}.json
  curl -s "https://new-router.dreamlab-ai.com$endpoint" > new-${endpoint//\//-}.json
  diff old-${endpoint//\//-}.json new-${endpoint//\//-}.json
done
```

Deviations classified:
- **Match-exactly**: response byte-equal. Most endpoints.
- **Match-modulo-noise**: differs only in irrelevant timestamps / request IDs. Acceptable.
- **Behaviour-divergent**: e.g. old returns 200, new returns 500. **ROLLBACK TRIGGER.**
- **Schema-incompatible**: missing fields / different types. **ROLLBACK TRIGGER.**
- **User-visible regression**: e.g. profile picture missing, thread mod-state wrong. **ROLLBACK TRIGGER on >1% rate.**

Rollback: `wrangler secret put ROUTING_MODE old-only` reverts to 100% old in <60s.

### D7 — Migration scripts: data + branding

The cutover does **not** migrate data — D3 establishes that both stacks share storage. Data migration is moot.

What IS migrated: **branding configuration** from old stack to new stack's `dreamlab.toml`. This happens once at T₂ (forum-config/ package build). Specifically:

```bash
# Migration helper at: dreamlab-ai-website/scripts/extract-branding.sh
# Reads existing constants from community-forum-rs and emits a canonical dreamlab.toml.

cat > forum-config/dreamlab.toml <<EOF
[deployment]
name = "DreamLab Community"
hostname = "https://dreamlab-ai.com"
contact = "ops@dreamlab-ai.com"

[webauthn]
rp_id = "dreamlab-ai.com"
expected_origin = "https://dreamlab-ai.com"

[pod]
base_url = "https://pods.dreamlab-ai.com"
storage_backend = "cf-r2"

[relay]
url = "wss://dreamlab-nostr-relay.solitary-paper-764d.workers.dev"
ingress_policy = "allowlist"

[admin]
mode = "static"
static_pubkeys = [
  $(cat community-forum-rs/admin-pubkeys.toml | jq -r '.pubkeys[]' | sed 's/^/  "/' | sed 's/$/",/')
]

# ... [zones], [trust], [invites], [moderation], [branding] sections similarly

EOF
```

Manual review at T₂ to verify:
- Admin pubkeys preserved exactly
- Zone definitions preserved (`lobby`, `members`, `trusted` cohort names)
- Trust progression thresholds preserved
- Branding text/logos preserved
- Welcome bot pubkey preserved

### D8 — Reconciliation tooling

`nostr-bbs-migrate` CLI (NEW; per PRD-011 §5.5 + this ADR D8):

```
nostr-bbs-migrate scan-divergence \
  --old-deployment=https://old-router.dreamlab-ai.com \
  --new-deployment=https://new-router.dreamlab-ai.com \
  --since=<unix-ts>

nostr-bbs-migrate reconcile-events \
  --old-deployment=... \
  --new-deployment=... \
  --since=<unix-ts> \
  --max-events=10000

nostr-bbs-migrate verify-pubkey-coverage \
  --old-deployment=... \
  --new-deployment=...
```

The tool's job:
1. Identify pubkeys that have published events on the old stack but not on the new.
2. For each: check if their session is still valid + correctly migrated.
3. Identify events with mismatched mod-state across stacks.
4. Identify pod resources with version mismatches.

Used during T₃-T₆ to surface anomalies; used at T₈ (post-cutover audit) to confirm zero data loss.

### D9 — Rollback plan + matrix

| Failure scenario | Detection | Rollback path | Time-to-recover |
|------------------|-----------|---------------|-----------------|
| Catastrophic 5xx spike in new stack | Prometheus alert on `errors_total{stack="new"} > 1%` | `wrangler secret put ROUTING_MODE old-only` | <60s |
| Session loss for >0.1% of users | session_drop_total{stack="new"} alert | router-worker reverts to old for affected cohorts | <60s |
| Schema corruption suspected | manual; data-integrity tests | `ROUTING_MODE=old-only` + freeze new-stack writes via R/O wrangler env | <90s |
| WebAuthn passkey breakage | `passkey_login_failure_rate{stack="new"} > 0.05%` sustained ≥5 min OR `> 0.5%` absolute in any 1-min window | rollback + emergency communication | <60s + ~2hr postmortem |
| Pod ACL drift | nightly ACL parity scanner — trigger on `pod_acl_mismatch_count >= 1` (alert) AND `pod_acl_mismatch_count >= 5` (rollback) on any 1-hour rolling window | replay ACLs from old-stack write log | ~30 min |
| Subtle behaviour regression (>1%) | nightly cutover/parity-check.sh — trigger on `parity_divergence_pct{endpoint=*} > 1%` for ≥2 consecutive runs | `ROUTING_MODE` step down (95→50→canary→old-only) | per-phase ~1hr |
| Catastrophic data loss | irrecoverable via dual-deploy because they share storage | last weekly R2 snapshot + D1 export | ≥4hr (not acceptable; D3 dual-deploy specifically mitigates this) |

Rollback drill at T₂: operator runs each rollback path in staging; documents elapsed time; refines runbooks.

### D10 — User-visible communication plan

The cutover is **transparent to users by default**. No banners, no explanations, no scheduled downtime communications. Users see no UI difference because:
- D5 session continuity → no re-login.
- D4 schema parity → no missing data.
- Branding preserved per D7 → identical UX.

If a regression is detected and rolled back partially, users on the affected cohort see no change because router-worker reverts them silently.

If a public-visible incident occurs (rare; rollback should prevent), DreamLab posts to `https://status.dreamlab-ai.com` with severity + ETA. No social-media campaign required.

Forum admins receive briefing email at T₃ (canary start) and T₆ (cutover complete). Admin tooling documentation updated to reflect new `dreamlab.toml` location.

### D11 — Cutover checklist (pre-flight)

Before T₃ traffic split begins:

```
[ ] Forum kit v3.0.0 GA tagged on nostr-rust-forum (PRD-011 Phase X6)
[ ] Cross-substrate fixture sync verified (ADR-082)
[ ] L2 contract tests passing for ≥7 consecutive nights
[ ] Federation smoke tests (ADR-077 P2 L3) passing for ≥7 consecutive nights
[ ] dreamlab-ai-website forum-config/ package built + tested
[ ] forum-config/dreamlab.toml manually reviewed by ops + admin team
[ ] Schema parity confirmed (D4)
[ ] Session continuity tested in staging (10+ test users)
[ ] Rollback drill completed in staging (D9)
[ ] On-call coverage scheduled for T₃ + T₄ + T₅ + T₆
[ ] PagerDuty alerts configured: errors_total, session_drop_total, parity violations
[ ] tests/cutover/parity-check.sh CI runs pass for all monitored endpoints
[ ] router-worker CF Worker deployed to production
[ ] OldRouter URL preserved (https://old-router.dreamlab-ai.com)
[ ] NewRouter URL preserved (https://new-router.dreamlab-ai.com)
[ ] Stakeholder briefing complete (DreamLab leadership, forum admins, on-call)
```

### D12 — T₆ → T₇ → T₈: deletion + audit

T₆ + 7 days: at this point new stack has been 100% production for a week with zero rollback triggers. `dreamlab-ai-website/community-forum-rs/` is deleted via PR:

```
git rm -r community-forum-rs/
git commit -m "chore: remove community-forum-rs/ — superseded by forum-config/ + nostr-bbs-rs Cargo deps (PRD-011 Phase X7)"
```

PR includes:
- Reference to PRD-011 Phase X5 + this ADR.
- List of Cargo dep replacements: `nostr-bbs-core`, `nostr-bbs-relay-worker`, etc.
- Confirmation that no symlinks, scripts, or external references to `community-forum-rs/` remain.

T₇ + 7 days: post-cutover audit complete. Closeout entry in `docs/operations/cutover-log.md`.

### D13 — Phase 0 dependency

This ADR's cutover is GATED on PRD-010 Phase 0 + PRD-011 Phase X1-X4 + ADR-076 absorption + ADR-081/082 deliverables. Cannot proceed until:
- C1 NIP-44 conversation key bug fixed (per PRD-010 P0 / ADR-076)
- C2 agentbox bech32 npub fixed (per PRD-010 P0 / ADR-074)
- C3 verificationMethod.type drift fixed (per PRD-010 P0)
- ADR-080 deployment topology selected (per PRD-011 Phase X4)
- ADR-082 fixture-sharing protocol active in CI (≥7 days clean)
- ADR-081 federation key custody protocols implemented (operator key custody decided)

If any of these slip, cutover slips.

## Consequences

### Positive

- **Zero-downtime cutover** achievable via dual-deploy + traffic split.
- **Zero session loss** by design — passkey-PRF + identical rp_id + shared SESSIONS KV.
- **Reversible at every phase**: rollback path documented + drilled.
- **Clean architecture endpoint**: no forks, no community-forum-rs/, single source of truth at nostr-rust-forum.
- **Data preserved by sharing storage** (D3) — the migration is purely deployment-layer, not data-layer.
- **Audit trail preserved**: parity reports + reconciliation logs in `docs/operations/cutover-log.md`.

### Negative

- **Operational complexity**: dual-deploy + router-worker + parity monitoring is a 3-week+ operations effort for the cutover team.
- **CF Workers cost during dual-deploy**: both old and new emit metrics; both consume KV/D1/R2; ~2x runtime cost for the parallel window.
- **Risk of lurking schema-coupling bug**: if new stack assumes some schema behaviour old stack didn't, cutover mid-phase could surface it. Mitigation: D11 schema-parity tests + per-phase validation.
- **Branding extraction may miss subtle DreamLab-specific behaviour**: e.g. a custom NIP-XX kind that DreamLab uses but isn't in the kit's standard. Mitigation: T₂ manual review + parity tests.
- **PR to delete community-forum-rs/** carries 7-day delay for safety.

### Neutral

- **No user communication needed** for happy path.
- **Operator runbook adds 10+ pages**: acceptable given scale of cutover.

## Alternatives Considered

### Alt-A — Big-bang cutover
Switch all traffic at T₃ in one step.

*Rejected*: PRD-011 R5 HIGH explicitly forbids. Big-bang has no canary; first regression seen by 100% of users.

### Alt-B — Long-running gradual ramp (months)
Slow drift from 0% to 100% over months instead of days.

*Rejected*: extends operational complexity; longer dual-deploy CF cost; longer time to validate kit GA. The 14-day window balances safety vs cost.

### Alt-C — Data migration (events table) via export/import
Export old D1 events, import to new D1 events.

*Rejected*: introduces data-divergence risk during ramp. D3 share-storage avoids this.

### Alt-D — Frozen old stack (read-only) + new stack write-only
At T₃, freeze old stack to read-only; route all writes to new.

*Rejected*: loses canary safety. New stack has zero rollback if old can't accept writes.

### Alt-E — Kit deployed as separate domain (forum-v2.dreamlab-ai.com)
Run kit in parallel at a new URL; no cutover.

*Rejected*: defeats the consolidation goal; requires user re-login at the new URL; bookmarks break; SEO regression.

## Implementation notes

### Router-worker CF Worker

A new minimal CF Worker (`router-worker`) acts as the traffic split. ~50 LOC Rust:

```rust
#[event(fetch)]
pub async fn main(req: Request, env: Env) -> Result<Response> {
    let routing_mode = env.var("ROUTING_MODE")?.to_string();
    let cohort = compute_cohort_from_pubkey(&req).await?;
    
    let target = match (routing_mode.as_str(), cohort) {
        ("old-only", _) => "https://old-forum.dreamlab-ai.com",
        ("new-canary", InCanarySet) => "https://new-forum.dreamlab-ai.com",
        ("new-canary", _) => "https://old-forum.dreamlab-ai.com",
        ("new-50", x) if x % 100 < 50 => "https://new-forum.dreamlab-ai.com",
        ("new-95", x) if x % 100 < 95 => "https://new-forum.dreamlab-ai.com",
        ("new-only", _) => "https://new-forum.dreamlab-ai.com",
        _ => "https://old-forum.dreamlab-ai.com",
    };
    
    let response = Fetch::Url(target.parse()?).send().await?;
    response.headers_mut().set("X-Forum-Stack", routing_mode.split('-').last().unwrap_or("old"))?;
    Ok(response)
}
```

### Data parity monitoring

Per-endpoint tests per phase:

```bash
# tests/cutover/api-parity.sh — runs every 5min during T₃-T₆
ENDPOINTS=( /api/setup-status /api/profile /api/profiles/batch /api/whitelist/list /api/reports )
PUBKEYS=( $(cat tests/cutover/sample-pubkeys.txt) )
for endpoint in "${ENDPOINTS[@]}"; do
  for pk in "${PUBKEYS[@]}"; do
    OLD=$(curl -s "https://old-router.../$endpoint" -H "Cookie: pubkey=$pk")
    NEW=$(curl -s "https://new-router.../$endpoint" -H "Cookie: pubkey=$pk")
    if [ "$OLD" != "$NEW" ]; then
      echo "DIVERGENCE: $endpoint pubkey=$pk"
    fi
  done
done
```

### CI integration

Each forum-config PR runs:
- `wrangler dev` against forum-config + spawn old + new stacks locally
- `tests/cutover/api-parity.sh --local` — verify shape parity
- `tests/cutover/schema-parity-check.sh` — verify D1 schemas match

### Stakeholder communication template

Pre-cutover email to forum admins (T₃):

```
Subject: DreamLab Forum Migration Beginning Today

DreamLab Community Forum Admins,

At <UTC time>, DreamLab will begin a migration of the forum technology stack.
This is a routine technical update — no user-visible changes are expected.

DURING THE MIGRATION (next ~14 days):
- Forum will function normally
- Sessions, messages, mods, profiles all preserved
- Two technical stacks running in parallel; users randomly assigned

WHAT YOU MIGHT NOTICE:
- HTTP response header X-Forum-Stack: old or new
- (Internally only) different metrics dashboards

WHAT WE'RE WATCHING:
- Cohort-based parity reports (5-min cadence)
- Error rates per stack
- Session continuity metrics
- Mod-action propagation

ROLLBACK READY:
- 60-second revert to all-old if any regression
- Drilled in staging; confident in process

Questions? ops@dreamlab-ai.com or #ops-tactical Slack.
```

## References

- PRD-010 — DID:Nostr Mesh Federation (cutover gated on Phase 0)
- PRD-011 — VisionFlow Forum Kit Extraction (G3, G8, R5, M5, Phase X5)
- ADR-076 — Forum `nostr-core` absorption (gating dependency)
- ADR-077 — Ecosystem QE Policy (P2 L3 federation smoke required pre-cutover)
- ADR-078 — Cross-substrate library convergence (kit dep alignment)
- ADR-079 — Forum-Setup Skill Provider Abstraction (forum-config/ uses skill)
- ADR-080 — Forum Kit Deployment Topology Patterns (D7 downstream-consumer pattern)
- ADR-081 — Federation key custody & rotation (operator/admin keys preserved across migration)
- ADR-082 — Cross-substrate test fixture sharing (must be active pre-cutover)
- `docs/integration-research/qe-fleet/Q2-security-primitive-audit.md` (security risks during migration)
- `docs/integration-research/qe-fleet/Q4-coverage-gap-audit.md` G7 (NIP-98 coordination during dual-deploy)
- Cloudflare Workers Routing docs: https://developers.cloudflare.com/workers/runtime-apis/routes-and-domains/
- GitHub repos:
  - https://github.com/DreamLab-AI/dreamlab-ai-website (cutover target)
  - https://github.com/DreamLab-AI/nostr-rust-forum (kit upstream)
