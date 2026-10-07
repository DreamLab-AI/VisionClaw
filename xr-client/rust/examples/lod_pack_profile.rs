//! Per-frame CPU cost of the node + edge LOD pack at production density, in the
//! profile the headset actually loads (dev, opt-level 2):
//!     cargo run --example lod_pack_profile [-- <nodes> <edges> <frames>]
//! Prints p50/p99 per stage in milliseconds.

use std::time::Instant;
use visionclaw_xr_gdext::perf_fixture::{production_store, PROD_EDGES, PROD_NODES};

fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f64 * q) as usize]
}

fn main() {
    let a: Vec<usize> = std::env::args()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    let (nodes, edges, frames) = (
        *a.first().unwrap_or(&PROD_NODES),
        *a.get(1).unwrap_or(&PROD_EDGES),
        *a.get(2).unwrap_or(&600),
    );
    let mut f = production_store(nodes, edges, 7);
    let cam = [0.0f32, 0.0, 0.0];
    let (mut tn, mut te, mut tall) = (Vec::new(), Vec::new(), Vec::new());
    for frame in 0..frames as u32 {
        f.advance(frame);
        let t0 = Instant::now();
        let near = f
            .store
            .build_node_buffer_lod(&f.ids, 1.0, 0.7, 1.9, cam, 80, f32::INFINITY)
            .len();
        let t1 = Instant::now();
        let enear = f
            .store
            .build_edge_buffer_lod(&f.pairs, 1.0, cam, 96, f32::INFINITY)
            .len();
        let t2 = Instant::now();
        std::hint::black_box((
            near,
            enear,
            f.store.impostor_node_buffer().len(),
            f.store.ribbon_edge_buffer().len(),
        ));
        if frame >= 30 {
            tn.push((t1 - t0).as_secs_f64() * 1e3);
            te.push((t2 - t1).as_secs_f64() * 1e3);
            tall.push((t2 - t0).as_secs_f64() * 1e3);
        }
    }
    println!(
        "LODPACK nodes={nodes} edges={edges} frames={} node_ms p50={:.3} p99={:.3} edge_ms p50={:.3} p99={:.3} total_ms p50={:.3} p99={:.3}",
        tall.len(),
        pct(&mut tn.clone(), 0.5), pct(&mut tn, 0.99),
        pct(&mut te.clone(), 0.5), pct(&mut te, 0.99),
        pct(&mut tall.clone(), 0.5), pct(&mut tall, 0.99),
    );
}
