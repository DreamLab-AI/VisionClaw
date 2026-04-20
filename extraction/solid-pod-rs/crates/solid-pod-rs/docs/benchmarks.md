# solid-pod-rs — benchmarks

Four criterion-based benches live in `crates/solid-pod-rs/benches/`.
Each is registered as `harness = false` so criterion drives the
executable directly.

## Running

```bash
# Run a single benchmark suite
cargo bench -p solid-pod-rs --bench storage_backend_bench
cargo bench -p solid-pod-rs --bench wac_eval_bench
cargo bench -p solid-pod-rs --bench ldp_content_negotiation_bench
cargo bench -p solid-pod-rs --bench nip98_verify_bench

# Run everything
cargo bench -p solid-pod-rs
```

HTML reports are written to `target/criterion/` and include flamegraph-
style before/after comparisons when you pass `--save-baseline` on one
run and `--baseline <name>` on the next.

## Suites

### `storage_backend_bench`

| Workload | Memory (target) | FS (target) | Notes |
|----------|------------------|-------------|-------|
| Sequential PUT, 1 MB body | <5 µs | <250 µs | Disk cache dominates FS — expect first-run variance. |
| Random GET from 10k entries | <1 µs | <50 µs | Memory is HashMap lookup + Bytes clone. |
| LIST of 10k children | <5 ms | <15 ms | FS walks the directory; memory scans HashMap keys. |

### `wac_eval_bench`

| Scenario | Target |
|----------|--------|
| Simple 1-rule authorisation | <5 µs |
| 10-deep container inheritance | <10 µs |
| Group membership check, 1000 members | <15 µs |

The group scenario uses a member near the end of the list to stress the
linear scan in `StaticGroupMembership`.

### `ldp_content_negotiation_bench`

| Workload | Target |
|----------|--------|
| Parse realistic `Accept` header | <1 µs |
| Parse 100-triple N-Triples payload | <200 µs |
| Parse + re-render as JSON-LD container | <300 µs |

### `nip98_verify_bench`

| Scenario | Target |
|----------|--------|
| Valid token, no body | <10 µs |
| Valid token, body with SHA-256 check | <30 µs (dominated by SHA-256) |
| Tampered body, fail path | <30 µs (fails at hash compare) |

## Hardware context

Targets above are "what modern x86_64 laptops running Linux with an
NVMe SSD are expected to hit" — they are guidance, not guarantees.
Real baselines should be captured in your CI on the actual runner
hardware using `criterion`'s built-in baseline tooling:

```bash
# Capture
cargo bench -p solid-pod-rs -- --save-baseline main

# Compare a branch against it
cargo bench -p solid-pod-rs -- --baseline main
```

Regressions of >10% on the hot WAC and NIP-98 paths should fail CI;
storage-backend numbers have higher variance and warrant a >25%
threshold.

## Caveats

- `tokio` runtime setup + `block_on` add a fixed ~500 ns overhead to
  every storage op — ignore it for relative comparisons, subtract it
  for absolute latency claims.
- `FsBackend::list` is O(children) walking the real directory; the
  OS page cache kicks in on repeated runs.
- The `nip98_verify_valid_with_body` number is dominated by the
  SHA-256 hash of the body; larger bodies scale linearly.
