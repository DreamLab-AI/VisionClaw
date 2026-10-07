//! Live check of the memory cloud against the RuVector sidecar.
//!
//! Ignored by default; it needs `RUVECTOR_PG_CONNINFO` (read-only use) and an
//! OpenAI-compatible embedder (`MEMORY_CLOUD_EMBED_URL`, default
//! `http://xinference:9997/v1`). Run with:
//!
//! ```text
//! cargo test --test memory_cloud_live_test -- --ignored --nocapture
//! ```

use std::time::Instant;

use visionclaw_memory_cloud::config::MemoryCloudConfig;
use visionclaw_memory_cloud::validate::validate_query;
use visionclaw_memory_cloud::vector::{decode_vectors_blob, dot};
use visionclaw_memory_cloud::wire::MemoryCloudQueryRequest;
use visionclaw_server::services::memory_cloud_service::{MemoryCloudService, CONNINFO_ENV};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs a live RuVector sidecar (RUVECTOR_PG_CONNINFO) and embedder"]
async fn live_snapshot_blob_query_and_recall_probe() {
    let Ok(conninfo) = std::env::var(CONNINFO_ENV) else {
        eprintln!("{CONNINFO_ENV} unset; nothing to check");
        return;
    };
    let config = MemoryCloudConfig::from_lookup(|k| std::env::var(k).ok());
    let excluded = config.excluded.clone();
    let svc = MemoryCloudService::new(config, Some(conninfo));

    // --- snapshot -------------------------------------------------------
    let t = Instant::now();
    let snap = svc.rebuild().await.expect("snapshot builds");
    let total_ms = t.elapsed().as_secs_f64() * 1000.0;
    let s = &snap.built.snapshot;
    println!(
        "snapshot {}: {} rows, dim {}, {} namespaces, skipped {}; counts {:.0} ms, sample {:.0} ms, projection+encode {:.0} ms, total {:.0} ms; json {} KiB, blob {} KiB",
        s.snapshot_id,
        s.count,
        s.dim,
        s.namespaces.len(),
        snap.timings.skipped,
        snap.timings.counts_ms,
        snap.timings.sample_ms,
        snap.timings.project_ms,
        total_ms,
        snap.json.len() / 1024,
        snap.blob.len() / 1024
    );
    let mut strata = s.strata.clone();
    strata.sort_by_key(|st| std::cmp::Reverse(st.sampled));
    for st in strata.iter().take(8) {
        println!(
            "  stratum {:<28} total {:>7} sampled {:>5}",
            st.namespace, st.total, st.sampled
        );
    }
    for ns in [
        "project-state",
        "patterns",
        "dream-cycle",
        "knowledge/pages",
    ] {
        let st = s.strata.iter().find(|st| st.namespace == ns);
        println!("  {ns}: {:?}", st.map(|st| (st.total, st.sampled)));
    }

    assert!(s.count > 0);
    assert_eq!(s.dim, 384);
    assert_eq!(s.positions.len(), s.count * 3);
    assert_eq!(s.metadata.len(), s.count);
    assert!(s.namespaces.iter().all(|ns| !excluded.matches(ns)));
    assert!(s.namespaces.iter().any(|ns| ns == "project-state"));
    assert!(s.namespaces.iter().any(|ns| ns == "patterns"));
    assert!(s.positions.iter().all(|p| p.is_finite()));

    // --- blob -----------------------------------------------------------
    assert_eq!(snap.blob.len(), s.count * s.dim * 4);
    let floats = decode_vectors_blob(&snap.blob).expect("blob decodes");
    for row in floats.chunks_exact(s.dim) {
        assert!((dot(row, row) - 1.0).abs() < 1e-4, "rows are unit length");
    }

    // --- query ----------------------------------------------------------
    let req = MemoryCloudQueryRequest {
        text: "Ontology Loom scaffold opt-out per request".into(),
        k: Some(10),
        namespace: None,
    };
    let q = validate_query(&req, &excluded).expect("valid query");
    let t = Instant::now();
    let resp = svc.query(q).await.expect("query succeeds");
    println!(
        "query: embed+search {:.0} ms total, sidecar {:.1} ms, {} hits",
        t.elapsed().as_secs_f64() * 1000.0,
        resp.sidecar.took_ms,
        resp.sidecar.results.len()
    );
    for hit in resp.sidecar.results.iter().take(5) {
        println!(
            "  {:.3} {:<20} sample={:?} {}",
            hit.score,
            hit.namespace,
            hit.sample_index,
            hit.snippet.chars().take(80).collect::<String>()
        );
    }
    assert_eq!(resp.query.vector.len(), s.dim);
    assert!(!resp.sidecar.results.is_empty() && resp.sidecar.results.len() <= 10);
    assert!(resp
        .sidecar
        .results
        .windows(2)
        .all(|w| w[0].score >= w[1].score - 1e-6));
    assert!(resp
        .sidecar
        .results
        .iter()
        .all(|h| !excluded.matches(&h.namespace)
            && h.score <= 1.0001
            && h.snippet.chars().count() <= 160));
    for hit in &resp.sidecar.results {
        if let Some(i) = hit.sample_index {
            assert_eq!(
                s.metadata[i].id, hit.id,
                "sampleIndex points at the same entry"
            );
        }
    }

    // --- namespace-restricted query -------------------------------------
    let req = MemoryCloudQueryRequest {
        text: "recall gate before retrieval changes".into(),
        k: Some(5),
        namespace: Some("project-state".into()),
    };
    let resp = svc
        .query(validate_query(&req, &excluded).unwrap())
        .await
        .expect("restricted query succeeds");
    println!(
        "restricted query: {} hits in {:.1} ms",
        resp.sidecar.results.len(),
        resp.sidecar.took_ms
    );
    assert!(resp
        .sidecar
        .results
        .iter()
        .all(|h| h.namespace == "project-state"));
    assert_eq!(
        resp.sidecar.results.len(),
        5,
        "exact namespace scan fills k"
    );

    // --- recall probe ---------------------------------------------------
    let t = Instant::now();
    let probe = svc.run_recall_probe(&snap).await.expect("probe runs");
    println!(
        "recall probe: recall@{} = {:.3} over {} queries; index {:.1} ms/q, exact {:.1} ms/q; hnsw plan {}, exact seq scan {}; wall {:.1} s",
        probe.summary.k,
        probe.summary.recall,
        probe.summary.probes,
        probe.summary.index_ms,
        probe.summary.exact_ms,
        probe.index_plan_uses_hnsw,
        probe.exact_plan_is_seq_scan,
        t.elapsed().as_secs_f64()
    );
    let per: Vec<String> = probe
        .per_query
        .iter()
        .map(|q| {
            format!(
                "{:.1}/{:.1}/{:.0}/{:.0}",
                q.recall, q.id_recall, q.index_ms, q.exact_ms
            )
        })
        .collect();
    println!(
        "  id-overlap recall {:.3}; per query recall/id_recall/index_ms/exact_ms: {}",
        probe.per_query.iter().map(|q| q.id_recall).sum::<f64>() / probe.per_query.len() as f64,
        per.join(" ")
    );
    assert!(
        probe.index_plan_uses_hnsw,
        "index query must use the HNSW index"
    );
    assert!(
        probe.exact_plan_is_seq_scan,
        "exact query must be a sequential scan"
    );
    assert_eq!(probe.summary.probes, 20);
    assert!((0.0..=1.0).contains(&probe.summary.recall));

    // --- health ---------------------------------------------------------
    let health = svc.health().await;
    println!(
        "health: sidecar reachable {} ext {:?}, embedder reachable {}, {} namespaces",
        health.sidecar.reachable,
        health.sidecar.extension_version,
        health.embedder.reachable,
        health.namespaces.len()
    );
    assert!(health.sidecar.reachable);
    assert!(health.sidecar.extension_version.is_some());
    assert_eq!(health.snapshot_id.as_deref(), Some(s.snapshot_id.as_str()));
    assert!(health
        .namespaces
        .iter()
        .all(|n| !excluded.matches(&n.namespace)));
}
