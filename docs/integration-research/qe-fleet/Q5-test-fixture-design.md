# Q5 — Cross-System Test Fixture & Contract Test Design

**Author**: QE Specialist Q5
**Status**: Design
**Scope**: Federated mesh — `forum/`, `agentbox/`, `solid-pod-rs/`, VisionClaw (`src/`)
**Mission**: Defend the federation against the bug class that produced C1 (NIP-44 conv-key drift), C2 (broken bech32 npub), and C3 (DID `type` drift). Specify shared fixtures, per-substrate runners, contract test taxonomy, conformance suite, and federation smoke tests.

---

## 0. Bug-class summary (the threat model)

The three known incidents share a single shape:

| ID | Substrate(s) | Drift | Root cause | What would have caught it |
|----|--------------|-------|------------|---------------------------|
| C1 | forum vs agentbox | NIP-44 v2 conversation-key derivation diverged from upstream | Hand-rolled HKDF salt; no upstream vector test | `paulmillr/nip44/javascript/test/vectors.json` consumed as a test fixture in **both** substrates |
| C2 | solid-pod-rs | bech32 npub encoding produced strings that downstream `nostr-tools` could not decode | Local bech32 impl with wrong HRP padding; no round-trip test against a reference impl | BIP-173 vectors + cross-substrate "encode-here, decode-there" contract |
| C3 | VisionClaw vs forum | DID document `verificationMethod[*].type` drifted between `Multikey`, `JsonWebKey2020`, and `Ed25519VerificationKey2020` | No shared DID-doc shape contract; each project picked its own type string | `did-doc-conformance.json` golden file enforced in every emitter substrate |

**Common pattern**: a cryptographic or schema invariant is locally re-implemented (or locally hard-coded) without being pinned to an external authoritative test suite, and there is no contract that the **same** input produces the **same** byte-output across substrates. Q5's design eliminates this pattern.

---

## T1 — Shared fixture taxonomy

Fixtures fall into three classes. Each has a distinct purpose, layout, and failure mode.

### T1.1 Reference vectors (RV)

**Definition**: Canonical input/output pairs lifted verbatim from upstream specifications or reference implementations.

**Purpose**: Asserts that this substrate's implementation of a public protocol matches the world's implementation. If RV tests pass, we are wire-compatible with every other compliant peer.

**Failure mode caught**: C1 (silent local divergence from spec).

**Directory layout**:

```
docs/specs/fixtures/
├── reference-vectors/
│   ├── nip01-events.json
│   ├── nip04-dm.json
│   ├── nip19-bech32.json
│   ├── nip26-delegation.json
│   ├── nip44-v2.json
│   ├── nip59-gift-wrap.json
│   ├── nip98-tokens.json
│   ├── rfc8785-jcs.json
│   ├── bip340-schnorr.json
│   ├── multibase.json
│   └── did-core-test-suite.json
└── UPSTREAM_PINS.md
```

**Naming convention**: `<spec-id-lower>-<feature>.json`, e.g. `nip44-v2.json`, `bip340-schnorr.json`. One file per spec; do not split by sub-feature unless the upstream split.

**Version-pinning strategy**: Each upstream commit hash recorded in `UPSTREAM_PINS.md` (T2). When upstream publishes new vectors, a CI job opens a PR updating both the JSON file and the pinned hash. The pinned hash is the source of truth — reviewers MUST diff the new file against `git show <pinned-hash>:<path>`.

### T1.2 Contract assertions (CA)

**Definition**: DreamLab-internal invariants asserting that for input X, output Y is identical across substrates A, B, C, D.

**Purpose**: Asserts cross-substrate consistency of *internal* data shapes that no upstream spec governs (DID-doc shape, IS-Envelope shape, mesh URI mint chokepoint behaviour).

**Failure mode caught**: C3 (internal schema drift between substrates).

**Directory layout**:

```
docs/specs/fixtures/
└── contract-assertions/
    ├── did-doc-conformance.json    # ADR-024 / personal-context portfolio shape
    ├── is-envelope-v1.json         # ADR-075 D15
    ├── mesh-federation.json        # ADR-073 D11
    ├── urn-visionclaw-mint.json    # urn:visionclaw:* grammar conformance
    ├── urn-agentbox-mint.json      # urn:agentbox:* grammar conformance
    └── nip98-cross-substrate.json  # token issued by A, validated by B
```

**Naming convention**: `<artefact>-<scope>.json`, scope being `conformance` (golden-shape), `mint` (constructive grammar), or `federation` (multi-relay behaviour).

**Version-pinning strategy**: Each fixture carries `"$schemaVersion"` (semver). Bumping major requires an ADR. CI fails any PR that mutates a fixture without bumping `$schemaVersion`.

### T1.3 Negative cases (NC)

**Definition**: Malformed inputs that MUST be rejected with a specific error code.

**Purpose**: Asserts that hostile or malformed input is rejected uniformly. A peer that *silently accepts* a malformed event is a federation hazard.

**Failure mode caught**: Hostile-relay attack class; parser undefined-behaviour drift; the silent C2 case where a broken bech32 string was emitted but never round-tripped.

**Directory layout**:

```
docs/specs/fixtures/
└── negative-cases/
    ├── nip01-malformed-events.json
    ├── nip19-broken-bech32.json
    ├── nip44-bad-mac.json
    ├── nip98-replay.json
    ├── is-envelope-rejects.json
    └── did-doc-rejects.json
```

Each entry has shape:

```json
{
  "name": "human-readable-id",
  "input": "<the malformed thing>",
  "expectedError": {
    "code": "ERR_NIP44_MAC_MISMATCH",
    "category": "auth",
    "humanReadable": "MAC verification failed"
  },
  "mustRejectIn": ["forum", "agentbox", "visionclaw", "solid-pod-rs"]
}
```

**Naming convention**: `<spec>-<failure-class>.json`, e.g. `nip44-bad-mac.json`, `nip98-replay.json`.

**Version-pinning strategy**: Adding NC entries is always allowed (strengthens defence). Removing or weakening requires ADR. Each substrate maintains a `expected-error-map.json` mapping the canonical `expectedError.code` to the substrate's local error type.

---

## T2 — Reference vector source plan

Single source of truth for upstream pins. Each entry in `UPSTREAM_PINS.md`:

```markdown
## NIP-44 v2 — paulmillr/nip44

- **Upstream**: https://github.com/paulmillr/nip44
- **Path**: `javascript/test/vectors.json`
- **Pinned commit**: `b5d9fc3a9e1a5b... ` (2026-01-12)
- **License**: MIT
- **Sync method**: `tools/sync-fixtures.sh nip44`
- **Last reviewed**: 2026-04-30 (devuser)
- **Why this source**: Definitive cross-language vectors covering `getConversationKey`,
  `messageKey`, padded length, encrypt round-trip, decrypt round-trip, MAC. Other
  NIP-44 implementations test against this file.
```

Full table:

| Spec | Canonical source | Path-in-source | Pin scheme | Notes |
|------|------------------|----------------|------------|-------|
| NIP-01 events | `rust-nostr/nostr` | `crates/nostr/tests/event.rs` (extract to JSON via build script) | git tag (`v0.36.0` or current pinned) | Event ID + serialisation determinism |
| NIP-04 DM (legacy) | `nostr-protocol/nips` repo | example payloads in NIP-04.md | git commit hash | Limited; NIP-44 is the load-bearing one |
| NIP-19 bech32 | `rust-nostr/nostr` proptest fixtures + `bitcoin/bips` BIP-173 | `crates/nostr/src/nips/nip19.rs` test cases + BIP-173 test vectors | git commit hash for both | C2 root-cause fixture |
| NIP-26 delegation | `rust-nostr/nostr` | `crates/nostr/src/nips/nip26.rs` examples | git commit hash | Delegation tag verification |
| NIP-44 v2 | `paulmillr/nip44` | `javascript/test/vectors.json` | git commit hash | **C1 root-cause fixture** |
| NIP-59 gift-wrap | `nostr-protocol/nips` | NIP-59.md examples | git commit hash | Sealed/wrapped envelope test |
| NIP-98 HTTP auth | `rust-nostr/nostr` + `nostr-protocol/nips` | crates/nostr/src/nips/nip98.rs + NIP-98.md | git commit hash | Token format + body-hash order |
| RFC 8785 JCS | `cyberphone/json-canonicalization` | `testdata/input/*.json` + `testdata/output/*.json` | git commit hash | Differential fuzzing target |
| BIP-340 Schnorr | `bitcoin/bips` | `bip-0340/test-vectors.csv` | git tag | Sig + verification 32-byte x-only |
| Multibase | `multiformats/multibase` | `tests/test-vectors.csv` | git tag | base58btc, base32, base64url alphabets |
| DID Core | `w3c/did-test-suite` | `packages/did-core-test-server/suites/*.json` | git tag | `@context`, `id`, `verificationMethod[*].type` |

**Sync mechanism**: `tools/sync-fixtures.sh <spec>` clones the pinned commit into a temp dir, copies the file, computes content hash, updates `UPSTREAM_PINS.md`. Any change requires a PR.

**License compliance**: Each upstream license noted in `UPSTREAM_PINS.md`. paulmillr/nip44 is MIT; rust-nostr is MIT; W3C is W3C-license; cyberphone/json-canonicalization is Apache-2.0. All compatible with DreamLab licensing.

---

## T3 — Fixture file layout (per-file schema and example entries)

The canonical home is `docs/specs/fixtures/`. Each substrate's `tests/fixtures/` is a CI-verified copy (T11 has the verify step).

### T3.1 `nip01-events.json`

**Schema**:

```json
{
  "$schemaVersion": "1.0.0",
  "vectors": [
    {
      "name": "kind-1-text-note-canonical",
      "unsignedEvent": {
        "pubkey": "ab...64-hex",
        "created_at": 1700000000,
        "kind": 1,
        "tags": [],
        "content": "hello"
      },
      "expectedSerialised": "[0,\"ab...\",1700000000,1,[],\"hello\"]",
      "expectedEventId": "<sha256-hex>"
    }
  ]
}
```

**Example entry**:

```json
{
  "name": "kind-1-empty-tags",
  "unsignedEvent": {
    "pubkey": "abababababababababababababababababababababababababababababababab",
    "created_at": 1700000000,
    "kind": 1,
    "tags": [],
    "content": "test"
  },
  "expectedSerialised": "[0,\"abababababababababababababababababababababababababababababababab\",1700000000,1,[],\"test\"]",
  "expectedEventId": "5b6ea5...d4"
}
```

### T3.2 `nip19-bech32.json`

**Schema**:

```json
{
  "$schemaVersion": "1.0.0",
  "encode": [
    { "name": "...", "hrp": "npub", "data": "<32-byte-hex>", "expected": "npub1..." }
  ],
  "decode": [
    { "name": "...", "input": "npub1...", "expectedHrp": "npub", "expectedData": "<32-byte-hex>" }
  ],
  "roundtrip": [
    { "name": "...", "hex": "<32-byte-hex>", "expectedNpub": "npub1..." }
  ]
}
```

**Why three sections**: C2 was a one-way bug (encode-here-correctly, decode-there-fails). Round-trip catches it.

### T3.3 `nip44-v2.json` (load-bearing)

**Schema** (mirrors paulmillr layout):

```json
{
  "$schemaVersion": "1.0.0",
  "$source": "paulmillr/nip44 javascript/test/vectors.json @ b5d9fc3a",
  "v2": {
    "valid": {
      "get_conversation_key": [
        { "sec1": "<hex>", "pub2": "<hex>", "conversation_key": "<hex>" }
      ],
      "get_message_keys": [
        { "conversation_key": "<hex>", "nonce": "<hex>", "chacha_key": "<hex>",
          "chacha_nonce": "<hex>", "hmac_key": "<hex>" }
      ],
      "calc_padded_len": [ { "len": 1, "padded_len": 32 } ],
      "encrypt_decrypt": [
        { "sec1": "<hex>", "sec2": "<hex>", "conversation_key": "<hex>",
          "nonce": "<hex>", "plaintext": "...", "payload": "..." }
      ]
    },
    "invalid": {
      "decrypt": [
        { "conversation_key": "<hex>", "nonce": "<hex>", "plaintext": "...",
          "payload": "...", "note": "wrong-mac" }
      ]
    }
  }
}
```

**Example entry** (one valid, one invalid):

```json
{
  "valid": {
    "get_conversation_key": [
      {
        "sec1": "315e54e5e2bf81b4dad8b1d1ce86a99f25f23e5b6f2a4e8b3c98a04e1f4f5b6a",
        "pub2": "684e29e2c2b9f0bf2e3a3a5e7b1d0c6f4f5a5b4b3c2d1e0f9a8b7c6d5e4f3a2b",
        "conversation_key": "c41c775356fd92eadc63ff5a0dc1da211b268cbea22316767095b2871ea1412d"
      }
    ]
  },
  "invalid": {
    "decrypt": [
      {
        "conversation_key": "c41c775356fd92eadc63ff5a0dc1da211b268cbea22316767095b2871ea1412d",
        "payload": "AgIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyAhIiMkJSYnKCkqKywtLi8wMTIzNDU2Nzg5Ojs8PT4/QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl9gYWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXp7fH1+f4CBgoOEhYaHiImKi4yNjo+QkZKTlJWWl5iZmpucnZ6foKGio6SlpqeoqaqrrK2urw==",
        "note": "wrong-mac"
      }
    ]
  }
}
```

### T3.4 `nip98-tokens.json`

**Schema**:

```json
{
  "$schemaVersion": "1.0.0",
  "vectors": [
    {
      "name": "POST-with-body-hash",
      "method": "POST",
      "url": "https://api.dreamlab.example/v1/post",
      "body": "<bytes-hex>",
      "ts": 1700000000,
      "signerSecret": "<hex>",
      "expectedToken": "Nostr <base64-event>",
      "expectedDecodedEvent": { "kind": 27235, "tags": [...], "content": "" }
    }
  ]
}
```

**Why body-hash order matters (S-Inv-02)**: NIP-98 spec mandates body hash computed over `bytes(body)` BEFORE adding the `payload` tag. solid-pod-rs originally did it after — fixture catches that.

### T3.5 `did-doc-conformance.json`

**DreamLab-internal contract**, not a public spec. Defines the shape every DreamLab DID emitter must produce:

```json
{
  "$schemaVersion": "1.0.0",
  "$adr": "ADR-024",
  "required": {
    "@context": [
      "https://www.w3.org/ns/did/v1",
      "https://w3id.org/security/multikey/v1"
    ],
    "verificationMethod[*].type": "Multikey",
    "verificationMethod[*].publicKeyMultibase": "z6Mk...|<multibase-bech32m-or-base58btc>"
  },
  "vectors": [
    {
      "name": "minimal-ed25519-multikey",
      "input": { "pubkeyHex": "ab".repeat(32) },
      "expectedDoc": {
        "@context": ["https://www.w3.org/ns/did/v1", "https://w3id.org/security/multikey/v1"],
        "id": "did:nostr:abababababababababababababababababababababababababababababababab",
        "verificationMethod": [
          {
            "id": "did:nostr:ab...#key-0",
            "type": "Multikey",
            "controller": "did:nostr:ab...",
            "publicKeyMultibase": "z6Mk..."
          }
        ],
        "authentication": ["did:nostr:ab...#key-0"],
        "assertionMethod": ["did:nostr:ab...#key-0"]
      }
    }
  ]
}
```

The C3 fix: `type` is pinned to `Multikey`. Any drift (`Ed25519VerificationKey2020`, `JsonWebKey2020`) fails the test in every substrate that emits DID docs.

### T3.6 `is-envelope-v1.json`

**ADR-075 D15 conformance**. Seven envelope kinds, plus catch-all unknown:

```json
{
  "$schemaVersion": "1.0.0",
  "$adr": "ADR-075",
  "envelopes": [
    {
      "name": "chat-minimal",
      "kind": "chat",
      "envelope": {
        "v": 1,
        "kind": "chat",
        "id": "urn:visionclaw:bead:ab...:chat-001",
        "ts": 1700000000,
        "from": "did:nostr:ab...",
        "to": ["did:nostr:cd..."],
        "body": { "text": "hello" }
      },
      "expectedJcs": "{\"body\":{\"text\":\"hello\"},\"from\":\"did:nostr:ab...\",\"id\":\"urn:visionclaw:bead:ab...:chat-001\",\"kind\":\"chat\",\"to\":[\"did:nostr:cd...\"],\"ts\":1700000000,\"v\":1}",
      "expectedKind1059": {
        "kind": 1059,
        "tags": [["p", "cd..."]],
        "content": "<NIP-44-encrypted-jcs>"
      }
    }
  ]
}
```

(All seven kinds populated; example shown for `chat`.)

### T3.7 `mesh-federation.json`

**ADR-073 D11 conformance**. Federation behaviour assertions:

```json
{
  "$schemaVersion": "1.0.0",
  "$adr": "ADR-073",
  "scenarios": [
    {
      "name": "kind-30200-bead-stamp-fanout",
      "setup": {
        "publishingPeer": "visionclaw",
        "subscribingPeers": ["forum", "agentbox"]
      },
      "publishedEvent": {
        "kind": 30200,
        "tags": [["d", "bead:ab...:exec-001"]],
        "content": "..."
      },
      "expected": {
        "deliveredTo": ["forum", "agentbox"],
        "withinMs": 5000,
        "kind1059Wrapped": false
      }
    }
  ]
}
```

---

## T4 — Test runner per substrate

Each substrate consumes the fixture files via a thin loader and asserts substrate-local behaviour matches.

### T4.1 Forum (Rust + Cloudflare Workers wasm32)

**Location**: `forum/nostr-core/tests/upstream_vectors.rs`

```rust
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct Nip44V2 { v2: Nip44V2Inner }
#[derive(Deserialize)]
struct Nip44V2Inner { valid: Nip44V2Valid }
#[derive(Deserialize)]
struct Nip44V2Valid {
    get_conversation_key: Vec<ConvKeyVec>,
    encrypt_decrypt: Vec<EncDecVec>,
}
#[derive(Deserialize)]
struct ConvKeyVec { sec1: String, pub2: String, conversation_key: String }
#[derive(Deserialize)]
struct EncDecVec {
    sec1: String, sec2: String, conversation_key: String,
    nonce: String, plaintext: String, payload: String,
}

#[test]
fn nip44_v2_paulmillr_vectors() {
    let raw = fs::read_to_string("tests/fixtures/nip44-v2.json").unwrap();
    let v: Nip44V2 = serde_json::from_str(&raw).unwrap();

    for vec in &v.v2.valid.get_conversation_key {
        let sk = SecretKey::from_hex(&vec.sec1).unwrap();
        let pk = PublicKey::from_hex(&vec.pub2).unwrap();
        let got = nostr::nips::nip44::ConversationKey::derive(&sk, &pk).unwrap();
        assert_eq!(hex::encode(got.as_bytes()), vec.conversation_key,
            "conv key drift on vector — this is the C1 trip-wire");
    }

    for vec in &v.v2.valid.encrypt_decrypt {
        let conv = ConversationKey::from_hex(&vec.conversation_key).unwrap();
        let pt = nostr::nips::nip44::decrypt(&conv, &vec.payload).unwrap();
        assert_eq!(pt, vec.plaintext);
    }
}

#[test]
fn nip01_event_id_canonical_serialisation() {
    let raw = fs::read_to_string("tests/fixtures/nip01-events.json").unwrap();
    let v: Nip01Vectors = serde_json::from_str(&raw).unwrap();
    for vec in v.vectors {
        let serialised = serialize_for_id(&vec.unsigned_event);
        assert_eq!(serialised, vec.expected_serialised);
        let id = sha256::hash(serialised.as_bytes());
        assert_eq!(hex::encode(id), vec.expected_event_id);
    }
}
```

**Run**: `cargo test --package nostr-core --test upstream_vectors`
**Duration**: ~3s (vectors are small).
**CI**: Required check on every PR to forum/.

### T4.2 VisionClaw (Rust)

**Location**: `tests/cross_system_contracts.rs`

```rust
use serde_json::Value;

fn assert_subset(got: &Value, golden: &Value, path: &str) {
    match (got, golden) {
        (Value::Object(g), Value::Object(o)) => {
            for (k, v) in o {
                let sub = g.get(k).unwrap_or_else(|| panic!("missing key {path}.{k}"));
                assert_subset(sub, v, &format!("{path}.{k}"));
            }
        }
        (Value::Array(g), Value::Array(o)) => {
            assert_eq!(g.len(), o.len(), "array len mismatch at {path}");
            for (i, (gi, oi)) in g.iter().zip(o.iter()).enumerate() {
                assert_subset(gi, oi, &format!("{path}[{i}]"));
            }
        }
        (g, o) => assert_eq!(g, o, "value mismatch at {path}"),
    }
}

#[test]
fn did_doc_emitted_matches_dreamlab_contract() {
    let pubkey_hex = "ab".repeat(32);
    let our_doc: Value = identity_did_handler::render(&pubkey_hex)
        .expect("render must succeed");

    let golden_raw = include_str!("fixtures/did-doc-conformance.json");
    let golden: Value = serde_json::from_str(golden_raw).unwrap();
    let expected = &golden["vectors"][0]["expectedDoc"];

    assert_subset(&our_doc, expected, "$");

    // Specifically guard C3 — verificationMethod[*].type must be "Multikey"
    let vm_type = our_doc["verificationMethod"][0]["type"].as_str().unwrap();
    assert_eq!(vm_type, "Multikey", "C3 trip-wire — DID type drift detected");
}

#[test]
fn urn_visionclaw_mint_grammar() {
    // F-Inv: only canonical mint paths produce urn:visionclaw:*
    let raw = include_str!("fixtures/urn-visionclaw-mint.json");
    let v: UrnVectors = serde_json::from_str(raw).unwrap();
    for vec in v.vectors {
        let urn = visionclaw::uri::mint::mint_kind(&vec.kind, &vec.scope, &vec.local).unwrap();
        assert_eq!(urn.to_string(), vec.expected);
    }
}
```

**Run**: `cargo test --test cross_system_contracts`
**Duration**: ~2s.
**CI**: Required check.

### T4.3 Agentbox (Node.js)

**Location**: `agentbox/tests/contract/upstream_vectors.test.js`

```js
const fs = require('fs');
const path = require('path');
const { nip44 } = require('nostr-tools');
const { hexToBytes, bytesToHex } = require('@noble/hashes/utils');

const FIX = path.join(__dirname, '..', 'fixtures');
const vectors = JSON.parse(fs.readFileSync(path.join(FIX, 'nip44-v2.json'), 'utf8'));

describe('NIP-44 v2 conformance (paulmillr vectors)', () => {
  test.each(vectors.v2.valid.get_conversation_key)(
    'conversation key derives correctly ($conversation_key)',
    ({ sec1, pub2, conversation_key }) => {
      const got = nip44.v2.utils.getConversationKey(hexToBytes(sec1), pub2);
      expect(bytesToHex(got)).toBe(conversation_key);
    }
  );

  test.each(vectors.v2.valid.encrypt_decrypt)(
    'decrypt round-trip ($plaintext)',
    ({ conversation_key, payload, plaintext }) => {
      const pt = nip44.v2.decrypt(payload, hexToBytes(conversation_key));
      expect(pt).toBe(plaintext);
    }
  );

  test.each(vectors.v2.invalid.decrypt)(
    'rejects malformed ($note)',
    ({ conversation_key, payload }) => {
      expect(() =>
        nip44.v2.decrypt(payload, hexToBytes(conversation_key))
      ).toThrow();
    }
  );
});

describe('IS-Envelope wrap → unwrap', () => {
  const envVectors = JSON.parse(fs.readFileSync(path.join(FIX, 'is-envelope-v1.json')));
  test.each(envVectors.envelopes)('$kind: byte-identical roundtrip', (vec) => {
    const wrapped = isEnvelope.wrapKind1059(vec.envelope, recipientPub);
    const unwrapped = isEnvelope.unwrapKind1059(wrapped, recipientSec);
    expect(JSON.stringify(unwrapped)).toBe(JSON.stringify(vec.envelope));
  });
});
```

**Run**: `npm test -- contract/`
**Duration**: ~5s.
**CI**: Required check.

### T4.4 solid-pod-rs (Rust)

**Location**: `solid-pod-rs/crates/solid-pod-rs-nostr/tests/upstream_vectors.rs`

```rust
#[test]
fn did_doc_renderer_matches_dreamlab_contract() {
    let golden = include_str!("../../tests/fixtures/did-doc-conformance.json");
    let v: DidDocConformance = serde_json::from_str(golden).unwrap();
    for vec in v.vectors {
        let pubkey = hex::decode(&vec.input.pubkey_hex).unwrap();
        let our = solid_pod_rs::nostr::did::render_doc(&pubkey).unwrap();
        let our_v: Value = serde_json::to_value(&our).unwrap();
        assert_subset(&our_v, &serde_json::to_value(&vec.expected_doc).unwrap(), "$");
    }
}

#[test]
fn nip98_token_body_hash_order() {
    // S-Inv-02: body hash MUST be computed before payload-tag insertion
    let raw = include_str!("../../tests/fixtures/nip98-tokens.json");
    let v: Nip98Vectors = serde_json::from_str(raw).unwrap();
    for vec in v.vectors {
        let token = nip98::issue_token(&vec.method, &vec.url, &vec.body, vec.ts, &vec.signer_secret).unwrap();
        assert_eq!(token, vec.expected_token);
    }
}

#[test]
fn nip19_bech32_round_trip() {
    let raw = include_str!("../../tests/fixtures/nip19-bech32.json");
    let v: Nip19Vectors = serde_json::from_str(raw).unwrap();
    for vec in v.roundtrip {
        let hex_bytes = hex::decode(&vec.hex).unwrap();
        let encoded = nostr_bech32::encode("npub", &hex_bytes).unwrap();
        assert_eq!(encoded, vec.expected_npub, "C2 trip-wire");
        let (hrp, decoded) = nostr_bech32::decode(&encoded).unwrap();
        assert_eq!(hrp, "npub");
        assert_eq!(decoded, hex_bytes);
    }
}
```

**Run**: `cargo test --package solid-pod-rs-nostr --test upstream_vectors`
**Duration**: ~2s.
**CI**: Required check.

---

## T5 — Contract test taxonomy

Three levels by blast radius and run cadence.

### Level 1 — Identity contract (within-substrate)

**Definition**: For input X, a substrate's local implementation matches the upstream library's implementation.

**Example assertion**:
```rust
// forum
assert_eq!(forum::event_id(unsigned), nostr_sdk::Event::event_id(unsigned));
```

**Cadence**: Every PR.
**Owner**: The substrate's maintainer.
**Failure recovery**: Immediate revert; the local divergence is the bug.

**Coverage target** (per Q4 G15): Every cryptographic primitive (signing, verifying, hashing, key derivation, encryption/decryption) used by the substrate.

### Level 2 — Cross-substrate contract (between two substrates)

**Definition**: Substrate A produces an artefact that Substrate B accepts and processes correctly.

**Example assertions**:
- forum issues NIP-98 token; VisionClaw `nip98::validate(token)` accepts.
- VisionClaw mints `urn:visionclaw:bead:...`; agentbox BC20 ACL maps to `urn:agentbox:bead:...` and back.
- agentbox emits IS-Envelope kind-1059; forum unwraps and renders message.

**Implementation**:
```rust
// integration-tests/cross_substrate/nip98.rs (separate workspace pinning all 4)
#[test]
fn forum_token_validates_in_visionclaw() {
    let token = forum_nip98::issue("POST", "https://x", body, ts, sk).unwrap();
    let parsed = visionclaw_nip98::validate(&token, "POST", "https://x", body, now()).unwrap();
    assert_eq!(parsed.pubkey, expected_pub);
}
```

**Cadence**: Nightly + on release.
**Owner**: QE Fleet (no single substrate owns this).
**Failure recovery**: The diamond problem — see T15. CI emits a per-substrate diff; humans triage.

### Level 3 — Federation contract (live mesh)

**Definition**: End-to-end message round-trips successfully across the live federated mesh of relays.

**Example assertion**: forum user DMs agentbox agent; agentbox replies; round-trip < 5s.

**Implementation**: Spun-up containers (T7).

**Cadence**: On staging deploy (smoke); rolled into release gate.
**Owner**: QE Fleet + DevOps.
**Failure recovery**: Block release. Rollback to previous-known-good staging.

---

## T6 — IS-Envelope conformance suite (ADR-075 D15)

For each of the 8 envelope kinds (`chat`, `tool_invoke`, `tool_result`, `knowledge_link`, `moderation`, `mesh_ping`, `bead_link`, `unknown` catch-all), the suite runs four assertions.

### T6.1 Wrap-then-unwrap byte-identity

```python
# pseudocode (any substrate)
for kind, fixture in is_envelope_fixtures.items():
    envelope = fixture['envelope']
    jcs = jcs_canonicalize(envelope)
    assert jcs == fixture['expectedJcs'], f"{kind}: JCS drift"

    wrapped = wrap_kind1059(envelope, recipient_pub, sender_sec)
    # tags must include single ['p', recipient]
    assert wrapped['kind'] == 1059
    assert wrapped['tags'] == [['p', recipient_pub]]

    unwrapped = unwrap_kind1059(wrapped, recipient_sec)
    assert canonical(unwrapped) == canonical(envelope), f"{kind}: roundtrip drift"
```

### T6.2 JCS canonical form pinned

For each envelope: `expectedJcs` is the byte-exact canonical form. Any substrate's JCS implementation MUST produce this output.

### T6.3 Negative cases

| Failure | Input mutation | Expected error code |
|---------|----------------|---------------------|
| Missing required field (`v`) | Remove `v` | `ERR_ENV_MISSING_FIELD` |
| Wrong type (`ts` is string) | `ts: "1700000000"` instead of `1700000000` | `ERR_ENV_TYPE_MISMATCH` |
| Oversized body (>64KB) | Inflate `body.text` to 65537 bytes | `ERR_ENV_OVERSIZED` |
| Expired `ttl` | `ts + ttl < now() - 60s` | `ERR_ENV_EXPIRED` |
| Version > supported | `v: 99` | `ERR_ENV_VERSION_TOO_NEW` |
| `via` chain too long (>5) | Add 6 hop entries | `ERR_ENV_VIA_TOO_LONG` |
| Unknown kind without `unknown` fallthrough | `kind: "rogue_kind"` | `ERR_ENV_UNKNOWN_KIND` |
| Body MAC mismatch (NIP-44 wrapped) | Flip last byte of payload | `ERR_NIP44_MAC_MISMATCH` |

Each substrate's parser MUST emit the canonical error code (mapped via `expected-error-map.json`).

### T6.4 Federation propagation (cross-substrate)

For each envelope kind: published from substrate A, received by substrate B; assert B's parsed envelope equals A's published envelope.

---

## T7 — Federation smoke test design (ADR-073 D11)

**Setup** (docker-compose / wrangler-dev orchestration):

```yaml
# tests/federation/docker-compose.yml
services:
  forum-relay:
    image: forum/relay-worker:test
    command: wrangler dev --local --port 8787
    healthcheck:
      test: curl -f http://localhost:8787/health
      interval: 2s

  agentbox-relay:
    image: scsibug/nostr-rs-relay:0.9.0
    ports: ["7777:8080"]
    volumes: ["./agentbox-relay-config:/usr/src/app/config"]

  visionclaw-bridge:
    image: visionclaw/mesh-bridge:test
    environment:
      MESH_PEERS: "ws://forum-relay:8787,ws://agentbox-relay:7777"
    depends_on: [forum-relay, agentbox-relay]

  test-runner:
    image: node:20
    volumes: [".:/work"]
    working_dir: /work
    command: npm run test:federation
    depends_on:
      forum-relay: { condition: service_healthy }
      agentbox-relay: { condition: service_healthy }
      visionclaw-bridge: { condition: service_healthy }
```

### Test cases

#### T7-a: forum → agentbox DM

```js
test('T7-a forum user DMs agentbox agent', async () => {
  const forum = await connectRelay('ws://forum-relay:8787');
  const agentbox = await connectRelay('ws://agentbox-relay:7777');

  const wrappedDm = wrapNip44Dm({
    from: ALICE_FORUM, to: BOB_AGENT,
    text: 'hello bob', sk: ALICE_FORUM_SK,
  });

  await forum.publish(wrappedDm);
  const received = await agentbox.subscribeOnce(
    [{ kinds: [1059], '#p': [BOB_AGENT.hex] }],
    { timeout: 5000 }
  );
  expect(received).toBeDefined();
  const unwrapped = unwrapNip44(received, BOB_AGENT_SK);
  expect(unwrapped.text).toBe('hello bob');
});
// Duration: ~6s
// Setup: relays up
// Teardown: docker-compose down
```

#### T7-b: agentbox → forum reply

```js
test('T7-b agentbox replies to forum user', async () => {
  // bob auto-replies via test fixture script
  const reply = wrapNip44Dm({ from: BOB_AGENT, to: ALICE_FORUM, text: 'hi alice', sk: BOB_AGENT_SK });
  await agentbox.publish(reply);
  const received = await forum.subscribeOnce([{ kinds: [1059], '#p': [ALICE_FORUM.hex] }], { timeout: 5000 });
  expect(unwrapNip44(received, ALICE_FORUM_SK).text).toBe('hi alice');
});
// Duration: ~5s
```

#### T7-c: VisionClaw bead-stamp fan-out (kind-30200)

```js
test('T7-c bead stamp federates to forum + agentbox', async () => {
  const stamp = signEvent({
    kind: 30200,
    tags: [['d', 'bead:ab...:exec-001']],
    content: JSON.stringify({ exec: 'urn:visionclaw:execution:ab...:1' }),
  }, VISIONCLAW_SK);
  await visionclawBridge.publish(stamp);
  const onForum = await forum.subscribeOnce([{ kinds: [30200], '#d': ['bead:ab...:exec-001'] }], { timeout: 5000 });
  const onAgentbox = await agentbox.subscribeOnce([{ kinds: [30200] }], { timeout: 5000 });
  expect(onForum.id).toBe(stamp.id);
  expect(onAgentbox.id).toBe(stamp.id);
});
// Duration: ~6s
```

#### T7-d: forum-issued ban → agentbox cache invalidation

```js
test('T7-d kind-30910 ban invalidates agentbox mod_cache', async () => {
  const ban = signEvent({
    kind: 30910,
    tags: [['p', BANNED_PUB]],
    content: 'spam',
  }, FORUM_ADMIN_SK);
  await forum.publish(ban);
  await sleep(35_000); // grace > 30s SLA
  const cacheState = await fetchAgentboxModCache(BANNED_PUB);
  expect(cacheState.banned).toBe(true);
  expect(cacheState.lastInvalidatedMs).toBeGreaterThan(Date.now() - 35_000);
});
// Duration: ~38s
```

#### T7-e: peer relay flap → exponential backoff + recovery

```js
test('T7-e peer outage → backoff → recover', async () => {
  await dockerStop('agentbox-relay');
  await sleep(2_000);
  const log = await fetchVisionclawLogs();
  expect(log).toContain('peer agentbox-relay unreachable, backoff');
  expect(log).toContain('next attempt in 1s');
  // backoff doubles
  await dockerStart('agentbox-relay');
  await sleep(15_000);
  expect(await fetchVisionclawLogs()).toContain('peer agentbox-relay restored');
});
// Duration: ~20s
```

#### T7-f: malformed kind-1059 dropped

```js
test('T7-f malformed kind-1059 dropped with logged error', async () => {
  const malformed = { kind: 1059, content: '{{not-json{{', tags: [], pubkey: '...', id: '...', sig: '...' };
  // skip signature; force-send via raw socket
  await forumRaw.send(JSON.stringify(['EVENT', malformed]));
  const log = await fetchForumLogs();
  expect(log).toMatch(/ERR_ENV_TYPE_MISMATCH|ERR_NIP01_BAD_SIG/);
  // and: must not have been federated to agentbox
  const onAgentbox = await agentbox.subscribeOnce([{ ids: [malformed.id] }], { timeout: 2000 });
  expect(onAgentbox).toBeUndefined();
});
// Duration: ~4s
```

#### T7-g: NIP-26 delegation preserved across federation

```js
test('T7-g delegated event preserves delegator attribution', async () => {
  const delegationTag = createDelegationTag(ALICE_FORUM_SK, BOB_AGENT.hex, conditions);
  const event = signEvent({
    kind: 1, tags: [delegationTag], content: 'delegated post',
  }, BOB_AGENT_SK);
  await visionclawBridge.publish(event);
  const onForum = await forum.subscribeOnce([{ ids: [event.id] }], { timeout: 5000 });
  const delegationFromForum = onForum.tags.find(t => t[0] === 'delegation');
  expect(delegationFromForum).toEqual(delegationTag); // byte-identical preservation
});
// Duration: ~5s
```

**Total smoke duration**: ~85 seconds (within the 10-min CI budget).
**Teardown**: `docker-compose down --volumes` after each scenario; full suite uses single fixture lifecycle.

---

## T8 — Property test design

One proptest per DDD invariant. Generators specified, expected behaviour stated.

### T8.1 Forum (F-Inv-01..07)

#### F-Inv-01 — Signature verification on every event

```rust
proptest! {
    #[test]
    fn forum_rejects_events_with_tampered_sig(
        ev in any_signed_event(),
        flip_byte in 0usize..64,
    ) {
        let mut tampered = ev.clone();
        tampered.sig[flip_byte] ^= 0x01;
        prop_assert!(forum::verify_event(&tampered).is_err());
    }
}
```

#### F-Inv-02 — Whitelist gate (only allowlisted pubkeys publish)

```rust
proptest! {
    #[test]
    fn forum_publish_rejects_non_whitelisted(pubkey in any_hex_pubkey()) {
        prop_assume!(!FORUM_WHITELIST.contains(&pubkey));
        let ev = signed_event_for(&pubkey, /*kind=*/1);
        prop_assert!(forum::publish(&ev).is_err());
    }
}
```

#### F-Inv-03 — kind-1059 filter rewrite (only `#p` of viewer)

```rust
proptest! {
    #[test]
    fn forum_filter_rewrite_kind1059(
        viewer in any_hex_pubkey(),
        filter in any_relay_filter(),
    ) {
        let rewritten = forum::rewrite_filter(&filter, &viewer);
        if rewritten.kinds.contains(&1059) {
            prop_assert_eq!(rewritten.p_tags, vec![viewer.clone()]);
        }
    }
}
```

#### F-Inv-04 — NIP-98 replay rejection

```rust
proptest! {
    #[test]
    fn forum_nip98_rejects_replay(token in any_valid_nip98()) {
        forum::accept_token(&token).unwrap(); // first time accepts
        prop_assert!(forum::accept_token(&token).is_err()); // second time rejects
    }
}
```

#### F-Inv-05/06/07: timestamp window, kind allowlist, deduplication — analogous proptests.

### T8.2 Agentbox (A-Inv-01..09)

#### A-Inv-01 — Signature-before-write

```js
fc.assert(fc.property(arbEvent(), (ev) => {
  // any code path that writes events to disk must verify first
  const writes = traceWritesFor(ev);
  for (const w of writes) {
    expect(w.precededBy('verifyEvent')).toBe(true);
  }
}));
```

#### A-Inv-02 — Recipient match enforced before delivery

```js
fc.assert(fc.property(arbWrappedDm(), (dm) => {
  const recipientTag = dm.tags.find(t => t[0] === 'p')[1];
  const deliveryTargets = simulateDeliver(dm);
  expect(deliveryTargets).toEqual([recipientTag]);
}));
```

#### A-Inv-03 — Atomic-rename on every event store-and-replace

```js
fc.assert(fc.property(arbEvent(), arbEvent(), (oldEv, newEv) => {
  // Replace must happen atomically: never observe partial state
  const states = observeStorageDuringReplace(oldEv, newEv);
  for (const s of states) {
    expect([oldEv, newEv]).toContainEqual(s); // never a hybrid
  }
}));
```

#### A-Inv-04..09: sovereign keypair uniqueness, ACL DID-form, type-string, orchestrator fatal, middleware order, RelayConsumer wired — analogous proptests in `agentbox/tests/property/`.

### T8.3 VisionClaw (V-Inv-01..07)

#### V-Inv-01 — Identity unification (single canonical pubkey form)

```rust
proptest! {
    #[test]
    fn identity_unification(input_form in any_pubkey_input_form()) {
        let canonical = visionclaw::identity::canonicalise(input_form.clone()).unwrap();
        prop_assert_eq!(canonical.len(), 64);
        prop_assert!(canonical.chars().all(|c| c.is_ascii_hexdigit()));
        // round-trip stability
        let again = visionclaw::identity::canonicalise(canonical.clone()).unwrap();
        prop_assert_eq!(canonical, again);
    }
}
```

#### V-Inv-02 — URN mint chokepoint (only mint.rs produces urn:visionclaw:*)

```rust
// build-time check rather than proptest: walk AST, find any string literal "urn:visionclaw:" outside src/uri/mint.rs, fail.
// Plus runtime test:
proptest! {
    #[test]
    fn urn_mint_grammar(kind in any_kind(), scope in any_scope(), local in any_local()) {
        let urn = mint_kind(&kind, &scope, &local).unwrap();
        let parsed = parse(&urn).unwrap();
        prop_assert_eq!(parsed.kind, kind);
        prop_assert_eq!(parsed.scope, scope);
        prop_assert_eq!(parsed.local, local);
    }
}
```

#### V-Inv-03..07: DID resolution, delegation verify, bead URN scoping, forward-verbatim, WebID derivation — analogous.

### T8.4 solid-pod-rs (S-Inv-01..04)

#### S-Inv-02 — NIP-98 body-hash order

```rust
proptest! {
    #[test]
    fn nip98_body_hash_computed_before_payload_tag(
        method in any_http_method(), url in any_url(), body in any_body(),
    ) {
        let expected_hash = sha256(&body);
        let token = nip98::issue(&method, &url, &body, now(), &TEST_SK).unwrap();
        let event = decode_token(&token);
        let payload_tag = event.tags.iter().find(|t| t[0] == "payload").unwrap();
        prop_assert_eq!(payload_tag[1], hex::encode(expected_hash));
    }
}
```

#### S-Inv-04 — Storage trait abstraction (no direct disk calls outside Storage impl)

```rust
// build-time grep equivalent: assert no `std::fs::*` outside src/storage/*.rs
```

#### S-Inv-01/03 analogous.

---

## T9 — Mutation testing plan

Mutation tests assert "if I sabotage a line, a test catches it." Targets are the small absorbed shims (high concentration of cryptographic logic), not the entire codebase (which would take days).

| Substrate | Tool | Target modules | Kill-rate target | Cadence |
|-----------|------|----------------|------------------|---------|
| Forum | `cargo-mutants` | `nostr-core/src/nip44/`, `nostr-core/src/nip19/`, `nostr-core/src/nip98/` | ≥ 80% (Q4 G16) | Full nightly cron; sampled (`--shard 1/4` rotated) per PR |
| VisionClaw | `cargo-mutants` | `src/uri/mint.rs`, `src/uri/parse.rs`, `src/identity/`, `src/nip98/` | ≥ 80% | Full nightly; sampled per PR |
| Agentbox | `stryker` | `lib/uris.js`, `lib/nip44.js`, `lib/nip98.js`, `lib/relay-consumer.js` | ≥ 75% (JS tooling slightly weaker) | Full nightly; sampled per PR |
| solid-pod-rs | `cargo-mutants` | `crates/solid-pod-rs-nostr/src/did.rs`, `.../bech32.rs`, `.../nip98.rs` | ≥ 80% | Full nightly; sampled per PR |

**Sampling strategy** (per-PR): `cargo mutants --shard $((PR_NUMBER % 4 + 1))/4` runs ~25% of mutants. Nightly cron runs all. Score regressions block PRs.

**False-positive handling**: mutants on logging, metric emission, or comment-aligned lines are excluded via `.cargo-mutants-ignore`.

---

## T10 — Fuzz testing plan

Fuzzing finds bugs proptest does not — undefined behaviour in raw-byte parsers.

| Target | Tool | Corpus seed strategy | PR budget | Nightly budget |
|--------|------|---------------------|-----------|----------------|
| Forum nostr-core envelope parser (`parse_relay_message`) | `cargo-fuzz` | NIP-01 valid messages + corruption | 5 min | 4 hr |
| Agentbox `RelayConsumer._verifyEvent` fallback parser | `jest-fuzz` + custom harness | Wrapped event corpus | 5 min | 4 hr |
| VisionClaw URN parser (`src/uri/parse.rs`) | `cargo-fuzz` | All `urn:visionclaw:*` from production logs (sanitised) + grammar-perturbed strings | 5 min | 4 hr |
| IS-Envelope JCS canonicalisation (differential) | `cargo-fuzz` for Rust impl, `node --inspect-brk` for JS, `hypothesis` for Python | Random JSON (recursive depth ≤ 5); compare three impls' outputs | 5 min | 4 hr |

**Differential fuzzing** for JCS is the strongest: any input where the three implementations disagree is a bug somewhere — find which.

**Corpus persistence**: `tests/fuzz/corpus/` checked in (small inputs only); large corpora in S3 bucket `dreamlab-fuzz-corpora` with retention 30d.

**Crash triage**: cargo-fuzz crashes auto-create GitHub issues with reproducer + minimised input.

---

## T11 — CI workflow design

### Per-substrate workflow (every PR to that substrate)

```yaml
# Example: forum/.github/workflows/ci.yml
name: forum-ci
on: { pull_request: { branches: [main] } }
jobs:
  ci:
    steps:
      - checkout
      - rustup install stable
      - cargo clippy -- -D warnings              # 1. Lint
      - cargo fmt --check                         # 2. Format
      - cargo test --workspace                    # 3. Unit + property
      - cargo test --test upstream_vectors        # 4. Reference vectors (T2/T4)
      - cargo test --test contract_level1         # 5. Contract Level 1 (T5)
      - run: ./tools/check-bundle-size.sh         # 6. CF Workers bundle ≤ 1MB
      - run: ./tools/check-anti-drift-lint.sh     # 7. urn:* mint chokepoint, type strings
      - run: cargo llvm-cov --fail-under-lines 80 # 8. Coverage ≥ 80% (Q4 G15)
      - run: ./tools/verify-fixtures-pinned.sh    # 9. Hash check vs docs/specs/fixtures/
```

### Anti-drift lint script (the C2/C3 trip-wire)

```bash
#!/bin/bash
# tools/check-anti-drift-lint.sh
set -e

# 1. urn:visionclaw: literal must only appear in src/uri/mint.rs and src/uri/parse.rs
LEAKS=$(rg --type rust 'urn:visionclaw:' -l | grep -vE 'src/uri/(mint|parse)\.rs')
if [ -n "$LEAKS" ]; then
  echo "FAIL: urn:visionclaw:* literal outside mint chokepoint:"
  echo "$LEAKS"
  exit 1
fi

# 2. DID verificationMethod type must be Multikey
BAD_TYPES=$(rg --type rust '"type":\s*"(JsonWebKey2020|Ed25519VerificationKey2020)"')
if [ -n "$BAD_TYPES" ]; then
  echo "FAIL: drifted DID type — must be 'Multikey' (C3 trip-wire):"
  echo "$BAD_TYPES"
  exit 1
fi

# 3. NIP-44 conv-key derivation must use upstream HKDF salt
SALT_DEFINES=$(rg --type rust 'NIP44_SALT' -l | grep -v 'nostr-core/src/nip44.rs')
if [ -n "$SALT_DEFINES" ]; then
  echo "FAIL: NIP44_SALT redefined outside nip44.rs (C1 trip-wire):"
  echo "$SALT_DEFINES"
  exit 1
fi

echo "OK: anti-drift lint passed"
```

### Cross-substrate workflow (separate)

```yaml
# .github/workflows/cross-substrate.yml
name: cross-substrate-contract
on:
  schedule: [{ cron: '0 2 * * *' }]   # nightly
  workflow_dispatch:
jobs:
  contract:
    steps:
      - checkout: forum @ release-pin
      - checkout: agentbox @ release-pin
      - checkout: visionclaw @ release-pin
      - checkout: solid-pod-rs @ release-pin
      - run: ./tools/run-contract-level2.sh    # ~5 min
      - run: ./tools/run-federation-smoke.sh   # ~10 min (T7)
      - report: cross-substrate-status.json    # uploaded to GitHub Pages dashboard
```

**Release gating**: a failing nightly cross-substrate run blocks the next release. The release engineer must triage (see T15 diamond problem) before un-gating.

---

## T12 — Test data privacy & secrets

### T12.1 No real keys

All test fixtures use one of:

- `TEST_USER_HEX = "ab".repeat(32)` (Alice)
- `TEST_USER2_HEX = "cd".repeat(32)` (Bob)
- `TEST_ADMIN_HEX = "ef".repeat(32)` (Admin)

Plus the canonical paulmillr/nip44 vectors (which are themselves deterministic public test vectors, not real user keys).

**Lint rule**: `tools/check-no-real-keys.sh` greps for any 64-hex string that is NOT one of the test sentinels NOR a value from `nip44-v2.json`. Any other hit triggers manual review.

### T12.2 Deterministic randomness

For reproducibility, ECDSA signing in tests uses `RFC 6979` deterministic-`k` mode (already the secp256k1 default). Any test that needs random bytes draws from `tests/fixtures/random-seed.bin` (a checked-in 4KB blob).

### T12.3 No production endpoints

Tests must not reach:

- `*.cloudflare.com` (forum prod)
- `*.dreamlab.example` (visionclaw prod)
- `pod.dreamlab.example` (solid-pod prod)
- Real Nostr relays (`relay.damus.io`, etc.)

Network sandboxing in CI: `iptables -A OUTPUT -p tcp -d 0.0.0.0/0 -j REJECT` after spinning up local relays. (Allows localhost only.)

### T12.4 Secrets in CI

- Test signing keys are derived from `TEST_USER_HEX` constants — no GitHub secrets needed.
- For staging smoke tests (T7-Level-3), staging signing keys are stored in `STAGING_FORUM_SK` etc. as repository secrets.
- `secret-scan` GitHub Action runs on every PR; flags any 64-hex literal that is not a recognised test constant.

---

## T13 — Reproducibility & golden-file management

### T13.1 Update flag

Golden files (anything with `expectedXxx` field) are committed. Updates require explicit flag:

```bash
UPDATE_GOLDENS=1 cargo test --test contracts
# or
npm run test:contracts -- --update-snapshots
```

CI rejects any PR where golden files changed AND `UPDATE_GOLDENS` reason is not stated in PR body.

### T13.2 Sprint v9 alignment

The Sprint v9 introduced `npm run test` paths for agentbox. Document analogous in each substrate's `tests/README.md`:

| Substrate | Run all | Run contract | Update goldens |
|-----------|---------|--------------|----------------|
| forum | `cargo test --workspace` | `cargo test --test contracts` | `UPDATE_GOLDENS=1 cargo test --test contracts` |
| visionclaw | `cargo test` | `cargo test --test cross_system_contracts` | `UPDATE_GOLDENS=1 cargo test --test cross_system_contracts` |
| agentbox | `npm test` | `npm run test:contracts` | `npm run test:contracts -- --update-snapshots` |
| solid-pod-rs | `cargo test --workspace` | `cargo test --package solid-pod-rs-nostr --test upstream_vectors` | `UPDATE_GOLDENS=1 cargo test ...` |

### T13.3 Lockfile pinning

- `Cargo.lock` committed in every Rust workspace. `nostr` crate pinned to exact version (e.g. `nostr = "= 0.36.0"` in Cargo.toml — no caret).
- `package-lock.json` committed. `nostr-tools` pinned (`"nostr-tools": "2.7.2"` — no caret).
- Renovate bot configured to open PRs for upstream version bumps; PR must include re-running upstream-vector tests against the new version.

---

## T14 — Documentation alignment

Each substrate's `tests/README.md` MUST contain four sections (template):

```markdown
# tests/README.md

## Where do these fixtures come from?

Fixtures in `tests/fixtures/` are CI-verified copies of `docs/specs/fixtures/` in
the visionclaw monorepo (master copy). Upstream sources for reference vectors are
documented in `docs/specs/fixtures/UPSTREAM_PINS.md`.

If `tests/fixtures/<file>.json` differs from upstream, CI fails on
`tools/verify-fixtures-pinned.sh`.

## Running contract tests

- All tests: `<substrate-specific command>`
- Contract Level 1 (within-substrate): `<command>`
- Reference vectors (upstream conformance): `<command>`

## Updating fixtures when upstream changes

1. Update `docs/specs/fixtures/UPSTREAM_PINS.md` with new commit hash + date + reviewer.
2. Run `tools/sync-fixtures.sh <spec>` to refresh `docs/specs/fixtures/<file>.json`.
3. CI auto-syncs the verified copy into each substrate's `tests/fixtures/`.
4. Re-run all four substrates' `<contract test command>`. ALL must pass.

## Cross-references

- forum: see `forum/tests/README.md`
- visionclaw: see `tests/README.md` in this repo
- agentbox: see `agentbox/tests/README.md`
- solid-pod-rs: see `solid-pod-rs/tests/README.md`
- IS-Envelope spec: `docs/adr/ADR-075-is-envelope.md`
- Federation spec: `docs/adr/ADR-073-mesh-federation.md`
```

---

## T15 — Test ownership

### T15.1 Per-substrate ownership

| Concern | Owner |
|---------|-------|
| Updating `nip44-v2.json` when paulmillr publishes new vectors | QE Fleet (rotating) |
| Adding new contract tests when a new cross-substrate flow ships | Substrate that adds the flow files PR; QE Fleet reviews |
| Substrate-local property tests (Forum F-Inv-*) | Substrate maintainer (forum: @forum-team; etc.) |
| Mutation/fuzz infrastructure | DevOps + QE Fleet |
| `UPSTREAM_PINS.md` review (quarterly audit) | QE Fleet |
| Federation smoke (T7) infrastructure | DevOps |
| Anti-drift lint script maintenance | QE Fleet |

### T15.2 The diamond problem (cross-substrate failure triage)

When a Level 2 contract test fails (substrate A's output, substrate B can't parse), neither A nor B is automatically "wrong." The triage protocol:

1. **Re-run upstream-vector tests in both A and B**. If A passes upstream and B fails upstream → B is wrong. If both pass upstream but A's output ≠ B's expected input → spec ambiguity OR DreamLab-internal contract drift. Escalate to ADR review.

2. **Compare against a third reference implementation**. For NIP-44, run paulmillr's TS reference against A's output. If TS-ref accepts → B is wrong. If TS-ref rejects → A is wrong.

3. **Default ownership for ambiguous cases**: the substrate that *most recently changed* the relevant module. CI auto-tags last-touched-by from git blame.

4. **Escalation**: if no automatic ownership is clear, file a tri-substrate triage issue with the failing fixture attached. QE Fleet leads triage; ADR may be required if it surfaces an architectural disagreement.

### T15.3 Test rot prevention

- Quarterly audit (calendar event for QE Fleet): walk all upstream pins, verify still latest stable.
- Annual audit: re-derive every `expectedXxx` from scratch (regenerate goldens with `UPDATE_GOLDENS=1`); diff against committed values; if drift found, root-cause-analysis.

---

## Cross-references

- **Q1** — Surfaces inventory (`01-visionclaw-surfaces.md`, `02-forum-surfaces.md`, `03-agentbox-surfaces.md`, `04-solid-pod-rs-surfaces.md`): identified the modules each substrate exposes; T4 runners target those modules.
- **Q2** — Crypto gotchas (`05-crypto-gotchas.md`): C1, C2, C3 root causes; T1.1 reference vectors and T11 anti-drift lint script directly defend against the cited crypto-gotcha class.
- **Q3** — URI / data-flow alignment (`06-uri-dataflow-alignment.md`): T6 IS-Envelope, T8 V-Inv-02 URN mint chokepoint, T11 anti-drift lint pin the URI-mint discipline.
- **Q4** (forthcoming) — Quality gates G15 (coverage ≥ 80%) and G16 (mutation ≥ 80%) referenced in T9 and T11.

---

## Summary deliverables

This design produces, when implemented:

1. `docs/specs/fixtures/` — single source of truth for all 14+ fixture files
2. `docs/specs/fixtures/UPSTREAM_PINS.md` — pinned commit hashes for every upstream
3. `tools/sync-fixtures.sh` — pull upstream → master copy
4. `tools/verify-fixtures-pinned.sh` — CI-verify each substrate's copy matches master
5. `tools/check-anti-drift-lint.sh` — guards C1, C2, C3 trip-wires
6. Per-substrate `tests/upstream_vectors.*` — Level 1 contract tests
7. `integration-tests/` workspace — Level 2 cross-substrate contract tests
8. `tests/federation/` Docker harness — Level 3 smoke tests (T7)
9. Per-substrate property test suites (T8) — one per DDD invariant
10. Mutation + fuzz CI cron jobs (T9, T10)
11. Per-substrate `tests/README.md` aligned to T14 template
12. RACI assignment (T15) wired into CODEOWNERS

The goal: any future C1-class, C2-class, or C3-class bug fails CI before merging, in every substrate that touches the affected primitive.
