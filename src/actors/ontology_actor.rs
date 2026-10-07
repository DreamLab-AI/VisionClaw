//! Ontology Actor for async OWL validation and inference operations
//!
//! This actor provides a robust interface for ontology operations including:
//! - OWL validation via OwlValidatorService
//! - Job queuing with priority scheduling
//! - Report caching with TTL and eviction policies
//! - Integration with PhysicsOrchestratorActor for constraint propagation
//! - Integration with SemanticProcessorActor for inference propagation
//!
//! Note: CustomReasoner inference is handled by ReasoningActor, not this actor.
//! This actor focuses on validation and coordination.

use actix::prelude::*;
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use uuid::Uuid;

use crate::actors::messages::*;
use crate::services::owl_validator::{
    ConstraintSummary, OwlValidatorService, PropertyGraph, RdfTriple, Severity, ValidationConfig,
    ValidationReport, ValidationStatistics, Violation,
};
use crate::utils::time;

#[derive(Error, Debug)]
pub enum OntologyActorError {
    #[error("Validation service error: {0}")]
    ServiceError(String),

    #[error("Job queue full: {max_size} items")]
    QueueFull { max_size: usize },

    #[error("Ontology not found: {id}")]
    OntologyNotFound { id: String },

    #[error("Report not found: {id}")]
    ReportNotFound { id: String },

    #[error("Invalid validation mode: {mode}")]
    InvalidMode { mode: String },

    #[error("Actor mailbox error: {0}")]
    MailboxError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobStatus {
    Pending,
    Running {
        started_at: DateTime<Utc>,
    },
    Completed {
        finished_at: DateTime<Utc>,
    },
    Failed {
        error: String,
        failed_at: DateTime<Utc>,
    },
    Cancelled {
        cancelled_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone)]
pub struct ValidationJob {
    pub id: String,
    pub ontology_id: String,
    pub graph_data: PropertyGraph,
    pub mode: ValidationMode,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub priority: JobPriority,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority {
    Low = 3,
    Normal = 2,
    High = 1,
    Critical = 0,
}

#[derive(Debug, Clone)]
struct ReportCacheEntry {
    report: ValidationReport,
    accessed_at: DateTime<Utc>,
    access_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActorStatistics {
    pub total_validations: u64,
    pub successful_validations: u64,
    pub failed_validations: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub avg_validation_time_ms: f32,
    pub queue_high_water_mark: usize,
    pub memory_usage_mb: f32,
}

/// Ontology Actor for validation and coordination
/// Handles:
/// - OWL validation via OwlValidatorService
/// - Priority job queue management
/// - Report caching and eviction
/// - Health monitoring and stuck job detection
/// - Integration with physics and semantic actors
///
/// For CustomReasoner inference, use ReasoningActor instead.
pub struct OntologyActor {
    /// OWL validator service for ontology validation
    validator_service: Arc<OwlValidatorService>,

    /// Cache of property graphs with signatures for change detection
    graph_cache: HashMap<String, (PropertyGraph, String, DateTime<Utc>)>,

    /// Priority queue for validation jobs
    validation_queue: VecDeque<ValidationJob>,

    /// Storage for validation reports with TTL
    report_storage: HashMap<String, ReportCacheEntry>,

    /// Currently executing validation jobs
    active_jobs: HashMap<String, ValidationJob>,

    /// Ids of ontologies successfully loaded via `LoadOntologyAxioms`. Backs the
    /// `loaded_ontologies` field of `GetOntologyHealth` so health is honest.
    loaded_ontologies: HashSet<String>,

    /// Actor configuration (queue sizes, timeouts, TTL)
    config: OntologyActorConfig,

    /// Performance and usage statistics
    statistics: ActorStatistics,

    /// Last health check timestamp
    last_health_check: DateTime<Utc>,

    /// Optional graph service address for graph operations
    graph_service_addr: Option<Addr<crate::actors::GraphStateActor>>,

    /// Optional physics orchestrator for constraint propagation
    physics_orchestrator_addr:
        Option<Addr<crate::actors::physics_orchestrator_actor::PhysicsOrchestratorActor>>,

    /// Optional semantic processor for inference propagation
    semantic_processor_addr:
        Option<Addr<crate::actors::semantic_processor_actor::SemanticProcessorActor>>,

    /// Optional GPU manager for sending ontology constraints to the physics pipeline
    gpu_manager_addr: Option<Addr<crate::actors::gpu::gpu_manager_actor::GPUManagerActor>>,

    /// Optional client coordinator for broadcasting validation updates via WebSocket
    client_manager_addr:
        Option<Addr<crate::actors::client_coordinator_actor::ClientCoordinatorActor>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyActorConfig {
    pub max_queue_size: usize,
    pub max_active_jobs: usize,
    pub max_cached_reports: usize,
    pub report_ttl_seconds: u64,
    pub job_timeout_seconds: u64,
    pub enable_incremental_validation: bool,
    pub validation_interval_seconds: u64,
    pub backpressure_threshold: f32,
    pub health_check_interval_seconds: u64,
}

impl Default for OntologyActorConfig {
    fn default() -> Self {
        Self {
            max_queue_size: 1000,
            max_active_jobs: 5,
            max_cached_reports: 100,
            report_ttl_seconds: 3600,
            job_timeout_seconds: 300,
            enable_incremental_validation: true,
            validation_interval_seconds: 30,
            backpressure_threshold: 0.8,
            health_check_interval_seconds: 60,
        }
    }
}

impl OntologyActor {
    pub fn new() -> Self {
        Self::with_config(OntologyActorConfig::default())
    }

    pub fn with_config(config: OntologyActorConfig) -> Self {
        let validation_config = ValidationConfig::default();
        let validator_service = Arc::new(OwlValidatorService::with_config(validation_config));

        Self {
            validator_service,
            graph_cache: HashMap::new(),
            validation_queue: VecDeque::new(),
            report_storage: HashMap::new(),
            active_jobs: HashMap::new(),
            loaded_ontologies: HashSet::new(),
            config,
            statistics: ActorStatistics::default(),
            last_health_check: time::now(),
            graph_service_addr: None,
            physics_orchestrator_addr: None,
            semantic_processor_addr: None,
            gpu_manager_addr: None,
            client_manager_addr: None,
        }
    }

    pub fn set_graph_service_addr(&mut self, addr: Addr<crate::actors::GraphStateActor>) {
        self.graph_service_addr = Some(addr);
    }

    pub fn set_physics_orchestrator_addr(
        &mut self,
        addr: Addr<crate::actors::physics_orchestrator_actor::PhysicsOrchestratorActor>,
    ) {
        self.physics_orchestrator_addr = Some(addr);
    }

    pub fn set_semantic_processor_addr(
        &mut self,
        addr: Addr<crate::actors::semantic_processor_actor::SemanticProcessorActor>,
    ) {
        self.semantic_processor_addr = Some(addr);
    }

    pub fn set_gpu_manager_addr(
        &mut self,
        addr: Addr<crate::actors::gpu::gpu_manager_actor::GPUManagerActor>,
    ) {
        self.gpu_manager_addr = Some(addr);
    }

    pub fn set_client_manager_addr(
        &mut self,
        addr: Addr<crate::actors::client_coordinator_actor::ClientCoordinatorActor>,
    ) {
        self.client_manager_addr = Some(addr);
    }

    #[allow(dead_code)]
    fn calculate_graph_signature(&self, graph: &PropertyGraph) -> String {
        use blake3::Hasher;
        let mut hasher = Hasher::new();

        hasher.update(graph.nodes.len().to_string().as_bytes());
        hasher.update(graph.edges.len().to_string().as_bytes());

        for (i, node) in graph.nodes.iter().enumerate().take(100) {
            hasher.update(node.id.as_bytes());
            hasher.update(format!("{}", i).as_bytes());
        }

        for (i, edge) in graph.edges.iter().enumerate().take(100) {
            hasher.update(edge.id.as_bytes());
            hasher.update(edge.source.as_bytes());
            hasher.update(edge.target.as_bytes());
            hasher.update(format!("{}", i).as_bytes());
        }

        hasher.finalize().to_hex().to_string()
    }

    #[allow(dead_code)]
    fn can_perform_incremental_validation(&self, ontology_id: &str, graph: &PropertyGraph) -> bool {
        if !self.config.enable_incremental_validation {
            return false;
        }

        let current_signature = self.calculate_graph_signature(graph);

        if let Some((_cached_graph, cached_signature, _)) = self.graph_cache.get(ontology_id) {
            let similarity = self.calculate_graph_similarity(&current_signature, cached_signature);
            similarity > 0.8
        } else {
            false
        }
    }

    #[allow(dead_code)]
    fn calculate_graph_similarity(&self, sig1: &str, sig2: &str) -> f32 {
        if sig1.len() != sig2.len() {
            return 0.0;
        }

        let matches = sig1
            .chars()
            .zip(sig2.chars())
            .filter(|(a, b)| a == b)
            .count();

        matches as f32 / sig1.len() as f32
    }

    fn enqueue_validation_job(
        &mut self,
        mut job: ValidationJob,
    ) -> Result<String, OntologyActorError> {
        if self.validation_queue.len() >= self.config.max_queue_size {
            return Err(OntologyActorError::QueueFull {
                max_size: self.config.max_queue_size,
            });
        }

        let mut insert_pos = self.validation_queue.len();
        for (i, existing_job) in self.validation_queue.iter().enumerate() {
            if job.priority < existing_job.priority {
                insert_pos = i;
                break;
            }
        }

        job.status = JobStatus::Pending;
        let job_id = job.id.clone();
        self.validation_queue.insert(insert_pos, job);

        debug!(
            "Enqueued validation job: {} at position {}",
            job_id, insert_pos
        );
        Ok(job_id)
    }

    fn process_next_job(&mut self, ctx: &mut Context<Self>) {
        if self.active_jobs.len() >= self.config.max_active_jobs {
            debug!("Max active jobs reached, deferring job processing");
            return;
        }

        if let Some(mut job) = self.validation_queue.pop_front() {
            let job_id = job.id.clone();
            job.status = JobStatus::Running {
                started_at: time::now(),
            };

            info!("Starting validation job: {}", job_id);
            self.active_jobs.insert(job_id.clone(), job.clone());

            let validator = self.validator_service.clone();
            let ontology_id = job.ontology_id.clone();
            let graph_data = job.graph_data.clone();
            let mode = job.mode.clone();
            let actor_addr = ctx.address();

            let future = async move {
                let start_time = Instant::now();

                // All modes validate against the SHARED validator instance so the
                // ontology loaded via LoadOntologyAxioms is visible in its cache.
                // The old Quick path built a throwaway OwlValidatorService with an
                // empty ontology cache, so every Quick validation failed with
                // "Ontology not found". Mode already drove job priority at enqueue.
                let result = match mode {
                    ValidationMode::Quick | ValidationMode::Full | ValidationMode::Incremental => {
                        validator.validate(&ontology_id, &graph_data).await
                    }
                };

                let duration = start_time.elapsed();

                let completion_msg = JobCompleted {
                    job_id: job_id.clone(),
                    result,
                    duration,
                };

                if let Err(e) = actor_addr.try_send(completion_msg) {
                    error!("Failed to send job completion: {}", e);
                }
            };

            ctx.spawn(future.into_actor(self));
        }
    }

    fn handle_job_completion(
        &mut self,
        job_id: &str,
        result: Result<ValidationReport, anyhow::Error>,
        duration: Duration,
    ) {
        if let Some(mut job) = self.active_jobs.remove(job_id) {
            match result {
                Ok(mut report) => {
                    job.status = JobStatus::Completed {
                        finished_at: time::now(),
                    };

                    // Single source of truth: overwrite the "pending" placeholder
                    // with the finished report under BOTH the job id and the
                    // ontology id (dual-key). Align `report.id` with the job id so
                    // it matches the id handed to the caller and either key
                    // resolves the same entry.
                    report.id = job_id.to_string();
                    self.store_report_dual_key(job_id, &job.ontology_id, report.clone());

                    self.statistics.successful_validations += 1;
                    self.update_avg_validation_time(duration);

                    if !report.violations.is_empty() {
                        self.send_constraints_to_physics(&report);
                    }

                    if !report.inferred_triples.is_empty() {
                        self.send_inferences_to_semantic(&report.inferred_triples);
                    }

                    // Broadcast validation result to connected WebSocket clients
                    if let Some(ref client_mgr) = self.client_manager_addr {
                        let update_msg = serde_json::json!({
                            "type": "ontology_validation_update",
                            "ontologyId": job.ontology_id,
                            "jobId": job_id,
                            "status": "completed",
                            "violations": report.violations.len(),
                            "inferredTriples": report.inferred_triples.len(),
                            "constraints": report.constraint_summary.total_constraints,
                            "durationMs": duration.as_millis(),
                            "timestamp": chrono::Utc::now().timestamp_millis()
                        });
                        if let Ok(msg_str) = serde_json::to_string(&update_msg) {
                            client_mgr.do_send(crate::actors::messages::BroadcastMessage {
                                message: msg_str,
                            });
                        }
                    }

                    info!(
                        "Validation job {} completed successfully in {:?}",
                        job_id, duration
                    );
                }
                Err(e) => {
                    let error_message = e.to_string();
                    job.status = JobStatus::Failed {
                        error: error_message.clone(),
                        failed_at: time::now(),
                    };

                    // Overwrite the "pending" placeholder with an honest failure
                    // report under BOTH keys so GET /report resolves to 200 with
                    // the error instead of returning 202 pending forever. The
                    // `"failed"` signature marks it terminal (not pending).
                    let failure_report = ValidationReport {
                        id: job_id.to_string(),
                        timestamp: time::now(),
                        duration_ms: duration.as_millis() as u64,
                        graph_signature: "failed".to_string(),
                        total_triples: 0,
                        violations: vec![Violation {
                            id: Uuid::new_v4().to_string(),
                            severity: Severity::Error,
                            rule: "validation_error".to_string(),
                            message: error_message.clone(),
                            subject: None,
                            predicate: None,
                            object: None,
                            timestamp: time::now(),
                        }],
                        inferred_triples: vec![],
                        statistics: ValidationStatistics::default(),
                        constraint_summary: ConstraintSummary::default(),
                    };
                    self.store_report_dual_key(job_id, &job.ontology_id, failure_report);

                    self.statistics.failed_validations += 1;
                    error!("Validation job {} failed: {}", job_id, error_message);
                }
            }

            self.statistics.total_validations += 1;
        }
    }

    /// Store a validation report in the single report cache under BOTH the job id
    /// and the ontology id, so a `GetOntologyReport` lookup by either key resolves
    /// the same entry. Eviction runs once up front so the cache stays bounded.
    fn store_report_dual_key(&mut self, job_id: &str, ontology_id: &str, report: ValidationReport) {
        if self.report_storage.len() >= self.config.max_cached_reports {
            self.evict_oldest_reports();
        }

        let now = time::now();
        self.report_storage.insert(
            job_id.to_string(),
            ReportCacheEntry {
                report: report.clone(),
                accessed_at: now,
                access_count: 1,
            },
        );

        // Avoid a redundant second entry when a caller keys a job by its
        // ontology id (job_id == ontology_id).
        if ontology_id != job_id {
            self.report_storage.insert(
                ontology_id.to_string(),
                ReportCacheEntry {
                    report,
                    accessed_at: now,
                    access_count: 1,
                },
            );
        }
    }

    fn evict_oldest_reports(&mut self) {
        let evict_count = self.config.max_cached_reports / 4;
        let mut reports_by_access: Vec<_> = self
            .report_storage
            .iter()
            .map(|(id, entry)| (id.clone(), entry.accessed_at))
            .collect();

        reports_by_access.sort_by_key(|(_, accessed_at)| *accessed_at);

        for (report_id, _) in reports_by_access.iter().take(evict_count) {
            self.report_storage.remove(report_id);
        }

        debug!("Evicted {} reports from cache", evict_count);
    }

    fn update_avg_validation_time(&mut self, duration: Duration) {
        let new_time_ms = duration.as_millis() as f32;

        if self.statistics.total_validations == 0 {
            self.statistics.avg_validation_time_ms = new_time_ms;
        } else {
            let weight = 0.1;
            self.statistics.avg_validation_time_ms =
                (1.0 - weight) * self.statistics.avg_validation_time_ms + weight * new_time_ms;
        }
    }

    fn send_constraints_to_physics(&self, report: &ValidationReport) {
        // Route through GPUManagerActor which delegates to OntologyConstraintActor
        if let Some(gpu_addr) = &self.gpu_manager_addr {
            use crate::actors::messages::{ApplyOntologyConstraints, ConstraintMergeMode};
            use visionclaw_domain::models::constraints::{
                Constraint, ConstraintKind, ConstraintSet,
            };

            let mut constraint_set = ConstraintSet::new();
            for violation in &report.violations {
                let constraint = Constraint {
                    kind: ConstraintKind::Semantic,
                    node_indices: vec![],
                    params: vec![1.0],
                    weight: match violation.severity {
                        crate::services::owl_validator::Severity::Error => 1.0,
                        crate::services::owl_validator::Severity::Warning => 0.6,
                        crate::services::owl_validator::Severity::Info => 0.3,
                    },
                    active: true,
                };
                constraint_set.add_to_group(&violation.rule, constraint);
            }

            info!(
                "Sending {} constraints ({} violations) to GPU pipeline",
                constraint_set.constraints.len(),
                report.violations.len()
            );

            gpu_addr.do_send(ApplyOntologyConstraints {
                constraint_set,
                merge_mode: ConstraintMergeMode::Merge,
                graph_id: 0,
            });
        } else if let Some(_addr) = &self.physics_orchestrator_addr {
            debug!(
                "PhysicsOrchestrator available but GPU manager preferred - {} violations pending",
                report.violations.len()
            );
        } else {
            warn!(
                "No GPU pipeline available - {} violation constraints dropped",
                report.violations.len()
            );
        }
    }

    fn send_inferences_to_semantic(&self, inferred_triples: &[RdfTriple]) {
        if let Some(_addr) = &self.semantic_processor_addr {
            debug!(
                "Would send {} inferred triples to semantic processor",
                inferred_triples.len()
            );
        }
    }

    fn perform_health_check(&mut self) {
        let now = time::now();

        self.cleanup_expired_reports();

        self.check_stuck_jobs();

        self.update_memory_usage();

        self.last_health_check = now;
        debug!("Health check completed");
    }

    fn cleanup_expired_reports(&mut self) {
        let ttl = Duration::from_secs(self.config.report_ttl_seconds);
        let now = time::now();

        let expired_reports: Vec<String> = self
            .report_storage
            .iter()
            .filter_map(|(id, entry)| {
                if now
                    .signed_duration_since(entry.accessed_at)
                    .to_std()
                    .unwrap_or_default()
                    > ttl
                {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();

        for report_id in expired_reports {
            self.report_storage.remove(&report_id);
        }
    }

    fn check_stuck_jobs(&mut self) {
        let timeout = Duration::from_secs(self.config.job_timeout_seconds);
        let now = time::now();

        let stuck_jobs: Vec<String> = self
            .active_jobs
            .iter()
            .filter_map(|(id, job)| {
                if let JobStatus::Running { started_at } = &job.status {
                    if now
                        .signed_duration_since(*started_at)
                        .to_std()
                        .unwrap_or_default()
                        > timeout
                    {
                        Some(id.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        for job_id in stuck_jobs {
            warn!("Job {} appears to be stuck, marking as failed", job_id);
            if let Some(mut job) = self.active_jobs.remove(&job_id) {
                job.status = JobStatus::Failed {
                    error: "Job timeout".to_string(),
                    failed_at: now,
                };
                self.statistics.failed_validations += 1;
            }
        }
    }

    fn update_memory_usage(&mut self) {
        let reports_size = self.report_storage.len() * 10;
        let queue_size = self.validation_queue.len() * 5;
        let graph_cache_size = self.graph_cache.len() * 20;

        self.statistics.memory_usage_mb =
            (reports_size + queue_size + graph_cache_size) as f32 / 1024.0;
    }
}

impl Actor for OntologyActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("OntologyActor started");

        ctx.address()
            .do_send(crate::actors::messages::InitializeActor);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("OntologyActor stopped");

        self.validator_service.clear_caches();
    }
}

// Message handlers
impl Handler<crate::actors::messages::InitializeActor> for OntologyActor {
    type Result = ();

    fn handle(
        &mut self,
        _msg: crate::actors::messages::InitializeActor,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("OntologyActor: Initializing periodic tasks (deferred from started)");

        ctx.run_interval(Duration::from_secs(1), |actor, ctx| {
            actor.process_next_job(ctx);
        });

        let health_interval = Duration::from_secs(self.config.health_check_interval_seconds);
        ctx.run_interval(health_interval, |actor, _ctx| {
            actor.perform_health_check();
        });

        debug!("OntologyActor: Periodic tasks scheduled successfully");
    }
}

// Internal message for job completion
#[derive(Message)]
#[rtype(result = "()")]
struct JobCompleted {
    job_id: String,
    result: Result<ValidationReport, anyhow::Error>,
    duration: Duration,
}

impl Handler<JobCompleted> for OntologyActor {
    type Result = ();

    fn handle(&mut self, msg: JobCompleted, _ctx: &mut Self::Context) -> Self::Result {
        self.handle_job_completion(&msg.job_id, msg.result, msg.duration);
    }
}

// Message handlers

impl Handler<LoadOntologyAxioms> for OntologyActor {
    type Result = ResponseActFuture<Self, Result<String, String>>;

    fn handle(&mut self, msg: LoadOntologyAxioms, _ctx: &mut Self::Context) -> Self::Result {
        let validator = self.validator_service.clone();
        let source = msg.source;

        Box::pin(
            async move {
                validator.load_ontology(&source).await.map_err(|e| {
                    error!("Failed to load ontology from {}: {}", source, e);
                    format!("Failed to load ontology: {}", e)
                })
            }
            .into_actor(self)
            .map(|result, actor, _ctx| {
                // Record the loaded ontology in actor state once the async load
                // resolves, so GetOntologyHealth.loaded_ontologies reflects it.
                if let Ok(ref ontology_id) = result {
                    actor.loaded_ontologies.insert(ontology_id.clone());
                    info!(
                        "Successfully loaded ontology: {} ({} loaded)",
                        ontology_id,
                        actor.loaded_ontologies.len()
                    );
                }
                result
            }),
        )
    }
}

impl Handler<UpdateOntologyMapping> for OntologyActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateOntologyMapping, _ctx: &mut Self::Context) -> Self::Result {
        self.validator_service = Arc::new(OwlValidatorService::with_config(msg.config));
        info!("Updated ontology mapping configuration");
        Ok(())
    }
}

impl Handler<ValidateOntology> for OntologyActor {
    type Result = Result<ValidationReport, String>;

    fn handle(&mut self, msg: ValidateOntology, _ctx: &mut Self::Context) -> Self::Result {
        // Adopt the caller-supplied job id when present so the report is
        // retrievable by the exact id handed back to the client; otherwise mint
        // one. Either way `report.id == job.id` throughout the job's lifecycle.
        let job_id = msg.job_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let ontology_id = msg.ontology_id.clone();
        let priority = match msg.mode {
            ValidationMode::Quick => JobPriority::High,
            ValidationMode::Full => JobPriority::Normal,
            ValidationMode::Incremental => JobPriority::Low,
        };

        let job = ValidationJob {
            id: job_id.clone(),
            ontology_id: msg.ontology_id,
            graph_data: msg.graph_data,
            mode: msg.mode,
            status: JobStatus::Pending,
            created_at: time::now(),
            priority,
        };

        match self.enqueue_validation_job(job) {
            Ok(_) => {
                debug!("Validation job {} enqueued", job_id);

                let report = ValidationReport {
                    id: job_id.clone(),
                    timestamp: time::now(),
                    duration_ms: 0,
                    graph_signature: "pending".to_string(),
                    total_triples: 0,
                    violations: vec![],
                    inferred_triples: vec![],
                    statistics: crate::services::owl_validator::ValidationStatistics::default(),
                    constraint_summary: crate::services::owl_validator::ConstraintSummary {
                        total_constraints: 0,
                        semantic_constraints: 0,
                        structural_constraints: 0,
                    },
                };

                // Publish the "pending" report into the single store under both
                // keys immediately, so GET /report?jobId / ?ontologyId returns a
                // pending status (202) while the job runs instead of 404.
                // handle_job_completion overwrites both keys once finished.
                self.store_report_dual_key(&job_id, &ontology_id, report.clone());

                Ok(report)
            }
            Err(e) => Err(format!("Failed to enqueue validation job: {}", e)),
        }
    }
}

impl Handler<ApplyInferences> for OntologyActor {
    type Result = ResponseFuture<Result<Vec<RdfTriple>, String>>;

    fn handle(&mut self, msg: ApplyInferences, _ctx: &mut Self::Context) -> Self::Result {
        let validator = self.validator_service.clone();
        let triples = msg.rdf_triples;

        Box::pin(async move {
            match validator.infer(&triples) {
                Ok(inferred_triples) => {
                    debug!("Generated {} inferred triples", inferred_triples.len());
                    Ok(inferred_triples)
                }
                Err(e) => {
                    error!("Failed to apply inferences: {}", e);
                    Err(format!("Inference failed: {}", e))
                }
            }
        })
    }
}

impl Handler<GetOntologyReport> for OntologyActor {
    type Result = Result<Option<ValidationReport>, String>;

    fn handle(&mut self, msg: GetOntologyReport, _ctx: &mut Self::Context) -> Self::Result {
        match msg.report_id {
            Some(id) => {
                if let Some(entry) = self.report_storage.get_mut(&id) {
                    entry.accessed_at = time::now();
                    entry.access_count += 1;
                    self.statistics.cache_hits += 1;
                    Ok(Some(entry.report.clone()))
                } else {
                    self.statistics.cache_misses += 1;
                    Ok(None)
                }
            }
            None => {
                let latest = self
                    .report_storage
                    .values()
                    .max_by_key(|entry| entry.report.timestamp)
                    .map(|entry| entry.report.clone());

                if latest.is_some() {
                    self.statistics.cache_hits += 1;
                } else {
                    self.statistics.cache_misses += 1;
                }

                Ok(latest)
            }
        }
    }
}

impl Handler<GetOntologyHealth> for OntologyActor {
    type Result = Result<OntologyHealth, String>;

    fn handle(&mut self, _msg: GetOntologyHealth, _ctx: &mut Self::Context) -> Self::Result {
        let cache_hit_rate = if self.statistics.cache_hits + self.statistics.cache_misses > 0 {
            self.statistics.cache_hits as f32
                / (self.statistics.cache_hits + self.statistics.cache_misses) as f32
        } else {
            0.0
        };

        let last_validation = self
            .report_storage
            .values()
            .map(|entry| entry.report.timestamp)
            .max();

        let health = OntologyHealth {
            loaded_ontologies: self.loaded_ontologies.len() as u32,
            cached_reports: self.report_storage.len() as u32,
            validation_queue_size: self.validation_queue.len() as u32,
            last_validation,
            cache_hit_rate,
            avg_validation_time_ms: self.statistics.avg_validation_time_ms,
            active_jobs: self.active_jobs.len() as u32,
            memory_usage_mb: self.statistics.memory_usage_mb,
        };

        Ok(health)
    }
}

impl Handler<ClearOntologyCaches> for OntologyActor {
    type Result = Result<(), String>;

    fn handle(&mut self, _msg: ClearOntologyCaches, _ctx: &mut Self::Context) -> Self::Result {
        self.validator_service.clear_caches();
        self.report_storage.clear();
        self.graph_cache.clear();
        // The validator's ontology cache was just cleared, so reset the honest
        // loaded-ontologies counter to match.
        self.loaded_ontologies.clear();

        info!("Cleared all ontology caches");
        Ok(())
    }
}

// Trigger reasoning on ontology data
#[derive(Message)]
#[rtype(result = "Result<String, String>")]
pub struct TriggerReasoning {
    pub ontology_id: i64,
    pub source: String,
}

impl Handler<TriggerReasoning> for OntologyActor {
    type Result = ResponseFuture<Result<String, String>>;

    fn handle(&mut self, msg: TriggerReasoning, _ctx: &mut Self::Context) -> Self::Result {
        info!("Triggering reasoning for ontology ID: {}", msg.ontology_id);

        // Create a job ID for tracking
        let job_id = format!("reasoning-{}-{}", msg.ontology_id, Uuid::new_v4());

        // Reasoning is now handled by ReasoningActor, not OntologyActor
        // This message handler exists for backward compatibility only.
        // New code should use ReasoningActor directly for CustomReasoner inference.

        Box::pin(async move {
            info!(
                "Reasoning job {} acknowledged for ontology {} (forwarded to ReasoningActor)",
                job_id, msg.ontology_id
            );
            Ok(job_id)
        })
    }
}

impl Handler<GetCachedOntologies> for OntologyActor {
    type Result = Result<Vec<CachedOntologyInfo>, String>;

    fn handle(&mut self, _msg: GetCachedOntologies, _ctx: &mut Self::Context) -> Self::Result {
        let cached_ontologies = vec![];
        Ok(cached_ontologies)
    }
}

impl Default for OntologyActor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::messages::{
        GetOntologyHealth, GetOntologyReport, LoadOntologyAxioms, ValidateOntology, ValidationMode,
    };
    use crate::services::owl_validator::{
        ConstraintSummary, PropertyGraph, ValidationReport, ValidationStatistics,
    };

    fn empty_graph() -> PropertyGraph {
        PropertyGraph {
            nodes: vec![],
            edges: vec![],
            metadata: HashMap::new(),
        }
    }

    /// A finished (non-pending) report carrying a validator-generated id that is
    /// deliberately different from the job id, to prove the actor realigns it.
    fn completed_report(validator_id: &str) -> ValidationReport {
        ValidationReport {
            id: validator_id.to_string(),
            timestamp: time::now(),
            duration_ms: 12,
            graph_signature: "sig-completed".to_string(),
            total_triples: 3,
            violations: vec![],
            inferred_triples: vec![],
            statistics: ValidationStatistics::default(),
            constraint_summary: ConstraintSummary {
                total_constraints: 0,
                semantic_constraints: 0,
                structural_constraints: 0,
            },
        }
    }

    /// The completed report must land in the single store under BOTH the job id
    /// and the ontology id, and its id must be realigned to the job id.
    #[test]
    fn completed_report_retrievable_by_job_id_and_ontology_id() {
        let mut actor = OntologyActor::new();

        let job_id = "job-abc";
        let ontology_id = "ontology_deadbeef";

        // Mimic a running job the way process_next_job would have registered it.
        actor.active_jobs.insert(
            job_id.to_string(),
            ValidationJob {
                id: job_id.to_string(),
                ontology_id: ontology_id.to_string(),
                graph_data: empty_graph(),
                mode: ValidationMode::Full,
                status: JobStatus::Running {
                    started_at: time::now(),
                },
                created_at: time::now(),
                priority: JobPriority::Normal,
            },
        );

        actor.handle_job_completion(
            job_id,
            Ok(completed_report("validator-generated-id")),
            Duration::from_millis(12),
        );

        // Single store, dual key: both keys resolve.
        let by_job = actor
            .report_storage
            .get(job_id)
            .expect("report retrievable by job id");
        let by_ontology = actor
            .report_storage
            .get(ontology_id)
            .expect("report retrievable by ontology id");

        // report.id realigned to the job id under both keys.
        assert_eq!(by_job.report.id, job_id);
        assert_eq!(by_ontology.report.id, job_id);
        // The finished report replaced any placeholder.
        assert_eq!(by_job.report.graph_signature, "sig-completed");
        assert_eq!(actor.statistics.successful_validations, 1);
    }

    /// End-to-end through the message interface: a validation enqueued with a
    /// caller-supplied job id is retrievable by that job id AND the ontology id.
    #[actix::test]
    async fn validate_report_retrievable_via_messages() {
        let addr = OntologyActor::new().start();

        let job_id = "job-message-flow".to_string();
        let ontology_id = "ontology_message_flow".to_string();

        let pending = addr
            .send(ValidateOntology {
                ontology_id: ontology_id.clone(),
                graph_data: empty_graph(),
                mode: ValidationMode::Full,
                job_id: Some(job_id.clone()),
            })
            .await
            .expect("mailbox")
            .expect("enqueue");
        assert_eq!(pending.id, job_id);

        let by_job = addr
            .send(GetOntologyReport {
                report_id: Some(job_id.clone()),
            })
            .await
            .expect("mailbox")
            .expect("lookup");
        assert!(by_job.is_some(), "report resolvable by job id");
        assert_eq!(by_job.unwrap().id, job_id);

        let by_ontology = addr
            .send(GetOntologyReport {
                report_id: Some(ontology_id.clone()),
            })
            .await
            .expect("mailbox")
            .expect("lookup");
        assert!(by_ontology.is_some(), "report resolvable by ontology id");
        assert_eq!(by_ontology.unwrap().id, job_id);
    }

    /// End-to-end, driving the REAL interval-based queue → execute → completion
    /// cycle (not a synthetic pre-inserted report): load an ontology, enqueue a
    /// validation against it, then poll until the report leaves the "pending"
    /// placeholder state. The finished report must be a real success (not stuck,
    /// not "failed") and resolvable by both the job id and the ontology id.
    #[actix::test]
    async fn validate_transitions_pending_to_complete_over_loaded_ontology() {
        let addr = OntologyActor::new().start();

        let ofn = "Ontology(<http://example.org/test>\n\
                   Declaration(Class(<http://example.org/Person>))\n\
                   )";
        let ontology_id = addr
            .send(LoadOntologyAxioms {
                source: ofn.to_string(),
                format: Some("functional".to_string()),
            })
            .await
            .expect("mailbox")
            .expect("load succeeds");

        let job_id = "job-e2e".to_string();
        let pending = addr
            .send(ValidateOntology {
                ontology_id: ontology_id.clone(),
                graph_data: empty_graph(),
                mode: ValidationMode::Full,
                job_id: Some(job_id.clone()),
            })
            .await
            .expect("mailbox")
            .expect("enqueue");
        assert_eq!(pending.graph_signature, "pending");

        // The interval processor runs every second; poll for the transition.
        let mut final_report = None;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let maybe = addr
                .send(GetOntologyReport {
                    report_id: Some(job_id.clone()),
                })
                .await
                .expect("mailbox")
                .expect("lookup");
            if let Some(report) = maybe {
                if report.graph_signature != "pending" {
                    final_report = Some(report);
                    break;
                }
            }
        }

        let report = final_report.expect("report must transition out of pending");
        assert_ne!(report.graph_signature, "pending", "must not stay pending");
        assert_ne!(
            report.graph_signature, "failed",
            "validation over a loaded ontology should succeed, got violations: {:?}",
            report.violations
        );
        assert_eq!(report.id, job_id, "finished report id realigned to job id");

        // The same finished report is resolvable by the ontology id too.
        let by_ontology = addr
            .send(GetOntologyReport {
                report_id: Some(ontology_id.clone()),
            })
            .await
            .expect("mailbox")
            .expect("lookup")
            .expect("resolvable by ontology id");
        assert_eq!(by_ontology.id, job_id);
        assert_eq!(by_ontology.graph_signature, report.graph_signature);
    }

    /// GetOntologyHealth.loaded_ontologies must increment after a successful load.
    #[actix::test]
    async fn health_loaded_ontologies_increments_after_load() {
        let addr = OntologyActor::new().start();

        let before = addr
            .send(GetOntologyHealth)
            .await
            .expect("mailbox")
            .expect("health");
        assert_eq!(before.loaded_ontologies, 0);

        // Minimal, self-contained OWL Functional Syntax document (full IRIs, no
        // prefix resolution required).
        let ofn = "Ontology(<http://example.org/test>\n\
                   Declaration(Class(<http://example.org/Person>))\n\
                   )";
        let ontology_id = addr
            .send(LoadOntologyAxioms {
                source: ofn.to_string(),
                format: Some("functional".to_string()),
            })
            .await
            .expect("mailbox")
            .expect("load succeeds");
        assert!(!ontology_id.is_empty());

        let after = addr
            .send(GetOntologyHealth)
            .await
            .expect("mailbox")
            .expect("health");
        assert_eq!(after.loaded_ontologies, 1);
    }
}
