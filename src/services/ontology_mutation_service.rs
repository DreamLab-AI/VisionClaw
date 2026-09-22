//! OntologyMutationService — Agent write path for the living ontology corpus.
//!
//! Agents propose new notes or amendments to existing notes. Each proposal runs
//! through the W-E governed transaction spine (ADR-049 / DDD-020):
//!
//!   0. Signature-envelope precondition (fail-closed; verified BEFORE any mutation)
//!   1. Idempotency reservation — a single proposal id + idempotency key span
//!      every stage; replay of a key with an identical payload returns the prior
//!      receipt, replay with a different payload is rejected.
//!   2. Write-ahead intent recorded `Pending` before the mutation.
//!   3. Conflict-integrity gate (W-A) — the four `conflicts.py` detectors.
//!   4. REAL Whelk EL++ consistency over corpus ∪ proposed axioms
//!      (`check_axiom_set`), replacing the former no-op gate.
//!   5. Governance / projection — currently a GitHub PR for human review; the
//!      native single-transaction asserted+provenance write lands in Phase 2
//!      once the provenance-writer (T3) quad builders and the injected Oxigraph
//!      store seam are available. The intent is marked `Committed` on success.
//!   6. A deterministic [`ProposalReceipt`] is minted and returned.
//!
//! Notes are per-user — each user's agents write to their own namespace.

use crate::adapters::whelk_inference_engine::WhelkInferenceEngine;
use crate::services::file_service::MARKDOWN_DIR;
use crate::services::github_pr_service::GitHubPRService;
use crate::services::ontology_conflict_gate::{evaluate as evaluate_conflicts, ProposedCandidate};
use crate::services::proposal_spine::{
    build_receipt, canonicalize, payload_hash, verify_envelope, EnvelopeError, EnvelopeSig,
    IdempotencyDecision, IdempotencyStore, IntentLog, IntentState, ReceiptInputs, WriteAheadIntent,
};
use crate::types::ontology_tools::*;
use chrono::Utc;
use log::{error, info, warn};
use oxigraph::store::Store;
use std::sync::Arc;
use tokio::sync::RwLock;
use visionclaw_adapters::{emit_activity_nonfatal, ActivityRecord};
use visionclaw_domain::ports::ontology_repository::{
    AxiomType, OntologyRepository, OwlAxiom, OwlClass,
};
use visionclaw_domain::vault;

/// Generate a vault page for a proposal: a §V2 YAML frontmatter block
/// followed by the definition prose (ADR-2040 §V5).
///
/// Replaces the former `generate_vault_markdown`, which emitted an
/// indented `- ### OntologyBlock` of `key:: value` lines. Obsidian renders
/// those as plain text, so every page this service wrote would have been
/// invisible to the editor the owner actually uses — and, after ADR-2040,
/// invisible to the §V4 gate as well. `owl-class` alone would admit the
/// page; `public: true` is emitted because these pages are published
/// ontology terms.
fn generate_vault_markdown(proposal: &NoteProposal, term_id: &str, user_id: &str) -> String {
    let today = Utc::now().format("%Y-%m-%d").to_string();

    let mut extra: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut set = |key: &str, value: String| {
        if !value.is_empty() {
            extra.insert(key.to_string(), value);
        }
    };

    set("ontology", "true".to_string());
    set("term-id", term_id.to_string());
    // `title` is the §V2 key; `preferred-term` is preserved verbatim
    // alongside it because the ontology pipeline still reads that name.
    set("preferred-term", proposal.preferred_term.clone());
    set("status", "agent-proposed".to_string());
    set("last-updated", today);
    set("definition", proposal.definition.clone());
    set("owl-physicality", proposal.physicality.clone());
    set("owl-role", proposal.role.clone());
    set("quality-score", "0.6".to_string());
    set("authority-score", "0.5".to_string());
    set("maturity", "draft".to_string());
    set("contributed-by", user_id.to_string());
    set("is-subclass-of", wikilink_list(&proposal.is_subclass_of));
    set("alt-terms", wikilink_list(&proposal.alt_terms));
    for (rel_type, targets) in &proposal.relationships {
        set(rel_type, wikilink_list(targets));
    }

    let meta = vault::PageMeta {
        public: true,
        owl_class: (!proposal.owl_class.is_empty()).then(|| proposal.owl_class.clone()),
        source_domain: (!proposal.domain.is_empty()).then(|| proposal.domain.clone()),
        title: (!proposal.preferred_term.is_empty()).then(|| proposal.preferred_term.clone()),
        extra,
        ..vault::PageMeta::default()
    };

    let body = if proposal.definition.trim().is_empty() {
        format!("# {}\n", proposal.preferred_term)
    } else {
        format!(
            "# {}\n\n{}\n",
            proposal.preferred_term,
            proposal.definition.trim()
        )
    };

    vault::render_page(&meta, &body)
}

/// Render targets as a comma-separated list of `[[wikilinks]]` — the §V2 form
/// for a multi-valued property whose entries are page references. `serde_yaml`
/// quotes the whole value, so the leading `[[` is never re-read as YAML.
fn wikilink_list(targets: &[String]) -> String {
    targets
        .iter()
        .filter(|t| !t.trim().is_empty())
        .map(|t| format!("[[{}]]", t.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Error-string sentinel prefix: a blocking conflict-integrity report. The
/// handler maps this to HTTP 409 and returns the serialised [`ConflictReport`].
pub const CONFLICT_BLOCKED_PREFIX: &str = "CONFLICT_BLOCKED:";
/// Error-string sentinel prefix: an idempotency-key replay with a divergent
/// payload. The handler maps this to HTTP 409.
pub const IDEMPOTENCY_CONFLICT_PREFIX: &str = "IDEMPOTENCY_CONFLICT:";
/// Error-string sentinel prefix: the signature-envelope precondition rejected
/// the proposal before any mutation (fail-closed). Handler maps to HTTP 403.
pub const ENVELOPE_REJECTED_PREFIX: &str = "ENVELOPE_REJECTED:";

/// Relationship keys that feed the conflict gate's `contrasts_with` axis.
const CONTRASTS_WITH_KEYS: [&str; 2] = ["contrasts-with", "contrasts_with"];

pub struct OntologyMutationService {
    ontology_repo: Arc<dyn OntologyRepository>,
    /// Retained for the future native-projection reload path; the live gate uses
    /// the static, re-entrant `WhelkInferenceEngine::check_axiom_set`.
    #[allow(dead_code)]
    whelk: Arc<RwLock<WhelkInferenceEngine>>,
    github_pr: Arc<GitHubPRService>,
    /// W-E: idempotency persistence (replay-safe proposal receipts).
    idempotency: Arc<dyn IdempotencyStore>,
    /// W-E: write-ahead intent log for deterministic recovery.
    intents: Arc<dyn IntentLog>,
    /// PRD-022 WS-2 provenance seam. The mutation door projects to a GitHub PR
    /// for human review (it does not write the asserted graph directly), but it
    /// STILL reifies each committed proposal as an append-only PROV-O activity
    /// in `urn:ngm:graph:provenance` so "who proposed class X, when, deriving
    /// from what" is queryable. Injected from `main.rs` via
    /// [`with_provenance_store`]; `None` in unit tests that don't assert
    /// provenance (emission then no-ops). The concrete Oxigraph store handle is
    /// reached through `OxigraphOntologyRepository::store()`.
    provenance_store: Option<Arc<Store>>,
}

impl OntologyMutationService {
    pub fn new(
        ontology_repo: Arc<dyn OntologyRepository>,
        whelk: Arc<RwLock<WhelkInferenceEngine>>,
        github_pr: Arc<GitHubPRService>,
        idempotency: Arc<dyn IdempotencyStore>,
        intents: Arc<dyn IntentLog>,
    ) -> Self {
        Self {
            ontology_repo,
            whelk,
            github_pr,
            idempotency,
            intents,
            provenance_store: None,
        }
    }

    /// Inject the shared Oxigraph store so committed proposals emit queryable
    /// PROV-O provenance (PRD-022 WS-2). Production wiring passes
    /// `app_state.ontology_repository.store().clone()`.
    pub fn with_provenance_store(mut self, store: Arc<Store>) -> Self {
        self.provenance_store = Some(store);
        self
    }

    /// Reify a committed proposal as an append-only PROV-O activity. Fail-open:
    /// a provenance error is logged, never surfaced to the proposer. `agent_id`
    /// is the authenticated x-only pubkey; the generated entity is the target
    /// class IRI (URN scheme preserved verbatim as the RDF subject).
    async fn emit_proposal_provenance(
        &self,
        activity_kind: &str,
        action: &str,
        proposal_id: &str,
        target_iri: &str,
        agent_id: &str,
    ) {
        let Some(store) = self.provenance_store.clone() else {
            return;
        };
        // ADR-2021 — mint through the typed constructors, not `format!`.
        //
        // The old inline mint produced `urn:visionclaw:execution:<kind>-<id>`,
        // which is not a `sha256-12-` content address and so does not satisfy
        // the execution grammar `uri::parse` enforces: the record it wrote could
        // never be parsed back. `uri::execution` content-addresses the same
        // components, matching every other execution URN in the estate.
        let activity_urn = crate::uri::execution(format!("{activity_kind}:{proposal_id}"));
        // `did_nostr` validates the pubkey. Provenance is fail-open by contract,
        // so an unusable agent id is logged and the emission skipped rather than
        // writing an unattributable record.
        let agent_did = match crate::uri::did_nostr(agent_id) {
            Ok(did) => did,
            Err(e) => {
                warn!(
                    "provenance skipped for {activity_kind}/{proposal_id}: \
                     agent id is not a valid DID subject: {e}"
                );
                return;
            }
        };
        let record = ActivityRecord {
            activity_urn,
            agent_did,
            timestamp: Utc::now().to_rfc3339(),
            action: action.to_string(),
            derivation: "proposed".to_string(),
            used: None,
            generated: Some(target_iri.to_string()),
            informed_by: None,
        };
        emit_activity_nonfatal(store, record).await;
    }

    /// Propose creating a new note in the ontology corpus.
    pub async fn propose_create(
        &self,
        proposal: NoteProposal,
        agent_ctx: AgentContext,
        idempotency_key: Option<String>,
        signature: Option<String>,
    ) -> Result<ProposalResult, String> {
        info!(
            "Ontology propose_create: term='{}', agent={} (user={})",
            proposal.preferred_term, agent_ctx.agent_id, agent_ctx.user_id
        );

        // W-E stage: single proposal id spanning every stage (minted at START).
        let proposal_id = uuid::Uuid::new_v4().to_string();

        // W-E stage 1: canonical payload — the SAME bytes hashed for idempotency
        // AND signed by the envelope, so the signed bytes are well-defined/stable.
        let payload = serde_json::json!({ "action": "create", "proposal": proposal });
        let phash = payload_hash(&payload);

        // W-E stage 0: signature-envelope precondition (fail-closed, pre-mutation).
        // Verified over `sha256(canonicalize(payload))` by the authenticated
        // principal's x-only pubkey (`agent_ctx.agent_id`, bound to `auth.pubkey`).
        let envelope = signature.map(EnvelopeSig::new);
        Self::verify_envelope_precondition(&agent_ctx, &canonicalize(&payload), envelope.as_ref())?;
        let idem_key = idempotency_key.unwrap_or_else(|| format!("auto:{}", phash));
        match self.idempotency.reserve(&idem_key, &phash) {
            IdempotencyDecision::Replay(receipt) => {
                info!("propose_create: idempotent replay for key={}", idem_key);
                return Ok(Self::replay_result("create", &proposal.owl_class, receipt));
            }
            IdempotencyDecision::Conflict => {
                return Err(format!(
                    "{}idempotency key '{}' already used with a different payload",
                    IDEMPOTENCY_CONFLICT_PREFIX, idem_key
                ));
            }
            IdempotencyDecision::Fresh => {}
        }

        // W-E stage 2: write-ahead intent (Pending) BEFORE any mutation.
        self.intents
            .record(WriteAheadIntent::pending(&proposal_id, &idem_key, &phash));

        // Build the axiom set used by BOTH the conflict gate and Whelk.
        let proposed_axioms: Vec<OwlAxiom> = proposal
            .is_subclass_of
            .iter()
            .map(|parent| OwlAxiom {
                id: None,
                axiom_type: AxiomType::SubClassOf,
                subject: proposal.owl_class.clone(),
                object: parent.clone(),
                annotations: std::collections::HashMap::new(),
            })
            .collect();

        // Load the corpus once — reused by the conflict gate and the Whelk gate.
        let corpus = self
            .ontology_repo
            .list_owl_classes()
            .await
            .unwrap_or_default();

        // W-E stage 3: conflict-integrity gate (W-A / DDD-020 I07).
        let candidate = ProposedCandidate {
            iri: proposal.owl_class.clone(),
            label: proposal.preferred_term.clone(),
            entity_type: "Class".to_string(),
            subclass_of: proposal.is_subclass_of.clone(),
            contrasts_with: extract_contrasts(&proposal.relationships),
        };
        let conflict_report = evaluate_conflicts(&corpus, &candidate);
        if !conflict_report.ok() {
            warn!(
                "propose_create rejected — {} blocking conflict(s), {} pre-existing advisory",
                conflict_report.blocking.len(),
                conflict_report.pre_existing.len()
            );
            self.intents.mark(&proposal_id, IntentState::Failed);
            return Err(format!(
                "{}{}",
                CONFLICT_BLOCKED_PREFIX,
                serde_json::to_string(&conflict_report).unwrap_or_default()
            ));
        }

        // W-E stage 4: REAL Whelk EL++ consistency (corpus ∪ proposed axioms).
        let consistency = self.check_consistency(&corpus, &proposed_axioms);
        if !consistency.consistent {
            warn!(
                "propose_create rejected — inconsistent: {:?}",
                consistency.explanation
            );
            self.intents.mark(&proposal_id, IntentState::Failed);
            let gates = GateSummary::pending()
                .with_conflict(GateOutcome::Pass)
                .with_whelk(false)
                .with_acsp(GateOutcome::Pending);
            return Ok(ProposalResult {
                proposal_id,
                action: "create".to_string(),
                target_iri: proposal.owl_class.clone(),
                consistency,
                quality_score: 0.0,
                markdown_preview: String::new(),
                pr_url: None,
                status: ProposalStatus::Rejected,
                receipt: None,
                gates: Some(gates),
            });
        }

        // ----- gates passed; proceed to projection/governance -----
        let term_id = match self.generate_term_id(&proposal.domain).await {
            Ok(t) => t,
            Err(e) => {
                self.intents.mark(&proposal_id, IntentState::Failed);
                return Err(e);
            }
        };
        let markdown = generate_vault_markdown(&proposal, &term_id, &agent_ctx.user_id);
        let quality_score = self.compute_quality_score(&proposal);

        let file_path = format!(
            "{}/{}/{}.md",
            MARKDOWN_DIR,
            proposal.domain,
            term_id.to_lowercase().replace('-', "_")
        );

        // W-E stage 5: governance projection (GitHub PR for human review).
        let pr_url = match self
            .github_pr
            .create_ontology_pr(
                &file_path,
                &markdown,
                &format!(
                    "[ontology] {}: Add {}",
                    agent_ctx.agent_type, proposal.preferred_term
                ),
                &self.build_pr_body(&proposal, &agent_ctx, &consistency, quality_score),
                &agent_ctx,
            )
            .await
        {
            Ok(url) => Some(url),
            Err(e) => {
                error!("Failed to create GitHub PR: {}", e);
                None
            }
        };

        // W-E stage 6: mint the deterministic receipt, commit idempotency + intent.
        let assert_triples = create_assert_triples(&proposal);
        let provenance_quads =
            provenance_seed(&proposal_id, &proposal.owl_class, &agent_ctx.agent_id);
        let receipt = build_receipt(&ReceiptInputs {
            proposal_id: &proposal_id,
            idempotency_key: &idem_key,
            assert_triples: &assert_triples,
            provenance_quads: &provenance_quads,
            // The verified envelope signature (if any) is content-addressed into
            // the receipt's envelope hash; `None` keeps the unsigned marker.
            envelope: envelope.as_ref().map(|e| e.as_hex()),
        });
        self.idempotency.commit(&idem_key, &phash, receipt.clone());
        self.intents.mark(&proposal_id, IntentState::Committed);

        // PRD-022 WS-2: reify the committed proposal as queryable provenance.
        self.emit_proposal_provenance(
            "propose",
            "propose",
            &proposal_id,
            &proposal.owl_class,
            &agent_ctx.agent_id,
        )
        .await;

        let status = if pr_url.is_some() {
            ProposalStatus::PRCreated
        } else {
            ProposalStatus::Staged
        };
        let gates = GateSummary::pending()
            .with_conflict(GateOutcome::Pass)
            .with_whelk(true)
            .with_acsp(GateOutcome::Pending);

        info!(
            "propose_create {} committed: iri={}, status={:?}",
            proposal_id, proposal.owl_class, status
        );

        Ok(ProposalResult {
            proposal_id,
            action: "create".to_string(),
            target_iri: proposal.owl_class,
            consistency,
            quality_score,
            markdown_preview: markdown.chars().take(500).collect(),
            pr_url,
            status,
            receipt: Some(receipt),
            gates: Some(gates),
        })
    }

    /// Propose amending an existing note in the ontology corpus.
    pub async fn propose_amend(
        &self,
        target_iri: &str,
        amendment: NoteAmendment,
        agent_ctx: AgentContext,
        idempotency_key: Option<String>,
        signature: Option<String>,
    ) -> Result<ProposalResult, String> {
        info!(
            "Ontology propose_amend: iri='{}', agent={} (user={})",
            target_iri, agent_ctx.agent_id, agent_ctx.user_id
        );

        let proposal_id = uuid::Uuid::new_v4().to_string();

        // Stage 1: canonical payload — the same bytes hashed for idempotency and
        // signed by the envelope.
        let payload = serde_json::json!({
            "action": "amend",
            "targetIri": target_iri,
            "amendment": amendment,
        });
        let phash = payload_hash(&payload);

        // Stage 0: signature-envelope precondition (fail-closed, pre-mutation).
        let envelope = signature.map(EnvelopeSig::new);
        Self::verify_envelope_precondition(&agent_ctx, &canonicalize(&payload), envelope.as_ref())?;
        let idem_key = idempotency_key.unwrap_or_else(|| format!("auto:{}", phash));
        match self.idempotency.reserve(&idem_key, &phash) {
            IdempotencyDecision::Replay(receipt) => {
                info!("propose_amend: idempotent replay for key={}", idem_key);
                return Ok(Self::replay_result("amend", target_iri, receipt));
            }
            IdempotencyDecision::Conflict => {
                return Err(format!(
                    "{}idempotency key '{}' already used with a different payload",
                    IDEMPOTENCY_CONFLICT_PREFIX, idem_key
                ));
            }
            IdempotencyDecision::Fresh => {}
        }

        // Stage 2: write-ahead intent.
        self.intents
            .record(WriteAheadIntent::pending(&proposal_id, &idem_key, &phash));

        // Fetch existing class.
        let existing = match self.ontology_repo.get_owl_class(target_iri).await {
            Ok(Some(c)) => c,
            Ok(None) => {
                self.intents.mark(&proposal_id, IntentState::Failed);
                return Err(format!("Class not found: {}", target_iri));
            }
            Err(e) => {
                self.intents.mark(&proposal_id, IntentState::Failed);
                return Err(format!("Failed to get class: {}", e));
            }
        };

        // ADR-2040 §V5: amend the page's frontmatter. A legacy page's leading
        // property block is converted to frontmatter by this same write, and
        // the body (prose + JSON-LD fences) is carried through untouched.
        // The former code appended `    - {rel}:: [[target]]` lines, which
        // ADR-2040 Invariant 1 forbids — and which landed AFTER the body, well
        // outside the leading block the gate now reads.
        let existing_markdown = existing.markdown_content.clone().unwrap_or_default();
        let (mut meta, body) = vault::split(&existing_markdown);

        if let Some(ref new_def) = amendment.update_definition {
            meta.extra.insert("definition".to_string(), new_def.clone());
        }

        // PRD-sovereign-corpus Q5: a relation is a LIST of wikilinks, not a
        // comma-joined scalar. Appending to one string re-emitted
        // `requires: '[[A]], [[B]]'`, which YAML reads back as a single target
        // named "[[A]], [[B]]" — two edges collapsed into one broken one.
        for (rel_type, targets) in &amendment.add_relationships {
            let entry = meta.extra_lists.entry(rel_type.clone()).or_default();
            // Seed from a legacy comma-joined value so converting a page does
            // not drop the edges it already had.
            if entry.is_empty() {
                if let Some(legacy) = meta.extra.get(rel_type) {
                    entry.extend(
                        legacy
                            .split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string),
                    );
                }
            }
            for target in targets {
                let link = format!("[[{}]]", target);
                if entry.iter().any(|existing| existing.trim() == link) {
                    continue;
                }
                entry.push(link);
            }
            // The scalar shadow would otherwise be re-rendered stale if the
            // list were ever emptied; the list is authoritative from here.
            meta.extra.remove(rel_type);
        }

        let new_markdown = vault::render_page(&meta, body);

        // New subclass axioms proposed by the amendment.
        let mut proposed_axioms = Vec::new();
        let mut new_parents: Vec<String> = Vec::new();
        for (rel_type, targets) in &amendment.add_relationships {
            if rel_type == "is-subclass-of" {
                for target in targets {
                    new_parents.push(target.clone());
                    proposed_axioms.push(OwlAxiom {
                        id: None,
                        axiom_type: AxiomType::SubClassOf,
                        subject: target_iri.to_string(),
                        object: target.clone(),
                        annotations: std::collections::HashMap::new(),
                    });
                }
            }
        }

        let corpus = self
            .ontology_repo
            .list_owl_classes()
            .await
            .unwrap_or_default();

        // Stage 3: conflict gate over the amended candidate.
        let mut subclass_of = existing.parent_classes.clone();
        subclass_of.extend(new_parents.iter().cloned());
        let candidate = ProposedCandidate {
            iri: target_iri.to_string(),
            label: existing.label.clone().unwrap_or_else(|| {
                existing
                    .preferred_term
                    .clone()
                    .unwrap_or_else(|| target_iri.to_string())
            }),
            entity_type: existing
                .class_type
                .clone()
                .unwrap_or_else(|| "Class".to_string()),
            subclass_of,
            contrasts_with: extract_contrasts(&amendment.add_relationships),
        };
        let conflict_report = evaluate_conflicts(&corpus, &candidate);
        if !conflict_report.ok() {
            warn!(
                "propose_amend rejected — {} blocking conflict(s), {} pre-existing advisory",
                conflict_report.blocking.len(),
                conflict_report.pre_existing.len()
            );
            self.intents.mark(&proposal_id, IntentState::Failed);
            return Err(format!(
                "{}{}",
                CONFLICT_BLOCKED_PREFIX,
                serde_json::to_string(&conflict_report).unwrap_or_default()
            ));
        }

        // Stage 4: Whelk consistency (skip only when no new axioms are proposed).
        let consistency = if proposed_axioms.is_empty() {
            ConsistencyReport {
                consistent: true,
                new_subsumptions: 0,
                explanation: None,
            }
        } else {
            self.check_consistency(&corpus, &proposed_axioms)
        };

        if !consistency.consistent {
            self.intents.mark(&proposal_id, IntentState::Failed);
            let gates = GateSummary::pending()
                .with_conflict(GateOutcome::Pass)
                .with_whelk(false)
                .with_acsp(GateOutcome::Pending);
            return Ok(ProposalResult {
                proposal_id,
                action: "amend".to_string(),
                target_iri: target_iri.to_string(),
                consistency,
                quality_score: 0.0,
                markdown_preview: new_markdown.chars().take(500).collect(),
                pr_url: None,
                status: ProposalStatus::Rejected,
                receipt: None,
                gates: Some(gates),
            });
        }

        // Stage 5: governance projection.
        let file_path = existing.source_file.clone().unwrap_or_else(|| {
            let domain = existing.source_domain.as_deref().unwrap_or("general");
            let term_id = existing.term_id.as_deref().unwrap_or("unknown");
            format!(
                "{}/{}/{}.md",
                MARKDOWN_DIR,
                domain,
                term_id.to_lowercase().replace('-', "_")
            )
        });

        let pr_url = match self
            .github_pr
            .create_ontology_pr(
                &file_path,
                &new_markdown,
                &format!(
                    "[ontology] {}: Amend {}",
                    agent_ctx.agent_type,
                    existing.preferred_term.as_deref().unwrap_or(target_iri)
                ),
                &self.build_amend_pr_body(target_iri, &amendment, &agent_ctx, &consistency),
                &agent_ctx,
            )
            .await
        {
            Ok(url) => Some(url),
            Err(e) => {
                error!("Failed to create GitHub PR for amendment: {}", e);
                None
            }
        };

        // Stage 6: receipt + commit.
        let assert_triples = amend_assert_triples(target_iri, &new_parents);
        let provenance_quads = provenance_seed(&proposal_id, target_iri, &agent_ctx.agent_id);
        let receipt = build_receipt(&ReceiptInputs {
            proposal_id: &proposal_id,
            idempotency_key: &idem_key,
            assert_triples: &assert_triples,
            provenance_quads: &provenance_quads,
            envelope: envelope.as_ref().map(|e| e.as_hex()),
        });
        self.idempotency.commit(&idem_key, &phash, receipt.clone());
        self.intents.mark(&proposal_id, IntentState::Committed);

        // PRD-022 WS-2: reify the committed amendment as queryable provenance.
        self.emit_proposal_provenance(
            "amend",
            "amend",
            &proposal_id,
            target_iri,
            &agent_ctx.agent_id,
        )
        .await;

        let status = if pr_url.is_some() {
            ProposalStatus::PRCreated
        } else {
            ProposalStatus::Staged
        };
        let gates = GateSummary::pending()
            .with_conflict(GateOutcome::Pass)
            .with_whelk(true)
            .with_acsp(GateOutcome::Pending);

        Ok(ProposalResult {
            proposal_id,
            action: "amend".to_string(),
            target_iri: target_iri.to_string(),
            consistency,
            quality_score: amendment.update_quality_score.unwrap_or(0.5),
            markdown_preview: new_markdown.chars().take(500).collect(),
            pr_url,
            status,
            receipt: Some(receipt),
            gates: Some(gates),
        })
    }

    /// ADR-049 Security: the signature-envelope precondition runs BEFORE any
    /// mutation and is fail-closed. This delegates to the SINGLE shared spine
    /// seam [`verify_envelope`] (reuse, not clone) so the ontology door and the
    /// decision door can never drift apart: it verifies a supplied BIP-340
    /// envelope over `sha256(canonical_payload)` by the authenticated principal's
    /// x-only pubkey (`agent_ctx.agent_id`), rejecting an invalid-but-present
    /// signature and — when `ONTOLOGY_REQUIRE_SIGNED_ENVELOPE` is set — an absent
    /// one. Default-off keeps the current unsigned authenticated-route behaviour.
    fn verify_envelope_precondition(
        agent_ctx: &AgentContext,
        canonical_payload: &str,
        envelope: Option<&EnvelopeSig>,
    ) -> Result<(), String> {
        verify_envelope(&agent_ctx.agent_id, canonical_payload, envelope).map_err(|e| {
            let detail = match e {
                EnvelopeError::Required => "signed envelope required but none supplied".to_string(),
                other => format!("envelope verification failed: {other}"),
            };
            format!(
                "{}{} for agent {}",
                ENVELOPE_REJECTED_PREFIX, detail, agent_ctx.agent_id
            )
        })
    }

    /// Reconstruct a minimal replay result from a stored receipt (no re-mutation).
    fn replay_result(action: &str, target_iri: &str, receipt: ProposalReceipt) -> ProposalResult {
        ProposalResult {
            proposal_id: receipt.proposal_id.clone(),
            action: action.to_string(),
            target_iri: target_iri.to_string(),
            consistency: ConsistencyReport {
                consistent: true,
                new_subsumptions: 0,
                explanation: None,
            },
            quality_score: 0.0,
            markdown_preview: String::new(),
            pr_url: None,
            status: ProposalStatus::PRCreated,
            receipt: Some(receipt),
            gates: Some(
                GateSummary::pending()
                    .with_conflict(GateOutcome::Pass)
                    .with_whelk(true)
                    .with_acsp(GateOutcome::Pending),
            ),
        }
    }

    /// Generate the next term-id for a domain (e.g., AI-0851)
    async fn generate_term_id(&self, domain: &str) -> Result<String, String> {
        let prefix = match domain {
            "ai" => "AI",
            "bc" => "BC",
            "rb" => "RB",
            "mv" => "MV",
            "tc" => "TC",
            "dt" => "DT",
            _ => "GEN",
        };

        let classes = self
            .ontology_repo
            .list_owl_classes()
            .await
            .unwrap_or_default();
        let max_seq = classes
            .iter()
            .filter_map(|c| {
                c.term_id.as_ref().and_then(|tid| {
                    if tid.starts_with(prefix) {
                        tid.split('-').last()?.parse::<u32>().ok()
                    } else {
                        None
                    }
                })
            })
            .max()
            .unwrap_or(0);

        Ok(format!("{}-{:04}", prefix, max_seq + 1))
    }

    /// REAL Whelk EL++ consistency check: builds the union of the corpus classes
    /// and the proposed axioms and runs the static, re-entrant `check_axiom_set`
    /// reasoner. This REPLACES the former no-op that ignored `proposed_axioms`
    /// and only inspected the already-loaded engine. A class whelk subsumes under
    /// `owl:Nothing` marks the proposal inconsistent.
    fn check_consistency(
        &self,
        corpus: &[OwlClass],
        proposed_axioms: &[OwlAxiom],
    ) -> ConsistencyReport {
        let outcome = WhelkInferenceEngine::check_axiom_set(corpus, proposed_axioms);
        ConsistencyReport {
            consistent: outcome.consistent,
            new_subsumptions: proposed_axioms.len(),
            explanation: if outcome.consistent {
                None
            } else {
                Some(outcome.explanation())
            },
        }
    }

    /// Compute quality score for a proposal based on completeness.
    fn compute_quality_score(&self, proposal: &NoteProposal) -> f32 {
        let mut score = 0.0f32;
        let mut fields = 0.0f32;

        if !proposal.preferred_term.is_empty() {
            score += 1.0;
        }
        fields += 1.0;
        if !proposal.definition.is_empty() {
            score += 1.0;
        }
        fields += 1.0;
        if !proposal.owl_class.is_empty() {
            score += 1.0;
        }
        fields += 1.0;
        if !proposal.is_subclass_of.is_empty() {
            score += 1.0;
        }
        fields += 1.0;
        if !proposal.physicality.is_empty() {
            score += 1.0;
        }
        fields += 1.0;
        if !proposal.role.is_empty() {
            score += 1.0;
        }
        fields += 1.0;

        if !proposal.alt_terms.is_empty() {
            score += 0.5;
            fields += 0.5;
        }
        if !proposal.relationships.is_empty() {
            score += 0.5;
            fields += 0.5;
        }

        (score / fields).min(1.0)
    }

    fn build_pr_body(
        &self,
        proposal: &NoteProposal,
        agent_ctx: &AgentContext,
        consistency: &ConsistencyReport,
        quality: f32,
    ) -> String {
        format!(
            r#"## Proposed Change

{task}

**Action**: Create new ontology note
**Agent**: {agent_type} ({agent_id})
**User**: {user_id}

## New Class

| Property | Value |
|----------|-------|
| IRI | `{iri}` |
| Term | {term} |
| Domain | {domain} |
| Parents | {parents} |

## Whelk Consistency Report

{consistency_status} {consistency_detail}

## Quality Assessment

- Quality Score: {quality:.2}/1.0
- Agent Confidence: {confidence:.2}/1.0
"#,
            task = agent_ctx.task_description,
            agent_type = agent_ctx.agent_type,
            agent_id = agent_ctx.agent_id,
            user_id = agent_ctx.user_id,
            iri = proposal.owl_class,
            term = proposal.preferred_term,
            domain = proposal.domain,
            parents = proposal.is_subclass_of.join(", "),
            consistency_status = if consistency.consistent {
                "**Consistent**"
            } else {
                "❌ **Inconsistent**"
            },
            consistency_detail = consistency
                .explanation
                .as_deref()
                .unwrap_or("No logical contradictions"),
            quality = quality,
            // FR2.4: absence renders as absence, in the PR body too.
            confidence = agent_ctx
                .confidence
                .map(|c| format!("{c}"))
                .unwrap_or_else(|| "not reported".to_string()),
        )
    }

    fn build_amend_pr_body(
        &self,
        target_iri: &str,
        amendment: &NoteAmendment,
        agent_ctx: &AgentContext,
        consistency: &ConsistencyReport,
    ) -> String {
        let mut changes = Vec::new();
        if amendment.update_definition.is_some() {
            changes.push("Updated definition".to_string());
        }
        for (rel_type, targets) in &amendment.add_relationships {
            for target in targets {
                changes.push(format!("Added {}: {}", rel_type, target));
            }
        }
        for (rel_type, targets) in &amendment.remove_relationships {
            for target in targets {
                changes.push(format!("Removed {}: {}", rel_type, target));
            }
        }

        format!(
            r#"## Proposed Amendment

{task}

**Action**: Amend existing note `{iri}`
**Agent**: {agent_type} ({agent_id})
**User**: {user_id}

## Changes

{changes}

## Whelk Consistency Report

{consistency_status}
"#,
            task = agent_ctx.task_description,
            iri = target_iri,
            agent_type = agent_ctx.agent_type,
            agent_id = agent_ctx.agent_id,
            user_id = agent_ctx.user_id,
            changes = changes
                .iter()
                .map(|c| format!("- {}", c))
                .collect::<Vec<_>>()
                .join("\n"),
            consistency_status = if consistency.consistent {
                "Consistent"
            } else {
                "❌ Inconsistent"
            },
        )
    }
}

// ---------------------------------------------------------------------------
// Pure projection helpers (asserted-graph triples + provenance seed)
// ---------------------------------------------------------------------------

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const OWL_CLASS: &str = "http://www.w3.org/2002/07/owl#Class";
const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";

/// Pull the `contrasts-with` / `contrasts_with` targets out of a relationship map.
fn extract_contrasts(rels: &std::collections::HashMap<String, Vec<String>>) -> Vec<String> {
    let mut out = Vec::new();
    for key in CONTRASTS_WITH_KEYS {
        if let Some(targets) = rels.get(key) {
            out.extend(targets.iter().cloned());
        }
    }
    out
}

/// The plain current triples a create proposal projects into the asserted graph.
fn create_assert_triples(proposal: &NoteProposal) -> Vec<String> {
    let mut triples = vec![format!(
        "<{}> <{}> <{}>",
        proposal.owl_class, RDF_TYPE, OWL_CLASS
    )];
    for parent in &proposal.is_subclass_of {
        triples.push(format!(
            "<{}> <{}> <{}>",
            proposal.owl_class, RDFS_SUBCLASS_OF, parent
        ));
    }
    triples
}

/// The plain current triples an amend proposal adds to the asserted graph.
fn amend_assert_triples(target_iri: &str, new_parents: &[String]) -> Vec<String> {
    new_parents
        .iter()
        .map(|parent| format!("<{}> <{}> <{}>", target_iri, RDFS_SUBCLASS_OF, parent))
        .collect()
}

/// A canonical provenance-content seed line for the receipt's provenance hash.
/// The full portable-reification quad set (ADR-049) is emitted by the T3
/// provenance writer and committed in Phase 2; this content-addresses the same
/// subject/agent/proposal tuple deterministically so the receipt is stable.
fn provenance_seed(proposal_id: &str, subject: &str, agent_iri: &str) -> Vec<String> {
    vec![format!(
        "assertion-version proposal:{} subject:{} attributedTo:{}",
        proposal_id, subject, agent_iri
    )]
}

#[cfg(test)]
mod provenance_wiring_tests {
    //! PRD-022 WS-2: mutation-path provenance emission — a proposal driven
    //! through the REAL `OntologyMutationService` reifies a queryable PROV-O
    //! chain into the unified `urn:ngm:graph:provenance` ledger.
    use super::*;
    use crate::services::proposal_spine::{InMemoryIdempotencyStore, InMemoryIntentLog};
    use oxigraph::store::Store;
    use std::collections::HashMap;
    use visionclaw_adapters::OxigraphOntologyRepository;

    const PUBKEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PROV_GRAPH: &str = "urn:ngm:graph:provenance";

    /// Build the service over an in-memory Oxigraph store. `GitHubPRService` is
    /// configured with an EMPTY token so the PR projection degrades gracefully
    /// (early Err, no network) and the proposal still commits + emits provenance.
    fn service_with_store(store: Arc<Store>, inject_store: bool) -> OntologyMutationService {
        let repo = Arc::new(OxigraphOntologyRepository::from_store(Arc::clone(&store)));
        let svc = OntologyMutationService::new(
            repo as Arc<dyn OntologyRepository>,
            Arc::new(RwLock::new(WhelkInferenceEngine::new())),
            Arc::new(GitHubPRService::with_config(
                String::new(),
                String::new(),
                String::new(),
                "main".to_string(),
            )),
            Arc::new(InMemoryIdempotencyStore::new()),
            Arc::new(InMemoryIntentLog::new()),
        );
        if inject_store {
            svc.with_provenance_store(store)
        } else {
            svc
        }
    }

    fn agent_ctx() -> AgentContext {
        AgentContext {
            agent_id: PUBKEY.to_string(),
            agent_type: "test-agent".to_string(),
            task_description: "provenance wiring test".to_string(),
            session_id: None,
            confidence: Some(0.9),
            user_id: "user-1".to_string(),
        }
    }

    fn proposal(iri: &str) -> NoteProposal {
        NoteProposal {
            preferred_term: "Quantum Widget".to_string(),
            definition: "A widget exhibiting quantum behaviour.".to_string(),
            owl_class: iri.to_string(),
            physicality: "conceptual".to_string(),
            role: "concept".to_string(),
            domain: "ai".to_string(),
            is_subclass_of: vec![],
            relationships: HashMap::new(),
            alt_terms: vec![],
            owner_user_id: Some("user-1".to_string()),
        }
    }

    /// INTEGRATION: a mutation performed through the service reifies a full
    /// PROV-O chain into Oxigraph, discoverable by SPARQL and the query surface.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn propose_create_emits_queryable_provenance_chain() {
        let store = Arc::new(Store::new().unwrap());
        let svc = service_with_store(Arc::clone(&store), true);

        let iri = "urn:ngm:class:quantum-widget";
        let res = svc
            .propose_create(proposal(iri), agent_ctx(), None, None)
            .await
            .expect("propose_create ok");
        assert_eq!(res.target_iri, iri);

        // SPARQL over the unified provenance graph finds the full triad: the
        // class URN is a prov:Entity, wasGeneratedBy the propose activity, which
        // wasAssociatedWith the agent DID; the entity wasAttributedTo the agent.
        let repo = OxigraphOntologyRepository::from_store(Arc::clone(&store));
        let ask = repo
            .sparql_select_json(format!(
                "PREFIX prov: <http://www.w3.org/ns/prov#> \
                 ASK FROM <{PROV_GRAPH}> {{ \
                   <{iri}> a prov:Entity ; \
                           prov:wasGeneratedBy ?act ; \
                           prov:wasAttributedTo <did:nostr:{PUBKEY}> . \
                   ?act a prov:Activity ; prov:wasAssociatedWith <did:nostr:{PUBKEY}> . \
                   <did:nostr:{PUBKEY}> a prov:Agent }}"
            ))
            .await
            .unwrap();
        assert_eq!(
            ask["boolean"],
            serde_json::Value::Bool(true),
            "full PROV-O triad present: {ask}"
        );

        // The query surface walks the chain for the entity.
        let chain = repo
            .provenance_for_entity(iri.to_string(), 8)
            .await
            .unwrap();
        assert_eq!(chain.root, iri);
        let node = chain
            .nodes
            .iter()
            .find(|n| n.entity == iri)
            .expect("entity node present");
        assert_eq!(node.action.as_deref(), Some("propose"));
        assert_eq!(node.derivation.as_deref(), Some("proposed"));
        assert_eq!(
            node.attributed_to.as_deref(),
            Some(format!("did:nostr:{PUBKEY}").as_str())
        );
        assert!(node.generated_by.is_some(), "wasGeneratedBy resolved");
        assert!(node.generated_at.is_some(), "generatedAtTime present");
    }

    /// URN round-trip: the exact class URN minted by the proposal is the RDF
    /// subject that comes back out of the provenance query, byte-for-byte.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn class_urn_round_trips_through_provenance() {
        let store = Arc::new(Store::new().unwrap());
        let svc = service_with_store(Arc::clone(&store), true);

        let iri = "urn:ngm:class:round-trip-subject";
        svc.propose_create(proposal(iri), agent_ctx(), None, None)
            .await
            .expect("ok");

        let repo = OxigraphOntologyRepository::from_store(Arc::clone(&store));
        let chain = repo
            .provenance_for_entity(iri.to_string(), 4)
            .await
            .unwrap();
        assert_eq!(chain.root, iri, "root URN preserved verbatim");
        assert!(
            chain.nodes.iter().any(|n| n.entity == iri),
            "the minted URN is the entity subject in the RDF"
        );
    }

    /// Fail-open: with no store injected the proposal still commits and simply
    /// writes zero provenance (unit-test path).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_store_injected_is_provenance_noop() {
        let store = Arc::new(Store::new().unwrap());
        let svc = service_with_store(Arc::clone(&store), false);
        svc.propose_create(proposal("urn:ngm:class:no-prov"), agent_ctx(), None, None)
            .await
            .expect("ok");
        assert_eq!(
            visionclaw_adapters::provenance_emitter::count_provenance_triples(&store).unwrap(),
            0,
            "no store injected → no provenance emitted"
        );
    }
}

#[cfg(test)]
mod vault_writer_tests {
    use super::*;
    use visionclaw_domain::vault::{self, PageFormat};

    fn proposal() -> NoteProposal {
        NoteProposal {
            preferred_term: "Arbitration Decision Engine".to_string(),
            definition: "Resolves competing claims: deterministically.".to_string(),
            owl_class: "mv:ArbitrationDecisionEngine".to_string(),
            physicality: "abstract".to_string(),
            role: "process".to_string(),
            domain: "mv".to_string(),
            is_subclass_of: vec!["Engine".to_string(), "Decision Process".to_string()],
            relationships: std::collections::HashMap::from([(
                "enables".to_string(),
                vec!["Consensus".to_string()],
            )]),
            alt_terms: vec!["Arbiter".to_string()],
            owner_user_id: None,
        }
    }

    #[test]
    fn generated_page_is_frontmatter_and_passes_the_gate() {
        let markdown = generate_vault_markdown(&proposal(), "MV-0042", "did:nostr:abc");

        assert!(markdown.starts_with("---\n"), "must open with frontmatter");
        assert!(
            !markdown.contains(":: "),
            "ADR-2040 Invariant 1: no writer emits `key:: value` lines"
        );

        let meta = vault::parse(&markdown);
        assert_eq!(meta.format, PageFormat::Obsidian);
        assert!(meta.is_kg_included());
        assert!(meta.public);
        assert_eq!(
            meta.owl_class.as_deref(),
            Some("mv:ArbitrationDecisionEngine")
        );
        assert_eq!(meta.source_domain.as_deref(), Some("mv"));
        assert_eq!(meta.title.as_deref(), Some("Arbitration Decision Engine"));
    }

    #[test]
    fn generated_page_preserves_the_ontology_properties_verbatim() {
        let markdown = generate_vault_markdown(&proposal(), "MV-0042", "did:nostr:abc");
        let meta = vault::parse(&markdown);

        assert_eq!(
            meta.extra.get("term-id").map(String::as_str),
            Some("MV-0042")
        );
        assert_eq!(
            meta.extra.get("maturity").map(String::as_str),
            Some("draft")
        );
        assert_eq!(
            meta.extra.get("contributed-by").map(String::as_str),
            Some("did:nostr:abc")
        );
        assert_eq!(
            meta.extra.get("is-subclass-of").map(String::as_str),
            Some("[[Engine]], [[Decision Process]]")
        );
        assert_eq!(
            meta.extra.get("enables").map(String::as_str),
            Some("[[Consensus]]")
        );
        // A definition containing a colon must survive YAML quoting intact.
        assert_eq!(
            meta.extra.get("definition").map(String::as_str),
            Some("Resolves competing claims: deterministically.")
        );
    }

    #[test]
    fn the_body_carries_the_heading_and_definition() {
        let markdown = generate_vault_markdown(&proposal(), "MV-0042", "did:nostr:abc");
        let (_, body) = vault::split(&markdown);
        assert!(body.contains("# Arbitration Decision Engine"));
        assert!(body.contains("Resolves competing claims: deterministically."));
    }

    #[test]
    fn wikilink_list_skips_blanks_and_wraps_each_target() {
        assert_eq!(
            wikilink_list(&["A".to_string(), "  ".to_string(), "B".to_string()]),
            "[[A]], [[B]]"
        );
        assert_eq!(wikilink_list(&[]), "");
    }
}
