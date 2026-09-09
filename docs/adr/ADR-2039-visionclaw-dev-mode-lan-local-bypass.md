---
id: ADR-2039
title: "VISIONCLAW_DEV_MODE — a peer-agnostic LAN-local full auth bypass for a 100%-local dev headset"
date: 2026-09-01
decision_status: proposed
implementation_status: complete
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit:
verified_paths: [src/utils/auth.rs, src/settings/auth_extractor.rs, src/handlers/socket_flow_handler/filter_auth.rs, src/main.rs, docker-compose.unified.yml]
owner: jjohare
review_trigger: any change to verify_access, the dev-auth feature gates, enforce_release_env_hygiene, the WS authenticate handler, or the dev compose environment block
repo: visionclaw
domain: SECURITY-profiles
lineage: ADR-06 §D11 (release refuses dev env/argv), ADR-2012 (Bearer dev-session-token triple-gated to loopback+opt-in), ADR-2037 (release images built without dev-auth, CI-asserted), ADR-142 (RBAC lattice), ADR-060 (pubkey visibility filter).
---

# ADR-2039 — VISIONCLAW_DEV_MODE: a peer-agnostic LAN-local full auth bypass for a 100%-local dev headset

## Context

The XR headset runs entirely on the LAN: the Godot client on HP-Desktop connects
over the 25 G rail to the backend in `visionclaw_container` (`ws://192.168.2.132:4000`).
Every graph *write* (layout-DAG trigger, 3D-view toggle, node drag, settings) is
gated behind NIP-98 Schnorr auth via `verify_access`. The client's NIP-98 signing
has a standing u-tag URL-mismatch bug, so all server-write HUD actions return 403 —
node sizing and reads work, writes never have. On a single-operator, 100%-local dev
headset, per-request Nostr signing is pure friction with no security benefit: the
LAN itself is the trust boundary.

The existing dev shortcut (`Bearer dev-session-token`, ADR-2012) cannot serve this
case. It is triple-gated on compile + `DEV_AUTH_LOOPBACK=1` + a **loopback peer**,
and the HP client is not loopback. Worse, Docker port-publishing SNATs the source:
the backend sees the bridge-gateway address, not the real HP, so neither a loopback
check nor a LAN-CIDR allow-list can express "trust my headset". A peer-based gate is
structurally the wrong tool here.

## Decision

Introduce `VISIONCLAW_DEV_MODE` — a **peer-agnostic** full bypass. When active,
`verify_access` returns an authenticated dev-admin principal
(`DEV_MODE_PUBKEY = "dev-mode-local-admin"`) for **every** request with no NIP-98
signature, dev-token header, or peer-origin check, granting any required
`AccessLevel` (incl. WriteGraph/Admin). The same grant is mirrored at the two
auth decision points that do not route through `verify_access`: the settings
`FromRequest` extractor (`try_dev_bypass`) and the WebSocket `authenticate`
handler (which grants `is_power_user` when the client sends its `authenticate`
frame — the Godot client always does — so the owner's private nodes render and
WS-side writes are ungated). The REST 403-ing HUD buttons (layout-DAG, 3D-view,
node drag) are covered by the `verify_access` return *unconditionally*; the WS
grant is not on the critical path for them.

Safety is by **construction, not by runtime check** — the same two-layer model as
ADR-2012/ADR-2037, minus the peer gate:

1. **Compile-time.** The reading code exists only under
   `#[cfg(any(debug_assertions, feature = "dev-auth"))]`. Release builds get an
   `#[inline(always)] fn dev_full_bypass_active() -> bool { false }` stub — the
   env var is unreadable and the bypass unreachable. ADR-2037 proposes a CI assertion that production images omit `dev-auth`;
   until it is implemented and tied to the shipped artefact, that guarantee
   remains open.
2. **Boot refusal.** `VISIONCLAW_DEV_MODE` is already in `enforce_release_env_hygiene`'s
   `SUSPECT_ENVS` (ADR-06 §D11): a release binary that merely *sees* the var
   present hard-fails boot (exit 2). Promoting a dev config to prod cannot start.

Deliberately **peer-agnostic**: since SNAT hides the real origin, the safety comes
from the compile-gate + boot-refusal, not from a peer allow-list. Default OFF —
the dev compose block sets `VISIONCLAW_DEV_MODE: "${VISIONCLAW_DEV_MODE:-0}"`;
the operator opts in with `=1`. When armed in a dev build, a loud multi-line boot
banner is logged — an unauthenticated write door must never be silent.

## Consequences

- **The HP headset's write buttons work** with zero client change: the backend
  ignores the client's broken NIP-98 header entirely in dev mode. The u-tag
  signing bug is now orthogonal to using the headset (it still wants fixing for
  any non-dev-mode / remote deployment — tracked separately).
- Writes in dev mode are attributed to the sentinel `dev-mode-local-admin` pubkey,
  so provenance/audit rows are unambiguous about which writes came in
  unauthenticated. It is not a real Nostr key.
- **New dev attack surface, bounded to dev builds:** with the flag on, anyone who
  can reach the port is admin. This is the *intended* LAN-local posture, but it
  means the dev port must never be exposed beyond the trusted LAN. The boot banner
  says so; the compile-gate + boot-refusal keep it out of production.
- One env var now has two independent defences (compile-strip AND boot-refuse);
  changing either the feature gate or the `SUSPECT_ENVS` list must keep both in
  lockstep — hence the review trigger.
- Cost: a third dev-bypass concept alongside `SETTINGS_AUTH_BYPASS` and the
  dev-session-token. Mitigated by routing all three through one helper
  (`dev_full_bypass_active`) and one sentinel principal.

## Verification

Implementation is complete and compiles under `--features dev-auth`
(`cargo check` green). Unit test `dev_full_bypass_respects_env_flag`
(`src/utils/auth.rs`) asserts only `1`/`true` (case/space-insensitive) arm the
flag; unset/`0`/other are off. The release stub returns `false` unconditionally
and `enforce_release_env_hygiene` already lists `VISIONCLAW_DEV_MODE`
(`src/main.rs`). Decision is `proposed` — the owner ratifies; `verified_commit`
is intentionally empty until then (an armed staleness gate would flag its own
arming commit). On acceptance, land the runtime confirmation (headset write
buttons succeed end-to-end with `VISIONCLAW_DEV_MODE=1`) and populate
`verified_commit` at the SHA whose governed paths are unchanged since.
Governing doc: `docs/SECURITY-profiles.md`. Cross-ref ADR-2012, ADR-2037, ADR-06 §D11.

**Adversarial review (codex, GPT, 2026-09-01).** Two findings, both adjudicated:
- *[High] "Compose makes release deployments unbootable"* — **false positive**,
  verified against surrounding code the reviewer's sandbox could not read: the
  `VISIONCLAW_DEV_MODE` line is in the **dev service only**, not the
  `*common-environment` anchor (which carries no auth vars) and not the prod
  service, so the always-present `:-0` default reaches only the dev build (which
  does not boot-refuse). No release deployment is newly unbootable. The one real
  interaction — a `.env` shared with a prod deploy triggering the §D11 boot-refuse
  — is the intended fail-safe and is now documented in the compose comment.
- *[Medium] "WS bypass fires on the `authenticate` frame, not on connect"* —
  **valid**, wording corrected here and in `filter_auth.rs`. Functionally
  immaterial (the client always authenticates; REST writes are covered
  unconditionally) but the spec now matches the code.
Confirmed-correct by the same review: default-off env parsing, the release stub's
inability to activate, and `verify_access` granting every level before role
resolution.

## Closeout extension — 2026-09-04

CP-01/04/06/08. Owner remains jjohare with release/authentication maintainers. The scoped implemented bypass and existing decision/activation declarations are retained. Nine extracted-helper runs verify conditional compilation and dev-mode behaviour, not shipped-image stripping or headset operation. ADR-2037 is proposed; its release-image assertion cannot yet be claimed as a guaranteed dependency.

**Acceptance condition:** Bind image digest, source, feature closure and effective profile to a receipt. Test production rejection before listener bind, including forbidden variables set to zero, and prevent promotion of a dev-auth artefact. Exercise full REST and WebSocket paths, report-mode interaction, network reachability and sentinel attribution separately from helper parsing. Preserve the distinction between the peer-agnostic full bypass and loopback dev-token mechanism. Reopen on build features, boot sequencing, bypass branches or profile policy. See the [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md#development-bypass-and-release-identity), [reproducer](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/dev-auth-probe.py) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/dev-auth-probe.json). No full image, listener, HTTP or headset execution ran.

## Amendment — 2026-09-08 (ADR-2108)

The "default OFF" clause of the Decision is amended by ADR-2108: the dev compose
service now defaults `VISIONCLAW_DEV_MODE` to `1` (`${VISIONCLAW_DEV_MODE:-1}`),
because the observed state on 2026-09-08 was a dev backend running with the flag
at `0` and every server-routed HUD write from the HP headset rejected. The
compile-gate, the release boot-refusal, the dev-service-only scoping and the
`.env` opt-out are unchanged.
