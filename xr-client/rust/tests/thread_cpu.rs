//! The pack timing gate measures per-thread CPU time, so a frame that loses the
//! CPU to another process (host load, preemption) does not count against the
//! pack: only cycles this thread actually ran are charged.

use std::time::{Duration, Instant};
use visionclaw_xr_gdext::thread_cpu::{thread_cpu_ns, CpuStopwatch};

fn spin(d: Duration) {
    let t = Instant::now();
    let mut x = 0u64;
    while t.elapsed() < d {
        x = std::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
    }
}

/// Spins until this thread has run for `d` of CPU (not wall) time; a wall-clock
/// spin is preempted on a loaded host and accrues less CPU than it waited.
/// Returns false if `limit` of wall time passes first.
fn spin_cpu(d: Duration, limit: Duration) -> bool {
    let t = Instant::now();
    let c0 = thread_cpu_ns();
    let mut x = 0u64;
    while thread_cpu_ns() - c0 < d.as_nanos() as u64 {
        if t.elapsed() > limit {
            return false;
        }
        x = std::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
    }
    true
}

#[test]
fn busy_work_is_charged() {
    let w = CpuStopwatch::start();
    assert!(
        spin_cpu(Duration::from_millis(30), Duration::from_secs(10)),
        "30 ms of CPU within 10 s of wall time"
    );
    let (cpu, wall) = w.stop();
    assert!(
        cpu >= Duration::from_millis(30),
        "cpu {cpu:?} for 30 ms of busy work"
    );
    // ~1 ms slack: the two clocks are read at slightly different instants
    assert!(
        wall + Duration::from_millis(1) >= cpu,
        "thread CPU {cpu:?} cannot exceed wall {wall:?}"
    );
}

#[test]
fn time_off_the_cpu_is_not_charged() {
    let w = CpuStopwatch::start();
    std::thread::sleep(Duration::from_millis(60));
    let (cpu, wall) = w.stop();
    assert!(wall >= Duration::from_millis(60));
    assert!(
        cpu < Duration::from_millis(10),
        "sleeping charged {cpu:?} of thread CPU"
    );
}

#[test]
fn clock_is_monotonic_and_per_thread() {
    let a = thread_cpu_ns();
    spin(Duration::from_millis(5));
    let b = thread_cpu_ns();
    assert!(b > a);
    // Another thread's work is not charged to this one. The window opens after
    // the spawn: creating a thread (stack mmap, guard page, faults; reclaim in a
    // loaded container) is real CPU of *this* thread and once read 12 ms here.
    let go = std::sync::Arc::new(std::sync::Barrier::new(2));
    let gate = go.clone();
    let worker = std::thread::spawn(move || {
        gate.wait();
        spin(Duration::from_millis(40))
    });
    let before = thread_cpu_ns();
    go.wait();
    worker.join().unwrap();
    let after = thread_cpu_ns();
    assert!(
        after - before < 10_000_000,
        "other thread's 40 ms leaked: {} ns",
        after - before
    );
}
