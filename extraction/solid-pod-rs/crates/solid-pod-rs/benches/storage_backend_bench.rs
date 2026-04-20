//! Storage-backend performance benchmarks.
//!
//! Compares `MemoryBackend` and `FsBackend` on three workloads:
//!
//! 1. **Sequential PUT** — 1 MB resource × 1000 iterations.
//! 2. **Random GET** — 10,000 iterations over a pre-populated set.
//! 3. **LIST** — a container with 10,000 children.
//!
//! Run with:
//! ```bash
//! cargo bench -p solid-pod-rs --bench storage_backend_bench
//! ```
//!
//! Sample targets on a modern x86_64 host (Linux, SSD, release build):
//!
//! | Workload                          | Memory    | FS        |
//! |-----------------------------------|-----------|-----------|
//! | PUT 1 MB (single op)              | ~2 µs     | ~200 µs   |
//! | GET (single op)                   | ~400 ns   | ~30 µs    |
//! | LIST 10k children (single op)     | ~3 ms     | ~8 ms     |

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use rand::prelude::*;
use solid_pod_rs::storage::{fs::FsBackend, memory::MemoryBackend, Storage};
use tempfile::TempDir;
use tokio::runtime::Runtime;

fn rt() -> Runtime {
    Runtime::new().expect("build tokio runtime")
}

fn make_memory() -> Arc<dyn Storage> {
    Arc::new(MemoryBackend::new())
}

fn make_fs() -> (Arc<dyn Storage>, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let fsb = rt().block_on(FsBackend::new(dir.path())).expect("fs backend");
    (Arc::new(fsb), dir)
}

fn bench_sequential_put(c: &mut Criterion) {
    let payload = vec![0xA5u8; 1024 * 1024];
    let mut group = c.benchmark_group("storage_put_1mb");
    group.throughput(Throughput::Bytes(payload.len() as u64));
    group.measurement_time(Duration::from_secs(8));
    group.sample_size(50);

    group.bench_function("memory", |b| {
        let rt = rt();
        let storage = make_memory();
        let mut counter = 0usize;
        b.iter(|| {
            let path = format!("/bulk/{counter}");
            counter += 1;
            rt.block_on(async {
                storage
                    .put(&path, Bytes::from(payload.clone()), "application/octet-stream")
                    .await
                    .unwrap();
            });
        });
    });

    group.bench_function("fs", |b| {
        let rt = rt();
        let (storage, _dir) = make_fs();
        let mut counter = 0usize;
        b.iter(|| {
            let path = format!("/bulk/{counter}");
            counter += 1;
            rt.block_on(async {
                storage
                    .put(&path, Bytes::from(payload.clone()), "application/octet-stream")
                    .await
                    .unwrap();
            });
        });
    });

    group.finish();
}

fn bench_random_get(c: &mut Criterion) {
    const N: usize = 10_000;
    let mut group = c.benchmark_group("storage_random_get");
    group.sample_size(30);

    // Memory
    {
        let rt = rt();
        let storage = make_memory();
        rt.block_on(async {
            for i in 0..N {
                storage
                    .put(
                        &format!("/grid/{i}"),
                        Bytes::from(format!("value-{i}")),
                        "text/plain",
                    )
                    .await
                    .unwrap();
            }
        });
        group.bench_function("memory", |b| {
            let mut rng = SmallRng::seed_from_u64(0xD15EA5E);
            b.iter_batched(
                || rng.gen_range(0..N),
                |i| {
                    rt.block_on(async {
                        let _ = storage.get(&format!("/grid/{i}")).await.unwrap();
                    });
                },
                BatchSize::SmallInput,
            );
        });
    }

    // FS
    {
        let rt = rt();
        let (storage, _dir) = make_fs();
        rt.block_on(async {
            for i in 0..N {
                storage
                    .put(
                        &format!("/grid/{i}"),
                        Bytes::from(format!("value-{i}")),
                        "text/plain",
                    )
                    .await
                    .unwrap();
            }
        });
        group.bench_function("fs", |b| {
            let mut rng = SmallRng::seed_from_u64(0xD15EA5E);
            b.iter_batched(
                || rng.gen_range(0..N),
                |i| {
                    rt.block_on(async {
                        let _ = storage.get(&format!("/grid/{i}")).await.unwrap();
                    });
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn bench_list_10k(c: &mut Criterion) {
    const N: usize = 10_000;
    let mut group = c.benchmark_group("storage_list_10k");
    group.sample_size(20);

    // Memory
    {
        let rt = rt();
        let storage = make_memory();
        rt.block_on(async {
            for i in 0..N {
                storage
                    .put(
                        &format!("/big/item-{i:05}"),
                        Bytes::from_static(b"x"),
                        "text/plain",
                    )
                    .await
                    .unwrap();
            }
        });
        group.bench_function("memory", |b| {
            b.iter(|| {
                rt.block_on(async {
                    let list = storage.list("/big").await.unwrap();
                    debug_assert_eq!(list.len(), N);
                });
            });
        });
    }

    // FS
    {
        let rt = rt();
        let (storage, _dir) = make_fs();
        rt.block_on(async {
            for i in 0..N {
                storage
                    .put(
                        &format!("/big/item-{i:05}"),
                        Bytes::from_static(b"x"),
                        "text/plain",
                    )
                    .await
                    .unwrap();
            }
        });
        group.bench_function("fs", |b| {
            b.iter(|| {
                rt.block_on(async {
                    let list = storage.list("/big").await.unwrap();
                    debug_assert_eq!(list.len(), N);
                });
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_sequential_put, bench_random_get, bench_list_10k);
criterion_main!(benches);
