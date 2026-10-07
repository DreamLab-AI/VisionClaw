# XR Perf Harness — Quest 3 native APK

Performance engineering for the Godot 4 + godot-rust + OpenXR Quest 3 client and the
`visionclaw-xr-presence` crate. Three layers:

1. **Rust micro-benchmarks** — Criterion benches for pose codec, validators, and the
   gdext crate's 0x42 graph-position decoder.
2. **Godot on-device scene** — `benchmark_scene.tscn` + `benchmark.gd` run a 1000-node /
   10-avatar scene for 30s, capture per-frame stats, and emit a single JSON line.
3. **Regression gate** — `xr-perf-regression` diffs a fresh result against
   `crates/visionclaw-xr-presence/benches/baseline.json` and exits non-zero on regression.

Authoritative perf budgets live in [PRD-008 §6](../../docs/archive/prd/PRD-008-xr-godot-replacement.md)
and the architecture deep-dive at [`docs/explanation/xr-architecture.md`](../../docs/explanation/xr-architecture.md).

## Perf budgets (from PRD-008 §6)

| Metric | Budget | Source |
|---|---|---|
| Frame rate | 90 fps stable on Quest 3 | G1 |
| Frame time p99 | ≤ 11.1 ms (= 1000/90) | G1 |
| CPU per frame | ≤ 8 ms p95 | §6 |
| GPU per frame | ≤ 8 ms p95 | §6 |
| Motion-to-photon | ≤ 20 ms p99 | G2 |
| Draw calls | ≤ 50 / frame | §6 |
| Triangles in view | ≤ 100 K | §6 |
| APK size | ≤ 80 MB | §6 |
| Wire decode (per frame) | ≤ 1 µs on Quest 3 ARM | §6 |
| Pose validation | ≤ 5 µs per inbound frame | §6 |

## Hot paths identified

| Path | Crate / file | Baseline (x86_64 dev) | Notes |
|---|---|---|---|
| `decode_pose_frame` (0x43, single avatar) | `crates/visionclaw-xr-presence/src/wire.rs` | **38 ns** | Per inbound presence frame. 26x under 1 µs/frame budget. |
| `encode_pose_frame` (0x43, single avatar) | `crates/visionclaw-xr-presence/src/wire.rs` | **200 ns** | Per outbound presence frame. 5x under budget. |
| `validate_pose` (velocity + bounds + monotonic) | `crates/visionclaw-xr-presence/src/validate.rs` | **10 ns** | Receive-loop gate. 500x under 5 µs budget. |
| `delta_compute` (PoseDelta::between) | `crates/visionclaw-xr-presence/src/delta.rs` | **10 ns** | Per outbound frame for transmit-side compression. |
| `decode_position_frame` (0x42, 1000 nodes) | `xr-client/rust/src/binary_protocol.rs` | **2.57 µs** | Full-graph frame, 28001 bytes. 388x under 1 ms/frame budget. |
| `presence_0x43_round_trip` | gdext crate | **234 ns** | Encode + decode sanity for transport stack. |

Quest 3 ARM perf is **not yet measured** — these are Linux x86_64 dev baselines. Self-hosted
Quest 3 runner (per [PRD-QE-002 §4.7](../../archive/visionclaw-process/PRD-QE-002-xr-godot-quality-engineering.md))
will replace these on first green nightly run via `--update-baseline`.

## Running locally

### Rust benches (any platform)

```bash
cargo bench -p visionclaw-xr-presence --bench wire
cargo bench -p visionclaw-xr-gdext --bench decode_throughput
```

Reduce sampling for fast smoke runs:

```bash
cargo bench -p visionclaw-xr-presence --bench wire -- --warm-up-time 1 --measurement-time 3
```

Criterion writes per-bench JSON to `target/criterion/<bench>/new/estimates.json`. Feed that
to the regression check:

```bash
cargo run --quiet --release --manifest-path xr-client/rust/Cargo.toml \
    -p xr-perf-regression -- \
    --current target/criterion/decode_pose_frame/new/estimates.json \
    --baseline crates/visionclaw-xr-presence/benches/baseline.json \
    --bench-name decode_pose_frame
```

### Godot benchmark — host headless (no Quest)

Useful for catching scene-load regressions and validating the harness itself before deploying
to a tethered headset.

```bash
godot --headless --path xr-client --script perf/run_benchmark.gd > /tmp/perf.txt 2>&1
grep '^\[XR_PERF_RESULT\]=' /tmp/perf.txt | sed 's/^\[XR_PERF_RESULT\]=//' > /tmp/perf.json
cargo run --quiet --release --manifest-path xr-client/rust/Cargo.toml \
    -p xr-perf-regression -- \
    --current /tmp/perf.json \
    --baseline crates/visionclaw-xr-presence/benches/baseline.json
```

The scene also mounts the memory cloud and a 12-hop query route (XR WP6/WP7) from a
synthetic snapshot fed through the real parse path; `memory_layers` in the result reports
their draw calls and triangles. Overrides go after `--`:

```bash
godot --path xr-client --rendering-driver opengl3 --script perf/run_benchmark.gd -- duration=8 memory_rows=0      # graph only
godot --path xr-client --rendering-driver opengl3 --script perf/run_benchmark.gd -- duration=8 memory_rows=20000  # capped cloud
```

Headless runs use the dummy renderer, so draw calls and triangles read 0; count them with a
display. `rust/src/frame_budget.rs` allocates the frame between the graph tiers, the cloud
and the route, and the scene applies its caps. Measured on HP-Desktop (GL window, Godot 4.6.1,
2026-10-07, synthetic graph with 20 000 edges and 32 hulls); the allocator's estimate equals the
renderer's count in every row:

| Nodes | Cloud rows | Route nodes / sidecar | Draw calls | Triangles | p99 | Result |
|---|---|---|---|---|---|---|
| 1 000 | 0 | — | 6 | 69 016 | 3.70 ms | pass |
| 1 000 | 6 000 | 13 / 5 | 10 | 82 260 | 3.70 ms | pass |
| 1 000 | 20 000 | 13 / 5 | 10 | 84 260 | 4.44 ms | pass |
| 1 000 | 20 000 | 64 / 64 | 10 | 81 060 | 3.55 ms | pass |
| 13 164 | 0 | — | 6 | 95 186 | 5.56 ms | pass |
| 13 164 | 6 000 | 13 / 5 | 10 | 100 000 | 6.06 ms | pass |
| 13 164 | 20 000 | 13 / 5 | 10 | 99 984 | 5.56 ms | pass |
| 13 164 | 20 000 | 64 / 64 | 10 | 99 982 | 5.64 ms | pass |

```
XR_BENCH_NODES=13164 godot --path xr-client --rendering-method gl_compatibility --xr-mode off \
  -s res://perf/run_benchmark.gd -- duration=8 memory_rows=20000 route_hops=63 route_sidecar=64
```

### Godot benchmark — on-device Quest 3

```bash
adb push xr-client/perf/fixtures/perf_graph_1k.json \
         /sdcard/Android/data/com.visionclaw.xr/files/perf_graph_1k.json
adb shell am start -n com.visionclaw.xr/com.godot.game.GodotApp \
                   -e benchmark_mode true \
                   -e benchmark_duration_s 30
adb logcat -d -s godot | grep '^\[XR_PERF_RESULT\]=' | tail -1 \
    | sed 's/^\[XR_PERF_RESULT\]=//' > /tmp/perf.json
cargo run --quiet --release --manifest-path xr-client/rust/Cargo.toml \
    -p xr-perf-regression -- --current /tmp/perf.json \
    --baseline crates/visionclaw-xr-presence/benches/baseline.json
```

The benchmark scene exits with code `0` on pass / `1` on fail; the wrapping intent /
`run_benchmark.gd` propagate that.

## CI entry points

The CI workflow (owned by the cicd agent in `.github/workflows/`) calls these — do not
hardcode them anywhere else.

```bash
cargo bench -p visionclaw-xr-presence -- --output-format json | \
    tee target/criterion-bench.jsonl

cargo bench -p visionclaw-xr-gdext --bench decode_throughput -- --output-format json | \
    tee target/criterion-gdext-bench.jsonl

cargo run --quiet --release --manifest-path xr-client/rust/Cargo.toml \
    -p xr-perf-regression -- \
    --current target/criterion/<bench>/new/estimates.json \
    --baseline crates/visionclaw-xr-presence/benches/baseline.json

godot --headless --path xr-client --script perf/run_benchmark.gd > /tmp/perf.txt
grep '^\[XR_PERF_RESULT\]=' /tmp/perf.txt | sed 's/^\[XR_PERF_RESULT\]=//' > /tmp/perf.json
cargo run --quiet --release --manifest-path xr-client/rust/Cargo.toml \
    -p xr-perf-regression -- --current /tmp/perf.json \
    --baseline crates/visionclaw-xr-presence/benches/baseline.json
```

## Updating baselines

Baseline updates require a PR with explicit reviewer approval. CI may **not** pass
`--update-baseline` itself.

1. Run the bench locally on the same class of hardware as CI (matters most for the Quest 3
   on-device numbers).
2. `cargo run --quiet --release --manifest-path xr-client/rust/Cargo.toml \
    -p xr-perf-regression -- --current <fresh.json> \
        --baseline crates/visionclaw-xr-presence/benches/baseline.json --update-baseline`
3. Commit the updated `baseline.json` with a message that names the hardware change,
   intentional optimisation, or new fixture.
4. Reviewer checks the delta is plausible against the prior baseline before merging.

Per-metric regression budgets live inside the baseline file (`regression_budget_pct` /
`regression_budget_abs`) so they versionsbump with the baseline itself.

## Files

- [`benchmark_scene.tscn`](benchmark_scene.tscn) — Godot scene (camera + 3 MultiMeshes + light).
- [`benchmark.gd`](benchmark.gd) — per-frame sampling, JSON emit, pass/fail exit code.
- [`run_benchmark.gd`](run_benchmark.gd) — headless `SceneTree` entry point.
- [`fixtures/perf_graph_1k.json`](fixtures/perf_graph_1k.json) — deterministic 1000-node /
  1500-edge / 10-avatar fixture (seed `0xC0FFEE`).
- [`../rust/perf-regression/`](../rust/perf-regression/) — the `xr-perf-regression` bin:
  Godot- and Criterion-aware diff against baseline; markdown table output for PR
  comments. Ported from `regression_check.py` (2026-09-03); same CLI flags, same
  exit codes (0 pass / 1 regression / 2 bad input), byte-identical report output.
- [`../../crates/visionclaw-xr-presence/benches/wire.rs`](../../crates/visionclaw-xr-presence/benches/wire.rs)
  — pose codec + validator + delta benches.
- [`../../crates/visionclaw-xr-presence/benches/baseline.json`](../../crates/visionclaw-xr-presence/benches/baseline.json)
  — committed baseline + per-metric regression budgets.
- [`../rust/benches/decode_throughput.rs`](../rust/benches/decode_throughput.rs)
  — gdext crate's 0x42 decode benches incl. 100 → 5000 node scaling.
