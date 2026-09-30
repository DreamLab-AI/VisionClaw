---
title: "VisionClaw Performance Benchmarks"
description: "Measured performance figures for VisionClaw, each with its receipt, plus the harnesses that produce them and operating targets."
category: reference
tags:
  - api
  - backend
  - frontend
updated-date: 2026-09-30
difficulty-level: advanced
---

# VisionClaw Performance Benchmarks

**Evidence rule:** a figure appears on this page only if a stored receipt or a reproducible run backs it. Each row names its source.

> **2026-09-30 audit.** The previous version of this page (dated November 2025) published tables for GPU-vs-CPU physics (the "55×" figure), binary-vs-JSON latency, SNOMED CT reasoning, Oxigraph queries, REST throughput, 250 concurrent users, a 48-hour stress test and a competitor comparison. None of them had a receipt, a harness run or a measured host behind them, and one of them (250 concurrent users) had already been copied into public marketing copy. They were removed rather than relabelled. Where a number is needed, run the harness below and add the row with its receipt.

---

## Measured figures

### Live graph scale

| Figure | Value | Source |
|--------|-------|--------|
| Nodes rendered live in the force-directed graph | 17,147 | Live capture recorded in the VisionFlow README ("GPU graph physics" row). Public claims round this down to **10,000+**. |

Larger node counts in older docs were projected capacity, not observed load.

### XR presence wire protocol (`visionclaw-xr-presence`)

Criterion baseline, captured 2026-05-02 on an x86_64 Linux workstation, release build. Receipt: [`crates/visionclaw-xr-presence/benches/baseline.json`](../../crates/visionclaw-xr-presence/benches/baseline.json). Budgets come from PRD-008 §6 (90 fps, so 11.1 ms per frame; the pose stack must use well under 1% of that for a 1k-node graph).

| Operation | Median | Budget |
|-----------|--------|--------|
| Encode pose frame | 200 ns | 1 µs |
| Decode pose frame | 38 ns | 1 µs |
| Validate pose | 10 ns | 5 µs |
| Delta compute | 10 ns | 2 µs |
| Presence round trip | 234 ns | 2 µs |
| Decode position frame, 1k nodes | 2.6 µs | 1 ms |

Quest 3 (ARM) is expected to run 3–5× slower than this x86_64 baseline. The frame-time, draw-call, triangle and APK-size fields in the baseline are still empty; they fill on the first green run of the self-hosted Quest runner.

---

## How to measure

| Subsystem | Harness | Command |
|-----------|---------|---------|
| XR presence wire | Criterion bench `wire` | `cargo bench -p visionclaw-xr-presence --bench wire -- --warm-up-time 1 --measurement-time 3` |
| Stress majorization | Rust test | `cargo test --release --test stress_majorization_benchmark -- --nocapture` |
| Reasoning, repository, constraints | Rust modules under `tests/benchmarks/` and `tests/performance/` | Not a standalone target yet; wire into a `[[bench]]` or `tests/*.rs` entry before quoting a figure |
| Client (graph, load, VR, network) | `client/scripts/run-benchmarks.ts` | `npm --prefix client run benchmark:ci` (writes `./ci-results`) |
| XR client frame time | Godot scene `xr-client/perf/benchmark_scene.tscn` | `godot --headless --path xr-client --script perf/run_benchmark.gd` (see `xr-client/perf/README.md`) |
| Voice / STT latency | Python scripts | `scripts/benchmark_stt_streaming.py`, `scripts/benchmark_stt_estate.py` |

When a run produces a number worth publishing, commit its output (or a JSON receipt with host, commit and command) and add a row above that links to it. [Performance profiling](../how-to/performance-profiling.md) covers the probes used to diagnose a slow path.

---

## Operating targets

These are the thresholds an operator watches. They are targets, not measurements.

| Metric | Target | Warning | Critical |
|--------|--------|---------|----------|
| WebSocket latency | < 10 ms | 20 ms | 50 ms |
| Frame rate | 60 FPS | 30 FPS | 15 FPS |
| GPU memory | < 60% | 80% | 95% |
| Server CPU | < 30% | 60% | 85% |
| API P95 latency | < 50 ms | 100 ms | 500 ms |
| Graph query time (Oxigraph) | < 20 ms | 100 ms | 500 ms |

---

## Algorithm complexity

Asymptotic cost of the graph algorithms in `src/`. These follow from the algorithms, not from measurement.

| Algorithm | Implementation | Complexity | Hardware |
|-----------|---------------|------------|----------|
| SSSP (Bellman-Ford) | GPU CUDA | O(V·E) amortised | GPU |
| SSSP (delta-stepping) | GPU CUDA | O(V+E+D·L) | GPU |
| APSP (landmark) | GPU CUDA | O(k·V log V + V²) | GPU |
| Dijkstra | CPU Rust | O((V+E) log V) | CPU |
| A* | CPU Rust | O(E log V) best case | CPU |
| Bidirectional Dijkstra | CPU Rust | O(V log V) typical | CPU |
| Semantic SSSP | CPU Rust | O((V+E) log V · embed) | CPU |
| Pairwise similarity | CPU + LSH | O(n) amortised | CPU |
| Force computation | CPU SIMD | O(V log V) | CPU AVX2 |
| Stress majorization | GPU CUDA | O(V·E) sparse | GPU |
| PageRank | GPU CUDA | O(V+E) per iteration | GPU |

V = vertices, E = edges, D = maximum delta bucket, L = maximum path length, k = landmark count, embed = embedding cost per node.

---

## References

- [Binary protocol specification](./binary-protocol.md)
- [WebSocket binary protocol](./websocket-protocol.md)
- [Physics parameters](./physics-parameters.md)
- [Performance profiling](../how-to/performance-profiling.md)
