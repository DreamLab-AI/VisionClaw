---
id: ADR-2108
title: Dev compose profile arms VISIONCLAW_DEV_MODE by default
date: 2026-09-08
decision_status: proposed
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit:
verified_paths: [docker-compose.unified.yml, src/utils/auth.rs, src/settings/auth_extractor.rs, xr-client/scripts/graph_scene.gd, xr-client/scripts/hud.gd]
owner: jjohare
review_trigger: any change to the dev compose environment block, dev_full_bypass_active, enforce_release_env_hygiene, or the XR client's _auth_headers / _on_physics_completed
repo: visionclaw
---

# ADR-2108 — Dev compose profile arms VISIONCLAW_DEV_MODE by default

## Context
ADR-2039 added `VISIONCLAW_DEV_MODE`, the peer-agnostic full auth bypass built
for the HP headset, but left it default-off (`${VISIONCLAW_DEV_MODE:-0}` in the
dev service). On 2026-09-08 the dev backend (`visionclaw_container`) was observed
running with the flag at `0`. Every server-routed HUD write from the headset —
View 3D/Flat, Hierarchy, Shells, Spread, Planes, Radial, Layout Mode, Reset —
was therefore rejected: the client's dev-bearer fallback needs a loopback peer
(the HP never is, ADR-2012), and no power-user `XR_NOSTR_SECRET` was provisioned.
The client dropped each rejection into `push_warning`, invisible in-headset.
Client-side buttons (Edges, Node size, Fold, type toggles) kept working, which
is the fingerprint of an auth gate rather than a dead HUD.

## Decision
1. The `dev` compose service defaults `VISIONCLAW_DEV_MODE` to `1`
   (`${VISIONCLAW_DEV_MODE:-1}`). `launch.sh up dev` therefore boots an armed
   dev backend; the operator opts **out** with `VISIONCLAW_DEV_MODE=0`. The
   scoping is unchanged: the line lives in the dev service only, never in the
   `*common-environment` anchor or the production service; release binaries
   still `#[cfg]`-strip the codepath and hard-fail boot on the var's presence.
2. The XR client surfaces a rejected server write instead of swallowing it:
   `graph_scene._describe_write_failure` names the HTTP status, the credential
   path used, and the remedy (`VISIONCLAW_DEV_MODE=1` on the dev backend, or a
   power-user `XR_NOSTR_SECRET`); `hud.flash_notice` shows it on the persistent
   bottom strip on every tab, and the Graph-tab status line appends it.

## Consequences
- A dev backend is admin-for-everyone by default. This is the intended posture
  of ADR-2039 (the LAN is the trust boundary), now the default rather than an
  opt-in; the dev port must never be exposed beyond the trusted LAN. The boot
  banner remains loud.
- ADR-2039's "default OFF" clause is amended (note appended there); its
  compile-gate, boot-refusal and `.env`-sharing caveat are untouched.
- An explicit `VISIONCLAW_DEV_MODE=0` in the project `.env` still wins over the
  compose default (compose interpolation reads it), so a deliberately hardened
  dev host stays hardened.
- The next auth misconfiguration is diagnosable from inside the HMD.

## Verification
- `docker exec visionclaw_container env | grep VISIONCLAW_DEV_MODE` returned `0`
  before the change (2026-09-08); the compose default is now `:-1`.
- Server codepaths unchanged and already unit-tested
  (`dev_full_bypass_respects_env_flag`, `src/utils/auth.rs`).
- Client: `xr-client/tests/unit/test_write_denied_notice.gd` (gate reopens,
  stage discarded, remedy recorded on 401/403/transport failure, cleared on 2xx)
  and `test_hud_tabs.gd::test_flash_notice_overrides_hint_then_expires` /
  `test_key_tab_lists_swatch_rows`. GUT runs in CI / a live Godot session (no
  Godot binary in the authoring container).
- Runtime confirmation pending: after `launch.sh up dev`, the boot banner reports
  dev mode armed and an unauthenticated `PUT /api/settings/physics` from a
  non-loopback peer returns 2xx; then a View 3D/Flat press on the HP headset
  flips the button face. Populate `verified_commit` on that receipt.

## Disposition — 2026-10-02

- **Suitability:** fits, as a dev-only default. The revision the triage asked for is supplied by the owner (below) and recorded in the amendment that follows.
- **Priority:** P1 — this cycle (§9 demonstrator path on the owner's own rig). It is no longer a Track C or CY-C item.
- **Owner decisions 2026-10-02, Q1 and Q3:** client hardware runs the **prod profile**. The Trust residential is never brought up with `launch.sh up dev`, so the "LAN is the trust boundary" premise never reaches a client's network. The headset there writes with an Owner/Admin `XR_NOSTR_SECRET` over NIP-98, which validates behind the prod nginx since `e7e6b61d8`.
- **Why:** On the owner's own rig the decision stands. `docker-compose.unified.yml:90` reads `${VISIONCLAW_DEV_MODE:-1}`, and the XR write-denied notice shipped in `a37433f5d`. Since `e7e6b61d8` the notice names an Owner/Admin key as the remedy and marks dev mode as dev-only. The prod path forbids the flag (`scripts/launch.sh:165`).
- **Next:** Take the joint runtime receipt with ADR-2039 on the dev rig. The record is then ready to accept.

## Amendment — 2026-10-02 (owner decision, Q1)

Decision 1 applies to the owner's own development rig only. A deployment on client hardware, starting with the Trust residential, uses the prod profile (`launch.sh up prod`), where `VISIONCLAW_DEV_MODE` is refused at launch and at boot. The armed-by-default dev profile must never be brought up on a client's network.
