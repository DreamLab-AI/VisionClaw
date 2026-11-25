# Regenerating `golden.json` (the ontomark oracle)

`golden.json` is **ontomark's** answer for every page of this fixture, so it has
to be regenerated whenever ontomark's rules, the fixture or the `Vc*` pages
change. The harness is a throwaway crate OUTSIDE this workspace with a path
dependency on an ontomark checkout; it calls ontomark's own predicates and
prints JSON. Nothing from VisionClaw is involved, which is the point.

1. Make a scratch directory with the two files below (`Cargo.toml`, `src/main.rs`)
   and copy ontomark's `Cargo.lock` next to `Cargo.toml` so the dependency
   versions match ontomark's. Replace `<ontomark checkout at v0.10.0>` in the `ontomark-compile` path with the
   checkout's directory.
2. Build and run it against this fixture, once per vocabulary version, at the
   fixed clock used by the tests (`now = 1794961600`, 2026-11-18T00:26:40Z):

```text
cargo build --offline            # CARGO_TARGET_DIR of your choice
om-oracle <this fixture dir> 3 1794961600 > g3.json
om-oracle <this fixture dir> 2 1794961600 > g2.json
```

3. Merge into `golden.json` as `{"now": 1794961600, "ontomark": "<commit>",
   "pages": {"<vault>/<page id>": {"v2": <g2 entry>, "v3": <g3 entry>}}}`,
   keys sorted. (`golden.json` was produced from ontomark `v0.10.0`, commit
   `fa173a183e65a29944becb6eb4d55b52b9ba6b06`. A journal's key is `working/<date>`.)
   The `human_certified` line must call `is_human_verified_under(&vocab, …)`: the
   older `is_human_verified_with` is the vocabulary 2 reading and still accepts
   the `human:` prefix under 3.
4. `cargo test -p vault-core --test ontomark_v3_parity -p visionclaw-domain --test ontomark_v3_parity`.
   A difference that is not a bug in VisionClaw goes into `EXCEPTIONS` in the
   vault-core test with its reason.

## `Cargo.toml`

```toml
[package]
name = "om-oracle"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
ontomark-compile = { path = "<ontomark checkout at v0.10.0>/crates/ontomark-compile" }
serde_json = "1"

[workspace]
```

## `src/main.rs`

```rust
//! Scratch oracle: ontomark's own answers for every page of a vault, for the
//! visibility / principal / human-certification / recall classification.
//! Usage: om-oracle <repo> <version 2|3> <now-unix>
use ontomark_compile::build::attribution::Roster;
use ontomark_compile::check::v3::state::{is_public_under, is_recallable};
use ontomark_compile::input::page::{Vault, VaultKind};
use ontomark_compile::input::vocabulary::Vocabulary;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(&a[1]);
    let version: u32 = a[2].parse().unwrap();
    let now: i64 = a[3].parse().unwrap();
    let text = std::fs::read_to_string(root.join("ontology/vocabulary.yaml")).unwrap();
    let text = text.replacen("version: 3", &format!("version: {version}"), 1);
    let vocab = Vocabulary::from_yaml_str(&text).unwrap();
    let roster = Roster::from_yaml_str(&std::fs::read_to_string(root.join("ontology/attribution.yaml")).unwrap());
    let mut out = serde_json::Map::new();
    for (dir, kind) in [("knowledge", VaultKind::Knowledge), ("working", VaultKind::Working)] {
        let vault = Vault::load(root.join(dir), kind).unwrap();
        for page in vault.pages.iter().chain(vault.journals.iter()) {
            let okf = page.okf();
            let principal = okf.generated.as_ref().and_then(|g| {
                let did = g.by.principal_did();
                did.as_deref()
                    .and_then(|d| roster.resolve_principal(d))
                    .map(|r| r.canonical)
                    .or(did)
            });
            let verified_principals: Vec<_> = okf
                .verified
                .iter()
                .map(|s| {
                    let did = s.by.principal_did();
                    did.as_deref()
                        .and_then(|d| roster.resolve_principal(d))
                        .map(|r| r.canonical)
                        .or(did)
                })
                .collect();
            out.insert(
                format!("{dir}/{}", page.id),
                serde_json::json!({
                    "public": is_public_under(page, kind, &vocab),
                    "principal": principal,
                    "generated_readable": okf.generated.is_some(),
                    "verified_readable": okf.verified.len(),
                    "verified_principals": verified_principals,
                    "human_certified": okf.is_human_verified_under(&vocab, Some(&roster)),
                    "recallable": is_recallable(page, &vocab, now),
                }),
            );
        }
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::Value::Object(out)).unwrap());
}
```
