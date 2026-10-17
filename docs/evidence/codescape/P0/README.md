# Codescape P0 evidence (ADR-2143)

Every item below was executed on 2026-10-17 at `git rev-parse HEAD` = `458b0ded5da35b968ebd7ad056eba8bc06da38a5`, branch `feat/codescape`.
- `crates/codescape/` is **untracked** at that commit, so the results describe the working tree.
- Toolchain: `cargo 1.99.0 (5f94df478 2026-08-27)`.
- `CARGO_TARGET_DIR=/home/devuser/workspace/project/target/codescape-adr`.
- Commands were run from `crates/codescape/`.

## Status against the P0 evidence list

| P0 evidence item (ADR-2143 phase plan) | Status |
|---|---|
| `cargo test -p codescape-model` green | **present**: 27 passed, 0 failed |
| No graphicle in the model's dependency tree | **present**: `cargo tree -p codescape-model \| grep -c graphicle` = `0` |
| Build-graph isolation test passes | **present**: `build_graph_contains_no_knowledge_work_crates` ok. Reproduced directly with `cargo metadata --locked`: 515 packages, `graphicle` present, forbidden `[]` |
| Isolation test catches an injected `visionclaw-ontology` | **present**: `the_check_itself_catches_an_injected_dependency` ok. Its skip path only triggers when `crates/visionclaw-ontology/Cargo.toml` is absent, and that file is present, so the check really ran |
| Whole isolation suite green | **failing (environmental)**: `no_ruvector_connection_string_reaches_the_process` panicked because the agentbox shell exports `RUVECTOR_PG_CONNINFO` to every process. See the note below |
| `cargo test -p codescape-collab` green | **missing**: at 18:10Z, `codescape-collab/src/lib.rs` declared modules whose files did not exist yet (E0583 ×8; collab is still in progress), so it could not compile |
| `cargo tree -p codescape-collab` has no graphicle | **present**: `0` |

## `cargo test -p codescape-model`

```
2026-10-17T18:07:43Z
458b0ded5da35b968ebd7ad056eba8bc06da38a5
cargo 1.99.0 (5f94df478 2026-08-27)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 9.15s
     Running unittests src/lib.rs (/home/devuser/workspace/project/target/codescape-adr/debug/deps/codescape_model-8286e4fcc9125f7e)

running 27 tests
test community::tests::a_split_keeps_the_bigger_half_and_mints_a_label_for_the_rest ... ok
test community::tests::permuted_labels_are_restored ... ok
test community::tests::unassigned_zero_passes_through ... ok
test delta::tests::diff_then_apply_reproduces_after ... ok
test events::tests::events_are_tagged_snake_case_json ... ok
test graph::tests::kinds_and_relations_round_trip_strings ... ok
test graph::tests::validate_catches_broken_graphs ... ok
test ids::tests::exhaustion_is_an_error_not_a_wrap ... ok
test ids::tests::ids_are_monotonic_and_stable ... ok
test ids::tests::next_epoch_forgets_everything ... ok
test ids::tests::rejects_garbage ... ok
test lod::tests::top_dir_handles_edges ... ok
test wire::fixtures::tests::action_event_body_is_header_plus_payload ... ok
test wire::fixtures::tests::broadcast_ack_frame_is_21_bytes_with_no_generic_header ... ok
test lod::tests::aggregate_positions_take_centroids ... ok
test wire::fixtures::tests::broadcast_ack_golden_bytes ... ok
test wire::fixtures::tests::misaligned_fixture_is_not_a_record_multiple ... ok
test lod::tests::tiers_are_built_and_clusters_propagate ... ok
test wire::fixtures::tests::node_record_is_exactly_the_frozen_52_bytes ... ok
test wire::fixtures::tests::overstated_count_keeps_the_real_bodies ... ok
test wire::fixtures::tests::truncated_v5_sequence_is_not_readable ... ok
test wire::fixtures::tests::v3_and_v5_bodies_are_identical_after_their_headers ... ok
test wire::tests::ack_round_trips_and_matches_fixture_shape ... ok
test wire::tests::actors_carry_the_agent_flag ... ok
test wire::tests::agent_action_frame_shape ... ok
test wire::tests::record_matches_frozen_fixture_bytes ... ok
test ids::tests::round_trips_through_bytes ... ok

test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
2026-10-17T18:07:53Z
```

The `wire::fixtures::tests::*` cases are `visionclaw-protocol/src/wire_fixtures.rs`'s own tests. They run here because `wire.rs:18-19` includes that file by `#[path]`, so Codescape is checked against the exact bytes the other decoders are pinned to.

## `cargo test -p codescape-server --test isolation`

The full output, including the backtrace, is in [`isolation-test-output.txt`](isolation-test-output.txt). Run from 18:08:02Z to 18:10:10Z:

```
test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.19s
failures:
    no_ruvector_connection_string_reaches_the_process
thread 'no_ruvector_connection_string_reaches_the_process' panicked at codescape-server/tests/isolation.rs:123:5:
RUVECTOR_PG_CONNINFO must not be set for Codescape
```

**Note on the failure.** `isolation.rs:118-124` asserts on the environment of the *test runner*, not of the Codescape service.
- Agentbox exports `RUVECTOR_PG_CONNINFO` to every shell, so this test fails on any agentbox dev box whatever Codescape does.
- The invariant it stands in for, "the Codescape process never receives the variable", belongs to the server's boot refusal (P1, not yet in `codescape-server/src`) and to the compose service's environment (P2).
- Recommendation for the server worker: test the boot refusal directly, by spawning the binary with the variable set and expecting a non-zero exit, and drop or `#[ignore]` the runner-environment assertion.

A re-run with `env -u RUVECTOR_PG_CONNINFO` at 18:10:46Z didn't reach the tests. The server crate no longer compiled, because the in-progress `codescape-collab` failed with E0583.

## `cargo metadata --locked` (the build-graph check, reproduced outside the test harness)

```
2026-10-17T18:11:00Z
packages: 515 graphicle: true visionclaw-*:  forbidden: []
```

**Observation.** No `visionclaw-*` package appears in the resolved graph yet. `visionclaw-gpu` is an optional dependency behind `codescape-layout`'s `cuda` feature (P2). When P2 enables it, the isolation test must cover the `cuda` feature graph as well, so that the allowed `visionclaw-gpu` → `visionclaw-domain` edge is checked and not just assumed.

## Injected-dependency check, reproduced without compiling

This is a throwaway manifest that depends on `codescape-model` and `crates/visionclaw-ontology` by path (the same shape `isolation.rs:82-116` builds). It is run through `cargo metadata --offline` and the same ban list:

```
2026-10-17T18:18:41Z
packages: 347 forbidden: ["horned-owl","oxigraph","visionclaw-ontology","whelk"]
```

So the ban list does fire on an injected knowledge-work crate. Compare that with the real workspace's `forbidden: []` above.

## Root workspace unaffected

- Root `Cargo.toml:2-18` lists its members explicitly, with no `crates/codescape`. The nested workspace has its own `[workspace]` table (`crates/codescape/Cargo.toml:12`).
- `cargo metadata --no-deps --offline` at the project root exits `0` (2026-10-17T18:18:32Z).

## Finding reported separately

[`finding-F-stale-fallback-ptx.md`](finding-F-stale-fallback-ptx.md): a latent ABI-stale fallback PTX in visionclaw-gpu. It is not fixed by Codescape.
