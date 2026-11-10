# Force-kernel golden baseline

A test harness for the force kernels in `src/cuda_sources/visionclaw_unified.cu` and
`src/cuda_sources/semantic_forces.cu`, recorded **before** the CUDA rework, so every
kernel change can be checked against today's behaviour and against each force law.

Each test runs the compiled PTX directly, using the same launch sequence the server
uses (`src/utils/unified_gpu_compute/execution.rs` and `semantic.rs`), and checks
three things:

1. **CPU reference.** Per-node forces match a plain-Rust reference of the law *as the
   kernel implements it today* (`reference.rs`, transcribed from the `.cu` source).
2. **Golden.** Forces match the committed golden for that scenario (`goldens/`).
3. **Determinism.** Repeated runs are compared (the `determinism` test).

## Running it in this container

```sh
cd crates/visionclaw-gpu     # or pass -p visionclaw-gpu from the workspace root
CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=<gpu> CUDA_ARCH=<sm> \
  cargo test -p visionclaw-gpu --test force_kernel_baseline -- --ignored --test-threads=1
```

- **`CUDA_DEVICE_ORDER=PCI_BUS_ID` is required** whenever you pick a card by index.
  CUDA numbers devices fastest-first by default, `nvidia-smi` numbers them by PCI bus,
  and on a mixed-card host the two disagree. Without this variable,
  `CUDA_VISIBLE_DEVICES=2` can select a different physical card from the one
  `nvidia-smi` calls GPU 2. A GPU UUID in `CUDA_VISIBLE_DEVICES` avoids the question
  entirely.
- **`CUDA_ARCH` is required on a mixed-card host.** `build.rs` otherwise auto-detects
  with `nvidia-smi --id=0`, which can be a different card from the one the tests run
  on. Ada is `89`.
- `VISIONCLAW_EXPECT_PCI_BUS=0xNN` (optional) makes every test refuse to run unless
  the selected card sits on that PCI bus. Each test prints the device name and bus.
- `NVCC_CCBIN` and `CUDA_PATH` come from the container profile; `build.rs` reports the
  host compiler and include directory it used.
- `--test-threads=1`: each test creates its own CUDA context. Running serially keeps
  device memory small (a few MB) and the timings meaningful.
- Add `--nocapture` to see the per-fixture error ratios, the bitwise-match counts, the
  determinism table and the size of each expected-failure departure.

A host with no CUDA driver or no device prints `SKIP` and passes the as-implemented
tests. Any other failure fails the test, including a PTX load or JIT failure.
Everything is `#[ignore]`d, so a CPU-only `cargo test` stays green.

### Regenerating goldens

```sh
VISIONCLAW_REGEN_GOLDENS=1 CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=<gpu> CUDA_ARCH=<sm> \
  cargo test -p visionclaw-gpu --test force_kernel_baseline -- --ignored --test-threads=1
```

The CPU-reference checks still run while regenerating, so a regenerated golden is
always one that matched its reference. Regenerate only when a change is *meant* to
alter a force. Do it in the same commit as that change, and name the affected
scenarios in the commit message. Each section header carries the fixture's input
fingerprint, so an edited fixture fails with "regenerate deliberately" rather than
passing as a kernel regression. The file header records the PTX content tags, the
`CUDA_ARCH` and the card that wrote the goldens.

### Loop time for one kernel change (measured here, RTX 6000 Ada, 56 cores)

| Step | Time |
|---|---|
| Rebuild after editing any one `.cu` file | about 43 s (`build.rs` recompiles all eight PTX modules and relinks the native library, whichever file changed) |
| One scenario, e.g. `--exact springs_linlog` | about 3.5 s |
| The nine semantic scenarios (filter `semantic_`) | about 25 s |
| The full suite (40 tests) | about 100 s |
| First build in a fresh worktree | about 50 s |

## What each golden protects

All goldens cover four fixtures (`fixtures.rs`):

- `pair`: a 3-4-5 pair.
- `triangle`: a 3-4-5 triangle with three weights.
- `degenerate`: coincident nodes joined by an edge, a pair 1/2048 apart, a zero-weight
  edge, a self-loop, a duplicate edge, an isolated node, and an edge type outside the
  relationship table.
- `mixed256`: 256 nodes, a 100-edge hub, 400 random edges with mixed weights and types.

The CSR is built exactly as the server builds it: both directions, self-loops once,
duplicates kept.

| Golden | Kernel | Protects |
|---|---|---|
| `repulsion_classic` | `force_pass_kernel` | `repel_k / (d² + ε)` inside the cutoff, the `dist_sq > 1e-6` floor, the grid stencil (the cell size is at least the cutoff) |
| `repulsion_class_modulated` | `force_pass_kernel` | same-domain ×0.1, cross-domain ×3, charge product for class-0 nodes |
| `repulsion_fa2` | `force_pass_kernel` | FA2 `scaling_ratio (deg_i+1)(deg_j+1) / (d + ε)` in LinLog mode |
| `repulsion_capped` | `force_pass_kernel` | the per-pair `max_force` cap |
| `repulsion_separation_radius` | `force_pass_kernel` | the linear hard-separation push inside `separation_radius` |
| `springs_linlog`, `springs_linlog_scaled` | `force_pass_kernel` | `log(1 + d) · w`, with and without the per-node spring multiplier |
| `springs_hooke` | `force_pass_kernel` | `spring_k (d − rest) w`; this is also the hold overlay **off** |
| `springs_hooke_sssp` | `force_pass_kernel` | the SSSP-adjusted rest length, including unreachable (infinite) distances |
| `hold_overlay_hooke`, `hold_overlay_linlog` | `snapshot_edge_rest_kernel` + `force_pass_kernel` | hold overlay **on**: the rest lengths are snapshot at the original positions and the forces are taken at displaced ones |
| `centering`, `gravity` | `force_pass_kernel` | `−k·p` (flag-gated) and the ungated `gravity` pull |
| `dag_radial` | `force_pass_kernel` | the radial shell spring around an off-origin centre, including the deterministic nudge for a ranked node at the centre |
| `plane_bias`, `layer_bias` | `force_pass_kernel` | the Z plane spring and the Y layer spring |
| `constraints` | `force_pass_kernel` | DISTANCE, POSITION, SEPARATION and ALIGNMENT, with the progressive ramp mid-way |
| `combined_linlog`, `combined_classic` | `force_pass_kernel` | every term together, on the LinLog/FA2 path and on the classic/SSSP path |
| `stability_combined_*` | `force_pass_with_stability_kernel` | the same combinations on the stability path with the gate open, *as implemented* (see the departures below) |
| `snapshot_edge_rest` | `snapshot_edge_rest_kernel` | per-CSR-slot edge lengths |
| `degree_weighted_gravity` | `degree_weighted_gravity_kernel` | undoing the uniform centring, the degree-weighted pull, the isolated-node shell |
| `cluster_cohesion` | `cluster_cohesion_kernel` | the pull toward the assigned centroid |
| `semantic_dag` … `semantic_maturity` | each `apply_*_force` in isolation | the law of each semantic kernel; cluster kernels are given known centroids |
| `semantic_attribute_spring` | `apply_attribute_spring_force` | as implemented: the rest length is unclamped |
| `semantic_dynamic` | `apply_dynamic_relationship_force` | the relationship table, all four force types, the zero-strength skip and the out-of-range type, as implemented |
| `semantic_production` | the whole `SemanticGpu::apply` sequence | gather, zero, three centroid tables, eight kernels, scatter-add. Golden only: the centroids are float-atomic sums, which no `f64` reference can match to the tolerance |

The `semantic_config_reads_back_through_the_device_struct` test reads the uploaded
configuration back through `sf_config_probe`, so a host/device layout mismatch fails
loudly.

## Tolerances

- **CPU reference:** per node and component,
  `|gpu − ref| ≤ ε_f32 · (16 + terms) · scale`.
  - `scale` sums each term's coefficient times the absolute values of the operands of
    its subtraction.
  - 16 ulp covers the fast-math approximate square root, reciprocal, division and
    `log1pf` in each term.
  - `terms · ε` is the recursive-summation bound.
  - A node with no terms must come back exactly zero.
  - Branch predicates are guarded: a fixture that puts a value within a relative 1e-5
    of a threshold fails with "move the fixture", so the device never has a choice of
    branch.
  - The worst observed case uses 7% of the bound.
- **Golden:** `|Δ| ≤ 1e-5·|golden| + 1e-6·M`, where `M` is the largest absolute
  component in the section. This admits reassociation and intrinsic changes at ulp
  level and rejects any change to a law. The bitwise-identical count is printed.

## Determinism (10 runs on `mixed256`, baseline)

- **Bitwise identical:** every kernel without float atomics on its output. That is both
  force passes, the snapshot, cohesion, and the DAG, type, collision, physicality, role
  and maturity kernels; each of those semantic kernels adds exactly once per node.
- **Varies run to run** (float `atomicAdd` over edges and centroids):

  | Kernel | Max relative difference |
  |---|---|
  | `apply_attribute_spring_force` | 1.2e-6 |
  | `apply_dynamic_relationship_force` | 5.0e-7 |
  | the production sequence | 2.6e-6 |

  All of these sit well inside the golden bound. The test asserts bitwise identity for
  the first group and the golden bound for the second.

## Expected failures: where today's kernels depart from their stated law

Each departure is an `intended_law_*` test marked
`#[should_panic(expected = "INTENDED-LAW DEPARTURE")]`. When the rework fixes the
kernel, that test starts failing: make it a plain test and regenerate the goldens it
affects in the same change.

| Test | Departure | Concrete case |
|---|---|---|
| `intended_law_dynamic_spring_is_hookean` | `semantic_forces.cu:561-563` divides by distance and then normalises, so the force is divided by `d` twice | k = 1, rest 5, d = 10: kernel 0.5, Hooke 5 |
| `intended_law_dynamic_repulsion_type_pushes_apart` | `force_type 3` is documented as repulsion, but inside the rest length a negative strength makes it an attraction (and it carries the same 1/d) | k = −0.3, rest 150, d = 50: kernel pulls with 0.6; intended push 30 |
| `intended_law_attribute_spring_rest_length_stays_in_bounds` | `rest = max − w(max − min)` is unclamped, so for w > 1 the rest length leaves `[rest_length_min, rest_length_max]` | min 10, max 50, w = 2, d = 20: kernel 50, in-bounds law 10 |
| `intended_law_stability_kernel_applies_gravity` | `force_pass_with_stability_kernel` claims to mirror the main kernel but has no `gravity` term; the server selects it whenever `stability_threshold > 0` | `gravity = 0.02` on `mixed256`: up to 1.97 per node versus 0 |
| `intended_law_stability_kernel_applies_class_repulsion` | the same kernel takes no class buffers, so there is no domain or charge modulation | `mixed256`: up to 71.8 per node (98.5 versus 32.8 on x at node 42) |

## Not covered

- The thrust grid sort. The harness does the same stable sort on the host.
- `integrate_pass_kernel` and the stability gate's skip branches. This baseline
  records forces, not integration.
- The compute mask (it is always null here).
- Constraint telemetry buffers.
- Grids with a cell size below the cutoff (the server floors the cell at the cutoff).
- The server's buffer marshalling around these launches.
- The analytics kernels, which `tests/analytics_oracle_conformance.rs` covers.
- `force_type 1` in the dynamic table: its comment promises a tangential "orbit"
  component that is not implemented, and no magnitude is stated to test against.
  Today it behaves as a plain spring, which the golden records.
