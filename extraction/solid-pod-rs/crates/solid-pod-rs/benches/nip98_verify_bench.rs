//! NIP-98 structural-verification benchmarks.
//!
//! Measures the per-request amortised cost the pod pays when a client
//! sends an `Authorization: Nostr <base64-event>` header.
//!
//! Two scenarios:
//!
//! 1. **Valid** — a well-formed token with matching URL, method,
//!    timestamp, and (when a body is present) a matching payload hash.
//! 2. **Tampered body** — same token, but the body argument differs
//!    from the one the `payload` tag was computed over, so the
//!    verifier returns an error. Exercises the SHA-256 computation +
//!    hex compare.
//!
//! Run with:
//! ```bash
//! cargo bench -p solid-pod-rs --bench nip98_verify_bench
//! ```

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sha2::{Digest, Sha256};
use solid_pod_rs::auth::nip98::{authorization_header, verify_at};

fn build_header(url: &str, method: &str, ts: u64, body: Option<&[u8]>) -> String {
    let mut tags = vec![
        vec!["u".to_string(), url.to_string()],
        vec!["method".to_string(), method.to_string()],
    ];
    if let Some(b) = body.filter(|b| !b.is_empty()) {
        tags.push(vec!["payload".to_string(), hex::encode(Sha256::digest(b))]);
    }
    let event = serde_json::json!({
        "id":         "0".repeat(64),
        "pubkey":     "a".repeat(64),
        "created_at": ts,
        "kind":       27235,
        "tags":       tags,
        "content":    "",
        "sig":        "0".repeat(128),
    });
    let b64 = BASE64.encode(serde_json::to_string(&event).unwrap());
    authorization_header(&b64)
}

fn bench_valid(c: &mut Criterion) {
    let url = "https://pod.example/public/thing.ttl";
    let ts = 1_700_000_000u64;
    let header = build_header(url, "GET", ts, None);

    c.bench_function("nip98_verify_valid_no_body", |b| {
        b.iter(|| {
            let r = verify_at(
                black_box(&header),
                black_box(url),
                black_box("GET"),
                black_box(None),
                black_box(ts),
            );
            debug_assert!(r.is_ok());
            black_box(r.ok());
        });
    });

    // With body — exercises the SHA-256 payload comparison.
    let body = b"@prefix ex: <https://ex.org/> . ex:a ex:p \"v\" .";
    let header_put = build_header(url, "PUT", ts, Some(body));
    c.bench_function("nip98_verify_valid_with_body", |b| {
        b.iter(|| {
            let r = verify_at(
                black_box(&header_put),
                black_box(url),
                black_box("PUT"),
                black_box(Some(body.as_slice())),
                black_box(ts),
            );
            debug_assert!(r.is_ok());
            black_box(r.ok());
        });
    });
}

fn bench_tampered(c: &mut Criterion) {
    let url = "https://pod.example/public/thing.ttl";
    let ts = 1_700_000_000u64;
    let original = b"@prefix ex: <https://ex.org/> . ex:a ex:p \"v\" .";
    let tampered = b"@prefix ex: <https://ex.org/> . ex:a ex:p \"EVIL\" .";
    let header = build_header(url, "PUT", ts, Some(original));

    c.bench_function("nip98_verify_tampered_body", |b| {
        b.iter(|| {
            let r = verify_at(
                black_box(&header),
                black_box(url),
                black_box("PUT"),
                black_box(Some(tampered.as_slice())),
                black_box(ts),
            );
            debug_assert!(r.is_err());
            black_box(r.err());
        });
    });
}

criterion_group!(benches, bench_valid, bench_tampered);
criterion_main!(benches);
