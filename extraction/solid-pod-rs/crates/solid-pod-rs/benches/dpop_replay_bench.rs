//! F5 (Sprint 4) — `DpopReplayCache` hot-path microbenchmarks.
//!
//! The cache sits on every DPoP-authenticated request, so its
//! `check_and_record` path must stay cheap (<1µs at 10k steady-state
//! entries, per the DDD §Invariants contract). Two scenarios:
//!
//! 1. **Fresh jti**    — no contention, single-threaded insert.
//! 2. **Concurrent fresh** — 10 tasks each recording 1 000 unique
//!    jtis; measures mutex-contention cost on the shared cache.
//!
//! Run with:
//! ```bash
//! cargo bench -p solid-pod-rs --features dpop-replay-cache \
//!     --bench dpop_replay_bench
//! ```

#![cfg(feature = "dpop-replay-cache")]

use std::sync::Arc;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use solid_pod_rs::oidc::replay::DpopReplayCache;
use tokio::runtime::Runtime;

/// Steady-state working set: pre-populate with 10 000 entries so the
/// LRU is at its typical high-water mark before we measure the hot
/// path.
const WARM_ENTRIES: usize = 10_000;

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}

fn warmed_cache(rt: &Runtime) -> DpopReplayCache {
    let cache = DpopReplayCache::with_config(Duration::from_secs(60), WARM_ENTRIES * 2);
    rt.block_on(async {
        for i in 0..WARM_ENTRIES {
            // Distinct jtis so the LRU fills without tripping replay.
            let _ = cache.check_and_record(&format!("warm-jti-{i:08}")).await;
        }
    });
    cache
}

fn bench_single_threaded_fresh(c: &mut Criterion) {
    let rt = runtime();
    let cache = warmed_cache(&rt);

    // Unique jti per iteration — never a replay. This is the true
    // hot-path of a healthy pod under load.
    let mut counter: u64 = 0;

    let mut group = c.benchmark_group("dpop_replay_fresh");
    group.throughput(Throughput::Elements(1));
    group.bench_function("check_and_record_hot", |b| {
        b.iter(|| {
            counter = counter.wrapping_add(1);
            let jti = format!("hot-jti-{counter:016}");
            let r = rt.block_on(cache.check_and_record(black_box(&jti)));
            debug_assert!(r.is_ok());
            black_box(r.ok());
        });
    });
    group.finish();
}

fn bench_concurrent_fresh(c: &mut Criterion) {
    let threads_list = [1usize, 4, 10];
    let ops_per_thread = 1_000usize;

    let mut group = c.benchmark_group("dpop_replay_concurrent");
    group.throughput(Throughput::Elements(ops_per_thread as u64));
    group.sample_size(20);

    for &threads in threads_list.iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(threads),
            &threads,
            |b, &threads| {
                // One multi-thread runtime per parameter — keeps worker
                // count deterministic.
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(threads)
                    .enable_all()
                    .build()
                    .expect("multi-thread runtime");
                let cache = Arc::new(warmed_cache(&rt));
                let mut epoch: u64 = 0;

                b.iter(|| {
                    epoch = epoch.wrapping_add(1);
                    rt.block_on(async {
                        let mut handles = Vec::with_capacity(threads);
                        for t in 0..threads {
                            let cache = Arc::clone(&cache);
                            let ep = epoch;
                            handles.push(tokio::spawn(async move {
                                for i in 0..ops_per_thread {
                                    let jti = format!("ct-{ep:08}-{t:04}-{i:06}");
                                    let _ = cache.check_and_record(&jti).await;
                                }
                            }));
                        }
                        for h in handles {
                            let _ = h.await;
                        }
                    });
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_single_threaded_fresh,
    bench_concurrent_fresh
);
criterion_main!(benches);
