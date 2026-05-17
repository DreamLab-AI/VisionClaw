# 01 — Security Perspective

Full regression audit of the sovereign-mesh sprint from a security standpoint.

## NIP-98 verification (`src/utils/nip98.rs`, `src/utils/auth.rs`)

**Replay resistance: solid.** `validate_nip98_token` binds `url` + `method` + optional `payload` SHA-256, rejects anything older than 60 s or more than 60 s in the future (clock skew), rejects the wrong event kind (27235), and `event.verify()`s the Schnorr signature against the bech32-derived x-only pubkey. `test_urls_match_rejects_different_host` confirms a token signed for `evil.com` is rejected even when the nginx `/solid/` → `/api/solid/` rewrite is in play. The only loose surface is the `urls_match` relative-path fallback (`src/utils/nip98.rs:384-426`): when the client signs a bare relative path (no host), host comparison is bypassed. This is necessary for the `/solid/` rewrite but means a token signed for a *relative* path against any host can be replayed against another VisionClaw host. Mitigation: keep the 60 s window tight (it is) and ensure all client SDKs switch to absolute URLs; document relative-path usage as dev-only.

**Body binding: partial.** `verify_nip98_auth` is called with `body=None` in `src/utils/auth.rs:197`. Any authenticated mutation that carries a body is therefore NOT hash-bound at the primary NIP-98 verifier — the payload-hash tag, if present in the client event, is ignored by the server-side check. `solid_proxy_handler::extract_user_identity` has the same gap (line 368). **Finding S1 (HIGH)**: a valid NIP-98 token can be paired with a different body and pass. The primitive supports this (the tests prove it) but the callers do not. Add body threading from the Actix handler down into `verify_nip98_auth` for all write verbs.

## Legacy `X-Nostr-Pubkey` / `X-Nostr-Token` gate

Gated by `APP_ENV == "production"` at `src/utils/auth.rs:246-257`. A misconfigured or absent `APP_ENV` **fails open** (dev behaviour). **Finding S2 (HIGH)**: prod must make the default fail-closed. Either flip the default (`.map(...).unwrap_or(true)` → treat as production when unset) or make container entrypoints refuse to start without an explicit `APP_ENV`. The dev-mode `Bearer dev-session-token` bypass at `src/utils/auth.rs:135-165` has the same unset-default exposure; grants `Admin`-equivalent access on the strength of an untrusted `X-Nostr-Pubkey` header.

## Server-Nostr identity (`src/services/server_identity.rs`)

Secret handling is clean. `from_env` loads hex / `nsec1` via `parse_secret_key`, logs **only** the public key (hex + npub), and the private key lives inside `Keys` behind `Arc`. `parse_relay_urls` rejects anything that is not `ws://` / `wss://`, which closes an obvious SSRF-via-env vector. Production refuses to start without `SERVER_NOSTR_PRIVKEY`; dev falls back to ephemeral generate when `SERVER_NOSTR_AUTO_GENERATE=true`. No handler returns the privkey; the only exposure surface is the `/api/server/identity` route which hands out the **public** identity. `sign_and_broadcast` never touches the privkey through the tokio `client.send_event` path — events are signed in-process, then the network layer sees only signed bytes.

## `/ontology-agent/*` gating

Wrapped with `RequireAuth::authenticated()` at `src/handlers/ontology_agent_handler.rs:319-327`. ADR-028-ext compliance criterion met. No bypass paths observed in `main.rs:808` (`configure_ontology_agent_routes`) — the scope is built inside the route configurator, so the wrap is structural and cannot be dropped without editing this file.

## Double-gated Pod writes (`src/handlers/solid_proxy_handler.rs:191-218`)

Both gates must pass: path prefix `public/kg/` **and** either `X-VisionClaw-Visibility: public` header or a `public:: true` line in the body. `body_marks_public` accepts several syntaxes (`public:: true`, `"public": true`, case-insensitive). **Finding S3 (MEDIUM)**: the body-marker matcher will accept a `public:: true` *anywhere* in the body, including inside a quoted code block or a bullet body — this is laxer than the parser's line-anchored page-property rule (`src/services/parsers/visibility.rs`). The result is that a user could sync a private document that contains a bullet `- public:: true` (e.g. a Logseq cheat-sheet note), classify it as private at ingest time, but still publish it via a naive client that posts the raw body to `/public/kg/`. Align `body_marks_public` with `classify_visibility`'s scan-until-first-bullet contract.

## HMAC opaque_id (`src/utils/opaque_id.rs`)

Construction is textbook: `HMAC-SHA256(salt, owner || '|' || iri)` truncated to 12 bytes (96 bits). Key length is validated at `from_env` (≥16 chars). Day-quantised salt derivation (`derive_salt`) with a 48 h dual-salt window. **Finding S4 (MEDIUM)**: if `OPAQUE_ID_SALT_SEED` leaks, an attacker who also compromises a dump of `(owner_pubkey, canonical_iri)` pairs (Neo4j backup, say) can recompute every opaque_id for the current **and** prior day and longitudinally link them. The rotation mitigates forward damage but not historical. Consider also hashing a per-node ephemeral nonce (e.g. a coarse session id) if longitudinal unlinkability matters — v1 docs accept this trade-off; ensure it is called out in the ADR-050 threat model section.

## solid-pod-rs WAC (`crates/solid-pod-rs/src/wac.rs`)

`evaluate_access` is a linear scan over authorizations that short-circuits on first match. `path_matches` treats `acl:default` as an inheritable prefix and `acl:accessTo` as an exact-or-child. `StorageAclResolver::find_effective_acl` walks up path segments and returns the first `.acl` it finds — correct inheritance. **Finding S5 (MEDIUM)**: an `acl:default` on a parent is surfaced even when a child `.acl` exists that is more restrictive, because `find_effective_acl` stops at the first ACL walking up. That is the Solid spec's intent (closest ancestor wins), but a child ACL **cannot denylist** — it can only grant. If a parent grants `foaf:Agent Read` and a child wants owner-only, the child's `.acl` must omit the public grant (it does, in the audit's `render_owner_only_acl`) but ACL evaluation at the parent level is unaffected. Document this explicitly: ACL inheritance is allow-override; there is no deny.

## Verdict

Two HIGH findings (S1 body binding, S2 APP_ENV fail-open default) and three MEDIUMs. No criticals. All findings are fixable without architectural rework.
