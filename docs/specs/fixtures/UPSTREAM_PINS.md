# Upstream Reference Vector Pins

Per ADR-082 D2. Each fixture file in this directory derives vectors from an upstream source pinned to a specific commit hash.

## Pin format

```
## <fixture-name>
- Repository: <github-url>
- Pinned commit: `<full-sha>` (<date>)
- Path: <path-in-repo>
- Last refresh: <iso-8601>
- Refresh policy: quarterly | on-spec-change | manual
- Vectors: <count>
- Notes: <commentary>
```

## Active pins

### nip44-v2 (paulmillr/nip44)
- Repository: https://github.com/paulmillr/nip44
- Pinned commit: `671a1f04bcfacaf125b0db68adc45bc9ce0e763b` (HEAD as of 2026-05-07)
- Path: `javascript/test/nip44.vectors.json`
- Last refresh: 2026-05-07
- Refresh policy: on NIP-44 spec amendment
- Vectors: 98 (44 valid.get_conversation_key + 32 valid.get_message_keys + 0 valid.calc_padded_len + …, see `nip44-v2.json#/_meta/kind_breakdown`)
- Notes: **LOAD-BEARING for C1 regression guard.** This is the file C1 (NIP-44 conv-key bug) shipped past because no vectors existed.

### bip340-schnorr (bitcoin/bips)
- Repository: https://github.com/bitcoin/bips
- Pinned commit: `acf99fc16099fc4464df6b4f3cff4cfe2a84fecd` (HEAD as of 2026-05-07)
- Path: `bip-0340/test-vectors.csv`
- Last refresh: 2026-05-07
- Refresh policy: on BIP-340 amendment
- Vectors: 19 reference vectors per BIP-340 spec (CSV → JSON in `bip340-schnorr.json`)
- Notes: Schnorr signature verification ground truth.

### rfc8785-jcs (cyberphone/json-canonicalization)
- Repository: https://github.com/cyberphone/json-canonicalization
- Pinned commit: `19d51d7fe467d4706a3ff08adf8a748f29fc21e0` (HEAD as of 2026-05-07)
- Path: `testdata/`
- Last refresh: 2026-05-07
- Refresh policy: stable; refresh on RFC 8785 errata
- Vectors: 6 input/output canonicalisation pairs (`arrays`, `french`, `structures`, `unicode`, `values`, `weird`)

### nip01-events (nostr-protocol/nips)
- Repository: https://github.com/nostr-protocol/nips
- Pinned commit: `05d3f198c61c2732ccf15ba8005299365dabb8e0` (HEAD as of 2026-05-07)
- Path: `01.md`
- Last refresh: 2026-05-07
- Refresh policy: on NIP-01 amendment
- Vectors: 11 (5 positive serialisation + 1 metadata + 1 escapes-tab/cr/quote + 1 backslash + 1 multi-tag + 4 negative)
- Notes: NIP-01 spec lacks worked example IDs; substrates compute id = sha256(serialised) at test time and assert matching with their implementation.

### nip04-dm (nostr-protocol/nips)
- Repository: https://github.com/nostr-protocol/nips
- Pinned commit: `05d3f198c61c2732ccf15ba8005299365dabb8e0` (HEAD as of 2026-05-07)
- Path: `04.md`
- Last refresh: 2026-05-07
- Refresh policy: on NIP-04 amendment (deprecated; superseded by NIP-17)
- Vectors: 4 (shape regex + ECDH rule + 2 negative)
- Notes: NIP-04 has no canonical test vectors in spec; substrates round-trip encrypt/decrypt and verify the wire-shape regex.

### nip19-bech32 (nostr-protocol/nips)
- Repository: https://github.com/nostr-protocol/nips
- Pinned commit: `05d3f198c61c2732ccf15ba8005299365dabb8e0` (HEAD as of 2026-05-07)
- Path: `19.md`
- Last refresh: 2026-05-07
- Refresh policy: on NIP-19 amendment
- Vectors: 12 (4 spec-canonical + 4 negative + 4 round-trip edge cases)

### nip26-delegation (nostr-protocol/nips)
- Repository: https://github.com/nostr-protocol/nips
- Pinned commit: `05d3f198c61c2732ccf15ba8005299365dabb8e0` (HEAD as of 2026-05-07)
- Path: `26.md`
- Last refresh: 2026-05-07
- Refresh policy: on NIP-26 amendment
- Vectors: 5 (1 full canonical with sig + 3 spec-conditions + 1 negative)
- Notes: Canonical NIP-26 example pair (delegator/delegatee privkey + sig) covers signing-string + Schnorr verification ground truth.

### nip59-gift-wrap (nostr-protocol/nips)
- Repository: https://github.com/nostr-protocol/nips
- Pinned commit: `05d3f198c61c2732ccf15ba8005299365dabb8e0` (HEAD as of 2026-05-07)
- Path: `59.md`
- Last refresh: 2026-05-07
- Refresh policy: on NIP-59 amendment
- Vectors: 6 (3 layer-shape positive + 3 negative)
- Notes: Spec example shows the rumor JSON; seal/wrap content is encrypted (deferred to substrate-side round-trip tests).

### nip98-tokens (nostr-protocol/nips)
- Repository: https://github.com/nostr-protocol/nips
- Pinned commit: `05d3f198c61c2732ccf15ba8005299365dabb8e0` (HEAD as of 2026-05-07)
- Path: `98.md`
- Last refresh: 2026-05-07
- Refresh policy: on NIP-98 amendment
- Vectors: 6 (1 spec-canonical signed event + 4 negative + 1 POST-with-payload)

### multibase (multiformats/multibase)
- Repository: https://github.com/multiformats/multibase
- Pinned commit: `d7406cdea189b82a0b3937f5737b440f5fa92f92` (HEAD as of 2026-05-07)
- Path: `tests/basic.csv` + `tests/leading_zero.csv` + `tests/case_insensitivity.csv`
- Last refresh: 2026-05-07
- Refresh policy: on multibase spec amendment
- Vectors: 27 (18 basic encodings + 4 leading-zero + 2 case-insensitive + 3 negative)

### did-doc-conformance (DreamLab-internal)
- Source: `/home/devuser/workspace/project/docs/adr/ADR-074-cross-system-did-nostr-canonicalisation.md` D2 (DID Document canonical shape)
- Pinned commit: in-tree (this monorepo)
- Last refresh: 2026-05-07
- Refresh policy: on ADR-074 amendment
- Vectors: 7 (2 positive Tier-3/Tier-1 + 5 negative for each major D-rule violation: stale-suite-2022, stale-suite-2025, missing-context, uppercase-id, mismatched-controller)

### is-envelope-v1 (DreamLab-internal)
- Source: `/home/devuser/workspace/project/docs/adr/ADR-075-is-envelope-message-contract.md` D1+D3 (envelope schema, per-kind body shapes)
- Pinned commit: in-tree
- Last refresh: 2026-05-07
- Refresh policy: on ADR-075 amendment
- Vectors: 11 (8 positive — one per envelope kind + delegation-mirrored + 3 negative — missing-from, unknown-kind, version-mismatch)

### mesh-federation (DreamLab-internal)
- Source: `/home/devuser/workspace/project/docs/adr/ADR-073-private-nostr-relay-mesh-topology.md` D2/D6/D9 + ADR-074 D9 (kind-30033 mesh service-list)
- Pinned commit: in-tree
- Last refresh: 2026-05-07
- Refresh policy: on ADR-073 or ADR-074 D9 amendment
- Vectors: 9 (covering fanout, lru-dedup, loop-avoidance, service-list, mode-config scenarios + 1 negative for standalone mode)

## Refresh workflow

1. Open "fixture refresh" PR in VisionClaw monorepo updating UPSTREAM_PINS.md
2. Update fixture file with new vectors from new commit
3. Re-run `tests/fixture-master-validity.sh` (must pass)
4. Re-run all consuming substrates' L1 reference vector tests via ADR-082 D5 sync mechanism
5. If any L1 fails, root-cause: upstream change vs our implementation drift
6. Merge refresh PR + any required implementation fixes together

## Audit trail

| Date | Fixture | Action | Author | Reason |
|------|---------|--------|--------|--------|
| 2026-05-07 | (all) | initial scaffold | sprint setup | per ADR-082 D2 |
| 2026-05-07 | nip44-v2.json | initial vendor (paulmillr@671a1f0) | mega-sprint Phase 0 | C1 regression guard; 98 vectors |
| 2026-05-07 | bip340-schnorr.json | initial vendor (bitcoin/bips@acf99fc) | mega-sprint Phase 0 | C2 BIP-340 regression guard; 19 vectors |
| 2026-05-07 | rfc8785-jcs.json | initial vendor (cyberphone@19d51d7) | mega-sprint Phase 0 | ADR-075 IS-Envelope canonicalisation; 6 vectors |
| 2026-05-07 | nip01-events.json | initial vendor (nostr-protocol/nips@05d3f19) | mega-sprint Phase 1 | NIP-01 serialisation + escapes; 11 vectors |
| 2026-05-07 | nip04-dm.json | initial vendor (nostr-protocol/nips@05d3f19) | mega-sprint Phase 1 | NIP-04 wire shape + ECDH rule; 4 vectors |
| 2026-05-07 | nip19-bech32.json | initial vendor (nostr-protocol/nips@05d3f19) | mega-sprint Phase 1 | NIP-19 bech32 entities; 12 vectors |
| 2026-05-07 | nip26-delegation.json | initial vendor (nostr-protocol/nips@05d3f19) | mega-sprint Phase 1 | NIP-26 delegation + canonical signing; 5 vectors |
| 2026-05-07 | nip59-gift-wrap.json | initial vendor (nostr-protocol/nips@05d3f19) | mega-sprint Phase 1 | NIP-59 layer shapes; 6 vectors |
| 2026-05-07 | nip98-tokens.json | initial vendor (nostr-protocol/nips@05d3f19) | mega-sprint Phase 1 | NIP-98 HTTP Auth; 6 vectors |
| 2026-05-07 | multibase.json | initial vendor (multiformats/multibase@d7406cd) | mega-sprint Phase 1 | Self-describing base encoding; 27 vectors |
| 2026-05-07 | did-doc-conformance.json | initial DreamLab-internal (ADR-074 D2) | mega-sprint Phase 1 | DID Document conformance; 7 vectors |
| 2026-05-07 | is-envelope-v1.json | initial DreamLab-internal (ADR-075 D1+D3) | mega-sprint Phase 1 | IS-Envelope per-kind shapes; 11 vectors |
| 2026-05-07 | mesh-federation.json | initial DreamLab-internal (ADR-073 D2/D6/D9 + ADR-074 D9) | mega-sprint Phase 1 | Mesh federation behaviour; 9 vectors |
