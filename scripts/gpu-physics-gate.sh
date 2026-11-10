#!/usr/bin/env bash
# GPU physics rework gate (stage S1a onwards). GitHub Actions is disabled on this repository,
# so this one command runs everything a physics pull request is judged by, on a GPU host:
#
#   1. cargo fmt --all --check
#   2. the CPU-only crates exactly as .github/workflows/ci.yml runs them (build, clippy, test,
#      and the hermetic integration-test targets)
#   3. visionclaw-server: the full lib test suite, the buffer-registry tests, and the GPU
#      buffer-lifecycle / SSSP tests under compute-sanitizer memcheck (0 errors required)
#   4. the force-kernel golden harness (crates/visionclaw-gpu/tests/force_kernel_baseline)
#   5. clippy on visionclaw-server, failing if the warning count rises above main's
#   6. the ADR index check, failing on any error except the records already stale on main
#
# Usage (from anywhere in the checkout):
#   scripts/gpu-physics-gate.sh
#
# GPU selection. CUDA numbers devices fastest-first unless CUDA_DEVICE_ORDER=PCI_BUS_ID, so an
# index alone can select the wrong card on a mixed host. The gate always sets PCI_BUS_ID order
# and refuses to run unless the selected card sits on the expected PCI bus. Override all three
# together for another host:
#   GATE_GPU=<index> GATE_CUDA_ARCH=<sm> GATE_PCI_BUS=<0xNN> scripts/gpu-physics-gate.sh
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

export CUDA_DEVICE_ORDER=PCI_BUS_ID
export CUDA_VISIBLE_DEVICES="${GATE_GPU:-2}"
export CUDA_ARCH="${GATE_CUDA_ARCH:-89}"
export VISIONCLAW_EXPECT_PCI_BUS="${GATE_PCI_BUS:-0xAF}"

# Root-crate clippy warnings on main at the S1a merge base (lib + lib test targets).
CLIPPY_BASELINE=33
# ADRs already stale on main for reasons unrelated to the physics rework.
KNOWN_STALE_ADRS='ADR-2004|ADR-2005|ADR-2037|ADR-2042|ADR-2147|ADR-2156|ADR-2168'
CPU_CRATES=(
  -p visionclaw-contracts -p visionclaw-domain -p visionclaw-protocol -p visionclaw-adapters
  -p visionclaw-ontology -p visionclaw-actors -p visionclaw-xr-presence
  -p visionclaw-analytics-oracle -p vault-core -p vault
)

start=$SECONDS
step() { printf '\n=== [%4ds] %s\n' "$((SECONDS - start))" "$*"; }
die() { printf '\nGATE FAILED: %s\n' "$*" >&2; exit 1; }

step "preflight: GPU ${CUDA_VISIBLE_DEVICES} (PCI order), sm_${CUDA_ARCH}, expected bus ${VISIONCLAW_EXPECT_PCI_BUS}"
command -v nvidia-smi >/dev/null || die "nvidia-smi not found: this gate needs a GPU host"
command -v compute-sanitizer >/dev/null || die "compute-sanitizer not found (CUDA toolkit)"
want_bus=$((VISIONCLAW_EXPECT_PCI_BUS))
have_bus=$(nvidia-smi --id="$CUDA_VISIBLE_DEVICES" --query-gpu=pci.bus --format=csv,noheader | tr -d ' ')
[[ $((have_bus)) -eq $want_bus ]] \
  || die "nvidia-smi GPU ${CUDA_VISIBLE_DEVICES} is on bus ${have_bus}, expected ${VISIONCLAW_EXPECT_PCI_BUS}"
nvidia-smi --id="$CUDA_VISIBLE_DEVICES" --query-gpu=name,pci.bus_id,memory.used,memory.total --format=csv,noheader
[[ -f agentbox/schema/federation-kinds.json ]] \
  || die "the agentbox submodule is not checked out (git submodule update --init agentbox)"

step "PTX for the gitignored include_str! paths (finding L4), from this tree's own build"
out_dir=$(cargo build -p visionclaw-gpu --message-format=json 2>/dev/null \
  | sed -n 's/.*"reason":"build-script-executed".*"package_id":"[^"]*visionclaw-gpu[^"]*".*"out_dir":"\([^"]*\)".*/\1/p' \
  | tail -1)
[[ -n "$out_dir" && -f "$out_dir/visionclaw_unified.ptx" ]] || die "could not locate visionclaw-gpu OUT_DIR"
mkdir -p src/utils/ptx
for m in sssp_compact gpu_landmark_apsp gpu_clustering_kernels; do
  cp "$out_dir/$m.ptx" "src/utils/ptx/$m.ptx"
done
echo "copied from $out_dir"

step "cargo fmt --all --check"
cargo fmt --all --check

step "CPU crates (ci.yml): build, clippy, test"
cargo build "${CPU_CRATES[@]}"
cargo clippy "${CPU_CRATES[@]}" --all-targets
cargo test "${CPU_CRATES[@]}"
cargo clippy -p visionclaw-integration-tests --all-targets
cargo test -p visionclaw-integration-tests \
  --test backup_posture --test dev_build_inputs --test prod_ingress --test layer_direction

step "visionclaw-server: lib tests (includes the CPU-only buffer-registry tests)"
cargo test --lib -- --test-threads=8

step "visionclaw-server: GPU buffer-lifecycle and SSSP tests under compute-sanitizer memcheck"
lib_test=$(cargo test --lib --no-run --message-format=json 2>/dev/null \
  | sed -n 's/.*"executable":"\([^"]*visionclaw_server-[^"]*\)".*/\1/p' | tail -1)
[[ -x "$lib_test" ]] || die "could not locate the visionclaw-server lib test binary"
compute-sanitizer --tool memcheck --error-exitcode 1 \
  "$lib_test" --ignored lifecycle_gpu_tests --test-threads=1

step "force-kernel golden harness (40 tests; the five intended_law_* are expected failures)"
cargo test -p visionclaw-gpu --test force_kernel_baseline -- --ignored --test-threads=1

step "clippy visionclaw-server (warnings must not exceed main's ${CLIPPY_BASELINE})"
clippy_log=$(mktemp)
cargo clippy -p visionclaw-server --lib --tests 2>"$clippy_log" >/dev/null \
  || { cat "$clippy_log"; die "clippy failed"; }
warnings=$(grep -E '^warning: ' "$clippy_log" \
  | grep -vE '^warning: (visionclaw-[a-z-]+@|`visionclaw-server`|the following packages)' | wc -l)
rm -f "$clippy_log"
echo "visionclaw-server clippy warnings: ${warnings} (baseline ${CLIPPY_BASELINE})"
(( warnings <= CLIPPY_BASELINE )) || die "clippy warnings rose from ${CLIPPY_BASELINE} to ${warnings}"

step "ADR index check (ignoring the records already stale on main)"
adr_out=$(node scripts/adr-index-gen.js docs/adr --check 2>&1 || true)
unexpected=$(printf '%s\n' "$adr_out" | grep -E '^error:' \
  | grep -vE "($KNOWN_STALE_ADRS)-" | grep -v 'validation error(s); README not generated' || true)
if [[ -n "$unexpected" ]]; then
  printf '%s\n' "$unexpected"
  die "ADR index check reported errors beyond the known-stale records"
fi
echo "ADR index: no errors beyond the known-stale records"

step "GATE PASSED in $((SECONDS - start)) s"
