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

#[test]
fn busy_work_is_charged() {
    let w = CpuStopwatch::start();
    spin(Duration::from_millis(30));
    let (cpu, wall) = w.stop();
    assert!(cpu >= Duration::from_millis(20), "cpu {cpu:?} for 30 ms of spinning");
    assert!(wall >= Duration::from_millis(30));
}

#[test]
fn time_off_the_cpu_is_not_charged() {
    let w = CpuStopwatch::start();
    std::thread::sleep(Duration::from_millis(60));
    let (cpu, wall) = w.stop();
    assert!(wall >= Duration::from_millis(60));
    assert!(cpu < Duration::from_millis(10), "sleeping charged {cpu:?} of thread CPU");
}

#[test]
fn clock_is_monotonic_and_per_thread() {
    let a = thread_cpu_ns();
    spin(Duration::from_millis(5));
    let b = thread_cpu_ns();
    assert!(b > a);
    // Another thread's work is not charged to this one.
    let before = thread_cpu_ns();
    std::thread::spawn(|| spin(Duration::from_millis(40))).join().unwrap();
    let after = thread_cpu_ns();
    assert!(after - before < 10_000_000, "other thread's 40 ms leaked: {} ns", after - before);
}
