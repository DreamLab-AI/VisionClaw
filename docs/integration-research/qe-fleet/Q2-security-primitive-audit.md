# Q2 — Security Primitive Audit, Full Ecosystem Sweep

> Author: QE Specialist Q2. Research date: **2026-05-07**.
> Substrates audited (all paths absolute):
> - VisionClaw — `/home/devuser/workspace/project/`
> - Forum — `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/`
> - Agentbox — `/home/devuser/workspace/project/agentbox/`
> - solid-pod-rs — `/home/devuser/workspace/project/solid-pod-rs/`
>
> Source-of-truth context: `docs/integration-research/05-crypto-gotchas.md`,
> `02-forum-surfaces.md`, `04-solid-pod-rs-surfaces.md`. Sprint v9 STREAM-B
> closed several listed gaps (replay store, ACL coercion, sessionStorage
> hardening); residual issues are flagged below per-substrate.

Verdicts use the standard severity ladder. Citations are file:line and refer
to the working tree at audit date.

---

## S1 — SSRF (Server-Side Request Forgery) guards

### S1.1 Forum preview-worker (`crates/preview-worker/src/ssrf.rs`)

**STATUS: aligned + hardened (Sprint v9 STREAM-B B4).**

The redirect-aware fetch is at `crates/preview-worker/src/ssrf.rs:71-104`
(`ssrf_fetch_with_redirects`). Properties:

- **Manual redirect handling** via `RequestRedirect::Manual` (line 59) —
  re-runs the SSRF policy on every hop (line 76-100). Cap: `MAX_REDIRECTS = 3`
  (line 10).
- **Body-size cap**: `MAX_BODY_BYTES = 2 * 1024 * 1024` enforced in
  `read_text_capped` (line 110-116).
- **IP-range refusal** in `is_private_url` (line 120-198) covers RFC 1918
  (line 212-228), loopback `127/8` + `::1` (line 173, 217), link-local
  (line 183, 226), cloud metadata `169.254.169.254` + `metadata.google.internal`
  + `metadata.goog` (line 154-159), IPv6 ULA `fc00::/7` (line 178), IPv4-mapped
  IPv6 (line 187-194), pure-integer / `0x`-hex IP obfuscation (line 138-146).
- **Tests** at line 240-389 confirm all classes block; `ssrf_constants_safe`
  at line 383 asserts caps stay small.

**Gaps:**

- **Per-hop `is_private_url` is hostname-based, not DNS-resolved.** A
  defender's `is_private_url("http://attacker.com/")` returns `false` (the
  hostname is not on the literal IP block-list). The Cloudflare runtime's
  `Fetch::Request(req).send()` then resolves DNS and connects. If the
  attacker's DNS rebinds to `127.0.0.1` between the policy check and the
  actual connect, the worker could fetch a private endpoint. The Workers
  runtime claims to refuse `localhost` / private CIDRs at the platform
  level (CF Edge), but this is **not enforced in code** here. **MEDIUM.**
  Recommendation: when feasible, enforce on the response-side too — refuse
  any response whose `Server` header / `cf-ray` / 1.1 chain implies a
  non-public origin. Or block any redirect whose target is hostnameless or
  whose host normalises to localhost variants (already done in `is_private_url`,
  but a same-name DNS rebind escapes that).
- **No IPv6-private regex coverage for non-bracketed hosts.** `is_private_url`
  strips brackets at line 167-170 but `Url::parse("http://fc00::1/").host_str()`
  in workers-rs returns the literal `fc00::1` (already lower-cased). The
  current code does pass these through `parse_ipv4` first (returns None
  for IPv6) and then through the prefix-match block (line 178). OK in
  practice but property-test coverage is thin: only one canonicalised
  form per IPv6 family is asserted. **LOW.**
- **No IPv4-mapped IPv6 hex-form parser.** Line 192-194 says "Hex-form
  mapped addresses that didn't match dotted-decimal above — Block since
  we can't reliably parse hex octets." This **fails-safe** (block on
  ambiguity). Confirmed defensible. **INFO.**

### S1.2 Forum oEmbed/parse path (`crates/preview-worker/src/oembed.rs`, `parse.rs`)

oEmbed providers and OG image fetches **flow through** `ssrf_fetch_with_redirects`
(per Sprint v9 B4 wiring). The OG-image follow-up fetch in
`preview-worker/src/lib.rs:322-330` constructs an SsrfFetchError-wrapped GET.
Spot-checks:

- `preview-worker/src/lib.rs:326-328` rate-limits at 30 req/60s per IP
  (`rate_limit::check_rate_limit(&env, &ip, 30, 60)`).
- HTML body parsed via regexes in `parse.rs:27-64` (`og_regexes()`); regex
  is anchored on `(?i)<meta…>` patterns. Regex injection / catastrophic
  backtracking surface: the patterns are linear-time, single-pass, and
  the input is capped at `MAX_BODY_BYTES = 2 MiB`. **OK.**

### S1.3 VisionClaw substrate (`/home/devuser/workspace/project/src/`)

**STATUS: GAP — no preview/proxy fetcher exists, no SSRF guard wired.**

VisionClaw's HTTP surface is the actix-web binary. No `preview*.rs` or URL
proxy handler exists in `/src/handlers/` (verified via filename search).
The substrate fetches GitHub markdown via `EnhancedContentAPI`
(`src/services/enhanced_content_api.rs` — not user-supplied URLs) and
xinference embedding endpoints (operator-configured). Both are **not
attacker-controlled URLs** under normal operation.

**Latent gap:** If a future feature accepts a user-supplied URL (e.g.
"add external KG source"), the substrate does **not import** the
solid-pod-rs `SsrfPolicy`. Recommendation:
`solid_pod_rs::security::SsrfPolicy::from_env()` should be wired into
the actix `AppData` and consulted before any future URL fetcher.
**INFO** until such a feature lands.

### S1.4 Agentbox JSON-LD context resolver

**STATUS: GAP — context documents pinned at build time, no runtime fetch
expected.**

Per agentbox `CLAUDE.md`: "Context documents are pinned at build time via
`lib/linked-data-contexts.nix` and never fetched at runtime." This is the
correct posture — a runtime fetcher would need the same SSRF guard. Verify
by inspecting `/home/devuser/workspace/project/agentbox/lib/linked-data-contexts.nix`
(present per agentbox docs). MCP / management-api code in
`/home/devuser/workspace/project/agentbox/management-api/lib/uris.js` does
not perform any HTTP fetch from JSON-LD context input — confirmed via
grep for `fetch(`/`http_get`/`reqwest` in that subtree (no hits). **OK.**

### S1.5 solid-pod-rs (`crates/solid-pod-rs/src/security/ssrf.rs`)

**STATUS: best-in-class.**

- `SsrfPolicy` aggregate (`security/ssrf.rs:103-270`): RFC 1918, RFC 4193,
  loopback, link-local, multicast, cloud metadata 169.254.169.254 +
  fd00:ec2::254, documentation prefixes, CGNAT 100.64/10, deprecated
  6to4 192.88.99/24, benchmarking 198.18/15, 240/4 reserved.
- IPv6 classifier (`security/ssrf.rs:329-383`) handles `fe80::/10`
  link-local, `fc00::/7` ULA, `fec0::/10` deprecated site-local (treated
  as Private), `100::/64` discard, `2001:db8::/32` documentation, and
  unwraps IPv4-mapped via `to_ipv4_mapped()`.
- DNS-rebinding defence: `resolve_and_check` returns the resolved IP so
  callers can bind the connect socket to that exact address (line 196-263);
  documented at `security/mod.rs:35-41`.
- DNS-failure-blocks-request behaviour (Sprint 12 hardening per
  `RELEASE_NOTES.md`) verified at tests 740-783.
- Allowlist + denylist (deny overrides allow, line 228-239): correct
  precedence.
- Sync-mode primitive `is_safe_url` (line 444-464) covers IP literal
  refusal without DNS, with hostname check for `metadata.google.internal`
  / bare `metadata` / `metadata.goog`.

`solid-pod-rs-nostr` `NostrWebIdResolver` consumes `DefaultSsrfCheck`
which delegates to the core (`solid-pod-rs-nostr/src/resolver.rs:46-51`).
WebID profile fetch uses 10s timeout (`resolver.rs:70`).

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| MEDIUM | Forum `is_private_url` is name-based, not resolved-IP-based | `preview-worker/src/ssrf.rs:120-198` | Adopt the solid-pod-rs `SsrfPolicy::resolve_and_check` model (delegate via WASM-compatible shim) so the resolved IP is checked, then bind connect to that IP |
| LOW | Forum IPv6 hex-mapped parser is fail-safe but coverage thin | `preview-worker/src/ssrf.rs:188-195` | Add prop-tests asserting all-zero, all-mapped, and bracketed forms |
| INFO | VisionClaw has no SSRF guard wired (no current need) | `src/handlers/*` (none) | Wire `solid_pod_rs::security::SsrfPolicy` into AppData ahead of any future URL-input feature |
| INFO | Agentbox JSON-LD contexts pinned at build time (correct) | agentbox `CLAUDE.md` | Maintain — never resolve `@context` at runtime |

### Cross-substrate consistency

solid-pod-rs ships a tested, configurable, DNS-rebinding-resistant
implementation. The forum has its own simpler in-WASM implementation that
shares the same intent but does not resolve hostnames. VisionClaw and
agentbox abstain. **Drift: MEDIUM** — recommend a single shared library
once WASM-vs-tokio split allows.

---

## S2 — Path traversal

### S2.1 Forum pod-worker (`crates/pod-worker/src/lib.rs`)

**STATUS: tight by construction. NO `extract_pod_path` exists**; the
brief refers to a method that has been replaced by `parse_pod_route`.

`parse_pod_route(path: &str) -> Option<(&str, &str)>` at
`pod-worker/src/lib.rs:36-52` does:

1. Strip prefix `/pods/`.
2. Require ≥ 64 bytes remainder.
3. Validate first 64 bytes are ASCII hex (line 43).
4. Require remainder either empty or starts with `/`.
5. Return `(pubkey, "/" if empty else remainder)`.

**No `..` decoding step** because `worker::Url::path()` already returns the
percent-decoded canonical path (Workers runtime hands a normalised URL).
The remainder is passed verbatim to:

- `format!("pods/{owner_pubkey}{resource_path}")` for R2 key lookup
  (line 558).
- `find_effective_acl(&bucket, &kv, &owner_pubkey, &resource_path)` for
  ACL discovery (line 537).

R2 keys are flat (no filesystem). A `..` segment in `resource_path` cannot
escape — R2 treats every byte (including `/`) as opaque. The danger is
**ACL escape** — an attacker writing path `/foo/../bar/secret` could match
an ACL rule for `/foo/` while accessing `/secret` if `find_effective_acl`
walks segments naively. Inspecting `acl.rs:115-149` (`normalize_path_owned`,
`path_matches`): the path is normalised by trimming `./` prefix and
trailing `/`, but **`..` segments are not collapsed**. An ACL rule for
`/foo/` would `starts_with("/foo/")` test against the literal
`/foo/../bar/secret`, which DOES match — even though the actual R2 fetch
would target `/bar/secret`. This is an ACL-mismatch surface. **HIGH.**

Mitigating factors:
- `worker::Url::path()` percent-decodes once but does NOT collapse `..`
  segments (it's spec-compliant raw path output). The Cloudflare frontend
  may reject `..` paths before the worker sees them — verify with curl.
- Sprint v9 STREAM-B B3 limit on ACL bytes (64KiB) is unrelated.

**Recommendation:** add a `normalise_pod_resource_path` helper that:
1. Rejects any `..` segment outright (return 400).
2. Rejects `%00` / null bytes anywhere in the path.
3. Rejects double-decoded `..%2f..` patterns (re-decode and re-check).
4. Rejects Windows backslash forms.
5. Then routes the cleaned path to ACL + R2 paths.

Wire at the entry point (`lib.rs:383` after `parse_pod_route` returns).

### S2.2 VisionClaw `solid_pod_handler.rs` (extract_solid_path)

**STATUS: HIGH — substring-matching is fragile by design.**

`src/handlers/solid_pod_handler.rs:369-382` implements
`extract_solid_path(req)` with `full.find("/solid")` — substring search,
not segment search. Drift cases:

- Request path `/api/solid` → matches at idx for `/solid` substring,
  returns `""` (then `"/"`). OK.
- Request path `/users/solid-evidence/foo` → `find("/solid")` matches
  inside `solid-evidence`, `tail = "-evidence/foo"`. Bug: the
  function returns `"-evidence/foo"` as the pod path. With ACL `agent_uri`
  resolution and dotfile guard at line 134-136, the malformed path lands
  in the storage backend. The FsBackend at
  `solid-pod-rs/crates/solid-pod-rs/src/storage/fs.rs` uses
  `tokio::fs::canonicalize` (line ~) which would error out on a
  path-not-rooted-at-storage-root.

But the **dotfile guard immediately above** (line 134-136 calling
`is_path_allowed`) checks if any segment starts with `.` — it does NOT
check `..` traversal. The same `is_path_allowed` defined at line 356-364
is segment-by-segment but only inspects leading-dot segments:

```
fn is_path_allowed(path: &str) -> bool {
    const ALLOWED_DOT_SEGMENTS: &[&str] = &[".well-known", ".acl", ".meta"];
    for segment in path.split('/') {
        if segment.starts_with('.') && !ALLOWED_DOT_SEGMENTS.contains(&segment) {
            return false;
        }
    }
    true
}
```

`.."` starts with `'.'` but it equals `".."`, not `.well-known` etc., so
it IS rejected here (good — `..` starts with `.` and is not in the allowlist).
**Confirmed: parent traversal IS blocked** by the dotfile filter as a
side-effect, although the comment doesn't explain that. **OK by accident.**

Remaining issues:
- **Substring `/solid` match** (line 371) — multi-tenant routing risk.
- **No percent-decode handling** — actix-web's `req.path()` returns the
  matched path; if a client sends `%2e%2e/secret`, the percent-encoded
  form is **not decoded** by actix-web's matcher (it stays
  `%2e%2e/secret`), bypassing the segment-by-segment dotfile filter.
  Then `extract_solid_path` returns the percent-encoded form, which the
  FsBackend on Linux interprets as a literal filename `..` after
  `tokio::fs` does its own decode. **HIGH.**
- **Null-byte truncation**: a `%00` is not stripped; the FsBackend on
  some FS could truncate. Linux `open(2)` would reject `\0` in the path.
  **LOW.**
- **Symlink handling**: FsBackend uses `tokio::fs` paths joined under
  `POD_DATA_ROOT`. No `canonicalize` + start-prefix-check is documented
  in `solid_pod_handler.rs`. The FS storage in solid-pod-rs core SHOULD
  refuse symlinks pointing outside the root, but I did not verify this
  in this audit. **MEDIUM** — needs follow-up.

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| HIGH | Substring `find("/solid")` match — request to `/users/solid-evidence/X` is mis-parsed | `src/handlers/solid_pod_handler.rs:371` | Replace with strict segment match. Use actix-web's `web::scope("/solid")` and `req.match_info().query("tail")` for the tail |
| HIGH | No percent-decode pass for `%2e%2e` before `is_path_allowed` | `src/handlers/solid_pod_handler.rs:134, 356-364` | Percent-decode once, reject any `..` segment explicitly, reject `%2e%2e/` and other double-encoding |
| MEDIUM | Symlink-out-of-root behaviour in FsBackend not verified | `solid-pod-rs/crates/solid-pod-rs/src/storage/fs.rs` | Add `canonicalize().starts_with(root)` check; deny if not |
| LOW | No null-byte rejection | `src/handlers/solid_pod_handler.rs:356-364` | Add `if path.contains('\0') { return false }` to `is_path_allowed` |

### S2.3 Agentbox pod-inbox writes (`relay-consumer.js`)

**STATUS: aligned but trust-bound.**

`agentbox/mcp/nostr-bridge/relay-consumer.js:204-226`
(`_onInbound` → `inboxPath = path.join(this._podRoot, 'pods', recipient, 'events', 'inbox')`).
The `recipient` is derived from `_findRecipientNpub(event)` which iterates
`event.tags` for `["p", hex]` tuples (line 353-360) and validates each is
in `_npubs` (the local pod-set Set). **`recipient` is constrained to
known-good npub hex** before reaching `path.join`.

`finalPath = path.join(inboxPath, ${event.id}.json)` (line 206) — `event.id`
is checked structurally by `_verifySig` BEFORE this point (line 184-189);
nostr-tools `verifyEvent` recomputes the id and compares. Any tampered
`event.id` containing slashes/dots would fail signature verification.

But: Node's `path.join` is **not** safe against `..` if the inputs are
attacker-influenced. The structural-only fallback at line 322-336 (when
`nostr-tools` is unavailable) only checks `typeof event.id === 'string'
&& typeof event.sig === 'string'`. **In test/dev mode without nostr-tools
installed, `event.id = "../../../etc/secret"` would write
`<podRoot>/etc/secret.json`.** The dev-fallback warning is logged but
acceptance proceeds. **MEDIUM.**

**Recommendation:** even in fallback mode, validate `event.id` matches
the regex `/^[0-9a-f]{64}$/i`. nostr event ids are SHA-256 hex.

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| MEDIUM | Fallback mode (no nostr-tools) accepts any `event.id` shape | `agentbox/mcp/nostr-bridge/relay-consumer.js:322-336` | Add a regex `/^[0-9a-f]{64}$/i` check on `event.id` even in fallback mode |
| INFO | Production path is correct (sig verify recomputes id) | `relay-consumer.js:184-189, 322-328` | Keep |

### S2.4 solid-pod-rs `security::PathTraversalGuard`

**STATUS: aligned + tested.**

The path traversal guard at the actix middleware layer in solid-pod-rs-server
(`solid-pod-rs/crates/solid-pod-rs-server/src/lib.rs:933-998` per
`04-solid-pod-rs-surfaces.md` §5) percent-double-decodes and rejects `..`.
`is_path_allowed` (`security/dotfile.rs:221-241`) is the segment-by-segment
free function used wherever a middleware isn't in scope:

- Skips empty segments and `.` (line 223-225).
- Rejects `..` explicitly with `DotfilePathError::ParentTraversal`
  (line 226-228).
- Allows the `STATIC_ALLOWED_DOTFILES = [".acl", ".meta", ".well-known",
  ".quota.json", ".acl.meta", ".account"]` set (line 180-194).

**Tests:** `dotfile.rs:399-412`:
```
fn blocks_double_dot() {
    match is_path_allowed("/pod/../etc/passwd") {
        Err(DotfilePathError::ParentTraversal(_)) => {}
        ...
    }
    ...
}
```

**Gap:** the segment is split on `'/'` only — a `\` separator (Windows)
is not normalised. Solid is HTTP and `\` is not a valid URL path
separator, so a request would be percent-encoded `%5c`. After the
middleware decode, `%5c` becomes `\`, which `path.split('/')` keeps as
part of one segment. The segment `..%5c..` would not contain `..` after
split — bypass via Windows-style. **LOW** on Linux deployments; **MEDIUM**
if any Windows server were to be added.

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| LOW | `\` separator not normalised in segment split | `solid-pod-rs/crates/solid-pod-rs/src/security/dotfile.rs:222` | Treat `\` as `/` before splitting OR reject any segment containing `\` |
| INFO | `..` and dotfile rules covered by tests | `dotfile.rs:399-412, 379-396` | Keep |

---

## S3 — NIP-98 verifier

There are **four implementations** identified in Q1. Cross-checked here:

### S3.1 Forum `nostr-core::nip98::verify_token_at` (canonical)

`crates/nostr-core/src/nip98.rs:227-322` is the reference. Properties:

1. **Token shape**: `Authorization: Nostr <base64>` (prefix `"Nostr "` at
   line 32). Base64 decoded once (line 246), JSON parsed once (line 250).
2. **Size cap**: `MAX_EVENT_SIZE = 64 * 1024` enforced both pre- and
   post-decode (lines 241-248).
3. **Kind**: must equal 27235 (line 253-255).
4. **Pubkey**: 64-hex enforced (line 258); but **case-insensitive** —
   `hex::decode` accepts upper. Lowercasing is NOT enforced. Compounded
   with WAC ACL string-comparison (S5), this is a known break point.
5. **Timestamp**: `now.abs_diff(event.created_at) > TIMESTAMP_TOLERANCE`
   (60s, line 263-268). **Symmetric** — both clock-too-fast and
   clock-too-slow rejected.
6. **Body hash**: SHA-256 hex computed (`hex::encode(Sha256::digest(body))`
   line 303), compared with `to_lowercase() != to_lowercase()` (line 304).
   **NOT constant-time.** Already flagged as M15 in §05-crypto-gotchas.md.
7. **URL canonicalisation**: trailing `/` stripped (line 277-278). Case
   NOT normalised: `https://example.com/Foo` ≠ `https://example.com/foo`.
   For the forum's specific deployment the URLs are exact-match, so OK.
   Query string IS preserved (no normalisation). A reordered query string
   would break — fine, since the signed event must commit to the exact
   URL.
8. **Method**: `to_uppercase` both sides (line 289). Correct.
9. **Replay protection**: `verify_token_at_with_replay` (line 339-363)
   adds the `Nip98ReplayStore` lookup AFTER all stateless checks pass.
   `seen_or_record` returns `true` on first observation (line 354). On
   duplicate, returns `Replayed` (line 359). The cache key is
   `compute_event_id_from_header(auth_header)` (line 352, 368-381) —
   re-decodes the header and reads `event.id`. Note: this trusts the
   client-provided `event.id` from the JSON for cache keying. If the
   verifier earlier in the chain (`verify_event` at line 271) recomputed
   the canonical id and rejected mismatches, this is fine; if not, an
   attacker could use a stale event with a forged `id` that hash-collides
   with a known-replayed event id. Looking at `event::verify_event`
   (referenced) — it recomputes the canonical id and verifies the
   signature. So `event.id` is trusted iff `verify_event` returns true,
   which gates the entire flow. **OK.**

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| MEDIUM | Body-hash compare is not constant-time | `nostr-core/src/nip98.rs:304` | Switch to `subtle::ConstantTimeEq` over the lowercased bytes |
| MEDIUM | Pubkey hex case not enforced lowercase | `nostr-core/src/nip98.rs:258` | Add `if event.pubkey != event.pubkey.to_lowercase()` reject; or normalise after decode and reject input that wasn't already lowercase |
| LOW | URL trailing-slash normalised but not query-string ordering | `nostr-core/src/nip98.rs:277-278` | Fine for current spec — document as canonicalisation contract |

### S3.2 Forum auth-worker / pod-worker / relay-worker / search-worker

Each worker imports `nostr_core::verify_nip98_token_at_with_replay`
(`pod-worker/src/auth.rs:68-89`) plus a worker-local `KvReplayStore`
implementation (`pod-worker/src/auth.rs:13-39`) that reads/writes the
`NIP98_REPLAY` KV namespace.

Confirmed in `wrangler.toml` files:
- auth-worker: binding = `NIP98_REPLAY`, EXPECTED_ORIGIN = `https://dreamlab-ai.com`
- pod-worker: binding = `NIP98_REPLAY`, EXPECTED_ORIGIN = `https://dreamlab-ai.com`
- relay-worker: binding = `NIP98_REPLAY`, ALLOWED_ORIGINS = `https://dreamlab-ai.com,https://thedreamlab.uk,https://dreamlab-ai.github.io`
- search-worker: binding = `NIP98_REPLAY`, ALLOWED_ORIGINS = `https://dreamlab-ai.com,https://thedreamlab.uk,https://dreamlab-ai.github.io`

**Drift confirmed: each worker's KV namespace has `id = "REPLACE_WITH_NEW_NIP98_REPLAY_KV_ID"`** —
ostensibly placeholder text awaiting deploy-time substitution. If each
worker is deployed with its **own** namespace ID, replay isolation per
worker holds (good for partition tolerance) but cross-worker replay is
possible (PRD-010 F20 — known issue). If all four are pinned to the
**same** namespace ID, they share the cache (recommended per crypto-gotchas
H6). **MEDIUM** until deploy mode confirmed.

The fallback `AlwaysFreshStore` in `pod-worker/src/auth.rs:41-47` returns
`Ok(true)` on every call (no replay protection) and emits a console
warning. **In production, missing KV binding = silent disable.** Better
to `return Err(...)` and 500 — fail closed. **MEDIUM.**

### S3.3 solid-pod-rs `Nip98Verifier` (`crates/solid-pod-rs/src/auth/nip98.rs`)

Per `04-solid-pod-rs-surfaces.md` §8:

- 484 LOC.
- `MAX_EVENT_SIZE: 64 * 1024` (line 24) — **same constant** as forum,
  good.
- `TIMESTAMP_TOLERANCE: 60` (line 23) — same.
- `Nip98Verifier` slots into `SelfSignedVerifier` trait so multiple
  proof types fan-out (line 248).
- Body hash optional (`body_hash: Option<&[u8]>`) — NOT bytes, hash
  pre-computed by caller. The `solid-pod-rs-server` wrapper
  (`solid-pod-rs-server/src/lib.rs:174-187`) **always passes None** for
  body_hash (line 184). This means the actix server reads requests but
  does NOT verify the `payload` tag against the actual upload.
  **Already documented in 04-solid-pod-rs-surfaces.md §5 as a gap.** **HIGH.**
- VisionClaw's `solid_pod_handler.rs:386-399` (`verify_nip98`) DOES pass
  the body bytes (`Some(body)` at line 397), so VisionClaw's mount of
  solid-pod-rs DOES verify payload hash for non-empty bodies. **OK
  there.**

### S3.4 solid-pod-rs-git auth bridge

`solid-pod-rs-git/src/auth.rs:113-160` accepts both `Basic` and `Nostr`
schemes and delegates to `nip98::verify_at` with **body_hash = None**
(line 154). Git push payloads are not signed. **Acceptable** — git smart
HTTP doesn't have a body until after auth, and re-reading the body for
hash would defeat streaming. **OK by design.**

### S3.5 VisionClaw — solid_pod_handler.rs uses solid-pod-rs

VisionClaw mounts `solid-pod-rs::auth::nip98::verify` (line 397, body
included). Single source of truth: solid-pod-rs library. **OK.**

### Cross-system properties summary

| Property | Forum nostr-core | solid-pod-rs core | VisionClaw mount | solid-pod-rs-server actix wrapper |
|---|---|---|---|---|
| Replay store | `Nip98ReplayStore` trait + KvReplayStore (4×) | not present | not present | not present |
| TS tolerance | 60s | 60s | 60s (delegated) | 60s |
| Body hash check | constant-time? **NO** (line 304) | yes if body provided | yes (body passed) | NO — body never passed (line 184) |
| URL canon | trailing `/` strip, no case | trailing `/` strip | delegated | delegated |
| Pubkey case | not enforced lowercase | not enforced lowercase | delegated | delegated |
| Method case | uppercase both | uppercase both | delegated | delegated |
| Webid-tag identity check | `did::verify_webid_tag` (lib.rs:430) | not present | not present | not present |

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| HIGH | solid-pod-rs-server actix wrapper drops body hash verification | `solid-pod-rs/crates/solid-pod-rs-server/src/lib.rs:174-187` | Plumb body bytes into `nip98::verify` — pre-buffer up to body cap |
| MEDIUM | Worker fallback to `AlwaysFreshStore` is silent fail-open | `crates/pod-worker/src/auth.rs:41-89` | Return 500 when KV binding missing in production env (`ENV != "test"`) |
| MEDIUM | Cross-worker replay isolation depends on deploy-time KV id pinning | all 4 `wrangler.toml` files | Pin same `id` for all 4 workers, OR document the per-worker isolation as intentional and ensure URL-rewrite layers (proxies) don't enable cross-worker replay |
| MEDIUM | Pubkey hex case not enforced (impacts ACL string-match) | `nostr-core/src/nip98.rs:258` | Reject non-lowercase hex; lowercase before returning |
| MEDIUM | Body hash compare not constant-time | `nostr-core/src/nip98.rs:304` | Use `subtle::ConstantTimeEq` |

---

## S4 — Replay store contract

### S4.1 Forum `Nip98ReplayStore` trait

`crates/nostr-core/src/nip98.rs:44-49`:
```rust
#[async_trait(?Send)]
pub trait Nip98ReplayStore {
    async fn seen_or_record(&self, event_id: &str) -> Result<bool, String>;
}
```

Contract: returns `Ok(true)` on first observation (records), `Ok(false)`
on subsequent observations within TTL. Storage error → `Err(String)`.

**Race-condition risk in `KvReplayStore::seen_or_record`**
(`pod-worker/src/auth.rs:22-38`):
```rust
match self.kv.get(&key).text().await {
    Ok(Some(_)) => return Ok(false),  // (A)
    Ok(None) => {}                     // (B)
    ...
}
let put = self.kv.put(&key, "1")...    // (C)
put.expiration_ttl(...).execute().await ...  // (D)
Ok(true)
```

Cloudflare KV is eventually consistent. Two concurrent requests with the
same `event_id`:
- Both hit (B) (cache miss).
- Both proceed to (C)/(D).
- Both return `Ok(true)`.

**Both observations succeed.** This is a TOCTOU bug — the trait's "atomic
from caller's point of view" semantics (line 41-43) is **not satisfied**
by the KV implementation. Cloudflare KV does not provide CAS. **HIGH.**

Mitigations: KV is fast enough that the race window is tens of ms; the
practical attacker would need to fire the same NIP-98 token twice in that
window. The replay window is bounded by `TIMESTAMP_TOLERANCE = 60s` —
attacker has 60s to do this. **A motivated attacker can replay.**

**Recommendation:**
- Use a Durable Object instead of KV for replay state (provides
  serialised single-writer access). The forum already has
  `NostrRelayDO` for relay state.
- OR accept the 60s race window as a documented limitation; reduce
  TS_TOLERANCE to 5s to compress the window.
- OR use KV's `metadata` field for an optimistic-lock token, retry on
  mismatch.

### S4.2 Worker-specific implementations

All 4 worker `KvReplayStore`s share the same shape (verified spot-check
of `pod-worker/src/auth.rs`). Likely identical issue across all four.

### S4.3 solid-pod-rs DPoP replay cache

`solid-pod-rs/crates/solid-pod-rs/src/oidc/replay.rs:38-200` —
`DpopReplayCache` is a process-local LRU with `tokio::sync::Mutex`.
Single-writer **within the process** so no race.

- TTL default: 60s (line 55, `DEFAULT_TTL_SECS`).
- Capacity default: 10_000 (line 59).
- Capacity-driven replay window risk: **at 10_000 entries with 60s TTL,
  if request rate exceeds 166 rps**, the LRU evicts entries before TTL
  expires, opening replay attacks against the evicted entry. Documented
  at line 22-25.
- Limitation: **process-local, NOT shared across replicas**. Multi-instance
  deployments need sticky DPoP-session routing OR a shared store
  (Redis-backed cache called out as out-of-scope at line 30-36).

`check_and_record` (line 162-180):
- Locks once.
- Uses `peek` (not `get`) so LRU promotion is ON insertion only — see
  comment line 166-167 ("we want LRU order to reflect insertion age, not
  every-check age"). Good — prevents replay-attempt from refreshing the
  entry.

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| HIGH | KvReplayStore TOCTOU race in `seen_or_record` (CF KV is not CAS) | `crates/pod-worker/src/auth.rs:22-38` (and 3 sibling workers) | Migrate replay state to a Durable Object OR document the race as accepted; reduce TS_TOLERANCE to 5-15s to compress the window |
| MEDIUM | DPoP LRU capacity is single-replica | `solid-pod-rs/.../oidc/replay.rs:30-36, 22-25` | Document HA caveat; sticky-session DPoP or future Redis-backed cache |
| INFO | Worker replay namespaces share common shape (good) | `crates/{auth,pod,relay,search}-worker/src/auth.rs` | Maintain |

### S4.4 VisionClaw and Agentbox

- VisionClaw: NO replay store. `solid_pod_handler.rs::verify_nip98`
  (line 386-399) calls `solid-pod-rs::auth::nip98::verify` — the
  non-replay variant. **HIGH if NIP-98 is exposed externally.**
- Agentbox: implicit replay via content-addressed file existence
  (`relay-consumer.js:207` `if (fs.existsSync(finalPath)) return`). The
  filesystem path **is** the cache. TTL = forever (file doesn't expire).
  Eviction: never (operator must clean). Race: `existsSync` then `mkdirSync`
  then `writeFileSync` then `renameSync` (line 213-222) — not atomic, but
  the rename is. Two racing consumers both pass `existsSync`, both write
  tmp files (different `process.pid` in tmp name — line 215), both rename.
  The second rename overwrites the first. **OK enough** — content-addressed
  by sha256 event id; both writes are identical. **OK.**

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| HIGH | VisionClaw `solid_pod_handler.rs::verify_nip98` lacks replay store | `src/handlers/solid_pod_handler.rs:386-399` | Add a replay store backed by RuVector or a tokio-mutex-guarded HashMap with TTL eviction |
| INFO | Agentbox uses filesystem as content-addressed dedup (correct) | `relay-consumer.js:204-226` | Keep |

---

## S5 — WAC ACL evaluation

### S5.1 Forum pod-worker `acl.rs`

`evaluate_access` at `crates/pod-worker/src/acl.rs:205-246`:

1. Extracts the `@graph` (returns false if missing — secure default at
   line 213).
2. For each authorisation: check `mode` matches `required_mode`
   (line 218-221).
3. `agent_matches` (line 162-188):
   - String-compares `acl:agent` IRIs (line 164-167) — **literal byte
     equality**, no normalisation.
   - Recognises `foaf:Agent` and `acl:AuthenticatedAgent` agent classes
     (line 175-184). NO group support.

**Drift from `did:nostr:<hex>` format**: at `lib.rs:447`, the agent URI
is `format!("did:nostr:{pk}")` where `pk` is the verified pubkey. The pk
case follows whatever NIP-98 hands back, which follows whatever the client
signed. **Mixed-case hex would silently fail ACL match.**

`acl:agentClass` only knows `foaf:Agent` and `acl:AuthenticatedAgent`.
No group resolution (no `vcard:Group` membership lookup). Bridges /
delegated agents are not modelled. **MEDIUM.**

`coerce_required_mode_for_acl` at line 276-286: any write-class method
on a `.acl` resource is upgraded to `Control`. Sprint v9 STREAM-B C3
hardening — verified.

`parse_acl_with_cap` (referenced from `lib.rs` — handle_acl_request) caps
ACL JSON at 64 KiB (Sprint v9 B3) — verified by upstream context.

### S5.2 VisionClaw `solid_pod_handler.rs` evaluate_access

`src/handlers/solid_pod_handler.rs:161` delegates to
`solid_pod_rs::wac::evaluate_access` with `request_origin = None` (origin
check intentionally off — comment at 158-160). The agent_uri is computed
by `derive_webid` (line 405-411) producing
`{base_url}/{pubkey_hex}/profile/card#me` — **WebID URI form**, not
`did:nostr:<hex>`. The pod-worker forum produces `did:nostr:<hex>`
agent_uri (`pod-worker/src/lib.rs:445-447`).

**Drift:** ACLs provisioned for `did:nostr:<hex>` agent will not match
when VisionClaw requests come in with `https://...:profile/card#me`
agent_uri. The two systems produce **different agent IRIs from the same
pubkey**. ACLs would need both forms in `acl:agent` to be accessible from
both. **HIGH.**

### S5.3 Agentbox sovereign-bootstrap.py ACL writes

Per `05-crypto-gotchas.md` §14: ACLs use `did:nostr:<hex>` exclusively
(`agentbox/scripts/sovereign-bootstrap.py:161-167`). Aligned with
forum pod-worker, **NOT aligned with VisionClaw**.

### S5.4 solid-pod-rs WAC

`solid-pod-rs/crates/solid-pod-rs/src/wac/` — 9 files, 2722 LOC, supports
`AccessMode` enum, conditions framework (`conditions.rs`), origin
enforcement (gated `acl-origin` feature), client/issuer conditions
(`evaluate_access_ctx_with_registry`).

`StaticGroupMembership` (line 52 of `wac/mod.rs` — re-export) → group
resolution **IS** supported in solid-pod-rs core, just **NOT** in the
forum pod-worker port.

Per `04-solid-pod-rs-surfaces.md` §5 — WAC is enforced via `enforce_write`
at `solid-pod-rs-server/src/lib.rs:204-251`. Includes the
`RequestContext` with `web_id`, `client_id`, `issuer` for WAC 2.0.

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| HIGH | VisionClaw produces WebID-shaped agent_uri; pod-worker uses did:nostr — ACLs don't cross-match | `solid_pod_handler.rs:140-141, 405-411` vs `pod-worker/src/lib.rs:445-447` | Standardise on `did:nostr:<hex>` as agent_uri across both. Or maintain dual entries in provisioned ACLs |
| MEDIUM | pod-worker `agent_matches` lacks group resolution | `pod-worker/src/acl.rs:162-188` | Port `solid_pod_rs::wac::evaluate_access_with_groups` semantics to pod-worker (or replace pod-worker WAC with the core lib) |
| MEDIUM | No agent IRI normalisation (case, trim, trailing-slash) before string compare | `pod-worker/src/acl.rs:166-167` | Apply lowercase-hex + trim + strip-trailing-slash on both sides before compare |
| HIGH | Pubkey case not enforced lowercase at NIP-98 ingress; agent IRI mis-matches downstream | `nostr-core/src/nip98.rs:258` | Reject non-lowercase hex at NIP-98 ingress |

---

## S6 — Rate limiting

| Surface | File:line | Limit | Window | Bucket | Per-IP / Per-pubkey | Distributed? |
|---|---|---|---|---|---|---|
| Forum auth-worker | `auth-worker/src/lib.rs:123-125` | 20 | 60s | sliding (KV-bucket) | per-IP | yes (KV) |
| Forum preview-worker | `preview-worker/src/lib.rs:326-328` | 30 | 60s | sliding (KV-bucket) | per-IP | yes (KV) |
| Forum search-worker | `search-worker/src/lib.rs:446-448` | 100 | 60s | sliding (KV-bucket) | per-IP | yes (KV) |
| Forum pod-worker `/.well-known/nostr.json` | `pod-worker/src/lib.rs:178-198` | 60 | 60s | bucket-of-minute | per-IP | yes (KV) |
| Forum relay-worker DO broadcast | `relay-worker/src/relay_do/broadcast.rs:75-91` | 10 | 1s | rolling vec | per-IP | per-DO |
| Agentbox sovereign mesh | `agentbox.toml:34, 100` | 5 | 1s (`messages_per_sec`) and 20 (`rate_limit`) per minute | nostr-rs-relay native | per-pubkey | single-replica |
| Agentbox WS auth-middleware | `mcp/auth/auth-middleware.js:14-17` | env `RATE_LIMIT_MAX_REQUESTS` (default 100) | env `RATE_LIMIT_WINDOW_MS` (default 60000ms) | rolling array | per-clientId (`ip:port`) | per-process |
| solid-pod-rs `rate-limit` feature | `crates/solid-pod-rs/src/security/rate_limit.rs` | configurable | configurable | LruRateLimiter | configurable subject | per-process |

Implementation: `crates/preview-worker/src/rate_limit.rs:12-37` is the
shared Workers-side fast path. Bucket = `js_sys::Date::now() / window_secs`,
TTL = window_secs. **Fail-open on KV error** (line 14-15: `Err(_) => return true`).

**Findings:**

- **Per-IP only at the edge** — no per-pubkey rate limiting on the forum.
  An authenticated user can churn through their CF-Connecting-IP (mobile
  or VPN bouncing) and effectively bypass the limit; conversely, sharing
  one IP (NAT, dorm, café) shares the budget. **MEDIUM.**
- **Sliding window is bucket-of-minute**, not true sliding. A bursty
  client at the bucket boundary can do 2× the limit in the boundary
  second. **LOW.**
- **Fail-open** is a deliberate availability tradeoff. In an attack
  scenario where KV is degraded, attacker can saturate. Document or
  consider fail-closed for security-sensitive endpoints (auth/login).
  **MEDIUM.**
- **Relay DO 10 events/sec/IP** is a reasonable hard ceiling for write
  volume. Per-IP, in-memory in the DO. Lost on DO restart. **LOW.**
- **Agentbox auth-middleware uses `ip:port` as clientId** — every new
  connection gets fresh budget. Should use IP only, or
  pubkey-after-handshake. **MEDIUM.**
- **Agentbox sovereign mesh** uses two independent counters that may
  conflict (5/sec + 20/min). Document precedence. **LOW.**

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| MEDIUM | No per-pubkey rate limiting; per-IP only | all 5 worker rate-limit sites | Add a second bucket keyed by `did:nostr:<pk>` for authed requests |
| MEDIUM | Fail-open on KV error for rate limit | `preview-worker/src/rate_limit.rs:14-15` | Fail-closed for auth/login endpoints |
| MEDIUM | Agentbox auth-middleware clientId = `ip:port` (per-connection) | `mcp/auth/auth-middleware.js:78` | Use IP-only or pubkey-after-handshake |
| LOW | Sliding window is bucket-of-minute; allows 2× burst at boundary | `preview-worker/src/rate_limit.rs:18-19` | Use two adjacent buckets and weight by elapsed |
| LOW | Two agentbox rate-limit knobs (per-sec / per-min) may not compose cleanly | `agentbox.toml:34, 100, 203-206` | Document precedence and tested-ceilings in agentbox docs |

---

## S7 — Dotfile / well-known guards

| Surface | Allowlist | Citation |
|---|---|---|
| Forum pod-worker | `.well-known/*` allowed via path-prefix early-return; other dotfiles blocked implicitly by ACL not matching, but no explicit segment-by-segment dotfile guard | `pod-worker/src/lib.rs:279, 302, 313, 362` (well-known endpoints handled before pod-route parsing) |
| VisionClaw `is_path_allowed` | `.well-known`, `.acl`, `.meta` | `src/handlers/solid_pod_handler.rs:357` |
| solid-pod-rs `DotfileAllowlist` (default) | `.acl`, `.meta`, `.account` | `solid-pod-rs/crates/solid-pod-rs/src/security/dotfile.rs:24` (`DEFAULT_ALLOWED`) |
| solid-pod-rs `is_path_allowed` (Sprint 9 free function) | `.acl`, `.meta`, `.well-known`, `.quota.json`, `.acl.meta`, `.account` | `solid-pod-rs/crates/solid-pod-rs/src/security/dotfile.rs:180-194` (`STATIC_ALLOWED_DOTFILES`) |
| Agentbox | delegates to solid-pod-rs (pod-server) | implicit |

### Findings

- **Drift between solid-pod-rs `DotfileAllowlist::DEFAULT_ALLOWED` (3
  entries: `.acl`, `.meta`, `.account`) and `STATIC_ALLOWED_DOTFILES`
  (6 entries adding `.well-known`, `.quota.json`, `.acl.meta`).** The
  middleware-style `DotfileAllowlist` does NOT permit `.well-known` by
  default — fine, since the server middleware mounts well-known endpoints
  separately. But the free-function variant DOES permit `.well-known`.
  Two different policies, both shipped, depending on which call-site
  picks which. **LOW.**
- **VisionClaw `is_path_allowed` is missing `.account`** — IdP login
  endpoint per JSS commit 32c0db2 — and missing `.quota.json`. If
  VisionClaw ever serves a Solid IdP at this mount, login URLs break.
  **LOW** (no IdP currently at this mount).
- **Case sensitivity**: solid-pod-rs and pod-worker are both case-sensitive
  (`Path::components` doesn't fold). On Linux this is fine. On macOS
  HFS+ the FS is case-insensitive; an attacker writing `.ACL` could
  confuse. Solid paths are case-sensitive per spec — **OK as designed**.
- **Prefix vs segment matching**: solid-pod-rs `is_path_allowed` splits
  on `/` (segment match — line 222). VisionClaw `is_path_allowed` also
  splits on `/` (segment match — line 358). Forum pod-worker has NO
  generic dotfile guard; each well-known endpoint is checked by
  startswith (`pod-worker/src/lib.rs:279, 302`). **OK.**
- **Normalised vs raw path**: solid-pod-rs Sprint 4 middleware
  `PathTraversalGuard` percent-double-decodes BEFORE dotfile check.
  VisionClaw `solid_pod_handler.rs::extract_solid_path` does NOT decode.
  **HIGH** — already flagged S2.2.

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| MEDIUM | VisionClaw allowlist lacks `.account` and `.quota.json` | `src/handlers/solid_pod_handler.rs:357` | Match solid-pod-rs `STATIC_ALLOWED_DOTFILES` |
| LOW | Two dotfile allowlists in solid-pod-rs (3-entry vs 6-entry) | `solid-pod-rs/.../security/dotfile.rs:24, 180-194` | Document which to use where; consider merging |
| LOW | Forum pod-worker has no generic dotfile guard, only per-route prefix matches | `pod-worker/src/lib.rs:279, 302, 313` | Add a segment-based dotfile guard before `parse_pod_route` |

---

## S8 — CORS posture

### S8.1 Forum 5 workers — drift table

| Worker | Allowlist | Source | Method scope |
|---|---|---|---|
| auth-worker | `https://dreamlab-ai.com` only | `wrangler.toml`, `auth-worker/src/lib.rs:34` | GET, POST, OPTIONS |
| pod-worker | `https://dreamlab-ai.com` (single) | `wrangler.toml`, `pod-worker/src/lib.rs:81-85` | GET, PUT, POST, DELETE, PATCH, HEAD, OPTIONS |
| relay-worker | `https://dreamlab-ai.com,https://thedreamlab.uk,https://dreamlab-ai.github.io` | `wrangler.toml` (ALLOWED_ORIGINS), `relay-worker/src/lib.rs:46-49` | GET, POST, OPTIONS |
| search-worker | `https://dreamlab-ai.com,https://thedreamlab.uk,https://dreamlab-ai.github.io` | `wrangler.toml` (ALLOWED_ORIGINS) | GET, POST, OPTIONS |
| preview-worker | (single, env-driven) | `preview-worker/src/lib.rs:100-104` | GET, OPTIONS |

**Drift confirmed:** auth-worker and pod-worker accept ONE origin
(`https://dreamlab-ai.com`); relay-worker and search-worker accept THREE.
Practical impact: a user on `https://thedreamlab.uk` can read the relay
and search but cannot login or write to their pod. **HIGH** for UX, but
**ALSO** a security concern — if the auth-worker accidentally added the
github.io origin, that would extend implicit trust to Pages-served pages
which can be hijacked via repo settings.

**Methods on pod-worker** include `PUT/DELETE/PATCH` — write methods. CORS
preflight `Access-Control-Max-Age: 86400` (1 day, line 101). Long preflight
TTL is fine for performance, **bad** for revoking origin access (a removed
origin still has cached preflight allowance for 24h on each browser).

### S8.2 VisionClaw

`src/main.rs:746-773` reads `CORS_ALLOWED_ORIGINS` env, splits comma, calls
`cors_builder.allowed_origin(origin)`. If empty, falls back to
`allowed_origin_fn` (line 773) — runtime closure decision. The closure
checks each request against the env list (line 771).

WS upgrade origin check at
`src/handlers/fastwebsockets_handler.rs:186-201`:
- Reads `ALLOWED_WS_ORIGINS` env.
- Allows if request origin starts-with any allowed (prefix match — bug:
  `https://dreamlab-ai.com.attacker.com` would prefix-match
  `https://dreamlab-ai.com` if the allowed entry doesn't have a trailing
  `/` or origin separator). **HIGH.**
- Same-host fallback at line 194-201 strips `http(s)://` and matches host.

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| HIGH | VisionClaw WS origin check uses `starts_with` not exact match | `src/handlers/fastwebsockets_handler.rs:192` | Parse origin URL, match scheme+host+port exactly |
| HIGH | Forum auth-worker / pod-worker CORS allowlist is a single origin (excludes `thedreamlab.uk`) | `wrangler.toml` (auth/pod) | Align with relay/search OR document why auth/pod are intentionally narrower |
| MEDIUM | `Access-Control-Max-Age: 86400` slows revocation on origin change | `pod-worker/src/lib.rs:101` | Reduce to 1h for security-sensitive origins |
| INFO | Agentbox `mcp/auth/auth-middleware.js` reads `CORS_ALLOWED_ORIGINS` env (per-deployment) | `auth-middleware.js:19` | Maintain |

### S8.3 Agentbox management-api

CORS is configured at the management-api Express server level (per
agentbox docs / DDD). Not inspected line-by-line in this audit; agentbox
CLAUDE.md says "agentbox is its own standalone project" so detailed
review is in agentbox repo. **INFO** — out-of-scope for VisionClaw/forum
integration concerns.

---

## S9 — Auth header parsing

### S9.1 NIP-98 `Authorization: Nostr <base64>`

Four parsers identified (all share the pattern `auth.strip_prefix("Nostr ")`):

1. `nostr-core/src/nip98.rs:235-238`:
   ```
   let token = auth_header.strip_prefix(NOSTR_PREFIX).ok_or(...)?.trim();
   ```
   `NOSTR_PREFIX = "Nostr "` (line 32). **Case-sensitive.** A client
   sending `nostr ` (lowercase) is rejected. RFC 7235 says auth-scheme
   tokens are case-insensitive. **MEDIUM.**
2. `solid-pod-rs/crates/solid-pod-rs/src/auth/nip98.rs` (484 LOC, per
   surfaces doc) — same prefix, same case-sensitivity (delegated by
   `verify`).
3. `solid-pod-rs-git/src/auth.rs:86, 113` — accepts both `Basic <…>` and
   `Nostr <…>`. The `Basic` form has username `nostr` literal (line 86).
   Re-wraps as `Nostr <b64>` and delegates. **OK** but adds a parsing
   surface.
4. VisionClaw `solid_pod_handler.rs:391-395`: reads
   `actix_web::http::header::AUTHORIZATION` and passes verbatim to
   `nip98::verify`. So same case-sensitivity from solid-pod-rs core.

### S9.2 Bearer tokens (agentbox auth-middleware)

`agentbox/mcp/auth/auth-middleware.js:75-80`:
```
const authHeader = req.headers.authorization;
const token = authHeader ? authHeader.replace('Bearer ', '') : null;
```

**Bugs:**
- `replace('Bearer ', '')` is case-sensitive — `bearer ` would not strip
  the prefix. **MEDIUM.**
- `replace` only removes the **first** match. If the value is
  `Bearer Bearer x`, the result is `Bearer x` — likely fine but
  surprising. **LOW.**
- No length cap on token — a 10MB header would not be rejected (well,
  Express has a default header size limit ~80KB).
- No padding tolerance for base64 (NIP-98 base64 — handled at
  base64 decoder).

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| MEDIUM | NIP-98 prefix `"Nostr "` case-sensitive (RFC 7235 says case-insensitive) | `nostr-core/src/nip98.rs:235-237` | Use a case-insensitive starts-with check; trim spaces after |
| MEDIUM | Bearer scheme strip uses case-sensitive `replace('Bearer ', '')` | `agentbox/mcp/auth/auth-middleware.js:77` | Use a regex `/^Bearer\s+/i` or split on whitespace |
| LOW | No explicit length cap on Authorization header at parse | both | Cap before strip |

---

## S10 — Random number sourcing

| Site | Source | File:line | Verdict |
|---|---|---|---|
| BIP-340 aux_rand (sign_event prod path) | `getrandom::getrandom` | `nostr-core/src/event.rs:129-132` | OK (CSPRNG) |
| BIP-340 aux_rand (keys.rs::SecretKey::sign test/helper) | hardcoded `[0u8;32]` | `nostr-core/src/keys.rs:63` | LOW — deterministic; documented as test-helper |
| Throwaway keypairs for gift-wrap | `getrandom::getrandom` | `nostr-core/src/keys.rs:202` | OK |
| NIP-44 nonce | `getrandom::getrandom` | `nostr-core/src/nip44.rs:78` | OK |
| NIP-04 IV | `getrandom::getrandom` | `nostr-core/src/nip04.rs:94` | OK |
| Calendar d-tag random | `getrandom::getrandom` | `nostr-core/src/calendar.rs:91` | OK |
| NIP-26 aux | `getrandom::getrandom` (return ignored) | `nostr-core/src/nip26.rs:163` | LOW — `let _ = getrandom::getrandom(&mut aux)` discards error result; if getrandom fails, aux stays zeroed and signing proceeds with deterministic aux. Should `expect(...)` or propagate. |
| **NIP-42 challenge generation** | `js_sys::Math::random()` (Math.random) | `relay-worker/src/relay_do/session.rs:284-292` | **HIGH — Math.random is not cryptographic** |
| NIP-42 sequencing (XOR with `session_id`) | `^ session_id` | `session.rs:290` | provides uniqueness but not unpredictability |
| Agentbox bootstrap | `ecdsa.SigningKey.generate` (Python `os.urandom` underneath) | `agentbox/scripts/sovereign-bootstrap.py` | OK (assuming default RNG) |
| Solid-OIDC PKCE / nonce | (in `oidc/mod.rs`, not inspected line-by-line here) | n/a | needs verification |
| WS auth tokens (agentbox) | `crypto.randomBytes(32)` | `agentbox/mcp/auth/auth-middleware.js:184` | OK (CSPRNG) |

**Critical finding:** NIP-42 challenge sourced from `Math.random()`. The
challenge is what binds the AUTH event to a specific session — its
unpredictability is the entire security property. `Math.random()` in V8
is xorshift128+, not cryptographic; an attacker who can observe a few
challenges (relay logs, MITM) can predict subsequent challenges. **HIGH.**

**Recommendation:** use `crypto.subtle.getRandomValues` via `web_sys`
in the Workers runtime, or `js_sys::global().crypto().get_random_values()`.
Existing patterns in the same codebase (e.g.
`forum-client/src/auth/passkey.rs` uses webauthn-rs which uses
`crypto.subtle`) show this is feasible.

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| HIGH | NIP-42 challenge uses `Math.random()` not CSPRNG | `relay-worker/src/relay_do/session.rs:284-292` | Use `crypto.getRandomValues` (Web Crypto API) — exposed in Workers runtime |
| LOW | NIP-26 aux discards `getrandom` error | `nostr-core/src/nip26.rs:163` | Propagate or `expect` |
| LOW | keys.rs hardcoded `aux_rand = [0u8;32]` is reachable from non-test paths in principle | `nostr-core/src/keys.rs:63` | Already documented in §05 (recommendation L19); add clippy lint |
| INFO | All other CSPRNG uses are correct | various | Maintain |

---

## S11 — Time / clock skew handling

| Window | Width | Source | File:line |
|---|---|---|---|
| NIP-98 timestamp tolerance | 60s symmetric | wall clock (`SystemTime::now()` or `js_sys::Date::now`) | `nostr-core/src/nip98.rs:20, 142-144, 263` |
| NIP-98 replay cache TTL | 120s (= 2 × 60) | wall clock | `nostr-core/src/nip98.rs:26` |
| NIP-26 created_at < / > conditions | per-event-rule | wall clock | `nostr-core/src/nip26.rs:64-95` (per spec) |
| NIP-42 AUTH challenge validity | 600s | wall clock | `relay-worker/src/relay_do/nip_handlers.rs:467` (per memory of cross-system audit) |
| Gift-wrap ±48h jitter | 48h | random (getrandom) for offset choice | `nostr-core/src/gift_wrap.rs:108-125` |
| DPoP iat-skew | 60s default | wall clock (Instant) | `solid-pod-rs/.../oidc/replay.rs:55` |
| DPoP replay TTL | 60s default | monotonic Instant | `solid-pod-rs/.../oidc/replay.rs:55, 162-180` |

**Findings:**

- **All time checks use wall-clock**, not monotonic, except DPoP cache
  which uses `tokio::time::Instant` (monotonic). Wall-clock is correct
  for cross-host verification (events signed on different machines
  must agree on UTC). Monotonic prevents NTP-jump bypasses but is
  meaningless across hosts.
- **NIP-98 ±60s window**: Sprint v9 consensus. Adequate for desync;
  too tight for batched/queued requests where the request waits in a
  proxy >60s. **OK.**
- **NIP-42 600s window**: 10 minutes between challenge issued and AUTH
  response. Allows a user to walk away, lock screen, return, sign.
  **OK.**
- **Gift-wrap ±48h jitter** is much wider than the others. By design
  — the goal is to prevent timestamp correlation between user activity
  and gift-wrap delivery. The forum's verification side does NOT enforce
  any particular wall-clock matching for kind 1059 (just signature).
  **OK.**
- **Leap seconds**: `SystemTime::now().duration_since(UNIX_EPOCH)` skips
  leap seconds (Unix time monotonic-by-skip). All systems agree.
  Hypothetical issue: a NIP-98 token signed in the second after a
  positive leap second would have a 1-sec discrepancy. Within tolerance.
  **OK.**
- **`now < event.created_at` (clock-too-fast)**: `now.abs_diff()` is
  symmetric (`nostr-core/src/nip98.rs:263`). Both sides bounded.
- **Replay cache TTL must be ≥ 2× tolerance** to ensure no event with
  `created_at + 60s` skew can sneak past a freshly-evicted cache. The
  120s = 2×60s is exactly at the boundary. Recommend 180s for safety.
  **LOW.**

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| LOW | Replay cache TTL exactly 2× tolerance leaves no buffer | `nostr-core/src/nip98.rs:26` | Increase to 180s or 3× |
| INFO | Wall-clock vs monotonic split is correctly chosen | various | Maintain |

---

## S12 — Privkey custody

| Custodian | Storage | Lifetime | File:line |
|---|---|---|---|
| Forum user (passkey) | PRF-derived in browser memory; sessionStorage hardened | per-session | `forum-client/src/auth/session.rs:96-109, 319-352` (Sprint v9 STREAM-B B8) |
| Forum user (NIP-07) | extension-managed, never reaches forum-client | extension lifetime | `forum-client/src/auth/nip07.rs:50` |
| Forum user (local nsec) | sessionStorage (deprecated) | per-session, zeroised on pagehide | `forum-client/src/auth/session.rs:96-109` |
| Agentbox agent | filesystem `/var/lib/agentbox/identities/<id>.json`, root-readable | persistent, never zeroised | `agentbox/scripts/sovereign-bootstrap.py:81-141` |
| VisionClaw server | env var `SERVER_NOSTR_PRIVKEY` (also `VISIONCLAW_NOSTR_PRIVKEY` per §05) | process lifetime | `src/services/server_identity.rs:65-90` |
| solid-pod-rs-idp | jwks key handling (RS256/ES256) per `04-solid-pod-rs-surfaces.md` §4 | process lifetime | `solid-pod-rs-idp/src/jwks.rs` |
| Forum auth-worker | none (CF Worker has no persistent privkey) | n/a | n/a |

### Findings

- **VisionClaw env var leak risk**: `SERVER_NOSTR_PRIVKEY` is read at
  startup and never re-read. A `printenv` or `/proc/<pid>/environ` read
  by another process exposes it. Docker `inspect` shows env vars unless
  `--env-file` is used. **MEDIUM** — standard env-var hygiene applies.
- **Agentbox filesystem custody**: `/var/lib/agentbox/identities/<id>.json`
  is **root-readable**. The bootstrap script `sovereign-bootstrap.py`
  runs as root during one-shot init, then the agentbox runtime drops to
  `devuser` (per agentbox CLAUDE.md "Supervisord runs as PID 1 root; all
  long-running supervised processes drop to devuser"). The identity
  files are written with default permissions (probably 0644 unless
  explicit chmod). **HIGH if not 0600 root:root.** Verify via
  `stat /var/lib/agentbox/identities/*.json` post-bootstrap.
- **No zeroisation in VisionClaw**: `String::from(env::var("SERVER_NOSTR_PRIVKEY"))`
  lives in heap until process exit. No `Zeroize` derive on the
  containing struct. **LOW.**
- **Forum sessionStorage hardening (Sprint v9 B8)** — confirmed in
  §05-crypto-gotchas.md hazard 2. Verified in `session.rs:319-352`
  (`pagehide` listener zeroises). **OK.**
- **Forum register-with-generated-key** path shows the privkey to user
  once for backup (`auth/mod.rs:339`). After that, never persisted.
  **OK.**
- **Log-leak risk**: search for `format!("{:?}", secret_key)` or
  `Display` impl on `SecretKey`. `nostr-core/src/keys.rs:34-38` has
  `Zeroize` derive. The `Debug` impl: I did not verify whether SecretKey
  prints the bytes in Debug output. If it does, any `tracing::debug!("{:?}", ...)`
  leaks. **MEDIUM** — needs verification.

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| HIGH | Agentbox identity file permissions need verification | `agentbox/scripts/sovereign-bootstrap.py:120-135` (likely needs explicit `os.chmod(0o600)`) | Add explicit 0600 root:root, document in DDD |
| MEDIUM | VisionClaw `SERVER_NOSTR_PRIVKEY` not zeroised post-load | `src/services/server_identity.rs:65-90` | Wrap loaded key in `Zeroizing<…>`; clear env var after read |
| MEDIUM | SecretKey `Debug` impl may leak bytes | `nostr-core/src/keys.rs:34-38` | Verify; if yes, override `Debug` to print only the first 4 bytes hex + ellipsis |
| LOW | No zeroisation on VisionClaw `String::from(env::var())` | same | Use `secrecy::SecretString` |

---

## S13 — Constant-time comparisons

| Site | Implementation | File:line | Verdict |
|---|---|---|---|
| NIP-98 payload hash compare | `String::eq_ignore_ascii_case` (line 304: `expected_hash.to_lowercase() != actual_hash.to_lowercase()`) | `nostr-core/src/nip98.rs:304` | **NOT constant-time** (M15 from §05) |
| NIP-44 HMAC verify | `hmac::verify_slice` | `nostr-core/src/nip44.rs:213-216` (per §05 finding) | constant-time ✓ |
| NIP-44 v1 vs v2 magic byte | (need verification) | `nostr-core/src/nip44.rs` | not inspected line-by-line; magic byte compare is NOT a secret so non-constant-time is OK |
| NIP-26 delegation sig hash compare | (need verification) | `nostr-core/src/nip26.rs:282` (hash domain) | sig verification via k256 — k256 is constant-time |
| WebAuthn signature compare | webauthn-rs library | `forum-client/src/auth/passkey.rs:333` | webauthn-rs uses `subtle::ConstantTimeEq` (assumed) |
| BIP-340 sig verify | `k256::schnorr::VerifyingKey::verify` | wherever sig verify happens | constant-time ✓ |
| `event.pubkey` compare to expected | string `==` | various | the pubkey is not secret — non-CT is OK |
| ACL agent IRI compare | `Vec::contains(&str)` | `pod-worker/src/acl.rs:166-167` | per-iter `==` — pubkey isn't secret in this context, so OK; but timing differences could leak which entries are first in the list — **LOW** |

**Findings:** the only material gap is the NIP-98 payload-hash compare
(M15). All cryptographic-primitive boundaries (HMAC, Schnorr, ECDSA)
flow through libraries that ARE constant-time.

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| MEDIUM | NIP-98 body-hash compare not constant-time (M15 from §05) | `nostr-core/src/nip98.rs:304` | `subtle::ConstantTimeEq` over normalised lowercase bytes |
| LOW | ACL IRI list `contains` is non-CT — timing attacker can probe list size | `pod-worker/src/acl.rs:166-167` | Document; agent IRI is not a secret |

---

## S14 — Origin-bound credentials

### S14.1 Solid-OIDC DPoP `htu` claim

`solid-pod-rs-idp` per `04-solid-pod-rs-surfaces.md` §4 supports DPoP
with `htu` claim binding the proof to a specific request URL.
Verification flow at `solid-pod-rs/.../oidc/mod.rs:366-432` (referenced
above for replay).

The `htu` (HTTP Target URI) check should:
- Match scheme + host + port (case-insensitive scheme; case-sensitive
  path).
- Drop fragment.
- Compare to the actual request URI, not a header.

I did not inspect the htu-check code line-by-line in this audit; per the
cross-reference, RFC 9449 §4.3 mandates these. solid-pod-rs claims RFC
9449 compliance per `04-solid-pod-rs-surfaces.md`. **Assumed OK pending
verification.**

### S14.2 WebAuthn `origin` field

`forum-client/src/auth/passkey.rs:306` (`check_credentials_intercepted`)
and PRF derivation tie the credential to the origin. webauthn-rs library
checks the `clientDataJSON.origin` against the relying-party origin.
`02-forum-surfaces.md` §1.1 confirms the registration/login flows enforce
this. **OK** assuming webauthn-rs is configured with the correct expected
origin (the auth-worker's `EXPECTED_ORIGIN` env).

### S14.3 NIP-98 `["u", url]` tag

Verified at `nostr-core/src/nip98.rs:276-284`:

```rust
let token_url = get_tag(&event, "u").ok_or_else(|| Nip98Error::MissingTag("u".into()))?;
let normalized_token = token_url.trim_end_matches('/');
let normalized_expected = expected_url.trim_end_matches('/');
if normalized_token != normalized_expected {
    return Err(Nip98Error::UrlMismatch { ... });
}
```

- **Trailing slash tolerated.**
- **Scheme/case-sensitive** — exact byte match after trimming.
- **Query string IS preserved** — no normalisation. A reordered query
  string would break (good).
- **Fragment**: not stripped. URL fragments are never sent to the server
  in HTTP, so this is fine — the client should not include them.

**Gap:** the comparison is partial in the sense that an attacker can
substitute a host-equivalent URL. Example:
- Signed: `https://api.dreamlab-ai.com/foo`
- Sent to: `https://api.dreamlab-ai.com:443/foo` (default port made
  explicit)

Both string-different. Reject? Probably yes (good — no attempt to
normalise default ports). **OK.**

| Severity | Finding | Citation | Recommendation |
|---|---|---|---|
| INFO | NIP-98 `u` tag matches expected URL string (after trailing-slash trim); no false-positive normalisation | `nostr-core/src/nip98.rs:276-284` | Document as canonicalisation contract; consider adding scheme-and-default-port normalisation if false-negatives appear in practice |
| INFO | DPoP htu check assumed OK | `solid-pod-rs/.../oidc/mod.rs:366-432` | Verify line-by-line in follow-up |

---

## Drift table

Where do the four substrates implement the same thing differently?

| Concern | Forum | VisionClaw | Agentbox | solid-pod-rs | Drift severity |
|---|---|---|---|---|---|
| SSRF guard | hostname-based, in WASM (`preview-worker/src/ssrf.rs`) | none wired | n/a (build-time pinned) | DNS-resolved, configurable, tested (`security/ssrf.rs`) | MEDIUM |
| Path traversal | implicit (R2 + parse_pod_route) | substring `find("/solid")` (S2.2) | path.join with sig-verified ids | percent-double-decode middleware + `is_path_allowed` | HIGH |
| NIP-98 verifier | `nostr-core` canonical + 4 worker wrappers | `solid-pod-rs::auth::nip98` (delegated) | nostr-tools (not NIP-98 specifically) | `solid-pod-rs::auth::nip98` (484 LOC, body hash bug in actix wrapper) | MEDIUM (impl drift) |
| Replay store | KV-backed `KvReplayStore` (×4 workers) | NONE | implicit content-addressed file dedup | DPoP LRU (process-local) | HIGH (VisionClaw missing) |
| WAC ACL evaluator | `pod-worker/src/acl.rs` (no groups, no normalisation) | delegated to solid-pod-rs WAC | sovereign-bootstrap writes ACLs | full WAC 2.0 with groups, conditions, origin | HIGH (forum is reduced subset) |
| Agent IRI form | `did:nostr:<hex>` | `{base}/{pubkey}/profile/card#me` (WebID) | `did:nostr:<hex>` | both supported | HIGH (cross-form ACL mismatch) |
| Rate limiting | per-IP, KV sliding window | n/a in handlers checked | per-pubkey `messages_per_sec` + `rate_limit` | feature-gated `rate-limit` LRU | MEDIUM |
| CORS posture | drift across 5 workers (1 vs 3 origins) | `CORS_ALLOWED_ORIGINS` env, prefix-match WS bug | CORS env in management-api | per-deployment | HIGH |
| Dotfile guard | per-route prefix (no generic guard) | 3-entry allowlist (missing `.account`, `.quota.json`) | delegates | 3 default + 6 free-fn | MEDIUM |
| RNG for security | `getrandom` ✓ except NIP-42 challenge using `Math.random` | nostr-sdk delegates | `crypto.randomBytes` ✓ | `getrandom` ✓ | HIGH (NIP-42) |
| NIP-44 conv-key | wrong (HKDF-Expand instead of -Extract — C1 from §05) | delegated | n/a | n/a | CRITICAL (existing finding) |
| Privkey custody | passkey/PRF in mem (good) | env var (no zeroisation) | filesystem (perms unverified) | jwks file | MEDIUM |
| Constant-time hash compare | NOT constant-time on body hash (M15) | inherits | n/a | inherits | MEDIUM |
| Origin-bound credentials | NIP-98 `u` tag exact match ✓ | inherits | n/a | DPoP `htu` (assumed ok) | LOW |

---

## Quick-win patches (single-line fixes for HIGH-severity issues)

1. **VisionClaw substring traversal** — `src/handlers/solid_pod_handler.rs:371`:
   replace `match full.find("/solid")` with `match full.strip_prefix("/api/solid").or_else(|| full.strip_prefix("/solid"))`. Eliminates the `solid-evidence` substring trap.

2. **VisionClaw WS origin prefix-match** —
   `src/handlers/fastwebsockets_handler.rs:192`:
   replace `origin_str.starts_with(allowed.trim())` with
   `origin_str == allowed.trim()`. Eliminates `dreamlab-ai.com.attacker.com`
   bypass.

3. **NIP-42 challenge RNG** —
   `crates/relay-worker/src/relay_do/session.rs:284-292`:
   replace the `Math::random()` body with a `crypto.subtle.getRandomValues`
   call via `js_sys::global().crypto().get_random_values_with_u8_array(&mut buf)`.

4. **NIP-98 body-hash CT compare** —
   `crates/nostr-core/src/nip98.rs:304`:
   replace `if expected_hash.to_lowercase() != actual_hash.to_lowercase()` with
   `if !subtle::ConstantTimeEq::ct_eq(expected_hash.to_lowercase().as_bytes(), actual_hash.to_lowercase().as_bytes()).into()`.
   Add `subtle = "2"` to Cargo.toml.

5. **NIP-98 lowercase pubkey enforcement** —
   `crates/nostr-core/src/nip98.rs:258`:
   add line `if event.pubkey != event.pubkey.to_lowercase() { return Err(Nip98Error::InvalidPubkey); }`.

6. **VisionClaw replay store** —
   `src/handlers/solid_pod_handler.rs:386-399`:
   add a process-local `Arc<Mutex<HashMap<String, Instant>>>` and check
   before passing to verify. TTL = 120s. Or wire
   `solid_pod_rs::oidc::DpopReplayCache`-style cache from the same crate.

7. **VisionClaw / forum agent IRI alignment** —
   `src/handlers/solid_pod_handler.rs:140-141`:
   replace `derive_webid(...)` with `format!("did:nostr:{pk}")` so ACLs
   provisioned with `did:nostr:` form match. Or add `acl:agent` entries
   for both forms in `provision.rs` everywhere a default ACL is written.

8. **Agentbox identity file permissions** —
   `agentbox/scripts/sovereign-bootstrap.py` (line where identity JSON is
   written): add `os.chmod(identity_file, 0o600)` post-write. Verify
   via `stat` in CI.

9. **Forum auth-worker / pod-worker CORS alignment** —
   `crates/auth-worker/wrangler.toml`, `crates/pod-worker/wrangler.toml`:
   either change `EXPECTED_ORIGIN` to a multi-origin allowlist matching
   relay/search OR write an ADR documenting the intentional narrowness.

10. **solid-pod-rs-server body-hash plumbing** —
    `solid-pod-rs/crates/solid-pod-rs-server/src/lib.rs:174-187`:
    pre-buffer body up to `body_cap`, pass `Some(&body)` to
    `nip98::verify` instead of `None`.

---

## CI gate proposals — what should automated checks enforce going forward

### CI-G1: SSRF guard parity test

For every commit touching `crates/preview-worker/src/ssrf.rs` OR
`solid-pod-rs/crates/solid-pod-rs/src/security/ssrf.rs`, run a property
test asserting both implementations classify a shared corpus of URLs
identically:

```text
- 169.254.169.254 → BlockedClass(Reserved)
- 169.254.169.254:8080 → BlockedClass(Reserved)
- [::ffff:127.0.0.1] → BlockedClass(Loopback)
- [::ffff:7f00:1] → Block (forum) or Reserved/IpClass::Loopback (solid-pod-rs)
- metadata.google.internal → Block
- 0x7f000001 → Block (forum); test in solid-pod-rs
```

Diverging classifications fail CI.

### CI-G2: NIP-98 verifier conformance suite

A shared corpus of NIP-98 tokens (valid, expired, replayed, body-mismatch,
url-mismatch, kind-mismatch, prefix-typo, base64-padding, mixed-case
pubkey) tested against:
- `nostr-core::verify_token_at`
- `solid_pod_rs::auth::nip98::verify_at`

Same input → same Verdict (or both Err). Detects implementation drift.

### CI-G3: Lowercase-hex enforcement

A clippy lint OR shell grep that fails if any file outside test code
constructs a `did:nostr:` URI from a non-lowercased hex string. Pattern:
`format!("did:nostr:{}", x)` where `x` is not statically known to be
lowercase. Hard to write as a perfect lint; suggest a `make_did_nostr(hex)`
helper that calls `to_lowercase()` and lint-banning the format-macro
form.

### CI-G4: Replay store contract conformance

Each `Nip98ReplayStore` impl must pass a contract test: 1000 concurrent
calls with the same id should result in EXACTLY ONE `Ok(true)` and 999
`Ok(false)`. KvReplayStore will fail this today (race). The contract
test forces the issue.

### CI-G5: Path traversal corpus

Each path-handling function (`is_path_allowed`, `parse_pod_route`,
`extract_solid_path`) tested against:
- `..`, `../`, `..%2f..`, `%2e%2e/etc`, `%252e%252e/etc`
- `\..\`, `..%5c..`
- `%00`, `\0`
- `.well-known/../etc`
- segment-injection via `Slug` header

Expected: 400 / Err on all. Drift = test fails.

### CI-G6: CORS allowlist parity

CI script reads `EXPECTED_ORIGIN` / `ALLOWED_ORIGINS` from all 5
`wrangler.toml`. Asserts the set of accepting origins is identical, or
flags drift to be acknowledged in a `cors-policy.md` doc.

### CI-G7: agent IRI form matrix

For each substrate, compute the agent_uri produced from a fixed pubkey
hex. Assert all four substrates produce the same IRI string. Or
document the dual-form mapping in a single source of truth.

### CI-G8: Privkey at-rest permissions

A post-bootstrap test in agentbox CI: `stat -c '%a' /var/lib/agentbox/identities/*.json`
must equal `600`. Fail if any is world- or group-readable.

### CI-G9: RNG sourcing audit

A grep-based lint in CI: any new occurrence of `Math::random()` or
`js_sys::Math::random` outside `tests/` modules fails the build. Replace
with `crypto.getRandomValues`.

### CI-G10: Constant-time compare audit

A grep-based lint: any string compare on a body-hash or HMAC output that
is NOT routed through `subtle::ConstantTimeEq` fails. Specifically flag
`hash.to_lowercase() == ...` patterns near `nip98`, `nip44`, `nip04`.

---

## Test surface gaps

### Unit tests missing

- **NIP-98 with mixed-case pubkey** (forum nostr-core): no test asserts
  `event.pubkey = "ABCDEF..."` is rejected. Add to nip98.rs tests.
- **NIP-98 with leading/trailing whitespace in URL `u` tag**: no test.
- **`extract_solid_path` substring traversal**: no test in
  `src/handlers/solid_pod_handler.rs`. Add cases:
  `"/users/solid-evidence/foo"`, `"/api/v1/solid-tools/x"`,
  `"/v2/solid"`.
- **VisionClaw WS origin prefix bypass**: no test asserts
  `https://dreamlab-ai.com.attacker.com` is rejected.
- **NIP-42 challenge predictability**: no statistical test on entropy.

### Integration tests missing

- **Cross-worker NIP-98 replay**: send the same token to auth-worker, then
  pod-worker. With separate KV namespaces, second succeeds. Test
  documents the gap or asserts shared-namespace.
- **WAC ACL form drift**: provision an ACL with `did:nostr:<hex>` agent,
  then access via VisionClaw (which sends WebID-form agent IRI). Assert
  403, document the drift.
- **Path traversal end-to-end**: through actix-web → `solid_pod_handler`
  → FsBackend. `%2e%2e/etc/passwd` request must 400 not 200.
- **Symlink escape**: write a symlink in POD_DATA_ROOT pointing to
  `/etc/passwd`; GET the path; assert 404 not 200.

### Property-based tests missing

- **Path normalisation idempotence**: `is_path_allowed(p) == is_path_allowed(normalise(p))`
  for any `p`.
- **SSRF classification totality**: for any `IpAddr`, classify returns
  exactly one variant. (solid-pod-rs claims totality at `security/ssrf.rs:43-45`
  but a property test would prove it.)
- **NIP-98 round-trip preserves all fields**: create_token →
  verify_token returns the same `Nip98Token`.

### Fuzz tests missing

- **NIP-98 token parser**: AFL/honggfuzz harness for
  `verify_token_at(<random>, "https://x", "GET", None, 0)`. Catches
  panics in base64, JSON, hex decoders.
- **JSON-LD ACL parser**: feed `parse_jsonld_acl` random byte streams up
  to 1MB. Catches OOM / panic.
- **`extract_solid_path`**: feed random URI-encoded strings; assert no
  panic and result is properly normalised.

### Adversarial / red-team scenarios missing

- **Time-skew adversary**: attacker controls one clock to be 90s behind;
  signs NIP-98 token; replays after server clock catches up. Should
  fail (expired) but verify the asymmetry.
- **DPoP key-rotation attack**: old DPoP key still has valid signed
  tokens; rotate JWKS; assert old tokens reject.
- **WebAuthn cross-device PRF mismatch**: register on Device A, attempt
  hybrid (QR) login on Device B; assert reject (per Hazard 1 in §05 §10).
- **Capacity-driven DPoP replay**: spawn 10_001 unique DPoP tokens;
  re-submit token #1; should be evicted from LRU and accepted as fresh.
  Document the capacity choice driven by RPS.

---

## Cross-cutting recommendations summary

1. **Single SSRF library**: promote `solid_pod_rs::security::SsrfPolicy`
   to the canonical implementation. The forum WASM build needs a
   workers-rs adapter (no DNS in WASM today) — implement as a CF Worker
   Service Binding to a non-WASM SSRF probe endpoint.
2. **Single NIP-98 verifier**: deprecate `nostr-core::verify_token_at`
   and have the forum workers consume `solid-pod-rs::auth::nip98`. Add
   the `Nip98ReplayStore` trait there. Eliminates the dual-implementation
   bookkeeping cost.
3. **Single path-traversal guard**: solid-pod-rs `PathTraversalGuard`
   middleware is the canonical. Forum pod-worker should add a similar
   per-segment normaliser. VisionClaw should adopt it.
4. **Agent IRI canonical form**: write an ADR. Recommendation:
   `did:nostr:<hex>` everywhere, with WebID URL only in WebID profile
   document `schema:identifier` (per `pod-worker/src/webid.rs:34`).
   ACL provisioning writes only `did:nostr:` form.
5. **Replay store contract test in nostr-core**: assert atomicity by
   running 1000 concurrent calls in tokio test. Forces KvReplayStore to
   move to DO or document the race.
6. **CORS posture ADR**: write
   `docs/adr/ADR-0XX-cross-substrate-cors-policy.md` enumerating which
   origins are accepted by which surface and why.
7. **Constant-time everywhere a hash is compared**: lint in CI.
8. **CSPRNG everywhere a security-relevant random is needed**: lint
   `Math::random()` outside test code.
9. **Privkey storage hardening**: 0600 root:root for at-rest;
   `Zeroizing<…>` for in-process; verify `Debug` impls don't leak.

---

## File-path reference (absolute, for follow-up)

### Forum (community-forum-rs)
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip98.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip44.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip04.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/event.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/keys.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/gift_wrap.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip26.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/preview-worker/src/ssrf.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/preview-worker/src/parse.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/preview-worker/src/oembed.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/preview-worker/src/rate_limit.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/lib.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/acl.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/auth.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/did.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/auth-worker/src/lib.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/relay-worker/src/lib.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/relay-worker/src/relay_do/broadcast.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/relay-worker/src/relay_do/session.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/relay-worker/src/relay_do/nip_handlers.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/search-worker/src/lib.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/auth-worker/wrangler.toml`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/wrangler.toml`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/relay-worker/wrangler.toml`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/search-worker/wrangler.toml`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/preview-worker/wrangler.toml`

### VisionClaw substrate
- `/home/devuser/workspace/project/src/main.rs`
- `/home/devuser/workspace/project/src/handlers/solid_pod_handler.rs`
- `/home/devuser/workspace/project/src/handlers/fastwebsockets_handler.rs`
- `/home/devuser/workspace/project/src/services/server_identity.rs`
- `/home/devuser/workspace/project/src/services/nostr_bridge.rs`

### Agentbox
- `/home/devuser/workspace/project/agentbox/agentbox.toml`
- `/home/devuser/workspace/project/agentbox/management-api/lib/uris.js`
- `/home/devuser/workspace/project/agentbox/mcp/auth/auth-middleware.js`
- `/home/devuser/workspace/project/agentbox/mcp/nostr-bridge/relay-consumer.js`
- `/home/devuser/workspace/project/agentbox/scripts/sovereign-bootstrap.py`

### solid-pod-rs
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs/src/security/mod.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs/src/security/ssrf.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs/src/security/dotfile.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs/src/security/cors.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs/src/security/rate_limit.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs/src/auth/nip98.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs/src/oidc/replay.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs/src/oidc/mod.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs/src/wac/mod.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs/src/wac/conditions.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs/src/wac/origin.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-server/src/lib.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-nostr/src/resolver.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-idp/src/jwks.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-git/src/auth.rs`

---

## Severity roll-up

### CRITICAL
- (None new beyond existing §05 C1 NIP-44 conv-key bug, C2 agentbox npub
  format, C3 verificationMethod.type.)

### HIGH
- **S2.1** Forum pod-worker: ACL match against `..`-uncollapsed paths
  (HIGH).
- **S2.2** VisionClaw `extract_solid_path` substring match
  (`/users/solid-X` ambiguity); no percent-decode pass.
- **S3.5** solid-pod-rs-server actix wrapper drops body-hash check.
- **S4.1** KvReplayStore TOCTOU race (CF KV not CAS).
- **S4.4** VisionClaw lacks any NIP-98 replay store.
- **S5.2** Agent IRI form drift between forum (`did:nostr:`) and
  VisionClaw (WebID URL).
- **S8.2** VisionClaw WS origin uses `starts_with` (subdomain bypass).
- **S8.1** Forum auth/pod CORS allowlist drifts from relay/search.
- **S10** NIP-42 challenge uses `Math.random()`.
- **S12** Agentbox identity file permissions need verification.

### MEDIUM
- **S1.1** Forum `is_private_url` is name-based, not resolved-IP-based.
- **S2.3** Agentbox fallback mode accepts arbitrary `event.id` shapes.
- **S2.4** solid-pod-rs path guard doesn't normalise `\` separators.
- **S3.1** NIP-98 body-hash compare not constant-time.
- **S3.1** NIP-98 pubkey case not enforced lowercase.
- **S3.2** Worker fallback to `AlwaysFreshStore` is silent fail-open.
- **S3.2** Cross-worker replay isolation depends on KV id pinning.
- **S5.1** pod-worker `agent_matches` lacks group resolution.
- **S5.1** No agent IRI normalisation before string compare.
- **S6** No per-pubkey rate limiting; per-IP only.
- **S6** Fail-open on KV error for rate limit.
- **S6** Agentbox auth-middleware clientId `ip:port` is per-connection.
- **S7** VisionClaw allowlist lacks `.account` and `.quota.json`.
- **S8.1** CORS preflight cached 24h slows revocation.
- **S9** NIP-98 prefix `"Nostr "` case-sensitive.
- **S9** Bearer scheme strip case-sensitive.
- **S12** VisionClaw `SERVER_NOSTR_PRIVKEY` not zeroised.
- **S12** SecretKey Debug impl may leak (verify needed).
- **S13** NIP-98 body-hash compare not constant-time.

### LOW / INFO
- (See inline severity tables above.)

---

## Closing observations

The four substrates share the security-primitives intent but ship parallel
implementations whose drift is now non-trivial. The dominant cost in this
audit is **proving each pair behaves identically** — which CI cannot today.
The shared library moves (S15 — solid-pod-rs as canonical for SSRF, NIP-98,
WAC, dotfile, CORS, rate-limit) are the clean fix. Workers-rs WASM target
is the constraint that prevents direct adoption today (no DNS, no
reqwest, no tokio in CF Worker). The two viable bridges are (a) Service
Binding to a non-WASM probe endpoint, (b) feature-flag the WASM build to
ship a reduced-functionality SSRF guard with guaranteed-equivalent
classification semantics (proved by shared property tests). Either is
strictly less expensive than maintaining two implementations in
perpetuity.

The largest residual risk in this audit is **VisionClaw's exposure to
NIP-98** without a replay store and with the substring-traversal mount.
Closing those two by sprint-end yields the largest security ROI.

End of Q2 audit.
