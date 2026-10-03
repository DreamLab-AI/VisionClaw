# Blocktrails verification fixtures (vendored)

Both files are copied unchanged from solid-pod-rs `v0.5.0-alpha.12`
(`d64131b`), `crates/solid-pod-rs/tests/fixtures/blocktrails/`. Nothing in
them is computed by solid-pod-rs or by this repository.

| File | sha256 | What it is |
|------|--------|------------|
| `verify-trail-vectors.json` | `1ec66ee55060263af7d0f8da3586372d63c94390c4ab5ebe321d02a59373beaa` | [blocktrails/verify](https://github.com/blocktrails/verify) `043e7af` `index.html` run as published under a stub DOM and fetch: every mark's label, the summary verdict and the walk's expected outputs for 13 trails (git-mark's live trail and variants, plain-profile trails); [blocktrails/git-mark](https://github.com/blocktrails/git-mark) `b852d7d` `Gitmark.verify` on the live trail and on two commits swapped. |
| `live-trail-txs.json` | `d5e9644c3d58b76ee6df90c1ea443e6d17e430255be67bd35f7f6ca64309aaf2` | mempool.space testnet4 `GET /api/tx/{txid}` for the three marks of the live trail git-mark `b852d7d` pins, captured 2026-10-02. |

Consumed by `src/web_contract/trail.rs` (the gitmark profile §5.2 shape) and
`src/web_contract/ritual.rs` (the host walker against blocktrails/verify and
against solid-pod-rs's `verify_blocktrail`). The generator,
`gen-verify-trail-vectors.mjs`, lives beside the originals in solid-pod-rs;
refresh by copying both files from the solid-pod-rs release the host pins.
