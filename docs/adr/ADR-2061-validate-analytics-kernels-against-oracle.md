---
id: ADR-2061
title: Validate the GPU analytics kernels against the CPU reference oracle
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 81929f1f3c3688d08f0b2311ec19a7dbe158686f
verified_paths: [crates/visionclaw-gpu/src/cuda_sources/gpu_clustering_kernels.cu, crates/visionclaw-gpu/tests/analytics_oracle_conformance.rs, docs/GPU-wire-abi.md]
owner: jjohare
review_trigger: Before any claim that Louvain, PageRank or DBSCAN output is trustworthy, or before a release that surfaces community/centrality/cluster values to users
repo: visionclaw
---

# ADR-2061 — Validate the GPU analytics kernels against the CPU reference oracle

## Context

`docs/GPU-wire-abi.md` "Analytics kernel trust status" records that Louvain, PageRank and
DBSCAN carry in-source fix markers but that **no reference-implementation benchmark has
confirmed their outputs**: the fixes are code-verified, not output-verified. Those values
are not internal — they reach clients on the wire at V3 offsets `cluster_id@36`,
`anomaly_score@40`, `community_id@44` and `centrality@48`, so a wrong kernel is a wrong
pixel and a wrong answer to a user-facing query.

The comparison basis already exists and is unused for this purpose.
`crates/visionclaw-analytics-oracle` is a dependency-free CPU reference crate containing
`modularity` (`:303`), `pagerank` (`:342`), `dbscan` (`:392`) and `lof` (`:443`), plus
deterministic graph fixtures — `two_clique` (`:192`), `triangle` (`:206`), `star` (`:215`),
`linear_chain` (`:225`) and `canonical_live_scale` (`:246`) — and
`two_clique_optimal_partition` (`:502`), which is a known-correct answer rather than merely
a comparison. Diagram VC-15.13 carries the divergence note.

This was proposed rather than accepted because closing it is a benchmark and a numerical
tolerance policy, not an edit — it exceeds the bounded scope Phase 2 sets for a FIX. It is
now **accepted**: the suite exists and has run. Three of the four kernels clear their bars
and one does not — see *Verification — 2026-09-05* below, which is the authority for what
`GPU-wire-abi.md` now records.

## Decision

A conformance test suite compares each GPU analytics kernel against the CPU oracle on the
oracle's own fixtures, and the trust table in `GPU-wire-abi.md` records a kernel as trusted
only once its conformance test passes.

Per kernel, the acceptance test is:

- **Louvain / community detection** — on `two_clique`, the partition must have exactly 2
  communities and match `two_clique_optimal_partition` up to label permutation; on `triangle`
  and `star`, exactly 1. On `canonical_live_scale`, GPU modularity must be within `0.02`
  absolute of the oracle's `modularity` for the GPU's own partition, and must not be lower
  than the oracle partition's modularity by more than `0.05`. Louvain is stochastic, so the
  test asserts on modularity quality and community count, never on exact labels.
- **PageRank** — on every fixture, per-node absolute difference from `pagerank(g, 0.85, 100)`
  must be `< 1e-4`, and the ranking order of the top decile must match exactly. Damping and
  iteration count are pinned to the oracle's.
- **DBSCAN** — for pinned `eps`/`min_pts`, the GPU labelling must match `dbscan` exactly up to
  cluster-label permutation, including which points are noise. DBSCAN is deterministic, so
  exact agreement is the bar.
- **LOF / anomaly** — per-point absolute difference from `lof(points, k)` must be `< 1e-3`,
  and the set of points above the 95th percentile must match exactly.

A kernel that fails is marked BROKEN in the trust table and its wire slot publishes a
documented sentinel rather than a plausible-looking wrong number.

## Consequences

Once landed, the trust table stops being an assertion of intent and becomes a test result,
and the "code-fixed but not output-validated" divergence closes for real rather than by
assertion. The suite also becomes the regression gate for any future kernel change, which is
the larger long-term value.

The costs are real: the tests need a GPU in CI or a documented skip, `canonical_live_scale`
makes them slower than a unit test, and the tolerances above are engineering judgement that
will need adjustment on first contact — the numbers are stated here so that adjustment is a
visible amendment to this ADR rather than a quiet edit to a test.

*(Landed 2026-09-05.)* `GPU-wire-abi.md` no longer carries a blanket trust-status caveat —
its table now records a per-kernel verdict — and diagram VC-15.13 reads `PARTIAL ADR-2061`.
The caveat survives only for LOF, which is the one kernel still unverified against its bar. The existing process-global counters
(`analytics_telemetry::snapshot()` / `total_cpu_fallbacks()`, surfaced unconditionally by
`/analytics/gpu-metrics`) remain the only live signal, and they measure execution path, not
correctness — a kernel can run entirely on the GPU and still be wrong.

## Verification — 2026-09-05

Working-tree base **`b0bc275f6501aae7751b85a72ce15fe1e730e7e8`** (uncommitted; the suite and
the doc edits below are the change). GPU: **NVIDIA RTX A6000, sm_86, driver 610.57.04**
(device 0 of three; the other two are RTX 6000 Ada, sm_89). Toolchain: **nvcc 12.9**, kernels
compiled by `build.rs` to **PTX ISA 8.8** under the ADR-2030/2056 downgrade policy.

### Commands

```sh
# build the suite (compiles all 9 .cu modules to PTX in OUT_DIR)
cargo test -p visionclaw-gpu --test analytics_oracle_conformance --no-run

# run it on the GPU host
cargo test -p visionclaw-gpu --test analytics_oracle_conformance \
    -- --ignored --nocapture --test-threads=1

# gates
cargo fmt --all --check
cargo clippy -p visionclaw-gpu --all-targets
```

`--test-threads=1` matters: each test creates its own CUDA context. Every test is
`#[ignore]`-gated, matching the root crate's `tests/analytics_correctness_test.rs`
convention, so a CPU-only CI job runs `cargo test` green; a host with no device or no
compiled PTX prints `SKIP:` and returns rather than failing.

**The skip path is why a result of "3 passed, 1 failed" is itself evidence the kernels
ran.** `setup()` returns `None` — printing `SKIP:` and passing vacuously — on any of PTX
load failure, `cust::quick_init()` failure, or `Module::from_ptx` failure. A driverless host
therefore yields four skips and zero numbers. Numbers in the table above can only come from
executed kernels.

### Driver resolution (recorded because it caused a false alarm)

This container ships **both** a real driver and Nix link-time stubs, and which one wins is
`LD_LIBRARY_PATH`-dependent. The run above resolved the real one:

```sh
$ ldd target/debug/deps/analytics_oracle_conformance-* | grep libcuda
        libcuda.so.1 => /usr/lib/libcuda.so.1        # -> libcuda.so.610.57.04, 112 MB, real
```

The stubs live under `/nix/store/*-cuda*/lib/stubs/libcuda.so` (≈109 KB). Putting one ahead
of `/usr/lib` turns every CUDA entry point into **error 34, `CUDA_ERROR_STUB_LIBRARY`
("CUDA driver is a stub library")** — verified directly:

```sh
$ ./apitest                     # driver cuInit OK, 3 devices, cuCtxCreate OK,
                                # runtime cudaSetDevice OK, cudaMalloc OK
$ LD_LIBRARY_PATH=/nix/store/…-cuda12.9-cuda_cudart-12.9.79/lib/stubs ./apitest
driver  cuInit(0)        -> 34 FAIL
runtime cudaSetDevice(0) -> 34 CUDA driver is a stub library
```

So a sweeping "this host has no CUDA driver" conclusion drawn from a failing GPU-test run is
an environment artefact, not a fact about the host. Check `ldd` on the test binary before
concluding a kernel could not be executed.

### Result: 3 passed, 1 failed

| Kernel | Test | Bar | Measured | Verdict |
|---|---|---|---|---|
| PageRank | `adr_2061_pagerank_matches_oracle` | per-node \|Δ\| < 1e-4; top-decile order exact | max \|Δ\| **3.4e-11**; order matches on all five fixtures | **PASS** |
| DBSCAN | `adr_2061_dbscan_matches_oracle` | exact up to label permutation, noise included | **exact** | **PASS** |
| Louvain | `adr_2061_louvain_matches_oracle` | 2 / 1 / 1 communities; Q deficit ≤ 0.05 | 2 (= optimal partition) / 1 / 1; **16** communities on `canonical_live_scale`, deficit **0.0186** | **PASS** |
| LOF | `adr_2061_lof_matches_oracle` | per-point \|Δ\| < 1e-3; >p95 set exact | >p95 set **matches**; max \|Δ\| **0.702** — **702× the bar** | **FAIL** |

Per-fixture PageRank (`damping = 0.85`, 100 iterations, D8 global-dangling path launched
every iteration): `triangle` 5.6e-17, `star(6)` 6.2e-8, `linear_chain(10)` 9.9e-9,
`two_clique` 4.3e-10, `canonical_live_scale` (n = 10,676) **3.4e-11**. Every vector sums to
1.000000.

DBSCAN on a 21-point fixture (two separated 3×3 lattices, one border point, two isolated
noise points; `eps = 1.5`, `min_pts = 4`): GPU `[0×9, 9×9, 0, -1, -1]` against oracle
`[0×9, 1×9, 0, -1, -1]` — identical under the permutation `9 ↦ 1`. The border point at
`[-1.4, 1.0]` joins its core's cluster, which is the ADR-031 D7 contract. One calling
convention is now pinned by the test: the kernel's `neighbor_counts` excludes the point
itself, so the device must be given `min_pts - 1` to mean the oracle's self-inclusive
`min_pts`.

Louvain: `two_clique` → 2 communities equal to `two_clique_optimal_partition` up to
permutation, Q **0.4524** = Q_optimal; `triangle` and `star(6)` → 1 community each;
`canonical_live_scale` → **16** communities, exactly the planted count, Q **0.8960** against
the planted partition's 0.9146.

### LOF: a recorded failure, not a loosened threshold

The bar stands at 1e-3 and the kernel misses it by 702×. The cause is located, not guessed.
`lof_lrd_from_neighbors` (`crates/visionclaw-gpu/src/cuda_sources/gpu_clustering_kernels.cu:404-417`)
computes `reach_sum = Σ_o fmaxf(nbr_dist[o], k_distance)` where `k_distance = nbr_dist[count-1]`
is the **query's own** k-distance. `nbr_dist` is sorted ascending, so that `fmaxf` returns
`k_distance` for every term; `reach_sum == count * k_distance`, and the whole expression
collapses to `lrd(p) == 1 / k_distance(p)`. Breunig's definition requires the **neighbour's**
k-distance: `reach-dist_k(p, o) = max(k_distance(o), d(p, o))`. The kernel therefore computes

    LOF_kdist(p) = k_distance(p) · mean_o( 1 / k_distance(o) )

The test asserts this closed form reproduces the kernel's output to **5.6e-7 on every
point**, so the diagnosis is measured. Consequences on the 8-point fixture (`k = 3`): the
>95th-percentile set still matches — the gross outlier at `[8, 8]` scores 7.12 (GPU) against
6.88 (oracle) — but the inlier ordering is materially wrong. The GPU calls points 3 and 4 the
most inlying at 0.69 where the oracle's minimum is point 1 at 0.86, and the GPU's highest
inlier is point 6 at 1.83 where the oracle's is point 3 at 1.24.

The fix was **not** attempted here because it is not bounded. Correct Breunig LOF needs
`k_distance(o)` for each neighbour, which the single-kernel structure cannot supply without
either a third level of nested gathering inside an already-nested loop, or a three-pass split
(k-distance array → lrd array → ratio). The latter is the right shape, but it adds kernels
and buffers to the FFI surface and so requires a matching change to the server-side driver in
`visionclaw-server`, outside this change's scope — and it would change the value published at
`anomaly_score@40` for every deployment. Recorded BROKEN in the `GPU-wire-abi.md` trust table
with these numbers; the failing test is the specification for the fix.

### Scope of the claim

The suite drives the **compiled kernels** directly: `visionclaw-gpu` owns the `.cu` sources
and the PTX loader, while the production driver (`UnifiedGPUCompute`) lives in the root
`visionclaw-server` crate, which depends on this one — asserting from there would invert the
dependency edge. The harness therefore reimplements each kernel's host-side launch sequence
against the same PTX the server loads. That covers every fix marker in the trust table, since
those all live in the kernels. It does **not** cover the thin server-side wrapper that
marshals buffers into these launches.

One harness finding is worth recording because it nearly produced a false accusation. A
first pass ran only Louvain's level-0 local move and measured 3,071 communities at Q 0.1824
on `canonical_live_scale` — an apparent gross failure. It was the harness, not the kernel:
`gpu_clustering_kernels.cu` says so at its aggregation kernels ("Running local-move again on
the contracted graph lets Louvain escape the first local optimum — the step the single-pass
kernel was missing"). Adding the contraction levels via `louvain_relabel_nodes_kernel` and
`louvain_aggregate_edges_kernel` moved the same graph to 16 communities at Q 0.8960 in three
levels. Convergence also needs **two** consecutive quiet passes rather than one, because the
kernel's symmetry break alternates move direction by `iteration` parity.

### Prior verification (retained)

The oracle's contents were verified to exist and to be suitable as a comparison basis at
`b00c28a0d766c8cf46cd00b100dab60ef2dd74a4`:
`grep -n "pub fn " crates/visionclaw-analytics-oracle/src/lib.rs` lists `modularity:303`,
`pagerank:342`, `dbscan:392`, `lof:443`, `two_clique:192`, `triangle:206`, `star:215`,
`linear_chain:225`, `canonical_live_scale:246`, `two_clique_optimal_partition:502` and
`distinct_communities:510`.

The APSP kernel is deliberately excluded from this suite: it is `#if 0`-quarantined
(`gpu_landmark_apsp.cu:25`, `#endif` at `:65`) and refused at
`src/actors/gpu/shortest_path_actor.rs:353-360` under NFR-7, so there is no live output to
validate.

Acceptance for this ADR: the suite exists, runs in CI (or documents its GPU skip), and every
kernel listed in the `GPU-wire-abi.md` trust table has a passing or explicitly-failing entry
traceable to a test name. **Met** — `implementation_status: partial` records that one of the
four entries is a failure, not that the suite is incomplete. The tolerances above survived
first contact unchanged: PageRank cleared its 1e-4 bar by seven orders of magnitude and
Louvain its 0.05 allowance by 2.7×, so no adjustment is proposed. The 1e-3 LOF bar is
deliberately left where it is; the kernel, not the number, is what needs to move.


## Numerical closeout — 2026-09-07

Implementation `1ad881cab` corrects neighbour k-distance and includes ties at the
kth distance within the existing 32-slot bounded search. Actual A6000 execution
passes all four governed oracle tests with zero skips. LOF max absolute delta is
4.759e-7 against the unchanged 1e-3 bar, and the >95th-percentile set matches.
The original 0.702 failing receipt and intermediate 0.071 failure are retained.
Only the old diagnostic assertion requiring the broken formula was removed;
the numerical acceptance tests were not weakened.

Implementation is complete for the stated fixture conformance decision; activation
is staged because the corrected kernel has not been verified in a rebuilt deployed
service. Earlier BROKEN/partial observations are historical. The bounded neighbour
buffer, radius/grid search and extra recomputation cost require separate broader
coverage/performance qualification; this result is not an unrestricted LOF claim.
See the [execution report](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/closeout/2026-09-07-execution-visionclaw.md)
and `vc-analytics-green2.log` in its evidence directory.
