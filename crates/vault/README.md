<!-- lands with ontomark step 10 -->
# `vault` — the AGPL governance adapter

`vault` is the thin binary through which proposals leave VisionClaw for the forum.
It owns three verbs and nothing else (ADR-2137, decision D4 of the ontomark
deprecation plan):

| Command | What it does |
|---|---|
| `vault propose <iri> --level L --hypothesis H [--diff D] [--title T] [--dry-run]` | build a `PatchProposal` over the ontomark crates, check it with the reasoner and conflict blockers, sign it with the proposing key and post it as a forum 31402 |
| `vault apply <file> --expect docs=1 [--set 'key+=…']` | land a human-approved `kind: amend` proposal on its page through the guarded patch in `ontomark-parse` |
| `vault panel publish [--dry-run]` | sign and publish the `ontology-governance` panel definition (kind 31400) that proposals are filed against |

Every verb accepts `--json`. Exit codes are **0** success, **1** a failed check or
execution error, **2** a refused operation or invalid argument.

Everything else the old `vault` binary did now belongs to the `ontomark` CLI
(`validate`, `find`, `retrieve`, `tree`, `edit`, `create`, `gate`, `conflicts`,
`build`, `repair`): see the ontomark repository. The `vault-core` crate and the
build, reasoning and validation modules of this crate were deleted at ontomark
step 10.

## Licence

`vault` is AGPL-3.0-only. `src/nostr.rs` (the forum ActionRequest, NIP-42 AUTH and
relay publishing) descends from solid-pod and sidestr code and stays AGPL; the
rest of the former `vault` and `vault-core` code was relicensed MIT OR Apache-2.0
for extraction into ontomark.

## Build

```bash
cargo build --release -p vault     # target/release/vault
```

Signing uses `nostr-bbs-core`; the key comes from `--secret` or
`VAULT_NOSTR_SECRET`, the relay from `--relay` or `VAULT_RELAY_URL`.
`propose` and `apply` locate the repository by walking up to
`ontology/vocabulary.yaml`, or take `--repo <dir>`.

## Governing records

[ADR-2137](../../docs/adr/ADR-2137-consume-shared-ontology-workspace.md) (consume the
shared ontology workspace), superseding
[ADR-2113](../../docs/adr/ADR-2113-crates-vault-is-the-single-corpus-parser-and-build.md);
the corpus contract is [VAULT-corpus-format](../../docs/VAULT-corpus-format.md).
