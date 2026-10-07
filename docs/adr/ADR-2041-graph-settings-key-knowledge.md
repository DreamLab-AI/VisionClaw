---
id: ADR-2041
title: "The knowledge-graph settings key and graph-type value are `knowledge`; `logseq` is a read-only alias for one release"
date: 2026-09-02
decision_status: superseded
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: [ADR-2115]
verified_commit: c5723490b2d86655acaf4b4c88ccbb2b96b785b2
verified_paths: [crates/visionclaw-domain/src/config/visualisation.rs, crates/visionclaw-domain/src/config/app_settings.rs, src/config/mod.rs, src/config/path_accessible_impls.rs, src/protocols/binary_settings_protocol.rs, xr-client/scripts/graph_scene.gd, client/src/features/graph/types/graphTypes.ts, client/src/features/settings/config/settings.ts, data/settings.yaml]
owner: jjohare
review_trigger: the release after ADR-2040's tolerance ends — remove the `logseq` alias and the client migration shim
repo: visionclaw
domain: VAULT-corpus-format
lineage: legacy settings consolidation (path-accessible settings, `GraphsSettings { logseq, visionclaw }`)
---

# ADR-2041 — Graph settings key and graph-type value are `knowledge`

## Context

`GraphsSettings` (`crates/visionclaw-domain/src/config/visualisation.rs:512`,
re-exported through `src/config/mod.rs`; the same-named files under `src/config/`
were orphaned dead copies) has two members,
`logseq` and `visionclaw`; the client `GraphType` is `'logseq' | 'visionclaw'`
(`client/src/features/graph/types/graphTypes.ts:3`) with 18 literal uses and
86 `graphs.logseq` path uses; `data/settings.yaml:67` persists the key; the
server already accepts `"logseq" | "knowledge"` when resolving physics
(`crates/visionclaw-domain/src/config/app_settings.rs:159-171`). After ADR-2040 the word names a tool the system no
longer uses, and the split brain (`knowledge` server-side, `logseq`
client-side) is a standing source of confusion.

## Decision

1. The Rust field is renamed `knowledge` with `#[serde(alias = "logseq")]`
   on deserialisation; serialisation emits `knowledge`. `path_accessible_impls`
   resolves both `knowledge` and `logseq` path segments to the same field.
2. `data/settings.yaml` and every generated type (`src/bin/generate_types.rs`
   output, `client/src/types/generated/settings.ts`) use `knowledge`.
3. The client `GraphType` becomes `'knowledge' | 'visionclaw'`. A one-line
   migration in the settings store maps a persisted `graphs.logseq` object to
   `graphs.knowledge` on load and drops the old key on next save.
4. Any query-string, WebSocket, or REST value `graph_type=logseq` is accepted
   as `knowledge` for one release, then rejected with 400.

## Consequences

- Persisted user settings survive the upgrade without a manual edit.
- The alias and the client shim are debt with a named removal trigger.
- The rename touches ~100 client sites; it is mechanical and covered by the
  existing registry/shell vitest suites plus a new settings-migration test.

## Verification

2026-09-02 on the `obsidian` branch. Field renamed at its authoritative home
`crates/visionclaw-domain/src/config/visualisation.rs` with
`#[serde(alias = "logseq")]`; the orphaned dead copies
`src/config/{visualisation,app_settings}.rs` (never declared in
`src/config/mod.rs`) were deleted. One `normalise_graph_type` in the domain
crate, re-exported from `src/config/mod.rs`, plus `knowledge_graph_value`,
`graphs_map_has_knowledge`, `path_targets_knowledge_graph` as the only alias
sites. `PathRegistry` ids are registration-order counters, so renaming the
nine pre-registered path strings leaves every `path_id` byte-identical;
`canonical_path()` maps an inbound `graphs.logseq` path to the same slot.
Client: 421 sites across 39 files (incl. the generated settings manifest,
regenerated), `GraphType = 'knowledge' | 'visionclaw'`,
`migrateGraphSettingsKey` in the zustand merge hook, frozen WP5
`legacy-paths.fixture.json` kept byte-identical. XR client: `?graph=knowledge`
in `graph_scene.gd`, render-store label flipped. Evidence:
`cargo check --workspace` clean; `cargo test settings` 11 green incl. the new
`adr2041_graph_settings_key` suite (legacy key loads into `knowledge`,
re-serialisation emits only `knowledge`, both path segments resolve);
`cargo test --lib services::` 276; xr-client `render_store` 52;
client `tsc --noEmit` clean and `vitest run` 68 files / 758 tests incl. the
8-test `settingsMigration.test.ts` (EXP-V06). `activation_status: staged`
until the branch merges and a persisted `settings.yaml` is loaded by the new
binary.

## Closeout extension — 2026-09-04

CP-01/02/06/08. Owner remains jjohare with settings/client/runtime maintainers. Three Rust alias tests and eight client migration tests pass. The client prefers knowledge when both keys exist and drops logseq; binary paths canonicalise the legacy segment before registration/lookup. Complete/staged is preserved for the scoped rename implementation, not a live persisted-settings migration.

**Acceptance condition:** Verify typed settings load/save, JSON patch, persistence merge, dotted path and transport values against one compatibility matrix. Include both keys, null/wrong types, legacy-only/canonical-only, repeated migration and rollback. Confirm binary registry IDs across independently built peers and registration order; unknown graph values must be rejected by their consuming route. Bind alias removal to a named release and migrated-consumer evidence. Reopen on settings schema, registry ordering, persistence hook or retirement changes. See the [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/configuration-projection.md#knowledge-settings-migration) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/graph-settings-migration.json). No real browser storage, server settings load/save, live patch or transport test ran.

## Disposition — 2026-10-02

- **Suitability:** discordant
- **Priority:** withdrawn
- **Why:** Superseded by accepted ADR-2115 ("the `logseq` settings alias is removed"), which already lists `supersedes: [ADR-2041]`. This record's one-release alias, which was its whole transitional content, has been retired by that successor.
- **Next:** None. `decision_status` set to `superseded`. The validator requires a `verified_commit`, so it is stamped `95e98ab12`. The other status fields are unchanged.

## Re-verification — 2026-10-02 at e7e6b61d8 (headset NIP-98 behind the prod nginx)

**Governed changes:** `xr-client/scripts/graph_scene.gd` changes only `_describe_write_failure`: the 401/403 text and its comment now name an Owner/Admin `XR_NOSTR_SECRET` as the remedy and mark `VISIONCLAW_DEV_MODE` as dev-only (owner decision 2026-10-02, Q1 and Q3). **Decision unaffected.** The physics writes still target `?graph=knowledge`, and no request URL or body changed. `verified_commit` moved to the landing commit. Source reading, plus the unit tests named in that commit.

## Re-verification — 2026-10-07 at b6fbe772d (XR beat clock, memory bursts, attention heat; ADR-2134)

**Governed change:** `client/src/features/settings/config/settings.ts` gains the memory explorer's embedding-cloud settings (35fd52237, merged from `feat/mce-render`). `xr-client/scripts/graph_scene.gd` gains the ADR-2134 hooks: `_ensure_beat`, the `beatClock`/`pong`/`memory_flash`/`memoryRoute` text routes, and the `beat_*`, `memory_bursts:*`, `teleport:*` and `visual_*` HUD routes. **Decision unaffected.** Neither change reads or writes a graph-type key: `knowledge` stays canonical and `logseq` stays an alias. Checked with `git diff e7e6b61d8..HEAD` on both paths (no `logseq`/`knowledge`/`graphs.` lines) and GUT on HP Godot 4.6.1 (123 pass).

## Re-verification — 2026-10-07 at 39f580e93 (merged XR parity tree)

The merge of `feat/xr-cloud` (1c03ffb2a; it carries `feat/xr-graph`) brings these into one tree with the ADR-2134 work:
- xr-cloud's memory cloud and route layers (WP6/7);
- xr-graph's palette, settings sync, hulls and node LOD (WP1/2/4).

Those branches did not move this record's `verified_commit`, so the combined state is re-verified here. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext` passes 344 library + 105 integration tests, and GUT on HP Godot 4.6.1 (`--xr-mode off`) passes 149 tests.

In `graph_scene.gd` the merged changes are the parity hook, the LOD tier and the memory-cloud layer. None reads or writes a graph-type key (no `logseq`, `graph_type` or `graphs.` lines in `git diff b6fbe772d..HEAD`). **Decision unaffected.**

## Re-verification — 2026-10-07 at 944cba88c (xr-graph halo quads and edge LOD merged)

The merge of `feat/xr-graph` (944cba88c) brings in xr-graph's halo quad layer (`NodesHaloMulti`, `node_halo_quad.gdshader`), its edge LOD (near cylinders plus far camera-facing ribbons sharing `edge_flow_common.gdshaderinc`) and the avatar quaternion slerp. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext` passes 344 library + 111 integration tests; GUT on HP Godot 4.6.1 (`--xr-mode off`) runs 161 tests, 158 passing and 3 GL-only tests pending headless.

`graph_scene.gd` adds the ribbon tier and the halo-quad feed. Neither reads or writes a graph-type key. **Decision unaffected.**

## Re-verification — 2026-10-07 (feat/xr-graph 4978356e5)

The merge of `feat/xr-graph` 4978356e5 (c5723490b) touches `graph_scene.gd` only by moving the `_ribbons` declaration; no settings key, sync path or HUD setting changed. **Still holds.**
