//! The per-frame LOD pack must not allocate in steady state: buffers are reused
//! frame to frame. A counting global allocator measures the node and edge LOD
//! builds (not the fixture's own motion step) after a short warm-up.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use visionclaw_xr_gdext::perf_fixture::{production_store, PROD_EDGES, PROD_NODES};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static COUNTING: AtomicBool = AtomicBool::new(false);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn steady_state_lod_pack_does_not_allocate() {
    let mut f = production_store(PROD_NODES, PROD_EDGES, 7);
    let cam = [0.0f32; 3];
    // Warm-up: buffers grow to their working size.
    for frame in 0..4 {
        f.advance(frame);
        let _ = f.store.build_node_buffer_lod(&f.ids, 1.0, 0.7, 1.9, cam, 80, f32::INFINITY).len();
        let _ = f.store.build_edge_buffer_lod(&f.pairs, 1.0, cam, 96, f32::INFINITY).len();
    }
    let mut per_frame = Vec::new();
    for frame in 4..12 {
        f.advance(frame);
        ALLOCS.store(0, Ordering::Relaxed);
        COUNTING.store(true, Ordering::Relaxed);
        let n = f.store.build_node_buffer_lod(&f.ids, 1.0, 0.7, 1.9, cam, 80, f32::INFINITY).len();
        let e = f.store.build_edge_buffer_lod(&f.pairs, 1.0, cam, 96, f32::INFINITY).len();
        COUNTING.store(false, Ordering::Relaxed);
        assert!(n > 0 && e > 0);
        per_frame.push(ALLOCS.load(Ordering::Relaxed));
    }
    assert!(per_frame.iter().all(|&a| a == 0), "allocations per steady-state frame: {per_frame:?}");
}
