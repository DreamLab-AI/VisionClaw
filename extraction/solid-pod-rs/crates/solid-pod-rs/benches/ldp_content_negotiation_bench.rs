//! LDP content-negotiation + graph-serialisation benchmarks.
//!
//! The bench measures the hot path a Solid pod walks when it has
//! to render an existing resource in a different RDF syntax on the
//! fly (e.g. client asks for JSON-LD, storage holds Turtle/N-Triples).
//!
//! Workloads:
//!
//! 1. **`negotiate_format`** — parse an `Accept` header with q-values
//!    and pick the best format (~100k iters).
//! 2. **N-Triples parse** — parse a ~100-triple payload.
//! 3. **Turtle (via N-Triples) → JSON-LD** — full round-trip: parse
//!    the triples, render them back as a JSON-LD container document.
//!
//! Run with:
//! ```bash
//! cargo bench -p solid-pod-rs --bench ldp_content_negotiation_bench
//! ```

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use solid_pod_rs::ldp::{
    negotiate_format, render_container_jsonld, Graph, PreferHeader,
};

fn ntriples_payload_100() -> String {
    let mut out = String::new();
    for i in 0..100 {
        out.push_str(&format!(
            "<https://ex.org/s/{i}> <https://ex.org/p/value> \"item-{i}\" .\n"
        ));
    }
    out
}

fn bench_negotiate(c: &mut Criterion) {
    // A realistic-ish Accept header — multiple candidates, q-values,
    // wildcard fallback — which exercises every path in the negotiator.
    let accept = "application/ld+json;q=0.9, text/turtle;q=0.95, \
                  application/n-triples;q=0.7, */*;q=0.1";
    c.bench_function("negotiate_format_realistic", |b| {
        b.iter(|| {
            let chosen = negotiate_format(black_box(Some(accept)));
            black_box(chosen);
        });
    });
}

fn bench_ntriples_parse(c: &mut Criterion) {
    let payload = ntriples_payload_100();
    c.bench_function("parse_ntriples_100_triples", |b| {
        b.iter(|| {
            let g = Graph::parse_ntriples(black_box(&payload)).unwrap();
            debug_assert_eq!(g.len(), 100);
            black_box(g);
        });
    });
}

fn bench_roundtrip_to_jsonld(c: &mut Criterion) {
    let payload = ntriples_payload_100();
    // Simulate a container listing of the same order of magnitude —
    // 100 members — rendered as JSON-LD. This is the shape the pod
    // returns on GET of a container.
    let members: Vec<String> = (0..100).map(|i| format!("item-{i}")).collect();

    c.bench_function("roundtrip_ntriples_to_jsonld_container", |b| {
        b.iter(|| {
            // Parse → re-render as JSON-LD container.
            let g = Graph::parse_ntriples(black_box(&payload)).unwrap();
            let jsonld = render_container_jsonld(
                "/docs/",
                &members,
                PreferHeader::default(),
            );
            black_box((g, jsonld));
        });
    });
}

criterion_group!(
    benches,
    bench_negotiate,
    bench_ntriples_parse,
    bench_roundtrip_to_jsonld
);
criterion_main!(benches);
