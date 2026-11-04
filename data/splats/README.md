# Splat manifest (ADR-2170)

`manifest.json` is the only splat file in the repository. Assets and their
derived SPZ variants are never committed; they live in the content-addressed
cache (`data/splat-cache/`, `SPLAT_CACHE_DIR`), keyed `sha256-12-<hex>.spz`.

A variant whose `sha256` is `null` has not been produced. Producing one needs
a WebGPU adapter (splat-transform decimation), so run the pipeline on a GPU
build host, never the HP while the headset is in use:

```sh
cargo build --release -p visionclaw-splat --features cli
splat-admit fetch  data/splats/manifest.json iss data/splat-work/iss.src
splat-admit build  data/splats/manifest.json iss data/splat-work/iss.src/meta.json data/splat-work
splat-admit admit  data/splats/manifest.json data/splat-work data/splat-cache
splat-admit verify data/splats/manifest.json data/splat-cache
```

`build` writes SPZ v3 (`--spz-version 3`), the one served version, and
records each variant's splat count, size and SHA-256 here. Commit the
updated manifest; copy the cache to the server. Only CC0, CC BY and CC BY-SA
assets (or an explicit `grant`) are admitted to a public cache;
`SPLAT_CACHE_MODE=private-dev` admits anything for a developer's own cache.
