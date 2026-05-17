# Cross-System Cryptographic Alignment

> Scope: VisionClaw substrate (`/home/devuser/workspace/project`), Dreamlab forum
> (`./dreamlab-ai-website/community-forum-rs/`), Agentbox container
> (`./agentbox/`), shared `solid-pod-rs/` workspace. All paths absolute.
>
> Identity glue: `did:nostr:<64-hex-pubkey>` is shared across all three. Bech32
> `npub1…` is wire-only. `urn:visionclaw:*` (6 kinds, src/uri/) and
> `urn:agentbox:*` (18 kinds, management-api/lib/uris.js) are parallel
> namespaces. The forum signs Schnorr secp256k1 BIP-340; agentbox signs ECDSA
> SECP256k1 (sovereign-bootstrap.py uses `ecdsa.SigningKey`); VisionClaw
> server delegates to `nostr-sdk`.
>
> Findings sorted by RFC 8141 / DID Core / NIP severity. Verdicts assume the
> three systems must accept events signed by the same hex pubkey as the same
> identity. Where they don't, that's drift.

## 1. Key formats (hex pubkey vs bech32 npub vs xonly serialisation)

**STATUS: drifted (CRITICAL on agentbox, aligned forum↔visionclaw).**

The forum and VisionClaw substrate both treat the **64-char lowercase hex
x-only Schnorr pubkey** as canonical; bech32 `npub1…` is decoded only at
NIP-19 wire ingress. Evidence:

- `dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/keys.rs:53-56` —
  `PublicKey::public_key()` derives from `k256::schnorr::SigningKey` and
  serialises as 32-byte x-only.
- `src/uri/parse.rs:222-253` — `validate_hex64` + `normalise_pubkey` accept
  hex / `did:nostr:<hex>` / `npub1…` and re-emit canonical 64-char hex.
- `agentbox/management-api/lib/uris.js:96-101, 177-199` — `_normalisePubkey`
  documents the same convention: hex is canonical, npub is best-effort.

**Agentbox sovereign-bootstrap.py is the outlier and is structurally wrong.**
`scripts/sovereign-bootstrap.py:81-92, 123-135` calls `ecdsa.SigningKey` on
SECP256k1, then bech32-encodes the **64-byte uncompressed public key** as
`npub` and the 32-byte private key as `nsec`. NIP-19 npub MUST encode the
32-byte x-only pubkey (BIP-340), not the 64-byte (X||Y) SEC1 raw encoding.
A consumer that decodes this `npub` with `nostr-tools::nip19.decode` or
`nostr-core::decode_npub` will reject it (length mismatch) or worse, accept
the first 32 bytes as a corrupted x-only pubkey that does not match
`AGENTBOX_X_ONLY_PUBKEY_HEX`.

Agentbox stores `x_only_pubkey_hex = public_bytes[:32].hex()` (line 89, 132,
217), then uses the (broken) 64-byte npub in pod filesystem paths (line 141,
176, 200) and the x-only hex in the DID URI (line 156). The two will not
round-trip through any standards-compliant Nostr decoder.

| evidence | severity |
|---|---|
| `agentbox/scripts/sovereign-bootstrap.py:90-91, 133-134` | **CRITICAL** |
| `nostr-core/src/keys.rs:163-166, 185-197` | aligned |
| `src/services/server_identity.rs:171-180, 397` | aligned (nostr-sdk handles it) |

**Recommendation:** rewrite sovereign-bootstrap.py to compute the BIP-340
x-only pubkey by **lift_x** parity (drop the y coordinate, force even-y) and
bech32-encode that 32-byte buffer. Until fixed, agentbox npubs are
**not interoperable** with the forum or with any external Nostr relay.

## 2. did:nostr method identifier (NostrSchnorrKey2024 vs SchnorrSecp256k1VerificationKey2019 vs ...2022)

**STATUS: drifted (CRITICAL).** Three different `verificationMethod.type`
strings exist in the codebase right now. A client receiving DID documents
from all three will reject two of them.

| Emitter | `verificationMethod[0].type` | Evidence |
|---|---|---|
| Forum pod-worker | `SchnorrSecp256k1VerificationKey2019` | `dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/did.rs:88, 146` |
| solid-pod-rs-nostr | `NostrSchnorrKey2024` | `solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs:98, 154` |
| Agentbox sovereign-bootstrap | `SchnorrSecp256k1VerificationKey2022` | `agentbox/scripts/sovereign-bootstrap.py:192` |
| VisionClaw substrate | (does not emit DID documents — see §13) | n/a |

The forum was patched in Sprint v9 (memory key `sprint-v9-stream-a-status`)
but the underlying solid-pod-rs crate at v0.4.0-alpha.2 was not — pod-worker
embeds a hand-rolled copy. The agentbox bootstrap uses a non-existent
`...2022` cryptosuite (W3C only registered `...2019`).

**Risk:** any external W3C VC verifier that receives the agentbox DID doc
will fail on the unknown `type`. Worse, a future generic resolver that
accepts only `...2019` will reject `NostrSchnorrKey2024` from
solid-pod-rs-nostr.

**Recommendation:** standardise on `SchnorrSecp256k1VerificationKey2019`
across all three emitters. Patch:

1. `solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs:98, 154` →
   `SchnorrSecp256k1VerificationKey2019`. Add `https://w3id.org/security/suites/secp256k1-2019/v1`
   to Tier-1 `@context` (line 93, currently only `did/v1`).
2. `agentbox/scripts/sovereign-bootstrap.py:192` → drop `...2022`, use
   `...2019`.
3. Add a CI assertion in each repo: parse own DID Doc and assert
   `vm[0].type == "SchnorrSecp256k1VerificationKey2019"`.

## 3. JSON-LD @context drift between Tier-1 / Tier-3 docs

**STATUS: drifted (HIGH).**

| Doc | Tier-1 @context | Tier-3 @context |
|---|---|---|
| pod-worker | `did/v1` + `secp256k1-2019/v1` ✓ | same ✓ (`did.rs:80-83, 138-141`) |
| solid-pod-rs-nostr | `did/v1` only ✗ (`did.rs:93`) | `did/v1` + `secp256k1-2019/v1` ✓ (`did.rs:147-149`) |
| agentbox-bootstrap | `did/v1` + `secp256k1-2019/v1` ✓ | n/a (single-tier) |

The solid-pod-rs-nostr Tier-1 doc declares a `NostrSchnorrKey2024` term but
omits the security suite that defines it — a strict JSON-LD processor will
silently ignore the term, leaving the verificationMethod **untyped** in the
expanded form.

**Recommendation:** every renderer must include both contexts:

```json
"@context": [
  "https://www.w3.org/ns/did/v1",
  "https://w3id.org/security/suites/secp256k1-2019/v1"
]
```

Pod-worker bundles these via `contexts.rs::include_str!` per Sprint v8 W4 —
solid-pod-rs-nostr should adopt the same bundling pattern so contexts are
not fetched at runtime (PRD-006 / agentbox CLAUDE.md "context documents
pinned at build time").

## 4. Event signing (Schnorr secp256k1 BIP-340) consistency across signer impls

**STATUS: aligned for forum/VisionClaw, hazardous for agentbox.**

- forum `nostr-core/src/keys.rs:60-70` — `SecretKey::sign` uses
  `k256::schnorr::SigningKey::sign_raw` with **random aux_rand** sourced
  from the caller (defaulted to all-zero in `sign`, line 63). `event::sign_event`
  (event.rs:114) uses random aux. Recomputes id from canonical JSON
  (`relay.rs:48-58`).
- VisionClaw `src/services/server_identity.rs:201-204` — delegates to
  `nostr-sdk::EventBuilder::sign_with_keys`, which uses BIP-340 + random aux.
- agentbox sovereign-bootstrap.py uses **ECDSA** (`from ecdsa import SECP256k1, SigningKey`)
  for keypair derivation only — the bootstrap never actually **signs**
  anything itself. But the persisted `private_key_hex` is then handed to
  downstream consumers who treat it as a Schnorr key. This is fine
  numerically (the scalar is the same), as long as no consumer calls
  `ecdsa.sign()` and expects the result to verify under k256 Schnorr.

**Sub-issue: aux_rand sourcing in nostr-core key signing path.**
`keys.rs:63` hard-codes `aux_rand = [0u8; 32]`. BIP-340 §3.3 explicitly
allows an all-zero aux for deterministic signing **but warns** that this
removes the synthetic-randomness defence against fault attacks. Production
code paths (`sign_event` in event.rs line 114) override this with `getrandom`
— the keys.rs path is only reached by `Keypair::sign` test-helpers. Verify
no production caller bypasses event.rs.

**Recommendation:** add a deny-list lint banning direct
`SecretKey::sign(...)` calls outside test/benchmark code, forcing all
production signing through `event::sign_event` which sources random aux.

## 5. NIP-98 HTTP auth (token shape, payload hash, replay window, replay store)

**STATUS: aligned + hardened (LOW).** Sprint v9 closed the major gaps.
Remaining items are minor.

Implementation: `nostr-core/src/nip98.rs` (forum). Wire:
`Authorization: Nostr <base64(JSON(signed_event))>` where event is kind
27235, tags `["u", url]`, `["method", METHOD]`, optional `["payload", hex(SHA-256(body))]`.

Verified properties:
- Pubkey in event is 64-hex (line 258).
- Timestamp tolerance ±60s (`TIMESTAMP_TOLERANCE` line 20).
- ID recomputed from canonical form, never trusted from input
  (line 271 `verify_event` recomputes; line 352 caches the **canonical**
  id, not client-claimed).
- Body hash verified after structural checks (line 297-313).
- URL trailing-slash normalised (line 277).
- Method case-insensitive (line 289).
- **Replay protection: `Nip98ReplayStore` trait + `KvReplayStore`**
  (`auth-worker`, `pod-worker`, `relay-worker`, `search-worker`) wired in
  Sprint v9 STREAM-B. TTL = `2 * TIMESTAMP_TOLERANCE = 120s` (line 26).
  Cache key is the canonical event id (line 352-357). Test coverage at
  lines 871-958 confirms first-use ok, second-use rejected, distinct
  events independent.

**Gaps:**

1. **Cross-system replay isolation.** Each worker has its own
   `KvReplayStore` namespace (`NIP98_REPLAY` in each `wrangler.toml`).
   A token validated by auth-worker can be replayed against pod-worker
   within the 120s window because the KV namespaces are not shared.
   Severity: **MEDIUM**. NIP-98 binds the URL into the signed event, so
   the only way to "replay" cross-worker is if both workers share the
   same hostname + path — which they don't on Cloudflare (each worker
   has its own route). But an internal proxy or caching layer that
   rewrites URLs (e.g. `pods.dreamlab-ai.com/api/foo` → `auth-worker`)
   could enable this. **Recommendation:** introduce a single shared
   `NIP98_REPLAY` KV binding across all four workers. The cost is one
   extra KV namespace; the benefit is replay correctness under URL
   rewriting.

2. **No replay protection between forum and VisionClaw.** VisionClaw
   does not implement NIP-98 verification — there is no equivalent of
   `verify_token_at_with_replay` on the substrate side. If VisionClaw
   ever exposes an HTTP API that accepts forum-signed NIP-98 tokens, it
   will be vulnerable to replay. Severity: **MEDIUM** (latent —
   VisionClaw currently accepts no NIP-98 traffic).

3. **VisionClaw does not implement NIP-98 verification _at all_.**
   `src/handlers/solid_pod_handler.rs` is being modified per the git
   status. If that file accepts authenticated requests, the verifier
   wiring belongs there. Severity: **HIGH** if the handler is exposed
   without NIP-98 enforcement.

## 6. NIP-04 vs NIP-44 DM encryption (deprecation status, conversation-key correctness)

**STATUS: drifted (CRITICAL).** The NIP-44 conversation-key derivation in
`nostr-core/src/nip44.rs:122-128` is **not the NIP-44 v2 reference
algorithm**.

Reference (NIP-44 §2.1): `conversation_key = HKDF-Extract(salt="nip44-v2", IKM=ECDH-x)`.
HKDF-Extract returns the PRK directly — 32 bytes equal to
`HMAC-SHA256(salt, IKM)`.

Forum implementation:
```rust
let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), &shared_point);
let mut conv_key = [0u8; 32];
hk.expand(&[], &mut conv_key)?; // <-- computes HKDF-Expand(PRK, "", 32)
```

`Hkdf::<Sha256>::new(salt, ikm)` calls HKDF-Extract internally and stores
the PRK; `.expand(info, okm)` then runs HKDF-Expand. The forum's
`conv_key = HKDF-Expand(PRK, info=empty, L=32) = HMAC-SHA256(PRK, 0x01)`,
**not** `PRK`. This means messages encrypted by the forum are **not
decryptable by reference NIP-44 implementations** (nostr-tools,
nostr-rs-relay, NDK), and vice-versa.

This is invisible inside the forum because both `encrypt` and `decrypt`
call the same `conversation_key`, so the round-trip works — but it means
the forum's NIP-44 DMs are **a fork**, not interoperable.

| evidence | severity |
|---|---|
| `nostr-core/src/nip44.rs:99-128` (wrong derivation) | **CRITICAL** |
| `nostr-core/src/nip44.rs:144-149` (correct padding ✓) | aligned |
| HMAC verification before decrypt (line 213-216) | aligned ✓ |
| `nip04.rs:43-79` (correct ECDH+SHA-256, AES-CBC) | aligned ✓ |
| gift_wrap.rs uses nip44 (lines 189, 242, 322, 339) | inherits the bug |

**Recommendation:** replace lines 122-128 of nip44.rs with:

```rust
use hmac::{Hmac, Mac};
let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(HKDF_SALT)?;
mac.update(&shared_point);
let mut conv_key = [0u8; 32];
conv_key.copy_from_slice(&mac.finalize().into_bytes());
```

This computes the correct HKDF-Extract output. Add a NIP-44 v2 reference
test vector (the canonical "alice"/"bob" vectors from
`github.com/paulmillr/nip44/blob/main/javascript/test/vectors.json`) to
prevent regression.

NIP-04 is correctly implemented but **deprecated** by the NIP authors.
Forum kind-4 events still flow through nip04 (`gift_wrap.rs:373-387` —
`process_kind4_event`), and pod-worker / forum-client ship forwards
compatibility.  NIP-07 signer fallback at `forum-client/src/auth/nip07.rs:188-207`
**silently routes NIP-04 calls through NIP-44** — which means an extension
that supports only NIP-04 will produce undecryptable output, and
vice-versa. This matters for cross-extension DM compatibility.

## 7. NIP-59 gift-wrap envelope (kind 1059, kind 13 seal, kind 14 chat)

**STATUS: aligned but inherits §6 bug.**

`nostr-core/src/gift_wrap.rs` correctly implements the three layers:

- Rumor: kind 14, sender_pk, real timestamp, no `id`/`sig`. Line 156-163.
- Seal: kind 13, sender_sk-signed, NIP-44(rumor), randomised ts ±48h.
  Line 179-208.
- Gift wrap: kind 1059, throwaway-key-signed, NIP-44(seal), `["p", recipient]`
  tag for routing. Line 226-266.

Throwaway secret zeroised after signing (line 261-263 — but the keypair's
`SecretKey` already zeroises on drop via `Zeroize` derive in keys.rs:34-38;
the second `zeroize` call is belt-and-braces).

Timestamps obfuscated via 48h jitter (`randomized_timestamp` line 108).

**Inherits the NIP-44 conversation-key bug from §6**: forum gift-wrap is
not interoperable with reference NIP-59 implementations.

**Routing trade-off (the question raised in the brief):** kind 1059 hides
sender identity and the inner kind from the relay. A private relay's
allowlist can either:
- (a) gate kind 1059 globally (simple — but accepts any DM payload kind
  hidden inside, including kind 14 chat and any future kind);
- (b) require AUTH and check `["p", recipient_pubkey]` matches the
  authenticated session's pubkey — which the forum already does, see
  `relay-worker/src/relay_do/nip_handlers.rs:348-387`.

The forum's choice is (b): the relay enforces that REQ filters touching
kind 1059 must come from an authed session whose pubkey appears in the
event's `p` tag. This **prevents cross-recipient leakage** and is the
right default. But it means the relay cannot enforce policy on the
**inner** rumor kind — anyone can wrap any kind inside kind 1059. Document
this explicitly: gift-wrap is metadata-private, not content-policed.

## 8. NIP-26 delegation (validity windows, kind filters)

**STATUS: aligned (LOW), with one wire-format extension.**

`nostr-core/src/nip26.rs` implements the spec:

- Hash domain: `"nostr:delegation:" || delegatee_pk_hex || ":" || conditions_str`
  (line 282) — matches NIP-26.
- Conditions parser: `kind=N`, `created_at>T`, `created_at<T` (line 64-95).
  Boundary semantics strict (`>` excludes equal, `<` excludes equal),
  matches the spec. Tested at line 326-332.
- Delegation tag: `["delegation", delegator_pk, conditions, sig]`
  (line 239-244). Conformant.

**Extension (non-blocking):** `DelegationTag::from_token` produces a
**5-element** tag (delegatee added at index 4, line 244) so
`DelegationToken::from_tag` can verify without out-of-band context. The
spec is silent on extra elements — most clients ignore them. Compatible
both ways.

**Cross-system question — "if user X holds passkey-derived nsec, can their
forum login key sign events that agentbox accepts as the SAME identity?"**
Yes IF agentbox runs the same Schnorr verification — the secret-derived
hex pubkey is the canonical identity. NO if agentbox tries to reverse the
agentbox-baked sovereign key (`/var/lib/agentbox/identities/<agent_id>.json`,
`AGENTBOX_PRIVKEY_HEX`) which is a **different** key. The clean answer:

- The forum holds **user keys** (PRF-derived, in-memory).
- Agentbox holds **agent keys** (filesystem-stored sovereign identity).
- VisionClaw holds **server keys** (`SERVER_NOSTR_PRIVKEY` / nsec).

These cannot and must not be the same key. **NIP-26 delegation is the
intended bridge:** the user's forum key signs a delegation token for the
agentbox agent key (or vice-versa) with `kind=N` constraints. The forum
already has `auth-worker /api/delegation/create` per Sprint v8 W6 plan.
The verifier wiring is missing on agentbox + VisionClaw. **Recommendation:**
add `DelegationToken::verify` calls in agentbox event ingest paths and in
VisionClaw `nostr_bridge.rs` (currently line 192 only verifies the
straight-forward signature, not delegation tags).

## 9. NIP-42 AUTH challenge (challenge-response vs token reuse)

**STATUS: aligned.**

`relay-worker/src/relay_do/nip_handlers.rs:432-484` implements the
challenge/response correctly:

- Server sends `["AUTH", <challenge>]` on connect (Sprint v8 W2).
- Client signs kind 22242 event with `["challenge", <token>]` and
  `["relay", <url>]` tags, content empty, fresh `created_at`.
- Server verifies: kind == 22242, signature, challenge tag matches
  session-stored challenge, timestamp within 600s (line 467 — note: NIP-42
  recommends ≤10 minutes, this matches).
- On success, `session.authed_pubkey` is set and persisted to DO storage
  (line 481).

**One-shot per connection: yes.** The challenge is set once when the
session is created; reusing the same AUTH event in a second connection
fails because that session has a different challenge. There is **no
explicit anti-replay store for kind 22242** — none is needed because the
challenge binds the event to a specific session.

**Cross-system: AUTH gate ALL writes? Read-without-auth which kinds?** The
forum currently:
- Reads: most kinds open. **Kind 1059 reads require AUTH** + `#p`
  rewriting (line 348-387) so a user only sees their own DMs.
- Writes: all writes are signature-checked at ingest; AUTH is not strictly
  required to publish, but moderation and zone-access checks
  (`auth::is_admin`, `trust::has_zone_access` at line 401-405) require an
  authed session.

**Recommendation for the private federated relay:** gate **all writes**
behind NIP-42 (require `session.authed_pubkey == event.pubkey` for ingest
of any kind from non-server entities). Allow reads of kinds {0, 3, 1, 7,
30000-39999} without AUTH; gate {1059, 14, 13, 4} behind AUTH. The kind
14/13 case is academic (they should never appear on a private relay
because they're wrapped inside 1059) but explicit allow-listing prevents
configuration drift.

## 10. PRF/passkey → nsec derivation path (HKDF salt, extractability)

**STATUS: aligned with one usability hazard.**

Forum derivation (`nostr-core/src/keys.rs:185-197`):

```rust
fn derive_from_prf(prf_output: &[u8; 32]) -> Result<Keypair, KeyError> {
    let hk = Hkdf::<Sha256>::new(Some(&[]), prf_output);
    let mut okm = [0u8; 32];
    hk.expand(b"nostr-secp256k1-v1", &mut okm)?;
    let secret = SecretKey::from_bytes(okm)?;
    ...
}
```

- IKM = WebAuthn PRF output (32 bytes from `hmac-secret` extension).
- Salt = empty (`Some(&[])`) — empty salt is valid per RFC 5869 but
  conventionally a domain-separator string is used instead. Empty salt
  means HKDF-Extract reduces to `HMAC-SHA256("", PRF)`. Fine because
  the `info = "nostr-secp256k1-v1"` provides the domain separator.
- Validation: `SecretKey::from_bytes` rejects scalar=0 and scalar≥n
  (line 43-47). The retry path in `generate_keypair` (line 199-215) is
  not reachable from `derive_from_prf` — if HKDF output happens to be
  invalid, `derive_from_prf` returns `KeyError::InvalidSecretKey` and the
  user must re-register. Probability ≈ 2⁻¹²⁸. Acceptable.

PRF salt is **server-issued and bound to (pubkey, credentialId)** post
Sprint v9 fix C2 (audit memory: `discover_pubkey_from_passkey` removed,
`/auth/login/options` no longer leaks salts to unauthenticated callers).
See `forum-client/src/auth/passkey.rs:206-242` — the deprecated discover
path is gone, callers must supply pubkey explicitly.

**Hazard 1 (Documented):** Hybrid (cross-device QR) WebAuthn does NOT
support PRF. The forum **detects and rejects** hybrid transports
(`passkey.rs:249` calls `check_hybrid_transport`). Good — but the error
message at line 363-372 misleadingly says "the PRF extension required
for Nostr key derivation"; this is correct. Issue: a user who registered
on Device A and tries to sign in via QR on Device B will silently
re-derive a different key (different PRF output across authenticator
boundaries). The hybrid block at line 249 prevents this — verify the
block runs **before** any state mutation.

**Hazard 2 (sessionStorage):** Local-key (imported `nsec`) callers persist
the privkey to sessionStorage (`session.rs:96-109`). Marked deprecated;
zeroised on `pagehide` (line 319-352). Audit B8 hardening confirmed.
Passkey users never hit this path — their key re-derives on every login.
**LOW** severity, but `save_privkey_session` should hard-error in
release builds, not warn.

**HKDF info string mismatch?** keys.rs:9 declares `b"nostr-secp256k1-v1"`.
Doc comment at line 178-184 says JS uses
`new TextEncoder().encode('nostr-secp256k1-v1')`. Verify the JS code
matches — search for the literal in the website client. If it ever
diverges, every user re-registering with the new info string gets a
different key from their old key. **HIGH** severity if drift sneaks in.

## 11. Key rotation / revocation story (or absence)

**STATUS: missing (HIGH).**

There is **no key rotation path** in any of the three systems:

- Forum: a user's pubkey is the PRF output. Rotating means generating a
  new passkey, which yields a new pubkey — i.e. a new identity, not a
  rotation. There is no `kind 5` tombstone or `did:nostr` rotation
  mechanism.
- Agentbox: `sovereign-bootstrap.py:116-121` reads the persisted
  identity file once and never rewrites it. `AGENTBOX_PRIVKEY_HEX` env
  var rewrite (line 109-113) wipes-and-replaces, but no event signals
  "old key revoked". Downstream consumers caching the old pubkey will
  trust both keys until cache expiry.
- VisionClaw: `SERVER_NOSTR_PRIVKEY` is loaded once at startup
  (`server_identity.rs:64`). Restart with new key = new identity.

**No `did:nostr` revocation registry.** DID Core supports `deactivated:
true` in the DID document, but none of the three emitters set it.

**Recommendation (medium-term):**
1. Define a NIP-26-based rotation protocol: old key signs a delegation
   to new key with `kind=10000` (replaceable list) marker for "rotation
   announcement". Short window (e.g. 7 days) where both keys verify.
2. Add `kind 10002` (relay list metadata) + a custom `kind 30033`
   "identity rotation" replaceable event listing the new pubkey.
3. Pod-worker writes the rotation event into the user's pod under
   `/profile/rotations.jsonld`. WebID resolvers check it.

## 12. Scope identifier mismatch in URNs (hex pubkey vs npub) — RFC 8141 character class

**STATUS: aligned in code, drifted in legacy data (MEDIUM).**

CLAUDE.md states: "hex pubkey is the canonical scope form everywhere;
bech32 npub is only used at the Nostr-relay wire boundary and in legacy
pod filesystem paths." This is **enforced by code in agentbox** (uris.js
line 156, `_normalisePubkey`) and **enforced by code in VisionClaw**
(parse.rs line 222-253, mint.rs line 36, 49, 58, 79). It is **NOT
enforced in agentbox sovereign-bootstrap.py:141, 176, 200** — pod
filesystem paths use `identity['npub']` (the broken 64-byte npub from §1)
as the scope segment, which violates the canonical grammar.

RFC 8141 §2.1 NSS character class:
```
NSS  = pchar *(pchar / "/")
pchar = unreserved / pct-encoded / sub-delims / ":" / "@"
```

Bech32 `npub1…` is all `unreserved` (a-z + 0-9), so it's syntactically
legal as a NSS segment. **Hex pubkey is also all `unreserved`.** Neither
form breaks the grammar; the issue is **uniqueness**: an npub-scoped URN
will not match an equivalent hex-scoped URN under literal byte equality.
A resolver that indexes by URN must normalise scope before lookup, or
maintain dual indexes.

`src/uri/parse.rs:166-167` lowercases hex, accepting any case as input —
correct.
`src/uri/legacy.rs:37-49` retains `canonical_iri_npub` which mints
`visionclaw:owner:<npub>/kg/<sha256-64>` for backward-compat. ADR-054
documents the dual-index approach. OK.
**Risk** is that new code paths copy the legacy pattern. Add a clippy lint
(PRD-006 §6 anti-drift gate already mentions this) flagging
`format!("urn:visionclaw:...")` outside `src/uri/`.

## 13. WebID + DID interplay (which is canonical when?)

**STATUS: drifted (HIGH).**

The forum uses a **bidirectional carrier**:
- DID document publishes the WebID URL via `alsoKnownAs[0]`
  (`pod-worker/src/did.rs:113, solid-pod-rs-nostr/src/did.rs:119-122`).
- WebID profile (`pod-worker/src/webid.rs:34`) publishes the DID via
  `schema:identifier "did:nostr:{pubkey}"`.

The bidirectional check `verify_webid_tag` (did.rs:50-60) accepts EITHER
form when a NIP-98 event carries `["webid", uri]`:
- `did:nostr:<hex>` → hex must equal event pubkey
- `https://pods.dreamlab-ai.com/<hex>/...` → path segment must equal pubkey

Wired into pod-worker auth at `lib.rs:430` — good.

**Drift:** agentbox ACL writes use `did:nostr:<hex>` exclusively (`sovereign-bootstrap.py:156, 161-167`).
solid-pod-rs-nostr publishes `alsoKnownAs` as caller-supplied (`did.rs:119`)
without enforcement. VisionClaw `server_identity.rs` knows nothing about
WebID; it never publishes a DID document or a WebID profile.

**Canonical-when question:**
- For **authentication** (NIP-98): the `event.pubkey` (hex) is canonical.
  WebID and DID are both verified against it.
- For **profile lookup** (Solid): the WebID URL is canonical. DID is a
  cross-reference.
- For **content-addressing** (URNs, beads): hex pubkey is canonical
  (per CLAUDE.md and §12).
- For **relay routing** (Nostr): hex pubkey is canonical (NIP-01).
- For **display** (forum UI): bech32 npub.

This is documented nowhere in code. **Recommendation:** add a one-page
`docs/identity-contracts.md` listing each surface and which form is
canonical there.

**Hazardous edge case:** `pod-worker/src/did.rs:55-58` only matches the
exact pod base `https://pods.dreamlab-ai.com/<pubkey>/...`. A WebID hosted
at a different domain (federation case) will be **silently rejected** as
"not controlled by this pubkey" even when the DID Doc's alsoKnownAs
linkage is valid. This blocks federation. Severity: **HIGH** for
multi-domain deployments. Fix: also accept any URL whose corresponding
DID Doc (resolved via `/.well-known/did/nostr/<hex>.json`) lists the
WebID under `alsoKnownAs`.

## 14. WAC + Nostr authz interplay (acl:agent IRI shape)

**STATUS: drifted (HIGH).**

The pod-worker WAC evaluator (`pod-worker/src/acl.rs:162-188`) compares
`acl:agent` IRIs as **strings** (line 166: `agents.contains(&uri)`).
Provisioned ACLs use `did:nostr:<hex>` consistently:
`provision.rs:148, 177, 200, 229, 258` and `sovereign-bootstrap.py:161-167`.

But there's **no normalisation**. If an ACL document was written with
`did:nostr:<HEX>` (uppercase) and the request agent URI is computed via
`format!("did:nostr:{pk}")` from a lowercase event pubkey, they will not
match. `lib.rs:447` does `format!("did:nostr:{pk}")` with whatever case
`pk` carries.

NIP-98 verifier `nip98.rs:258` accepts any-case hex (just checks
`hex::decode` succeeds and length=64), so the `token.pubkey` could be
uppercase if the client signed with uppercase hex (illegal per BIP-340
but not enforced). Then ACL match fails silently → 403.

**Recommendation:** in `lib.rs:447`, lowercase the pubkey before
constructing the agent URI. Add a regex check rejecting non-lowercase
pubkeys at NIP-98 ingress.

**Sprint v9 STREAM-B added Control-mode coercion** for ACL writes
(`acl.rs::coerce_required_mode_for_acl`, `parse_acl_with_cap` 64KiB) per
audit C3. Good. But `acl:agentClass` matching (line 172-186) only
recognises `foaf:Agent` and `acl:AuthenticatedAgent` — there is no
mechanism to map a NIP-26 delegation tag onto a WAC agentClass. So
delegated signers cannot be granted access without explicit per-key
ACL entries. Severity: **MEDIUM** until the federation requires
delegated agent access.

## 15. Side-channel risks (relay-side correlation, bandwidth, timing)

**STATUS: hazardous (MEDIUM).**

1. **Timestamp correlation across kind 1059 + kind 1.** Gift-wrap jitters
   created_at by ±48h (`gift_wrap.rs:108-125`). But the user's regular
   posts (kind 1) carry true timestamps. An adversary observing both
   streams from the same relay can match active periods to user pubkeys.
   Mitigation: post-only DMs cannot leak this; a chatty user can.
   Out-of-scope for crypto fix.

2. **Throwaway pubkey reuse.** `wrap_seal` generates a fresh keypair per
   call (line 228) — **no reuse**. Confirmed.

3. **NIP-44 padded length leaks rough message size.** `calc_padded_len`
   (line 138-149) rounds up to next chunk; chunks scale with size. A
   1024-byte message is distinguishable from a 32-byte one. Mitigation:
   spec-compliant; fix would need fixed-size padding which the spec
   explicitly avoids for bandwidth reasons. Document only.

4. **Relay AUTH challenge is per-session, not per-event.** A relay
   operator who logs (challenge, AUTH-event) pairs can prove a pubkey
   was online at a given moment. Mitigation: rotate sessions (close
   WS, reconnect) periodically.

5. **NIP-98 timing attack on body hash.** SHA-256 comparison at
   `nip98.rs:304-306` uses `String::eq_ignore_ascii_case` — not
   constant-time. Severity: LOW — body content is not secret, the hash
   is published in the signed event already. Still fix for hygiene
   (use `subtle::ConstantTimeEq`).

6. **NIP-44 HMAC verification IS constant-time** (`nip44.rs:316-322,
   214`) ✓.

7. **VisionClaw → forum bridge** (`src/services/nostr_bridge.rs:181-247`)
   verifies signatures (line 192) but **does not verify NIP-26 delegation
   tags** if present. An attacker who compromises the JSS relay can
   forge events from any pubkey; signature verification catches that.
   But if a user delegates to the bridge's pubkey, the bridge re-signs
   under its own key (line 219-222) — **this loses the original user's
   identity attribution**. The forum sees `kind 9` events authored by
   the bridge pubkey, with the original event id in a `source_event`
   tag (line 213-217). Forum readers must know to trust the bridge's
   re-attribution. **MEDIUM** severity.

## 16. Recommended hardening ordered by severity

### CRITICAL — fix before launch

- **C1.** `nostr-core/src/nip44.rs:122-128` — replace HKDF-Expand
  miscall with HMAC-SHA256 to compute correct NIP-44 v2 conversation
  key. Add reference test vectors. Without this, gift-wrap and DMs are
  not interoperable with any other Nostr client.
- **C2.** `agentbox/scripts/sovereign-bootstrap.py:90-91, 133-134` —
  bech32-encode the **32-byte x-only pubkey**, not the 64-byte SEC1
  point. Recompute `y_parity` and apply BIP-340 lift_x to ensure the
  even-y form. Without this, agentbox npubs are not interoperable.
- **C3.** Standardise `verificationMethod.type` on
  `SchnorrSecp256k1VerificationKey2019` across all three emitters.
  Patch `solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs:98, 154` and
  `agentbox/scripts/sovereign-bootstrap.py:192`.

### HIGH — fix this sprint

- **H4.** Add Tier-1 secp256k1-2019 @context to
  `solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs:93`.
- **H5.** Add NIP-26 delegation verification path on agentbox
  ingest and on VisionClaw `nostr_bridge.rs::forward_to_forum`. Without
  it, "user X's passkey signs events accepted by agentbox" requires
  agentbox to import the user's hex pubkey out-of-band, which doesn't
  scale.
- **H6.** Federate the NIP-98 replay store across workers. Single
  `NIP98_REPLAY` KV namespace shared by auth-worker, pod-worker,
  relay-worker, search-worker — not four separate ones.
- **H7.** Lowercase pubkey in `pod-worker/src/lib.rs:447` before
  constructing the `acl:agent` IRI. Add regex enforcement at NIP-98
  ingress (line 258 of nip98.rs).
- **H8.** WAC agent-IRI normalisation: trim, lowercase hex, strip
  trailing `/`. Apply consistently in `acl.rs::agent_matches`.
- **H9.** `pod-worker/src/did.rs::verify_webid_tag` — accept federated
  WebID URLs by resolving the target's DID Doc and checking the
  alsoKnownAs linkage. Currently hardcoded to `pods.dreamlab-ai.com`.
- **H10.** Implement key-rotation announcement: kind 30033 replaceable
  event signed by old key delegating to new key (NIP-26), plus
  `deactivated: true` flag in DID Doc once rotation completes.
- **H11.** Verify the JS HKDF info string (`'nostr-secp256k1-v1'`) is
  byte-identical to `nostr-core/src/keys.rs:9`. Add a CI test that
  imports both implementations and asserts they produce the same
  pubkey for a fixed PRF input.

### MEDIUM — soon

- **M12.** All-write AUTH gating on the federated relay. Reads of
  public kinds (0, 1, 3, 7, 30000-39999) stay open; writes and reads
  of {4, 13, 14, 1059} require NIP-42.
- **M13.** Add NIP-26 delegation signing and verification to
  forum-client `auth/nip07.rs` so NIP-07 extensions can also delegate
  to agentbox / server keys.
- **M14.** Document the bridge re-signing trade-off
  (`src/services/nostr_bridge.rs:219`): forum readers see kind 9
  signed by bridge pubkey, original author lives only in the
  `source_event` tag. Either propagate the original signed event
  verbatim (kind 1059 wrap with bridge as recipient) or document
  explicitly.
- **M15.** Constant-time payload-hash comparison in
  `nip98.rs:304-306` — switch to `subtle::ConstantTimeEq`.
- **M16.** Replay store TTL audit: confirm `REPLAY_CACHE_TTL_SECS = 120`
  matches actual KV write→read latency under load. Recommend ≥180s
  buffer.
- **M17.** `forum-client/src/auth/nip07.rs:188-207` — when the
  extension does not support NIP-04, fail explicitly instead of
  silently routing through NIP-44. The current behaviour produces
  undecryptable kind-4 messages.

### LOW — hygiene

- **L18.** `forum-client/src/auth/session.rs:96-109` —
  `save_privkey_session` should `panic!` in release builds to
  guarantee passkey-derived keys are never persisted.
- **L19.** Lint-ban direct calls to `SecretKey::sign` outside
  `event::sign_event` (forces randomised aux_rand).
- **L20.** Add a NIP-44 v2 reference test vector to
  `nostr-core/src/nip44.rs` to detect any future drift in conversation
  key derivation.
- **L21.** Lint-gate `format!("urn:visionclaw:...")` outside `src/uri/`
  (already mentioned in PRD-006 §6, not yet wired).
- **L22.** `nostr-core/src/nip26.rs::DelegationTag::from_token` produces
  5-element tags (delegatee at index 4) — non-spec extension. Document
  in code comment that wire-out is 5 elements but verify accepts 4 or
  5; standard relays will pass through extra elements unchanged.

---

## Cross-cutting answer: shared identity vs delegation

The user's question — "if user X holds passkey-derived nsec, can their
forum login key sign events that agentbox accepts as the SAME identity?"
— pivots on three facts:

1. **The keys cannot be the same.** Forum nsec lives in the browser,
   re-derived on every login from PRF. Agentbox key lives on the
   container filesystem under `/var/lib/agentbox/identities/`. VisionClaw
   key lives in `SERVER_NOSTR_PRIVKEY` env. Three separate custody
   regimes.

2. **The pubkey IS the identity.** All three systems agree on hex pubkey
   as canonical. So if user X publishes to a relay with their forum key,
   any of the three systems can verify the signature and recognise
   `event.pubkey == user_X.hex` and treat it as user X.

3. **What's missing is the trust pivot.** Today, agentbox's WAC ACL
   writes hardcode `did:nostr:<sovereign_agentbox_hex>` — the agentbox's
   own key, not the user's. The user X never gets write access to the
   agent's pod by default. To grant access, agentbox must:
   (a) accept user X's events at all (NIP-42 AUTH gate must allow them),
   (b) recognise user X's pubkey in an ACL (currently no provision for
   user-by-user ACL entries on agentbox-owned pods), or
   (c) accept a NIP-26 delegation from user X to the agentbox key
   (currently no verifier wired).

The clean answer is **(c)**: build a NIP-26 verifier into agentbox's
event ingest (and into VisionClaw's `nostr_bridge.rs`). Then user X
delegates with `kind=N` constraints to the agentbox key, the agentbox
key signs on behalf of user X, and external observers see two valid
signatures (delegator + delegatee) attesting to the delegated identity.

Without this delegation glue, the three systems share a vocabulary
(`did:nostr:<hex>`) but cannot share authority. They are three islands
that can read each other's identifiers but cannot act on each other's
behalf.

---

## File-path reference (absolute, for follow-up)

- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/keys.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip04.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip44.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip19.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip26.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip98.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/gift_wrap.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/signer.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/did.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/acl.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/webid.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/provision.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/pod-worker/src/lib.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/forum-client/src/auth/passkey.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/forum-client/src/auth/session.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/forum-client/src/auth/nip07.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/forum-client/src/auth/nip98.rs`
- `/home/devuser/workspace/project/dreamlab-ai-website/community-forum-rs/crates/relay-worker/src/relay_do/nip_handlers.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-nostr/src/relay.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-nostr/src/resolver.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-nostr/src/ws.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-idp/src/schnorr.rs`
- `/home/devuser/workspace/project/solid-pod-rs/crates/solid-pod-rs-idp/src/lib.rs`
- `/home/devuser/workspace/project/agentbox/scripts/sovereign-bootstrap.py`
- `/home/devuser/workspace/project/agentbox/management-api/lib/uris.js`
- `/home/devuser/workspace/project/src/services/server_identity.rs`
- `/home/devuser/workspace/project/src/services/nostr_bridge.rs`
- `/home/devuser/workspace/project/src/uri/mint.rs`
- `/home/devuser/workspace/project/src/uri/parse.rs`
- `/home/devuser/workspace/project/src/uri/kinds.rs`
- `/home/devuser/workspace/project/src/uri/legacy.rs`
