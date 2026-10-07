//! REC-10 + REC-11 (PRD-023 WP-12) integration receipts — the loop trace and the
//! data-moat unified trace driven end to end through the REAL SQLite stores and
//! the services, not just the pure assemblers.
//!
//! REC-10: a fixture insight closes the five-stage loop across the governed
//! write-back queue (propose → queued → broker decision → merged enrichment),
//! and the loop trace reports monotonic per-stage timestamps with a computable
//! Mesh Velocity (insight-to-integration time).
//!
//! REC-11: a fixture agent-event trajectory and a broker decision that share one
//! `did:nostr` are joined by the `ProvenanceTraceService` over the two live
//! source stores; the trace reports the pod git-mark source absent (default-off)
//! and still joins the two live kinds.

use std::sync::Arc;

use visionclaw_server::adapters::sqlite_enrichment_repository::{
    EnrichmentProposal, SqliteEnrichmentRepository, StoredDecision,
};
use visionclaw_server::adapters::sqlite_kpi_repository::{NewAgentTrajectory, SqliteKpiRepository};
use visionclaw_server::services::insight_loop;
use visionclaw_server::services::provenance_trace::{
    ProvenanceTraceService, SOURCE_AGENT_EVENT, SOURCE_BROKER_DECISION, SOURCE_POD_GIT_MARK,
};

fn temp_path(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rec10-11-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!(
        "{tag}-{}.sqlite3",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

async fn enrichment_repo() -> SqliteEnrichmentRepository {
    SqliteEnrichmentRepository::open(&temp_path("enrichment"))
        .await
        .expect("open enrichment store")
}

async fn kpi_repo() -> SqliteKpiRepository {
    SqliteKpiRepository::open(&temp_path("kpi"))
        .await
        .expect("open kpi store")
}

/// REC-10: the insight loop closes once end to end with monotonic stage
/// timestamps and a computed Mesh Velocity.
#[tokio::test]
async fn rec10_insight_loop_closes_end_to_end_with_monotonic_stamps() {
    let repo = enrichment_repo().await;

    // Stage 1+2: the ontology_propose event lands in the governed write-back
    // queue, carrying its capture + queue-entry instants on the body.
    let proposal = EnrichmentProposal {
        case_id: "insight-1".into(),
        category: Some("knowledge_enrichment".into()),
        source_iri: Some("urn:ngm:node/foo".into()),
        proposal_json: serde_json::json!({
            "target_path": "pages/foo.md",
            "proposed_at_ms": 1_700_000_000_000_i64,
            "queued_at_ms":   1_700_000_001_000_i64,
        }),
        status: "pending".into(),
        created_at: 0,
        updated_at: 0,
    };
    repo.create_or_update(&proposal).await.unwrap();

    // Stage 3: a broker decides (approve, attributed).
    let decision = StoredDecision {
        case_id: "insight-1".into(),
        outcome: "approve".into(),
        attributed: true,
        broker_pubkey: Some("a".repeat(64)),
        reasoning: Some("looks good".into()),
        writeback_triggered: true,
        writeback_committed: false,
        activity_urn: "urn:visionclaw:execution:sha256-12-abcabcabcabc".into(),
        proposal_urn: Some("urn:visionclaw:kg:pk:sha256-12-deadbeef0000".into()),
        owner_did: Some("did:nostr:aaaa".into()),
        decided_at_ms: 1_700_000_002_000,
        // ADR-2006: locally minted fixture decision — no signed kind-31403 event behind it.
        decision_event_id: None,
        decision_created_at_s: None,
    };
    repo.record_decision(&decision).await.unwrap();

    // Stage 4: the fenced KG write lands — the merged-enrichment instant.
    repo.mark_writeback_committed("insight-1", &decision.activity_urn, 1_700_000_003_000)
        .await
        .unwrap();

    // Read the joined loop-trace and assemble the five-stage trace.
    let rows = repo.loop_traces(100).await.unwrap();
    let summary = insight_loop::summarise(&rows);
    assert_eq!(summary.total, 1);
    assert_eq!(summary.closed_loops, 1, "the loop closed once end to end");
    assert_eq!(summary.amplification_status, "planned");

    let trace = &summary.traces[0];
    assert_eq!(trace.case_id, "insight-1");
    assert_eq!(trace.stages.len(), 5);
    assert!(trace.loop_closed);
    assert!(trace.monotonic, "propose ≤ queued ≤ decided ≤ merged");
    // Mesh Velocity = merged(…003_000) − propose(…000_000) = 3_000 ms.
    assert_eq!(trace.mesh_velocity_ms, Some(3_000));
    assert_eq!(summary.mesh_velocity_mean_ms, Some(3_000));
    // Amplification is the fifth stage, planned.
    assert_eq!(trace.stages[4].stage, insight_loop::STAGE_AMPLIFICATION);
    assert_eq!(trace.stages[4].status, "planned");

    // Every completed stage carries a non-decreasing timestamp.
    let stamps: Vec<i64> = trace.stages.iter().filter_map(|s| s.at_ms).collect();
    assert!(
        stamps.windows(2).all(|w| w[0] <= w[1]),
        "stage stamps monotonic"
    );
}

/// REC-11: the unified trace joins two LIVE source kinds (agent-events + broker
/// decisions) under one `did:nostr`, over the real stores, with the pod source
/// reported absent (default-off).
#[tokio::test]
async fn rec11_trace_joins_two_live_source_kinds_over_real_stores() {
    let enrichment = Arc::new(enrichment_repo().await);
    let kpi = Arc::new(kpi_repo().await);
    let did = "did:nostr:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    // Live source A: an agent-event trajectory attributed to `did` (agentbox
    // emits the CTC handoff_id since P1).
    kpi.record_agent_trajectory(&NewAgentTrajectory {
        event_id: 9,
        source_agent_id: 3,
        action_type: 2,
        action_type_name: Some("create".into()),
        agent_did: Some(did.into()),
        source_urn: None,
        target_urn: Some("urn:visionclaw:kg:pk:sha256-12-deadbeef0000".into()),
        handoff_id: Some("urn:agentbox:activity:chain-7".into()),
        token_count: Some(1234),
        verification: Some("pass".into()),
        // This fixture models a P1 envelope that declares no intent (FR5.1,
        // added after REC-11): it must persist as NULL, never synthesised.
        intent: None,
        observed_at_ms: 1_700_000_002_000,
    })
    .await
    .unwrap();

    // Live source B: a broker decision attributed to the SAME `did`.
    enrichment
        .create_or_update(&EnrichmentProposal {
            case_id: "trace-1".into(),
            category: Some("knowledge_enrichment".into()),
            source_iri: None,
            proposal_json: serde_json::json!({}),
            status: "pending".into(),
            created_at: 0,
            updated_at: 0,
        })
        .await
        .unwrap();
    enrichment
        .record_decision(&StoredDecision {
            case_id: "trace-1".into(),
            outcome: "approve".into(),
            attributed: true,
            broker_pubkey: None,
            reasoning: None,
            writeback_triggered: true,
            writeback_committed: true,
            activity_urn: "urn:visionclaw:execution:sha256-12-abcabcabcabc".into(),
            proposal_urn: None,
            owner_did: Some(did.into()),
            decided_at_ms: 1_700_000_003_000,
            // ADR-2006: locally minted fixture decision — no signed kind-31403 event behind it.
            decision_event_id: None,
            decision_created_at_s: None,
        })
        .await
        .unwrap();

    // The service reads BOTH live stores and joins on did:nostr. A very large
    // window includes the fixture rows regardless of wall clock.
    let service = ProvenanceTraceService::new(enrichment.clone(), kpi.clone());
    let trace = service.query(i64::MAX, None).await.expect("trace query");

    // Both live source kinds present; the pod git-mark source is absent
    // (default-off) and reported, not silently dropped.
    assert!(trace.sources_present.contains(&SOURCE_AGENT_EVENT));
    assert!(trace.sources_present.contains(&SOURCE_BROKER_DECISION));
    assert!(trace.sources_absent.contains(&SOURCE_POD_GIT_MARK));

    // The two live sources joined under one did:nostr.
    assert!(
        trace.joins_multiple_source_kinds(),
        "trace joined ≥2 live source kinds"
    );
    assert_eq!(trace.max_join_span, 2);
    let join = trace
        .joins
        .iter()
        .find(|j| j.agent_did == did)
        .expect("a join keyed on the shared did:nostr");
    assert!(join.sources.contains(&SOURCE_AGENT_EVENT));
    assert!(join.sources.contains(&SOURCE_BROKER_DECISION));
    assert_eq!(join.record_count, 2);
    assert_eq!(trace.total_records, 2);

    // FR5.1/FR5.2 over the real stores: an undeclared intent reads back as
    // absent and yields no verdict — "no claim" is never reported as "false".
    let agent_record = trace
        .records
        .iter()
        .find(|r| r.source == SOURCE_AGENT_EVENT)
        .expect("the agent-event record");
    assert_eq!(agent_record.intent, None);
    assert_eq!(agent_record.intent_match, None);

    // The did filter narrows to the same identity and still joins.
    let filtered = service.query(i64::MAX, Some(did)).await.expect("filtered");
    assert!(filtered.joins_multiple_source_kinds());
    assert_eq!(filtered.total_records, 2);
}
