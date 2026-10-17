# Finding F: the local fallback PTX for `visionclaw_unified` is ABI-stale

Reported 2026-10-17 during Codescape P0 (ADR-2143). This finding is **not fixed here**: it lies outside Codescape's scope, and the owner is visionclaw-gpu.

**Verified at:** `458b0ded5da35b968ebd7ad056eba8bc06da38a5`, against this working tree and the `visionclaw_container` mount of it.

## Evidence (quoted)

**Source struct: 212 bytes, 53 fields.**
- `crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu:22` opens `struct SimParams {`, and the struct closes at `:116`.
- Counting the scalar declarations in `:22-116` gives 53 four-byte fields.
- The source asserts its own size at `:118`:
  ```
  static_assert(sizeof(SimParams) == 212, "SimParams size mismatch with Rust");
  ```
- `:121` declares `__constant__ SimParams c_params;`.

**Source kernel: 30 parameters.**
- `force_pass_kernel` is declared at `visionclaw_unified.cu:385`, with its signature at `:385-431`.
- It takes 30 parameters, ending with `compute_mask` and `mask_len`, then `node_rank`, then `node_plane`.

**Fallback PTX: 172 bytes and 28 parameters.** In `crates/visionclaw-gpu/src/ptx/visionclaw_unified.ptx` (2,089,995 B, mtime 2026-08-21):
- `:29` reads
  ```
  .const .align 4 .b8 c_params[172];
  ```
- `:265` reads `.visible .entry force_pass_kernel(`. It has 28 `.param` entries, and the last is `.param .u32 force_pass_kernel_param_27`, which is `mask_len`.
- So this PTX predates `node_rank` and `node_plane`, and predates the 40 bytes of `SimParams` that came with them.

**Copy path: warned, not checked.**
- When nvcc fails, `crates/visionclaw-gpu/build.rs:161-166` prints `cargo:warning=… {diagnosis}`.
- `:169-175` then searches `src/ptx/<name>.ptx`, `/app/src/utils/ptx/<name>.ptx` and `/app/crates/visionclaw-gpu/src/ptx/<name>.ptx`.
- `:178-181` warns `using pre-compiled PTX from …`.
- `:182` copies the file with `std::fs::copy(fb, &ptx_output)`.
- `:183` records `PtxProvenance` as fallback.
- Nothing compares the PTX's `c_params` size or kernel arity with the source before the copy.

## Corrections to the design review

The review called this PTX "committed" and the copy "silent". Both are wrong:
- **Not committed.** `.gitignore:138-139` (`*.ptx`, `**/*.ptx`) ignores it, and `git ls-files crates/visionclaw-gpu/src/ptx` is empty. It is a local working-tree artefact that reaches the container only through the read-only bind mount (`/app/crates` → `crates/`).
- **Not silent.** The build emits `cargo:warning` and records provenance. What is missing is an ABI check.

## Impact

The defect is **latent**.
- `nvcc` is present (`/opt/cuda/bin/nvcc` in `visionclaw_container`), so the compiled path wins and the fallback is never consulted.
- If nvcc does fail on this tree, the build succeeds with a warning. The driver then launches a 28-parameter kernel with 30 arguments and uploads 212 bytes into a 172-byte constant. The result is wrong forces or a launch failure at runtime, not a build failure.
- A clean checkout has no `src/ptx` file, so there a failed nvcc panics at `build.rs:186-195`, which is correct.
- The image path `/app/src/utils/ptx/` in the container holds no `visionclaw_unified.ptx` (checked with `docker exec … ls`).

**Adjacent.** `pagerank.ptx`, `semantic_forces.ptx` and `gpu_connected_components.ptx` in the same directory are 119-byte stubs with no kernels (each begins `// Stub PTX`, then declares only `.version`/`.target`/`.address_size`). They have the same exposure if they are ever used as fallbacks.

## Recommended fix (visionclaw-gpu owner)

1. Before the copy at `build.rs:182`, parse the fallback's `.const … c_params[N]` and the `.entry` parameter counts, compare them with the source (212 bytes and 30 parameters for `force_pass_kernel`), and `panic!` on a mismatch. Apply the same check to stub PTX with no `.entry`.
2. Delete the stale local `src/ptx/visionclaw_unified.ptx`. It isn't tracked, so there is nothing to regenerate in git. If fallbacks are meant to be shipped, generate them from source in CI and commit them together with the check from step 1.

Codescape does not wait on this fix. ADR-2143 decision 7 requires its P2 cust driver to refuse fallback-provenance PTX and to assert 212 bytes and the arity. The P2 evidence must include that test failing against this exact file.
