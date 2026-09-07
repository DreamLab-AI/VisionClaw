**How to work against this pack** (engineering/build-with-quality agents start here):

The ADR pack for any domain is **its living governing document in `docs/` plus the
ledger records below that amend it**. The living docs are normative — their
*Invariants* sections are the compliance surface and their *Change process*
sections say how to amend them:

| Domain | Governing document |
|---|---|
| Stores, processes, actors, trust boundaries | [`../BASELINE-architecture.md`](../BASELINE-architecture.md) |
| Identity, NIP-98, RBAC, delegation | [`../IDENTITY-authority-chain.md`](../IDENTITY-authority-chain.md) |
| Data ownership, dual-writes, erasure, backup | [`../DATA-authority-erasure.md`](../DATA-authority-erasure.md) |
| Wire frames, endpoints, versioning | [`../PROTOCOL-registry.md`](../PROTOCOL-registry.md) |
| IRIs, URNs, node ids, DID documents | [`../IDENTIFIER-taxonomy.md`](../IDENTIFIER-taxonomy.md) |
| Security flags, named profiles, illegal combos | [`../SECURITY-profiles.md`](../SECURITY-profiles.md) |
| SimParams ABI, force channels, kernels | [`../GPU-wire-abi.md`](../GPU-wire-abi.md) |
| Godot XR client, HUD, deploy | [`../XR-client.md`](../XR-client.md) |
| Authored corpus: vault layout, frontmatter, inclusion gate, converter | [`../VAULT-corpus-format.md`](../VAULT-corpus-format.md) |

**Lookup order:** governing doc → its `file:line` citations into code → the ledger
records below → `docs/archive/` **only for rationale and history — never as
authority** (the archive is the pre-2026-08-31 corpus, frozen precisely because it
drifted from the code; see [`../MIGRATION-plan.md`](../MIGRATION-plan.md) for the
legacy-number redirect table).

**Making a decision:** copy [`TEMPLATE.md`](TEMPLATE.md) to `ADR-NNNN-slug.md`
(next free number), fill the three-axis status honestly, update the affected
governing document **in the same change**, and regenerate this index
(`node scripts/adr-index-gen.js docs/adr` — CI-enforced via
`.github/workflows/docs-ci.yml`: invalid frontmatter, asymmetric supersession
edges, and stale `verified_commit`+`verified_paths` claims all fail the
build).

The [historical closeout routing note](../adr-history-closeout.md) points each frozen archive record at the estate section-level review and the complete VisionClaw historical map; a lineage mention there does not supersede every section of a predecessor.

The [estate status/evidence contract](../../../VisionFlow/docs/architecture/adr-status-contract.md) defines the independent decision, implementation and activation axes and distinguishes lineage from supersession (2026-09-07).
