// ADR-110 — ACSP producer: agentic actors project control panels into the
// forum governance page and receive human decisions (kinds 31400-31405).
pub mod acsp;
pub mod agent_visualization_processor;
pub mod agent_visualization_protocol;
pub mod audio_router;
pub mod bots_client;
pub mod file_service;
pub mod github;
pub mod github_sync_service;
pub mod graph_serialization;
pub mod local_file_sync_service;
pub mod management_api_client;
pub mod mcp_relay_manager;
pub mod multi_mcp_agent_discovery;
pub mod natural_language_query_service;
pub mod nostr_service;
// ADR-2064: wired into `src/bin/load_ontology.rs` so the real corpus ingest
// path runs horned-owl extraction over classes' `markdown_content` after
// `OntologyParser`/`save_ontology` persist them — this module previously
// existed but was never declared here, so it was dead code.
pub mod owl_extractor_service;
pub mod owl_validator;
pub mod parsers;
pub mod perplexity_service;
pub mod ragflow_service;
pub mod role_store;
pub mod schema_service;
pub mod semantic_analyzer;
pub mod semantic_pathfinding_service;
// COM-15 / V1 / D6 / M5 (PRD-023 WP-5): the governed voice loop's consumer —
// signs a kind-31402 targeted at the selected agent's did:nostr and POSTs it to
// the agentbox `/v1/voice-intent` producer (ADR-037 D7).
pub mod voice_intent_client;
// V3 (PRD-023 WP-10): the conversational-grounding confidence gate that holds a
// low-confidence / under-specified spoken command for a clarification turn
// instead of dispatching it.
pub mod edge_classifier;
pub mod inferred_edge_materialiser;
pub mod ontology_class_index;
pub mod ontology_content_analyzer;
pub mod ontology_converter;
pub mod ontology_enrichment_service;
pub mod ontology_file_cache;
#[cfg(feature = "solid-pod-embed")]
pub mod ontology_generation;
pub mod ontology_mutation_service;
pub mod ontology_pipeline_service;
#[cfg(feature = "solid-pod-embed")]
pub mod ontology_pull;
pub mod ontology_query_service;
pub mod ontology_reasoner;
pub mod ontology_reasoning_service;
pub mod pathfinding;
pub mod semantic_type_registry;
pub mod speech_service;
pub mod speech_voice_integration;
pub mod voice_clarification;
pub mod voice_context_manager;
pub mod voice_tag_manager;
// W-E transaction spine (ADR-049 / DDD-020): idempotency store, write-ahead
// intent log, deterministic receipt builder. Pure/in-memory, store-agnostic.
pub mod ontology_conflict_gate;
pub mod proposal_spine;
// T3 (W-C/W-D, ADR-049): pure portable-reification provenance quad builders +
// bi-temporal projection. Executed inside the spine's single commit transaction.
pub mod provenance_writer;
// W-B (PRD-022 / ADR-048): decision-layer vocabulary, URN minting, quad + SPARQL
// builders, bounded traversal, and the governed DecisionService write door.
pub mod decision_service;
// ADR-050 — decision elevation (the inverse corpus path): pure page draft/parse,
// significance predicate, and the fire-and-forget sink the write door calls.
pub mod briefing_service;
pub mod decision_elevation;
pub mod github_pr_service;
pub mod nostr_bead_publisher;
pub mod nostr_bridge;
// PRD-008 §5.3 — Schnorr identity verifier for the XR presence handshake
pub mod nostr_identity_verifier;

// RES-a: sprint-wide live-traffic observer + KG-backend watchdog (ADR-130 D3)
pub mod liveness_harness;

// REC-4: four-KPI compute engine (Augmentation Ratio, Trust Variance) with
// SQLite snapshots + lineage; ADR-043 resurrected per ADR-130 Decision 5.
pub mod kpi_compute;

// RES-a / WP-11 AC3: Nostr-relay tap so Nostr-only repositories (nostr-rust-forum,
// solid-pod-rs) can fire canaries they cannot POST over HTTP (ADR-130 D3).
pub mod canary_nostr_tap;

// REC-2 / D3: broker case-queue WebSocket events (broker:new_case /
// broker:case_decided) over the multiplexed graph socket (ADR-130 D2).
pub mod broker_events;

// REC-10 (PRD-023 WP-12): Insight Ingestion Loop v1 — the five-stage loop trace
// (propose → queue → decide → merge → amplification[planned]) with per-stage
// timestamps so Mesh Velocity is computable.
pub mod insight_loop;

// REC-11 (PRD-023 WP-12): the data-moat unified provenance trace — a query layer
// joining agent-events/hook-trajectory + broker decisions (+ pod git-marks when a
// --features git pod supplies them) on the did:nostr attribution (ADR-130).
pub mod provenance_trace;

// JSON-LD validator (Data Sprint Phase D-2). Pure markdown + JSON-LD
// validation; does NOT depend on the persistence-oxigraph feature.
pub mod jsonld_validator;

// JSON-LD ingest pipeline (Migration Sprint Phase 2 M1). Parses Logseq
// markdown JSON-LD blocks → oxigraph::model::Quad sets routed to Phase 1
// repository ports.
pub mod jsonld_ingest;

// Re-export semantic type registry types for convenience
pub use semantic_type_registry::{
    DynamicForceConfigGPU, RelationshipForceConfig, SemanticTypeRegistry, SEMANTIC_TYPE_REGISTRY,
};

pub mod data_reconciliation;
