---
id: ADR-2119
title: Production ingress is declared, LAN or tunnel
date: 2026-10-02
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: b43a2a1e6d1355341b8140a161663517bbf484d3
verified_paths: [scripts/launch.sh, docker-compose.unified.yml, crates/visionclaw-integration-tests/tests/prod_ingress.rs]
owner: jjohare
review_trigger: "a third ingress (reverse proxy, VPN, public port), any change to the prod pre-flight in scripts/launch.sh, or the cloudflared service's profiles"
repo: visionclaw
domain: BASELINE-architecture
lineage: "owner decision 2026-10-02 Q1 (Trust runs the prod profile) and R2 (the tunnel may stay down; the Trust box is LAN-only); TODO-unified CY-C-D1; ADR-2037 and ADR-2039/2108 (dev bypass stays out of prod)"
---

# ADR-2119 — Production ingress is declared, LAN or tunnel

## Context
The Trust residential runs the prod profile (owner decision 2026-10-02, Q1) and is
LAN-only; its Cloudflare tunnel may stay down (R2). `launch.sh up prod` demanded a
concrete `CLOUDFLARE_TUNNEL_TOKEN` and started the whole `prod` compose profile,
which included `cloudflared` with `restart: unless-stopped` and no token check. A
LAN host could therefore not start prod honestly: it needed a placeholder token,
and that token left `cloudflared` crash-looping (TODO-unified CY-C-D1). Treating an
empty token as "no tunnel" would be worse, because a forgotten token on a tunnelled
host must keep failing loudly.

## Decision
1. A prod host declares its ingress in `.env.prod` as `VISIONCLAW_INGRESS=lan` or
   `VISIONCLAW_INGRESS=tunnel`. Any other value refuses to start. An `.env.prod`
   that does not declare it keeps the tunnel contract (the pre-ADR behaviour).
2. `cloudflared` runs only under its own compose profile, `tunnel`. `launch.sh`
   activates `prod,tunnel` for tunnel ingress and `prod` alone for LAN ingress.
   LAN `up` removes a tunnel container left from an earlier tunnel start; prod
   `down` and `clean` always include the `tunnel` profile.
3. The prod pre-flight (`up`/`restart`) requires `MANAGEMENT_API_KEY`,
   `VISIONCLAW_AGENT_KEY` and `SOLID_PROXY_SECRET_KEY` in both modes. Tunnel
   ingress also requires `CLOUDFLARE_TUNNEL_TOKEN`. LAN ingress instead requires
   an explicit `CORS_ALLOWED_ORIGINS`, because the compose default names the
   owner's public domain.
4. Both modes refuse `SETTINGS_AUTH_BYPASS`, `ALLOW_INSECURE_DEFAULTS`,
   `VISIONCLAW_DEV_MODE` and `DEV_AUTH_LOOPBACK` in `.env.prod`, even as false:
   the launcher's list now matches the release binary's `FORBIDDEN_DEV_VARS`
   (`src/config/security_profile.rs`). LAN ingress is not a dev mode.

## Consequences
What LAN ingress changes against tunnel ingress, per tunnel assumption:

| Assumption | Tunnel | LAN |
|---|---|---|
| `CLOUDFLARE_TUNNEL_TOKEN` | required | ignored (a warning if set) |
| `cloudflared` container | started | never started; a stale one is removed |
| Public hostname | the tunnel's | none; clients use `http://<host>:3001` |
| `CORS_ALLOWED_ORIGINS` | compose default allowed | must be set to the host's own origin |
| Headset transport | `wss://` via the tunnel | `XR_BACKEND_WS=ws://<host>:3001` on the LAN; NIP-98 `u`-tags sign `host:port`, which prod nginx keeps since `e7e6b61d8` (owner decision Q3) |
| `FORUM_RELAY_URL` | owner's relay by default | unchanged: outbound, not ingress; set it if the box has no internet egress |
| OAuth callbacks | none in the server | none in the server |

- LAN ingress is plain HTTP and WebSocket on the LAN. The LAN is the network
  boundary; authentication is unchanged (NIP-98, RBAC, the security profile).
- `docker compose --profile prod` run by hand no longer starts the tunnel; a
  tunnelled host started outside `launch.sh` must add `--profile tunnel`. The
  standalone `docker-compose.cloudflared.yml` is unaffected.
- Existing tunnelled `.env.prod` files keep working unchanged.

## Verification
At `verified_commit`, `cargo test -p visionclaw-integration-tests --test
prod_ingress` passes 9 hermetic cases, wired into CI's integration-contract step.
They run the real `scripts/launch.sh` against a scratch root with a fake `docker`
that records every call, so nothing is launched:
`lan_prod_without_tunnel_token_plans_a_start_without_cloudflared`,
`tunnel_prod_without_token_refuses_before_touching_docker`,
`undeclared_ingress_keeps_the_tunnel_contract`,
`tunnel_prod_with_token_starts_the_tunnel_profile`,
`lan_prod_still_refuses_every_dev_flag`,
`lan_prod_still_demands_the_secrets_and_explicit_cors`,
`unknown_ingress_value_is_refused`, `prod_down_reaches_a_leftover_tunnel` and
`cloudflared_runs_only_under_the_tunnel_profile`. Seven failed before the change
(the two refusal cases already held). `docker compose config --services` lists
`visionclaw-production` alone for `--profile prod`, and adds `cloudflared` only with
`--profile tunnel`. Activation is staged: no Trust host has started yet.

## Re-verification — 2026-10-07 at ed5644d03 (live memory cloud, ADR-2133)

**Governed change:** `docker-compose.unified.yml` adds `RUVECTOR_PG_CONNINFO` (empty default) and five `MEMORY_CLOUD_*` variables to the `visionclaw` and `visionclaw-production` environment blocks, after `FORUM_RELAY_URL`; no other key, profile, port, volume or build argument changes. **Decision unaffected.** Ingress (LAN or tunnel) is untouched; the sidecar is reached over the internal `visionclaw_network`. `verified_commit` moved to `ed5644d03`. Source reading of the diff (`git diff 20499efc6..ed5644d03` on the governed paths) plus `cargo check --lib --bins` and `cargo test --lib -- auth rbac memory_cloud` (62 + 5 pass) at the landing commit.

## Re-verification — 2026-10-07 at b43a2a1e6 (memory-cloud security review)

**Governed change:** `docker-compose.unified.yml` adds `MEMORY_CLOUD_QUERY_PER_MINUTE: ${MEMORY_CLOUD_QUERY_PER_MINUTE:-30}` to the visionclaw (after line 135) and visionclaw-production (after line 253) environment blocks; nothing else changes. Ingress declaration and ports are unchanged. The decision holds.
