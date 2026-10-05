//! ElevationActor — the flagship ACSP agentic actor (ADR-110).
//!
//! Closes the knowledge-elevation loop: the formal ontology's *frontier* —
//! classes referenced by axioms but never authored as pages (`owl_class`
//! stubs) — is a ready-made work queue. The actor selects the most-referenced
//! frontier concepts, drafts a canonical Class page for each, and opens a
//! `knowledge_enrichment` broker case on the forum governance page with a
//! custom control surface carrying everything a human needs to judge:
//! proposed canonical name, slug, inferred domain, draft definition and the
//! referencing classes. An `approve` decision (kind 31403) commits the draft
//! to the corpus repo as a PR via [`GitHubPRService`]; `reject` skips the
//! candidate for the session.
//!
//! Boot is env-gated. Per ADR-130 Decision 2 (REC-2), the gate now defaults ON
//! in dev/staging and stays opt-in in production: `ELEVATION_ACTOR_ENABLED`
//! defaults to `true` unless `APP_ENV`/`NODE_ENV` is `production`, and an
//! explicit `ELEVATION_ACTOR_ENABLED=0`/`=1` always wins. The actor still
//! additionally requires `FORUM_RELAY_URL` + a panel secret to publish and sign
//! ACSP events, so a dev box without a relay configured stays dormant even with
//! the gate open. The ACSP panel identity must be registered in the relay's
//! `agent_registry` (the pubkey is logged at startup for the admin).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use actix::prelude::*;
use log::{error, info, warn};
use serde_json::json;

use crate::actors::elevation_voice::{
    harvest_mentions, parse_elevation_intent, ConceptIndex, VoiceDemandLedger,
};
use crate::adapters::sqlite_enrichment_repository::{
    EnrichmentProposal as StoredProposal, SqliteEnrichmentRepository, StoredDecision,
};
use crate::adapters::WhelkInferenceEngine;
use crate::ports::knowledge_graph_repository::KnowledgeGraphRepository;
use crate::ports::ontology_repository::{AxiomType, OntologyRepository, OwlAxiom, OwlClass};
use crate::services::acsp::events::{
    ActionDef, ActionStyle, FieldDef, FieldType, LayoutHint, PanelDefinition, PanelSchema,
};
use crate::services::acsp::{
    build_action_request, build_case_status_update, build_panel_definition, build_panel_state,
    AcspClient, ActionPriority, ActionRequest, CaseCategory, CaseDecision, CaseSpec, SubjectKind,
};
use crate::services::github_pr_service::{GitHubPRService, PrState};
use crate::services::kpi_compute::SYSTEM_WHELK_GATE;
use crate::services::speech_service::SpeechService;
use crate::types::ontology_tools::AgentContext;
use crate::types::speech::SpeechOptions;

/// NIP-33 panel id (the `d` tag) for the elevation control surface.
const PANEL_ID: &str = "vc-elevation";
/// Case-id namespace; the decision subscription filters on this prefix.
const CASE_PREFIX: &str = "vc-elev-";
/// How many broker cases may be open at once.
const MAX_OPEN_CASES: usize = 5;
/// Candidate scan cadence.
const CYCLE_INTERVAL: Duration = Duration::from_secs(600);
/// GOV-2 merge-poll cadence: how often opened elevation PRs are checked for a
/// terminal git state (merged → `concept_elevated`, closed → abandoned).
const PR_POLL_INTERVAL: Duration = Duration::from_secs(120);
/// FR4.5 (EXP-AC-004): how long an opened case may sit awaiting a human before
/// boot reconciliation times it out with a receipt. 14 days, the same TTL
/// `decision_elevation_actor.rs` uses — one panel should not age a case out on a
/// different clock from its sibling.
const OPEN_CASE_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);
/// Kind-31404 status published when reconciliation times a stale case out.
const EXPIRED_STATUS: &str = "elevation_expired";
/// Durable status written for a timed-out case.
const EXPIRED_STORE_STATUS: &str = "expired";
/// How many durable rows boot reconciliation scans. Generous next to
/// `MAX_OPEN_CASES`, so a store carrying a backlog is still fully recovered.
const RECONCILE_SCAN_LIMIT: i64 = 500;

#[derive(Message)]
#[rtype(result = "()")]
struct RunCycle;

#[derive(Message)]
#[rtype(result = "()")]
struct Decision(CaseDecision);

/// Poll tracked elevation PRs for a terminal git state (GOV-2).
#[derive(Message)]
#[rtype(result = "()")]
struct PollPrs;

/// FR4.5: rebuild the in-memory working set from the durable projection at boot.
#[derive(Message)]
#[rtype(result = "()")]
struct Reconcile;

/// An opened elevation PR being tracked to its terminal state (GOV-2). Keyed by
/// case id in [`ElevationActor::elevating`] so the merge poll can fire the
/// terminal `concept_elevated` event and mark the store row `elevated`.
#[derive(Debug, Clone)]
struct TrackedPr {
    pr_url: String,
    label: String,
}

/// One transcription line from the local Whisper STT stream.
#[derive(Message)]
#[rtype(result = "()")]
struct VoiceTranscript(String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingCase {
    label: String,
    file_path: String,
    draft: String,
}

pub struct ElevationActor {
    kg_repo: Arc<dyn KnowledgeGraphRepository>,
    /// Durable projection of the actor's working set. Opened cases are persisted
    /// here as `state=pending` at case-open so `/api/broker/inbox` shows a
    /// pending proposal BEFORE any decision (previously the inbox was empty until
    /// the decide-time stub), and the Decision handler reconciles the row so the
    /// REST and actor views agree. The in-memory `pending` map stays the working
    /// set; this store is the durable projection (ADR-130 Decision 2 / gap-close).
    enrichment_repo: Arc<SqliteEnrichmentRepository>,
    /// GOV-7 (ADR-130): the base ontology source for the EL++ consistency gate.
    /// On `approve`, base-ontology ∪ draft is checked for consistency BEFORE the
    /// PR is opened. `None` means the gate is UNAVAILABLE for this profile — and
    /// canon (no advisory write path) requires the gate to then FAIL CLOSED:
    /// approvals are blocked, not waved through. The whelk reasoner itself is
    /// stateless ([`WhelkInferenceEngine::check_axiom_set`]); only this base
    /// source is threaded state.
    consistency_base: Option<Arc<dyn OntologyRepository>>,
    acsp: Option<Arc<AcspClient>>,
    /// Local PocketTts TTS / Whisper STT bridge: transcripts guide candidate
    /// selection; the actor speaks confirmations back into the session.
    speech: Option<Arc<SpeechService>>,
    panel_secret: String,
    forum_relay_url: String,
    /// Open broker cases awaiting a human decision, keyed by case id.
    pending: HashMap<String, PendingCase>,
    /// GOV-2: elevation PRs opened on approve, tracked to their terminal git
    /// state. The merge poll fires `concept_elevated` (kind-31404) + marks the
    /// store row `elevated` on merge, or `elevation_abandoned` + `abandoned` on
    /// close-without-merge, then removes the entry.
    elevating: HashMap<String, TrackedPr>,
    /// Frontier labels already cased/decided this session (skip list).
    seen: HashSet<String>,
    /// Conversational demand (decaying) — the PRIMARY elevation signal.
    voice: VoiceDemandLedger,
    /// Concept lookup over the latest graph snapshot's elevatable labels.
    concept_index: Arc<ConceptIndex>,
    elevated_count: u32,
    rejected_count: u32,
    voice_case_count: u32,
    last_pr_url: Option<String>,
}

/// Is this a production deployment? Production is signalled by `APP_ENV` or
/// `NODE_ENV` set to `production` (the prod docker-compose profile sets
/// `NODE_ENV=production`; `main.rs` reads `APP_ENV=production` for the same
/// intent). Anything else — dev, staging, an unset env — is non-production.
fn is_production_env() -> bool {
    is_production_from(
        std::env::var("APP_ENV").ok(),
        std::env::var("NODE_ENV").ok(),
    )
}

/// Pure production check over the two environment signals, factored out so the
/// dev-default-ON / prod-opt-in policy is unit-testable without mutating
/// process-global env state.
fn is_production_from(app_env: Option<String>, node_env: Option<String>) -> bool {
    let is_prod = |v: &Option<String>| {
        v.as_deref()
            .map(|s| s.eq_ignore_ascii_case("production"))
            .unwrap_or(false)
    };
    is_prod(&app_env) || is_prod(&node_env)
}

impl ElevationActor {
    pub fn new(
        kg_repo: Arc<dyn KnowledgeGraphRepository>,
        enrichment_repo: Arc<SqliteEnrichmentRepository>,
        speech: Option<Arc<SpeechService>>,
        consistency_base: Option<Arc<dyn OntologyRepository>>,
    ) -> Option<Self> {
        // REC-2 / ADR-130 Decision 2: the case queue only carries real cases if
        // this consumer runs, so default the gate ON in dev/staging while
        // keeping production opt-in. An explicit env value always wins.
        let enabled = match std::env::var("ELEVATION_ACTOR_ENABLED") {
            Ok(v) => v == "1" || v.eq_ignore_ascii_case("true"),
            Err(_) => !is_production_env(),
        };
        if !enabled {
            return None;
        }
        let forum_relay_url = std::env::var("FORUM_RELAY_URL").ok()?;
        let panel_secret = match crate::services::acsp::key_file::load_panel_secret() {
            Ok(Some(s)) => s.into_secret_hex(),
            Ok(None) => return None,
            Err(e) => {
                error!("[Elevation] panel signing key unusable, actor disabled: {e}");
                return None;
            }
        };
        Some(Self {
            kg_repo,
            enrichment_repo,
            consistency_base,
            acsp: None,
            speech,
            panel_secret,
            forum_relay_url,
            pending: HashMap::new(),
            elevating: HashMap::new(),
            seen: HashSet::new(),
            voice: VoiceDemandLedger::new(),
            concept_index: Arc::new(ConceptIndex::build(std::iter::empty())),
            elevated_count: 0,
            rejected_count: 0,
            voice_case_count: 0,
            last_pr_url: None,
        })
    }

    /// Speak a short confirmation into the immersive session via local PocketTts
    /// TTS. Fire-and-forget: voice feedback must never block case handling.
    fn speak(&self, text: String) {
        if let Some(speech) = self.speech.clone() {
            tokio::spawn(async move {
                if let Err(e) = speech.text_to_speech(text, SpeechOptions::default()).await {
                    warn!("[Elevation] TTS confirmation failed: {e}");
                }
            });
        }
    }

    fn panel_definition() -> PanelDefinition {
        PanelDefinition {
            title: "Knowledge Elevation".into(),
            description: "Frontier ontology concepts proposed for formalisation. \
                          Approve to commit a draft Class page to the corpus as a PR."
                .into(),
            version: "1.0.0".into(),
            schema: PanelSchema::ActionInbox,
            fields: vec![
                FieldDef {
                    name: "name".into(),
                    field_type: FieldType::String,
                    label: "Proposed class".into(),
                },
                FieldDef {
                    name: "domain".into(),
                    field_type: FieldType::String,
                    label: "Domain".into(),
                },
                FieldDef {
                    name: "referenced_by".into(),
                    field_type: FieldType::Json,
                    label: "Referencing classes".into(),
                },
                FieldDef {
                    name: "definition".into(),
                    field_type: FieldType::String,
                    label: "Draft definition".into(),
                },
                FieldDef {
                    name: "file_path".into(),
                    field_type: FieldType::String,
                    label: "Corpus path".into(),
                },
            ],
            actions: vec![
                ActionDef {
                    id: "approve".into(),
                    label: "Elevate".into(),
                    style: ActionStyle::Primary,
                },
                ActionDef {
                    id: "reject".into(),
                    label: "Skip".into(),
                    style: ActionStyle::Secondary,
                },
            ],
            layout: LayoutHint::InboxTable,
            capabilities: vec![],
            refresh_secs: 60,
        }
    }

    fn state_snapshot(&self, frontier_size: usize) -> serde_json::Value {
        json!({
            "frontier_size": frontier_size,
            "open_cases": self.pending.len(),
            "elevated": self.elevated_count,
            "rejected": self.rejected_count,
            "voice_cases": self.voice_case_count,
            "voice_guided": self.speech.is_some(),
            "last_pr_url": self.last_pr_url,
            // GOV-2/GOV-7 visibility: PRs awaiting merge, and whether the EL++
            // consistency gate is armed (false ⇒ approvals fail closed).
            "awaiting_merge": self.elevating.len(),
            "consistency_gate": self.consistency_base.is_some(),
        })
    }

    /// Open a broker case for a candidate (shared by the cycle path and the
    /// explicit voice-intent path). Returns the case spec to publish plus the
    /// pending record.
    fn case_for(
        c: &FrontierCandidate,
        voice: Option<&crate::actors::elevation_voice::VoiceDemand>,
        priority: ActionPriority,
    ) -> (CaseSpec, PendingCase) {
        let (file_path, draft) = draft_class_page(c);
        let name = canonical_name(&c.label);
        let case_id = format!("{CASE_PREFIX}{}", slugify(&name));
        let mut fields = json!({
            "name": name,
            "domain": c.domain,
            "referenced_by": c.referenced_by,
            "file_path": file_path.clone(),
            "degree": c.degree,
        });
        let reasoning = match voice {
            Some(v) => {
                fields["voice_mentions"] = json!(v.mentions);
                fields["voice_excerpts"] = json!(v.excerpts);
                if !v.speakers.is_empty() {
                    fields["voice_speakers"] = json!(v.speakers);
                }
                format!(
                    "Raised in conversation: {} mention(s) in recent voice sessions \
                     (latest: \"{}\"). Graph degree {}.",
                    v.mentions,
                    v.excerpts.last().cloned().unwrap_or_default(),
                    c.degree
                )
            }
            None => format!(
                "Frontier concept with {} axiom references — most-cited unauthored class in the current graph snapshot.",
                c.degree
            ),
        };
        let spec = CaseSpec {
            case_id,
            title: format!("Elevate: {name}"),
            priority,
            category: CaseCategory::KnowledgeEnrichment,
            subject_kind: SubjectKind::AutomationProposal,
            subject_id: crate::uri::ngm::class_iri(&slugify(&name)),
            request: ActionRequest {
                fields,
                reasoning: Some(reasoning),
                context_url: None,
            },
        };
        let pending = PendingCase {
            label: c.label.clone(),
            file_path,
            draft,
        };
        (spec, pending)
    }

    /// Build the durable `state=pending` projection row for a freshly opened
    /// case. The `proposal_json` carries the fields the WS-12 broker-inbox
    /// projection reads (`target_path`, `content`, `enrichment_type`,
    /// `reasoning_summary`, `proposed_by`) so the pending proposal renders in the
    /// inbox before any decision. `created_at`/`updated_at` are `0` here — the
    /// store stamps `unixepoch()` on write.
    fn pending_proposal(spec: &CaseSpec, pending: &PendingCase) -> StoredProposal {
        StoredProposal {
            case_id: spec.case_id.clone(),
            category: Some(spec.category.as_tag_value().to_string()),
            source_iri: Some(spec.subject_id.clone()),
            proposal_json: json!({
                "target_path": pending.file_path,
                "content": pending.draft,
                "enrichment_type": "class_elevation",
                "reasoning_summary": spec.request.reasoning,
                "proposed_by": format!("elevation-{}", slugify(&pending.label)),
                "title": spec.title,
            }),
            status: "pending".to_string(),
            created_at: 0,
            updated_at: 0,
        }
    }
}

/// One frontier candidate: an `owl_class` stub referenced by axioms but never
/// authored, ranked by graph degree.
#[derive(Debug, Clone)]
pub struct FrontierCandidate {
    pub label: String,
    pub degree: usize,
    pub domain: String,
    pub referenced_by: Vec<String>,
}

/// Pure candidate selection over a loaded graph snapshot — unit-testable
/// without actors or relays. Frontier = `owl_class` nodes with no
/// `source_file` metadata (nothing authored them). Degree ranks importance;
/// `referenced_by` carries up to 8 neighbouring labels for the case panel;
/// the domain is the majority `source_domain` among referencing neighbours.
pub fn select_frontier_candidates(
    graph: &visionclaw_domain::models::graph::GraphData,
    skip: &HashSet<String>,
    limit: usize,
) -> Vec<FrontierCandidate> {
    let mut degree: HashMap<u32, usize> = HashMap::new();
    let mut neighbours: HashMap<u32, Vec<u32>> = HashMap::new();
    for e in &graph.edges {
        *degree.entry(e.source).or_insert(0) += 1;
        *degree.entry(e.target).or_insert(0) += 1;
        neighbours.entry(e.source).or_default().push(e.target);
        neighbours.entry(e.target).or_default().push(e.source);
    }
    let by_id: HashMap<u32, &visionclaw_domain::models::node::Node> =
        graph.nodes.iter().map(|n| (n.id, n)).collect();

    let mut candidates: Vec<FrontierCandidate> = graph
        .nodes
        .iter()
        .filter(|n| n.node_type.as_deref() == Some("owl_class"))
        .filter(|n| !n.metadata.contains_key("source_file"))
        .filter(|n| !n.label.trim().is_empty())
        .filter(|n| !skip.contains(&n.label))
        .map(|n| {
            let nbrs = neighbours.get(&n.id).cloned().unwrap_or_default();
            let mut domains: HashMap<String, usize> = HashMap::new();
            let mut referenced_by: Vec<String> = Vec::new();
            for nb in nbrs.iter().take(64) {
                if let Some(node) = by_id.get(nb) {
                    if referenced_by.len() < 8 && !node.label.trim().is_empty() {
                        referenced_by.push(node.label.clone());
                    }
                    if let Some(d) = node.metadata.get("source_domain") {
                        *domains.entry(d.clone()).or_insert(0) += 1;
                    }
                }
            }
            let domain = domains
                .into_iter()
                .max_by_key(|(_, c)| *c)
                .map(|(d, _)| d)
                .unwrap_or_else(|| "infrastructure".into());
            FrontierCandidate {
                label: n.label.clone(),
                degree: degree.get(&n.id).copied().unwrap_or(0),
                domain,
                referenced_by,
            }
        })
        .collect();

    candidates.sort_by(|a, b| b.degree.cmp(&a.degree).then(a.label.cmp(&b.label)));
    candidates.truncate(limit);
    candidates
}

/// Canonical Title Case page name from a frontier label (the corpus
/// convention: descriptive Title Case filenames, e.g. "Fairness Auditing
/// Tools"). Slug derivation mirrors the server slugifier.
pub fn canonical_name(label: &str) -> String {
    label
        .split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = true;
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Draft a canonical Class page for a frontier concept.
///
/// **Frontmatter only** (PRD-sovereign-corpus Q4). The page carries the OKF
/// typed properties `vault validate` requires of a `knowledge/` page —
/// `type: Class`, `resource` (the minted `urn:ngm:class:` IRI), `status: draft`
/// so the quality layers treat an un-reviewed elevation honestly, and the
/// `generated` trust stamp — plus `is-a` / `related-to` as wikilink lists using
/// the keys `ontology/vocabulary.yaml` declares. A ```json-ld fence is a
/// `rejected_construct`: the two fences per page are exactly what this
/// migration folds into properties.
///
/// `public` is deliberately absent rather than `false`: `type: Class` admits the
/// page to the ontology on its own (§V4 inclusion), and a frontier concept
/// nobody has reviewed is not something to publish.
pub fn draft_class_page(c: &FrontierCandidate) -> (String, String) {
    let name = canonical_name(&c.label);
    let slug = slugify(&name);
    let definition = format!(
        "Draft elevation of the frontier concept '{}': referenced by {} graph relationships \
         (including {}) but not yet formally authored. Refine this definition during review.",
        name,
        c.degree,
        c.referenced_by
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );

    // `domain` and `maturity` are declared scalars; `draft` maturity is the
    // honest reading of a page nobody has authored yet. `definition` and
    // `source-domain` are NOT declared in the live vocabulary, so the draft
    // definition goes into the body as prose (vocabulary.yaml migration rule C)
    // — which is where a reviewer rewrites it anyway.
    let mut extra = std::collections::BTreeMap::new();
    extra.insert("domain".to_string(), c.domain.clone());
    extra.insert("maturity".to_string(), "draft".to_string());

    let mut extra_lists = std::collections::BTreeMap::new();
    // The concept is a frontier *because* the graph already points at it; the
    // referencing pages are the evidence, authored as the declared associative
    // relation. `is-a` stays empty — inventing a parent is what review is for.
    let referenced: Vec<String> = c
        .referenced_by
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .map(|r| format!("[[{r}]]"))
        .collect();
    if !referenced.is_empty() {
        extra_lists.insert("related-to".to_string(), referenced);
    }

    let meta = visionclaw_domain::vault::PageMeta {
        page_type: Some("Class".to_string()),
        resource: Some(crate::uri::ngm::class_iri(&slug)),
        status: Some("draft".to_string()),
        title: Some(name.clone()),
        generated: Some(visionclaw_domain::vault::GeneratedStamp {
            by: format!("process:visionclaw/{}", env!("CARGO_PKG_VERSION")),
            at: chrono::Utc::now().to_rfc3339(),
            rule: None,
        }),
        extra,
        extra_lists,
        ..Default::default()
    };

    let body = format!("# {name}\n\n**Definition:** {definition}\n");
    let content = visionclaw_domain::vault::render_page(&meta, &body);

    (
        format!(
            "{}/{name}.md",
            crate::services::decision_elevation::KNOWLEDGE_PAGES_DIR
        ),
        content,
    )
}

/// The canonical class IRI a class reference names, whatever form it is in:
/// wikilink text (a page title such as `Vehicle Part`, or a slug), a minted
/// `urn:ngm:class:<slug>`, or a published
/// `https://narrativegoldmine.com/class/<slug>`. Every form goes through the
/// mapping this actor mints draft IRIs with ([`slugify`] then
/// `crate::uri::ngm::class_iri`, as in [`draft_class_page`]); an IRI
/// contributes its local name through the shared normaliser
/// [`vault_core::consistency::class_key`].
///
/// This is a **comparison key**, not a resolver: a title's slug is not always
/// the slug the corpus stored (`VeChain` slugifies to `vechain`, the page is
/// `urn:ngm:class:ve-chain`). Draft text is resolved against the base first,
/// by [`BaseClassIndex::resolve`].
fn class_ref_iri(reference: &str) -> String {
    let r = reference.trim();
    let local = if r.contains("://") || r.starts_with("urn:") {
        vault_core::consistency::class_key(r)
    } else {
        r.to_string()
    };
    crate::uri::ngm::class_iri(&slugify(&local))
}

/// The base repository's classes, indexed by every name a draft wikilink can
/// use for one (ADR-2125): its page file stem (a wikilink's own target), its
/// `rdfs:label` and preferred term (the page title), all case-insensitively
/// as the ingest's `project_ontology` matches them, and its stored IRI's
/// local name ([`vault_core::consistency::class_key`]).
///
/// Minting a title with [`slugify`] does not reproduce the stored resource for
/// 430 of 9,380 real classes (`VeChain` → `vechain`, stored `ve-chain`;
/// `FigJam` → `figjam`, stored `fig-jam`), so a draft naming them would reason
/// about a fresh, parentless class instead of the real one, and the sibling
/// rule would see no parents. Resolving against the base fixes both.
#[derive(Debug, Default)]
struct BaseClassIndex {
    /// Lower-cased page file stem → stored IRI. Outranks a title: two pages
    /// may share a title (`Cryptographic Primitive.md` and
    /// `bc-cryptographic-primitive.md`), never a file.
    by_stem: HashMap<String, String>,
    /// Lower-cased label or preferred term → stored IRI.
    by_title: HashMap<String, String>,
    /// `class_key` of the stored IRI → stored IRI.
    by_key: HashMap<String, String>,
}

impl BaseClassIndex {
    /// Index `classes`. On a name two classes share within one tier, the
    /// first wins, as in the ingest.
    fn new(classes: &[OwlClass]) -> Self {
        fn claim(map: &mut HashMap<String, String>, name: &str, iri: &str) {
            let name = name.trim().to_lowercase();
            if !name.is_empty() {
                map.entry(name).or_insert_with(|| iri.to_string());
            }
        }
        let mut index = Self::default();
        for class in classes {
            let iri = class.iri.trim();
            if iri.is_empty() {
                continue;
            }
            index
                .by_key
                .entry(vault_core::consistency::class_key(iri))
                .or_insert_with(|| iri.to_string());
            if let Some(stem) = class
                .source_file
                .as_deref()
                .and_then(|f| std::path::Path::new(f).file_stem())
                .and_then(|s| s.to_str())
            {
                claim(&mut index.by_stem, stem, iri);
            }
            for title in [&class.label, &class.preferred_term].into_iter().flatten() {
                claim(&mut index.by_title, title, iri);
            }
        }
        index
    }

    /// The IRI a draft wikilink target names. An IRI is kept as written. Page
    /// text resolves to the stored class it names — by page file stem, then
    /// label or preferred term, then by its slug against the stored local
    /// names — and only a name the base does not hold is minted with
    /// [`class_ref_iri`].
    fn resolve(&self, target: &str) -> String {
        let t = target.trim();
        if t.contains("://") || t.starts_with("urn:") {
            return t.to_string();
        }
        let name = t.to_lowercase();
        self.by_stem
            .get(&name)
            .or_else(|| self.by_title.get(&name))
            .or_else(|| self.by_key.get(&slugify(t)))
            .cloned()
            .unwrap_or_else(|| class_ref_iri(t))
    }
}

/// A minimal `owl_class` declaration for the EL++ gate (IRI only — the reasoner
/// needs the term to exist; the rest of `OwlClass` is irrelevant to consistency).
fn declare_class(iri: &str) -> OwlClass {
    OwlClass {
        iri: iri.to_string(),
        ..Default::default()
    }
}

/// Parse a drafted Class page's **frontmatter** into the class + the OWL axioms
/// the EL++ engine checks (GOV-7).
///
/// The inverse of [`draft_class_page`] and the shape the corpus ingest reads
/// after PRD-sovereign-corpus Q4: `resource` is the class IRI, `is-a` is the
/// `rdfs:subClassOf` relation (`vault validate` rejects a json-ld fence, so
/// there is no longer a block to read), and `disjoint-with` — when a reviewer
/// has added one by hand — becomes a `DisjointWith` axiom. Relation targets are
/// wikilinks; the bracket text is the target IRI (kept as written) or a page
/// title or slug, minted to its class IRI ([`class_ref_iri`]). The gate parses
/// with [`parse_draft_axioms_against`] instead, which resolves page text to
/// the class the base repository stores first.
///
/// `disjoint-with` is held to the sibling-only rule `vault validate` applies
/// (ADR-2125, [`vault_core::consistency::check_disjoint_pair`]). Parsing cannot
/// apply it alone, because a target's parents live in the base ontology, so
/// [`run_consistency_gate`] applies it via [`check_draft_disjointness`] before
/// it reasons.
///
/// Axiom targets are also *declared* as classes so the reasoner can resolve
/// them. A page with no frontmatter, a non-`Class` `type`, or no `resource`
/// yields empty vecs — the caller treats "nothing drafted to check" honestly
/// (a draft with no relations is trivially consistent against the base).
pub fn parse_draft_axioms(draft: &str) -> (Vec<OwlClass>, Vec<OwlAxiom>) {
    parse_draft_axioms_against(draft, &BaseClassIndex::default())
}

/// [`parse_draft_axioms`], with every wikilink target resolved against the
/// base repository's classes ([`BaseClassIndex::resolve`]), so the axioms
/// name the stored IRI and the reasoner and the sibling rule see the real
/// class.
fn parse_draft_axioms_against(
    draft: &str,
    base: &BaseClassIndex,
) -> (Vec<OwlClass>, Vec<OwlAxiom>) {
    let meta = visionclaw_domain::vault::parse(draft);
    if meta.page_type.as_deref() != Some("Class") {
        return (Vec::new(), Vec::new());
    }
    let Some(id) = meta.resource.clone() else {
        return (Vec::new(), Vec::new());
    };

    let class = OwlClass {
        iri: id.clone(),
        label: meta.title.clone(),
        maturity: meta.extra.get("maturity").cloned(),
        source_domain: meta
            .extra
            .get("domain")
            .cloned()
            .or_else(|| meta.source_domain.clone()),
        ..Default::default()
    };

    let mut classes = vec![class];
    let mut axioms = Vec::new();

    let mk = |axiom_type: AxiomType, object: &str| OwlAxiom {
        id: None,
        axiom_type,
        subject: id.clone(),
        object: object.to_string(),
        annotations: HashMap::new(),
    };

    let targets = |key: &str| -> Vec<String> {
        meta.extra_lists
            .get(key)
            .map(|items| {
                items
                    .iter()
                    .map(|s| {
                        s.trim()
                            .trim_start_matches("[[")
                            .trim_end_matches("]]")
                            .trim()
                            .to_string()
                    })
                    .filter(|s| !s.is_empty())
                    .map(|s| base.resolve(&s))
                    .collect()
            })
            .unwrap_or_default()
    };

    for parent in targets("is-a") {
        axioms.push(mk(AxiomType::SubClassOf, &parent));
        classes.push(declare_class(&parent));
    }
    for other in targets("disjoint-with") {
        axioms.push(mk(AxiomType::DisjointWith, &other));
        classes.push(declare_class(&other));
    }
    (classes, axioms)
}

/// ADR-2125 item 1 on the elevation path: every `DisjointWith` axiom the draft
/// declares must pair two **siblings** — classes sharing a direct
/// `SubClassOf` parent across the base and the draft — and neither member may
/// be a domain root or taxonomy category. The rule is
/// [`vault_core::consistency::check_disjoint_pair`], the one `vault validate`
/// runs as `DISJOINT_NOT_SIBLINGS`, so the two write paths cannot diverge.
/// Disjointness the base already holds is not re-judged here.
fn check_draft_disjointness(
    base_axioms: &[OwlAxiom],
    draft_axioms: &[OwlAxiom],
) -> Result<(), String> {
    // Subjects compare through one slug→IRI mapping (`class_ref_iri`): the
    // base may store a class as a published IRI while the draft names it by
    // wikilink title or slug. Parents keep their written form; the sibling
    // rule compares them by `class_key`.
    let mut parents_by_class: HashMap<String, Vec<String>> = HashMap::new();
    for a in base_axioms.iter().chain(draft_axioms) {
        if a.axiom_type == AxiomType::SubClassOf {
            parents_by_class
                .entry(class_ref_iri(&a.subject))
                .or_default()
                .push(a.object.clone());
        }
    }
    let parents_of = |class: &str| -> Vec<String> {
        parents_by_class
            .get(&class_ref_iri(class))
            .cloned()
            .unwrap_or_default()
    };
    let refusals: Vec<String> = draft_axioms
        .iter()
        .filter(|a| a.axiom_type == AxiomType::DisjointWith)
        .filter_map(|a| {
            vault_core::consistency::check_disjoint_pair(
                &a.subject,
                &parents_of(&a.subject),
                &a.object,
                &parents_of(&a.object),
            )
            .err()
            .map(|b| b.to_string())
        })
        .collect();
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals.join("; "))
    }
}

/// The GOV-7 EL++ consistency gate: `Ok(())` to proceed to the PR, `Err(reason)`
/// to BLOCK the approval. Canon (no advisory write path): a `None` base source
/// means the gate is UNAVAILABLE and fails CLOSED. Otherwise:
///
/// 0. the draft is parsed against the base's classes, so a wikilink title
///    names the stored IRI ([`BaseClassIndex`]);
/// 1. the draft's own `disjoint-with` is held to the sibling-only rule
///    ([`check_draft_disjointness`], `[DISJOINT_NOT_SIBLINGS]`);
/// 2. the base ontology (classes + axioms) is combined with the drafted class
///    + axioms and whelk checks the union;
/// 3. an inconsistency blocks **only on the classes the draft makes
///    unsatisfiable** (ADR-2125 item 5, delta-scoped like the conflict gate),
///    each reported as `[WHELK_INCONSISTENT] <class> is subsumed by
///    owl:Nothing` — the blocker `vault propose` reports
///    ([`vault_core::consistency::whelk_inconsistent`]). Classes already
///    unsatisfiable in the base are logged, not blamed on the draft.
async fn run_consistency_gate(
    base_src: Option<Arc<dyn OntologyRepository>>,
    draft: &str,
) -> Result<(), String> {
    let Some(repo) = base_src else {
        return Err(
            "consistency gate unavailable (no ontology source configured for this profile)"
                .to_string(),
        );
    };
    let base_classes = repo.get_classes().await.unwrap_or_else(|e| {
        warn!("[Elevation] consistency gate: base classes load failed ({e:?}); checking draft in isolation");
        Vec::new()
    });
    let base_axioms = repo.get_axioms().await.unwrap_or_else(|e| {
        warn!("[Elevation] consistency gate: base axioms load failed ({e:?}); checking draft in isolation");
        Vec::new()
    });

    // Draft wikilinks name the classes the base stores, not fresh slugs.
    let (draft_classes, draft_axioms) =
        parse_draft_axioms_against(draft, &BaseClassIndex::new(&base_classes));

    check_draft_disjointness(&base_axioms, &draft_axioms)?;

    let mut classes = base_classes.clone();
    classes.extend(draft_classes);
    let mut axioms = base_axioms.clone();
    axioms.extend(draft_axioms);

    let outcome = WhelkInferenceEngine::check_axiom_set(&classes, &axioms);
    if outcome.consistent {
        return Ok(());
    }
    let before = WhelkInferenceEngine::check_axiom_set(&base_classes, &base_axioms);
    let (introduced, preexisting): (Vec<&String>, Vec<&String>) = outcome
        .unsatisfiable_classes
        .iter()
        .partition(|c| !before.unsatisfiable_classes.contains(c));
    if !preexisting.is_empty() {
        warn!(
            "[Elevation] consistency gate: {} class(es) were already unsatisfiable in the base \
             and are not blamed on this draft: {}",
            preexisting.len(),
            preexisting
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if introduced.is_empty() {
        return Ok(());
    }
    Err(introduced
        .iter()
        .map(|c| vault_core::consistency::whelk_inconsistent(c).to_string())
        .collect::<Vec<_>>()
        .join("; "))
}

/// GOV-2: map a terminal PR git state to `(31404 status, store status)`.
/// `Open` yields `None` (keep polling).
fn terminal_for_pr_state(state: PrState) -> Option<(&'static str, &'static str)> {
    match state {
        PrState::Merged => Some(("concept_elevated", "elevated")),
        PrState::ClosedUnmerged => Some(("elevation_abandoned", "abandoned")),
        PrState::Open => None,
    }
}

// ---------------------------------------------------------------------------
// FR4.5 — boot reconciliation (EXP-AC-004)
// ---------------------------------------------------------------------------
//
// The in-memory `pending` map is the working set, but it dies with the process.
// Before this, a kind-31403 arriving after a restart hit the `self.pending
// .remove(&d.case_id)` miss in the `Decision` handler and returned early — the
// human's signed decision was silently dropped, and the case sat `pending` for
// ever. Reconciliation rebuilds the map from the durable projection at boot, and
// closes out anything past `OPEN_CASE_TTL` with a receipt rather than leaving it
// to rot.

/// Durable proposal statuses that need no further work.
///
/// Anything else is an OPEN case: still answerable by a human, or old enough to
/// be timed out. Kept explicit (rather than "not pending") so a status this
/// actor has never seen is treated as open and surfaced, not silently dropped.
fn is_terminal_status(status: &str) -> bool {
    matches!(
        status,
        "approved" | "rejected" | "reviewed" | "elevated" | "abandoned" | "expired" | "applying"
    )
}

/// One open case recovered from the durable projection at boot.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RecoveredCase {
    case_id: String,
    /// Proposal-row creation instant (unix **seconds**) — the case's age clock.
    created_at_s: i64,
    /// The working-set shape the `Decision` handler needs to apply a decision.
    pending: PendingCase,
}

/// What boot reconciliation must do with one recovered open case.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ElevationReconcileAction {
    /// Within the TTL — re-arm it so a late kind-31403 is still matched.
    ResumePending(RecoveredCase),
    /// Unanswered past the TTL — close it out with a receipt.
    Expire(RecoveredCase),
}

/// Rehydrate an open elevation case from its durable row, or `None` when the row
/// is not a recoverable case of THIS panel.
///
/// Returns `None` for a case id outside [`CASE_PREFIX`] (another panel's row), a
/// terminal status (already closed), or a body carrying no draft. The last is
/// deliberate: an `approve` commits the stored draft to the corpus, so a row
/// without one cannot be honoured — and fabricating a replacement draft would
/// commit text no agent authored and no human reviewed.
fn recovered_case(p: &StoredProposal) -> Option<RecoveredCase> {
    if !p.case_id.starts_with(CASE_PREFIX) || is_terminal_status(&p.status) {
        return None;
    }
    let j = &p.proposal_json;
    let s = |k: &str| {
        j.get(k)
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
    };
    let draft = s("content")?;
    let file_path = s("target_path")?;
    // The label is recovered from the row that opened the case — never re-derived
    // from a fresh graph read, which may have moved on since.
    let label = s("title")
        .and_then(|t| t.strip_prefix("Elevate: ").map(str::to_string))
        .or_else(|| s("label"))
        .unwrap_or_else(|| p.case_id.trim_start_matches(CASE_PREFIX).to_string());
    Some(RecoveredCase {
        case_id: p.case_id.clone(),
        created_at_s: p.created_at,
        pending: PendingCase {
            label,
            file_path,
            draft,
        },
    })
}

/// Pure reconciliation policy: resume every open case inside the TTL, expire the
/// rest. Free of I/O so the TTL boundary is unit-testable without a relay, a
/// store or an actor system — the same posture as
/// `decision_elevation_actor::plan_reconciliation`.
///
/// The boundary is EXCLUSIVE: a case exactly at the TTL is still answerable.
fn plan_elevation_reconciliation(
    cases: Vec<RecoveredCase>,
    now_s: i64,
    ttl_s: i64,
) -> Vec<ElevationReconcileAction> {
    cases
        .into_iter()
        .map(|case| {
            if now_s.saturating_sub(case.created_at_s) > ttl_s {
                ElevationReconcileAction::Expire(case)
            } else {
                ElevationReconcileAction::ResumePending(case)
            }
        })
        .collect()
}

/// Split a plan into the working set to re-arm and the cases to expire.
fn split_reconciliation(
    plan: Vec<ElevationReconcileAction>,
) -> (HashMap<String, PendingCase>, Vec<RecoveredCase>) {
    let mut resumed = HashMap::new();
    let mut expired = Vec::new();
    for action in plan {
        match action {
            ElevationReconcileAction::ResumePending(c) => {
                resumed.insert(c.case_id, c.pending);
            }
            ElevationReconcileAction::Expire(c) => expired.push(c),
        }
    }
    (resumed, expired)
}

impl Actor for ElevationActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("[Elevation] actor starting (panel '{PANEL_ID}', case prefix '{CASE_PREFIX}')");
        let secret = self.panel_secret.clone();
        let relay = self.forum_relay_url.clone();
        let addr = ctx.address();

        // Connect the ACSP client, publish the panel definition, and start the
        // decision subscription. All async; results land back via messages.
        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                match AcspClient::connect(&secret, &relay).await {
                    Ok(client) => {
                        let client = Arc::new(client);
                        let def = build_panel_definition(PANEL_ID, &Self::panel_definition());
                        if let Err(e) = client.publish(&def).await {
                            warn!("[Elevation] panel definition publish failed: {e}");
                        }
                        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CaseDecision>();
                        let sub_client = client.clone();
                        tokio::spawn(async move {
                            sub_client
                                .run_decision_subscription(CASE_PREFIX.into(), tx)
                                .await;
                        });
                        let fwd_addr = addr.clone();
                        tokio::spawn(async move {
                            while let Some(d) = rx.recv().await {
                                fwd_addr.do_send(Decision(d));
                            }
                        });
                        Some(client)
                    }
                    Err(e) => {
                        error!("[Elevation] ACSP connect failed: {e}; actor idle");
                        None
                    }
                }
            })
            .map(|client, act, ctx| {
                act.acsp = client;
                if act.acsp.is_some() {
                    // FR4.5: rebuild the working set from the durable projection
                    // BEFORE opening any new case, so a decision that arrives
                    // after a restart still finds its case. Scheduling the cycle
                    // is the reconciliation's job — it runs after the recovery.
                    ctx.address().do_send(Reconcile);
                }
            }),
        );

        ctx.run_interval(CYCLE_INTERVAL, |_act, ctx| {
            ctx.address().do_send(RunCycle);
        });

        // GOV-2: poll opened elevation PRs for a terminal git state so a merge
        // fires `concept_elevated` (not "claimed at PR-creation"). Degraded-
        // visible: without a GitHub token no PR can open OR resolve, so say so
        // loudly at boot rather than silently never firing the terminal event.
        if GitHubPRService::has_github_token() {
            info!(
                "[Elevation] GOV-2 merge poll armed (every {}s) — merged PRs fire concept_elevated",
                PR_POLL_INTERVAL.as_secs()
            );
        } else {
            warn!("[Elevation] GOV-2 DEGRADED: no GitHub token (PRIVATE_REPO_GITHUB_PAT) — elevation PRs cannot be opened and merge polling cannot resolve; concept_elevated will never fire until a token is configured");
        }
        ctx.run_interval(PR_POLL_INTERVAL, |_act, ctx| {
            ctx.address().do_send(PollPrs);
        });

        // Voice guidance: forward every local-Whisper transcription line into
        // the actor. Conversation is the primary elevation signal; the stream
        // is fire-and-forget and lossy (broadcast lag is tolerated).
        if let Some(speech) = self.speech.clone() {
            let addr = ctx.address();
            tokio::spawn(async move {
                let mut rx = speech.subscribe_to_transcriptions();
                loop {
                    match rx.recv().await {
                        Ok(line) => addr.do_send(VoiceTranscript(line)),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!("[Elevation] transcription stream lagged ({n} lines skipped)");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            info!("[Elevation] voice guidance active (local Whisper STT → demand ledger; PocketTts TTS confirmations)");
        }
    }
}

impl Handler<VoiceTranscript> for ElevationActor {
    type Result = ();

    fn handle(&mut self, VoiceTranscript(line): VoiceTranscript, ctx: &mut Self::Context) {
        let now = std::time::Instant::now();

        // Explicit spoken command — jumps the queue entirely.
        if let Some(phrase) = parse_elevation_intent(&line) {
            let label = self
                .concept_index
                .lookup(&phrase)
                .map(str::to_string)
                .unwrap_or(phrase);
            if self.seen.contains(&label) || self.pending.values().any(|p| p.label == label) {
                self.speak(format!("{label} is already in elevation review."));
                return;
            }
            self.voice.note(&label, &line, "", now);
            let Some(acsp) = self.acsp.clone() else {
                return;
            };
            let candidate = FrontierCandidate {
                label: label.clone(),
                degree: 0,
                domain: "infrastructure".into(),
                referenced_by: Vec::new(),
            };
            let demand = self.voice.demand(&label).cloned();
            let (spec, pending) = Self::case_for(&candidate, demand.as_ref(), ActionPriority::High);
            let case_id = spec.case_id.clone();
            let proposal = Self::pending_proposal(&spec, &pending);
            let repo = self.enrichment_repo.clone();
            ctx.spawn(
                actix::fut::wrap_future::<_, Self>(async move {
                    let published =
                        acsp.publish(&build_action_request(&spec)).await.map(|_| ());
                    // Durable projection of the voice-commanded open case.
                    if published.is_ok() {
                        if let Err(e) = repo.create_or_update(&proposal).await {
                            warn!("[Elevation] voice pending-case persist failed: {e}");
                        }
                    }
                    published
                })
                .map(move |result, act, _ctx| match result {
                    Ok(()) => {
                        info!("[Elevation] voice-commanded case {case_id} for '{}'", pending.label);
                        act.voice_case_count += 1;
                        act.seen.insert(pending.label.clone());
                        let spoken = pending.label.clone();
                        act.pending.insert(case_id, pending);
                        act.speak(format!(
                            "Opened an elevation case for {spoken}. Review it on the governance page."
                        ));
                    }
                    Err(e) => warn!("[Elevation] voice case publish failed: {e}"),
                }),
            );
            return;
        }

        // Ambient mentions feed the demand ledger that ranks the next cycle.
        for label in harvest_mentions(&line, &self.concept_index) {
            self.voice.note(&label, &line, "", now);
        }
    }
}

impl Handler<RunCycle> for ElevationActor {
    type Result = ();

    fn handle(&mut self, _msg: RunCycle, ctx: &mut Self::Context) {
        let Some(acsp) = self.acsp.clone() else {
            return;
        };
        let now = std::time::Instant::now();
        self.voice.prune(now);
        if self.pending.len() >= MAX_OPEN_CASES {
            return;
        }
        let kg = self.kg_repo.clone();
        let repo = self.enrichment_repo.clone();
        let skip: HashSet<String> = self
            .seen
            .iter()
            .cloned()
            .chain(self.pending.values().map(|p| p.label.clone()))
            .collect();
        let budget = MAX_OPEN_CASES - self.pending.len();

        // Snapshot the conversational demand so the async block needs no
        // access to the ledger. Voice is the PRIMARY ranking signal; degree
        // breaks ties and carries the queue when nobody is talking.
        let voice_scores: HashMap<String, f32> = skip
            .iter()
            .map(|l| (l.clone(), 0.0))
            .chain(
                self.voice
                    .labels()
                    .map(|l| (l.to_string(), self.voice.score(l, now))),
            )
            .collect();
        let voice_demands: HashMap<String, crate::actors::elevation_voice::VoiceDemand> = self
            .voice
            .labels()
            .filter_map(|l| self.voice.demand(l).map(|d| (l.to_string(), d.clone())))
            .collect();

        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                let graph = match kg.load_graph().await {
                    Ok(g) => g,
                    Err(e) => {
                        warn!("[Elevation] load_graph failed: {e}");
                        return (Vec::new(), 0, Vec::new());
                    }
                };
                // Wide candidate pool, then voice-first ordering.
                let mut candidates = select_frontier_candidates(&graph, &skip, budget * 8);
                candidates.sort_by(|a, b| {
                    let va = voice_scores.get(&a.label).copied().unwrap_or(0.0);
                    let vb = voice_scores.get(&b.label).copied().unwrap_or(0.0);
                    vb.partial_cmp(&va)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(b.degree.cmp(&a.degree))
                        .then(a.label.cmp(&b.label))
                });
                candidates.truncate(budget);

                let frontier_size = graph
                    .nodes
                    .iter()
                    .filter(|n| n.node_type.as_deref() == Some("owl_class"))
                    .filter(|n| !n.metadata.contains_key("source_file"))
                    .count();

                // Refresh the concept index from this snapshot: frontier stubs
                // plus working pages are the vocabulary voice mentions match.
                let index_labels: Vec<String> = graph
                    .nodes
                    .iter()
                    .filter(|n| matches!(n.node_type.as_deref(), Some("owl_class") | Some("page")))
                    .map(|n| n.label.clone())
                    .collect();

                let mut opened: Vec<(String, PendingCase)> = Vec::new();
                for c in candidates {
                    let voice = voice_demands.get(&c.label);
                    let priority = if voice.is_some() {
                        ActionPriority::High
                    } else {
                        ActionPriority::Medium
                    };
                    let (spec, pending) = ElevationActor::case_for(&c, voice, priority);
                    let case_id = spec.case_id.clone();
                    match acsp.publish(&build_action_request(&spec)).await {
                        Ok(_) => {
                            // Durable projection: persist the open case as
                            // state=pending so /api/broker/inbox shows it BEFORE
                            // any decision. Failure is logged loudly, never fatal
                            // to the working set.
                            let proposal = ElevationActor::pending_proposal(&spec, &pending);
                            if let Err(e) = repo.create_or_update(&proposal).await {
                                warn!("[Elevation] pending-case persist failed for {case_id}: {e}");
                            }
                            info!(
                                "[Elevation] opened case {case_id} for '{}' (voice={})",
                                c.label,
                                voice.is_some()
                            );
                            opened.push((case_id, pending));
                        }
                        Err(e) => warn!("[Elevation] case publish failed for '{}': {e}", c.label),
                    }
                }
                (opened, frontier_size, index_labels)
            })
            .map(|(opened, frontier_size, index_labels), act, ctx| {
                if !index_labels.is_empty() {
                    act.concept_index =
                        Arc::new(ConceptIndex::build(index_labels.iter().map(String::as_str)));
                }
                for (case_id, pending) in opened {
                    act.seen.insert(pending.label.clone());
                    act.pending.insert(case_id, pending);
                }
                act.publish_state(ctx, frontier_size);
            }),
        );
    }
}

impl Handler<Reconcile> for ElevationActor {
    type Result = ();

    /// FR4.5 (EXP-AC-004): re-arm the working set from durable `pending` rows,
    /// and time out anything past [`OPEN_CASE_TTL`] with an `expired` receipt.
    ///
    /// Mirrors `decision_elevation_actor`'s reconciliation: the durable status
    /// write is NOT conditional on the 31404 receipt publishing — a relay that
    /// is down must not leave the case permanently un-aged. Only the panel's own
    /// cases are touched; another panel's rows are left alone.
    fn handle(&mut self, _msg: Reconcile, ctx: &mut Self::Context) {
        let repo = self.enrichment_repo.clone();
        let acsp = self.acsp.clone();
        let now_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let ttl_s = OPEN_CASE_TTL.as_secs() as i64;

        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                let rows = match repo.list(Some("pending"), RECONCILE_SCAN_LIMIT, 0).await {
                    Ok(rows) => rows,
                    Err(e) => {
                        warn!("[Elevation] boot reconciliation read failed: {e}; working set stays cold");
                        return HashMap::new();
                    }
                };
                let recovered: Vec<RecoveredCase> = rows.iter().filter_map(recovered_case).collect();
                let (resumed, expired) = split_reconciliation(plan_elevation_reconciliation(
                    recovered, now_s, ttl_s,
                ));

                for case in &expired {
                    // The receipt first (so the forum shows WHY the case
                    // vanished), then the terminal durable status regardless.
                    if let Some(acsp) = acsp.as_ref() {
                        if let Err(e) = acsp
                            .publish(&build_case_status_update(
                                PANEL_ID,
                                &case.case_id,
                                EXPIRED_STATUS,
                                &format!(
                                    "unanswered for more than {} days",
                                    ttl_s / (24 * 60 * 60)
                                ),
                            ))
                            .await
                        {
                            warn!(
                                "[Elevation] expiry receipt publish failed for {}: {e}",
                                case.case_id
                            );
                        }
                    }
                    if let Err(e) = repo.set_status(&case.case_id, EXPIRED_STORE_STATUS).await {
                        warn!(
                            "[Elevation] expiry status persist failed for {}: {e}",
                            case.case_id
                        );
                    }
                }
                info!(
                    "[Elevation] boot reconciliation: {} case(s) re-armed, {} timed out",
                    resumed.len(),
                    expired.len()
                );
                resumed
            })
            .map(|resumed, act, ctx| {
                for (case_id, pending) in resumed {
                    act.seen.insert(pending.label.clone());
                    act.pending.insert(case_id, pending);
                }
                // Only now open new cases — the window budget must count the
                // recovered ones.
                ctx.address().do_send(RunCycle);
            }),
        );
    }
}

impl Handler<Decision> for ElevationActor {
    type Result = ();

    fn handle(&mut self, Decision(d): Decision, ctx: &mut Self::Context) {
        let Some(case) = self.pending.remove(&d.case_id) else {
            return; // replayed/foreign decision
        };
        info!(
            "[Elevation] case {} decided '{}' by {} — {}",
            d.case_id, d.action, d.responder_pubkey, d.reasoning
        );

        // The durable decision record is written PER BRANCH so it reflects the
        // true terminal outcome: a `reject` records the human decision here; an
        // `approve` defers its record into the gate future — the human `approve`
        // iff the EL++ consistency gate passes, or a gate-reject (carrying the
        // inconsistency/unavailability reason) iff it blocks. No advisory pass.
        match d.action.as_str() {
            "approve" => {
                // GOV-7: consistency gate then (GOV-2) PR tracking.
                self.approve_with_gate(ctx, d, case);
            }
            _ => {
                // reject / amend / delegate — record the human decision now (the
                // pending row transitions to its decided status atomically) and
                // skip. `writeback_committed` stays false (no PR, no write).
                let repo = self.enrichment_repo.clone();
                let stored = decision_record(&d);
                let case_id = d.case_id.clone();
                ctx.spawn(
                    actix::fut::wrap_future::<_, Self>(async move {
                        if let Err(e) = repo.record_decision(&stored).await {
                            warn!(
                                "[Elevation] decision reconcile persist failed for {case_id}: {e}"
                            );
                        }
                    })
                    .map(|_, _, _| ()),
                );
                self.rejected_count += 1;
                self.publish_state(ctx, 0);
                ctx.address().do_send(RunCycle);
            }
        }
    }
}

/// Result of the gated approve future, carried to the actor-context `.map`.
enum ApproveOutcome {
    /// Gate passed, PR opened — carries the PR url for tracking (GOV-2).
    Elevated(String),
    /// Gate passed but the GitHub PR call failed (already logged).
    PrFailed,
    /// Gate BLOCKED the approval (inconsistent draft or gate unavailable).
    Blocked,
}

impl ElevationActor {
    /// The GOV-7-gated approve path. Runs the EL++ consistency gate over
    /// base-ontology ∪ draft BEFORE opening the PR; on a consistent draft it
    /// records the human approve and opens the PR, then (GOV-2) tracks that PR to
    /// its terminal state; on an inconsistent draft OR an unavailable gate it
    /// records a gate-reject with the reason, blocks the PR, and counts the case
    /// rejected. Canon: no advisory write path — the gate fails closed.
    fn approve_with_gate(&mut self, ctx: &mut Context<Self>, d: CaseDecision, case: PendingCase) {
        let base_src = self.consistency_base.clone();
        let repo = self.enrichment_repo.clone();
        let approve_record = decision_record(&d);
        let case_id = d.case_id.clone();
        let case_id_map = case_id.clone();
        let label = case.label.clone();
        let label_map = label.clone();
        let file_path = case.file_path.clone();
        let draft = case.draft.clone();

        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                match run_consistency_gate(base_src, &draft).await {
                    Err(reason) => {
                        warn!(
                            "[Elevation] GOV-7 consistency gate BLOCKED approval of {case_id}: {reason}"
                        );
                        // Record a gate-reject (with the reason) instead of the approve,
                        // so the store shows the case was NOT elevated and why.
                        // A locally minted decision: it answers no signed 31403,
                        // so it carries no event id (ADR-2006) and falls back to
                        // the local correlation form in `decision_record`.
                        //
                        // FR5.4 / DDD invariant 8 (EXP-AC-005): the gate is NOT
                        // the human who opened the case. Attributing its
                        // rejection to `responder` counted a reasoner outcome as
                        // a human override — inflating HITL Precision and
                        // scattering Trust Variance with dispersion no human
                        // produced. The decider is the reserved non-DID actor
                        // `system:whelk-gate`, which the KPI compute excludes
                        // from every human-reviewer series.
                        let synthetic = CaseDecision {
                            case_id: case_id.clone(),
                            action: "reject".to_string(),
                            reasoning: format!("GOV-7 consistency gate blocked elevation: {reason}"),
                            responder_pubkey: SYSTEM_WHELK_GATE.to_string(),
                            event_id: String::new(),
                            created_at: 0,
                        };
                        if let Err(e) = repo.record_decision(&decision_record(&synthetic)).await {
                            warn!("[Elevation] gate-reject persist failed for {case_id}: {e}");
                        }
                        ApproveOutcome::Blocked
                    }
                    Ok(()) => {
                        // Gate passed: record the human approve, then open the PR.
                        if let Err(e) = repo.record_decision(&approve_record).await {
                            warn!("[Elevation] approve reconcile persist failed for {case_id}: {e}");
                        }
                        let pr = GitHubPRService::new();
                        let agent_ctx = AgentContext {
                            agent_id: format!("elevation-{}", slugify(&label)),
                            agent_type: "elevation".into(),
                            task_description: format!(
                                "ACSP-approved elevation of frontier concept '{label}'"
                            ),
                            session_id: None,
                            // FR2.4 (EXP-AC-002): no model produced a confidence
                            // for this elevation, so it has none. `0.5` was a
                            // fabricated self-assessment the UI then rendered as
                            // the agent's own. Absence renders as absence.
                            confidence: None,
                            user_id: "acsp-governance".into(),
                        };
                        match pr
                            .create_ontology_pr(
                                &file_path,
                                &draft,
                                &format!("feat(ontology): elevate '{label}' (draft)"),
                                &format!(
                                    "Draft Class page for frontier concept **{label}**, approved \
                                     via the forum governance panel (ACSP case) and cleared by the \
                                     EL++ consistency gate. Definition is a draft — refine during \
                                     PR review.\n\n🤖 Generated by Claude Code"
                                ),
                                &agent_ctx,
                            )
                            .await
                        {
                            Ok(url) => ApproveOutcome::Elevated(url),
                            Err(e) => {
                                error!("[Elevation] PR creation failed: {e}");
                                ApproveOutcome::PrFailed
                            }
                        }
                    }
                }
            })
            .map(move |outcome, act, ctx| {
                match outcome {
                    ApproveOutcome::Elevated(url) => {
                        act.elevated_count += 1;
                        act.last_pr_url = Some(url.clone());
                        // GOV-2: track the opened PR — the merge poll fires the
                        // terminal `concept_elevated`, not this PR-creation moment.
                        act.elevating.insert(
                            case_id_map.clone(),
                            TrackedPr {
                                pr_url: url.clone(),
                                label: label_map,
                            },
                        );
                        info!("[Elevation] PR created: {url} (tracking case {case_id_map} for merge → concept_elevated)");
                    }
                    ApproveOutcome::PrFailed => {}
                    ApproveOutcome::Blocked => {
                        // GOV-7: a blocked approval is a rejection, not a silent pass.
                        act.rejected_count += 1;
                    }
                }
                act.publish_state(ctx, 0);
                // Refill the case window.
                ctx.address().do_send(RunCycle);
            }),
        );
    }
}

impl Handler<PollPrs> for ElevationActor {
    type Result = ();

    fn handle(&mut self, _msg: PollPrs, ctx: &mut Self::Context) {
        if self.elevating.is_empty() {
            return;
        }
        // Degraded-visible each cycle while PRs are stuck untrackable.
        if !GitHubPRService::has_github_token() {
            warn!(
                "[Elevation] GOV-2 merge poll DEGRADED: {} PR(s) tracked but no GitHub token; concept_elevated cannot fire until PRIVATE_REPO_GITHUB_PAT is configured",
                self.elevating.len()
            );
            return;
        }
        let Some(acsp) = self.acsp.clone() else {
            return;
        };
        let repo = self.enrichment_repo.clone();
        let tracked: Vec<(String, TrackedPr)> = self
            .elevating
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let pr = GitHubPRService::new();

        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                let mut resolved: Vec<String> = Vec::new();
                for (case_id, tracked) in tracked {
                    match pr.pr_state(&tracked.pr_url).await {
                        Ok(state) => {
                            let Some((event_status, store_status)) = terminal_for_pr_state(state)
                            else {
                                continue; // still open — keep tracking
                            };
                            // Publish the terminal 31404 CaseStatusUpdate.
                            if let Err(e) = acsp
                                .publish(&build_case_status_update(
                                    PANEL_ID,
                                    &case_id,
                                    event_status,
                                    &tracked.pr_url,
                                ))
                                .await
                            {
                                warn!("[Elevation] GOV-2 31404 publish failed for {case_id}: {e}");
                            }
                            // Mark the store row terminal.
                            if let Err(e) = repo.set_status(&case_id, store_status).await {
                                warn!(
                                    "[Elevation] GOV-2 terminal store status persist failed for {case_id}: {e}"
                                );
                            }
                            info!(
                                "[Elevation] case {case_id} terminal: {event_status} for '{}' ({})",
                                tracked.label, tracked.pr_url
                            );
                            resolved.push(case_id);
                        }
                        Err(e) => {
                            warn!("[Elevation] GOV-2 PR state poll failed for {case_id}: {e}");
                        }
                    }
                }
                resolved
            })
            .map(|resolved, act, _ctx| {
                for case_id in resolved {
                    act.elevating.remove(&case_id);
                }
            }),
        );
    }
}

impl ElevationActor {
    fn publish_state(&self, ctx: &mut Context<Self>, frontier_size: usize) {
        let Some(acsp) = self.acsp.clone() else {
            return;
        };
        let state = self.state_snapshot(frontier_size);
        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                if let Err(e) = acsp.publish(&build_panel_state(PANEL_ID, &state)).await {
                    warn!("[Elevation] panel state publish failed: {e}");
                }
            })
            .map(|_, _, _| ()),
        );
    }
}

/// Build the durable [`StoredDecision`] reconciliation record from a forum
/// [`CaseDecision`] (kind-31403). The responding admin's pubkey attributes the
/// decision when it is a canonical x-only hex key; a non-hex key downgrades to
/// unattributed (never an error). `writeback_committed` stays `false`: the
/// elevation "commit" is the merged ontology PR (tracked separately), not the
/// enrichment-decide Oxigraph `:summary` write.
fn decision_record(d: &CaseDecision) -> StoredDecision {
    let attributed = crate::uri::is_pubkey_hex(&d.responder_pubkey);
    let owner_did = if attributed {
        crate::uri::did_nostr(&d.responder_pubkey).ok()
    } else {
        None
    };
    let proposal_urn = if attributed {
        crate::uri::kg(
            &d.responder_pubkey,
            format!("enrichment-proposal:{}", d.case_id),
        )
        .ok()
    } else {
        None
    };
    // ADR-2006 — correlate on the SIGNED event id, not on the tuple.
    //
    // `(case_id, action, responder_pubkey)` is not unique: a replayed 31403, or
    // an admin who answers the same case the same way twice, produced an
    // identical activity URN, so the second decision overwrote the first in the
    // provenance graph and the two became indistinguishable. The signed event
    // id is unique per decision by construction, so it is what the record
    // correlates on. A decision carrying no event id (a synthetic gate-reject
    // minted locally) falls back to the tuple plus its decision timestamp,
    // which is still unique per occurrence.
    let correlation = if d.event_id.is_empty() {
        format!(
            "elevation-decide:{}:{}:{}:local",
            d.case_id, d.action, d.responder_pubkey
        )
    } else {
        format!("elevation-decide:{}:{}", d.case_id, d.event_id)
    };
    let activity_urn = crate::uri::execution(&correlation);
    let writeback_triggered =
        crate::adapters::sqlite_enrichment_repository::status_for_outcome(&d.action) == "approved";
    let decided_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    StoredDecision {
        case_id: d.case_id.clone(),
        outcome: d.action.clone(),
        attributed,
        broker_pubkey: Some(d.responder_pubkey.clone()),
        reasoning: Some(d.reasoning.clone()),
        writeback_triggered,
        writeback_committed: false,
        activity_urn,
        proposal_urn,
        owner_did,
        decided_at_ms,
        // ADR-2006 — the signed-event correlation, persisted alongside the
        // record so a restart can tell a re-delivered decision from a new one.
        decision_event_id: (!d.event_id.is_empty()).then(|| d.event_id.clone()),
        decision_created_at_s: (d.created_at > 0).then_some(d.created_at as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_domain::models::edge::Edge;
    use visionclaw_domain::models::graph::GraphData;
    use visionclaw_domain::models::node::Node;

    // -----------------------------------------------------------------
    // FR4.5 (EXP-AC-004): the actor recovers its open cases across a restart,
    // so a kind-31403 arriving after a restart is APPLIED rather than dropped
    // at the in-memory map miss, and an unanswered case expires with a receipt.
    // -----------------------------------------------------------------

    fn stored_pending(case_id: &str, created_at_s: i64, status: &str) -> StoredProposal {
        StoredProposal {
            case_id: case_id.to_string(),
            category: Some("knowledge_enrichment".into()),
            source_iri: Some("urn:ngm:class:finality-mechanism".into()),
            proposal_json: json!({
                "target_path": "pages/Finality Mechanism.md",
                "content": "# Finality Mechanism\n\ndraft body",
                "enrichment_type": "class_elevation",
                "proposed_by": "elevation-finality-mechanism",
                "title": "Elevate: Finality Mechanism",
            }),
            status: status.to_string(),
            created_at: created_at_s,
            updated_at: created_at_s,
        }
    }

    const DAY_S: i64 = 24 * 60 * 60;

    #[test]
    fn ttl_is_fourteen_days_matching_the_decision_elevation_actor() {
        assert_eq!(OPEN_CASE_TTL.as_secs() as i64, 14 * DAY_S);
    }

    #[test]
    fn a_pending_row_rehydrates_into_the_working_set() {
        let recovered = recovered_case(&stored_pending(
            "vc-elev-finality-mechanism",
            100,
            "pending",
        ))
        .expect("a pending elevation row rehydrates");
        assert_eq!(recovered.case_id, "vc-elev-finality-mechanism");
        assert_eq!(recovered.pending.file_path, "pages/Finality Mechanism.md");
        assert!(recovered.pending.draft.contains("draft body"));
        // The label is recovered from the durable row, never re-derived from a
        // fresh graph read (the graph may have moved on since the case opened).
        assert_eq!(recovered.pending.label, "Finality Mechanism");
        assert_eq!(recovered.created_at_s, 100);
    }

    #[test]
    fn a_row_from_another_panel_is_not_ours_to_recover() {
        assert!(recovered_case(&stored_pending("vc-decelev-other", 100, "pending")).is_none());
    }

    #[test]
    fn a_row_with_no_draft_cannot_be_rehydrated() {
        let mut row = stored_pending("vc-elev-x", 100, "pending");
        row.proposal_json = json!({ "target_path": "pages/x.md" });
        assert!(
            recovered_case(&row).is_none(),
            "an approve would have no draft to commit; never fabricate one"
        );
    }

    #[test]
    fn reconciliation_resumes_a_case_inside_the_ttl_and_expires_one_outside_it() {
        let now = 100 * DAY_S;
        let fresh =
            recovered_case(&stored_pending("vc-elev-fresh", now - DAY_S, "pending")).unwrap();
        let stale = recovered_case(&stored_pending(
            "vc-elev-stale",
            now - 15 * DAY_S,
            "pending",
        ))
        .unwrap();
        let plan =
            plan_elevation_reconciliation(vec![fresh, stale], now, OPEN_CASE_TTL.as_secs() as i64);
        assert_eq!(plan.len(), 2);
        assert!(
            matches!(&plan[0], ElevationReconcileAction::ResumePending(c) if c.case_id == "vc-elev-fresh")
        );
        assert!(
            matches!(&plan[1], ElevationReconcileAction::Expire(c) if c.case_id == "vc-elev-stale")
        );
    }

    #[test]
    fn the_ttl_boundary_is_exclusive() {
        let now = 100 * DAY_S;
        let ttl = OPEN_CASE_TTL.as_secs() as i64;
        let exactly =
            recovered_case(&stored_pending("vc-elev-edge", now - ttl, "pending")).unwrap();
        let plan = plan_elevation_reconciliation(vec![exactly], now, ttl);
        assert!(
            matches!(&plan[0], ElevationReconcileAction::ResumePending(_)),
            "a case exactly at the TTL is still answerable"
        );
    }

    #[test]
    fn terminal_rows_are_never_reconciled() {
        let now = 100 * DAY_S;
        let cases: Vec<RecoveredCase> =
            ["approved", "rejected", "elevated", "abandoned", "expired"]
                .iter()
                .filter_map(|st| {
                    recovered_case(&stored_pending("vc-elev-done", now - 30 * DAY_S, st))
                })
                .collect();
        assert!(cases.is_empty(), "a decided case is not an open case");
        assert!(
            plan_elevation_reconciliation(cases, now, OPEN_CASE_TTL.as_secs() as i64).is_empty()
        );
    }

    #[test]
    fn a_decision_arriving_after_a_restart_finds_its_case() {
        // The regression EXP-AC-004 names: "a post-restart 31403 returning early
        // at the in-memory map miss". Before reconciliation the map is empty and
        // the decision is dropped; after it, the same decision resolves.
        let now = 100 * DAY_S;
        let case_id = "vc-elev-finality-mechanism";
        let recovered = recovered_case(&stored_pending(case_id, now - DAY_S, "pending")).unwrap();

        let mut cold: HashMap<String, PendingCase> = HashMap::new();
        assert!(
            cold.remove(case_id).is_none(),
            "cold start drops the decision"
        );

        let plan =
            plan_elevation_reconciliation(vec![recovered], now, OPEN_CASE_TTL.as_secs() as i64);
        let (restored, expired) = split_reconciliation(plan);
        assert!(expired.is_empty());
        cold.extend(restored);

        let matched = cold.remove(case_id).expect("the late decision now applies");
        assert_eq!(matched.file_path, "pages/Finality Mechanism.md");
    }

    #[test]
    fn production_gate_defaults_dev_on_prod_off() {
        // Unset env → non-production → ElevationActor defaults ON.
        assert!(!is_production_from(None, None));
        // Either signal at `production` (any case) → production → opt-in only.
        assert!(is_production_from(Some("production".into()), None));
        assert!(is_production_from(None, Some("Production".into())));
        assert!(is_production_from(
            Some("PRODUCTION".into()),
            Some("development".into())
        ));
        // Dev/staging values are not production.
        assert!(!is_production_from(
            Some("development".into()),
            Some("development".into())
        ));
        assert!(!is_production_from(Some("staging".into()), None));
    }

    fn node(id: u32, label: &str, node_type: &str, authored: bool, domain: Option<&str>) -> Node {
        let mut n = Node::default();
        n.id = id;
        n.label = label.into();
        n.metadata_id = slugify(label);
        n.node_type = Some(node_type.into());
        if authored {
            n.metadata
                .insert("source_file".into(), format!("{label}.md"));
        }
        if let Some(d) = domain {
            n.metadata.insert("source_domain".into(), d.into());
        }
        n
    }

    fn edge(source: u32, target: u32) -> Edge {
        Edge {
            id: format!("{source}_{target}"),
            source,
            target,
            weight: 1.0,
            edge_type: None,
            metadata: None,
            owl_property_iri: None,
        }
    }

    fn frontier_graph() -> GraphData {
        let mut g = GraphData::new();
        g.nodes = vec![
            node(1, "finality mechanism", "owl_class", false, None),
            node(2, "search space definition", "owl_class", false, None),
            node(3, "Authored Class", "owl_class", true, Some("blockchain")),
            node(
                4,
                "Consensus Layer",
                "ontology_node",
                true,
                Some("blockchain"),
            ),
            node(5, "Some Page", "page", true, Some("infrastructure")),
        ];
        // 'finality mechanism' degree 3 (hub), 'search space definition' degree 1.
        g.edges = vec![edge(4, 1), edge(3, 1), edge(5, 1), edge(4, 2)];
        g
    }

    #[test]
    fn frontier_selection_ranks_by_degree_and_skips_authored() {
        let g = frontier_graph();
        let picked = select_frontier_candidates(&g, &HashSet::new(), 10);
        assert_eq!(picked.len(), 2, "only unauthored owl_class stubs qualify");
        assert_eq!(picked[0].label, "finality mechanism");
        assert_eq!(picked[0].degree, 3);
        assert_eq!(picked[0].domain, "blockchain");
        assert!(picked[0]
            .referenced_by
            .contains(&"Consensus Layer".to_string()));
    }

    #[test]
    fn frontier_selection_honours_skip_list_and_limit() {
        let g = frontier_graph();
        let mut skip = HashSet::new();
        skip.insert("finality mechanism".to_string());
        let picked = select_frontier_candidates(&g, &skip, 10);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].label, "search space definition");

        let limited = select_frontier_candidates(&g, &HashSet::new(), 1);
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn canonical_name_title_cases() {
        assert_eq!(canonical_name("finality mechanism"), "Finality Mechanism");
        assert_eq!(
            canonical_name("ar content positioning"),
            "Ar Content Positioning"
        );
    }

    #[test]
    fn draft_page_has_canonical_identity_and_draft_maturity() {
        let c = FrontierCandidate {
            label: "finality mechanism".into(),
            degree: 7,
            domain: "blockchain".into(),
            referenced_by: vec![
                "Consensus Layer".into(),
                "Bitcoin Proof-of-Work Protocol".into(),
            ],
        };
        let (path, content) = draft_class_page(&c);
        // PRD-sovereign-corpus Q1: the governed vault role, not the retired
        // `mainKnowledgeGraph/` layout.
        assert_eq!(path, "knowledge/pages/Finality Mechanism.md");

        // Q4: frontmatter only — a fence is a `rejected_construct`, and
        // Invariant 1 forbids `key:: value` lines.
        assert!(content.starts_with("---\n"));
        assert!(!content.contains("```"), "no fence of any kind: {content}");
        assert!(!content.contains(":: "), "no Logseq property lines");

        let meta = visionclaw_domain::vault::parse(&content);
        assert_eq!(meta.page_type.as_deref(), Some("Class"));
        assert_eq!(
            meta.resource.as_deref(),
            Some("urn:ngm:class:finality-mechanism")
        );
        assert_eq!(meta.status.as_deref(), Some("draft"));
        assert_eq!(meta.title.as_deref(), Some("Finality Mechanism"));
        assert_eq!(
            meta.extra.get("maturity").map(String::as_str),
            Some("draft")
        );
        assert_eq!(
            meta.extra.get("domain").map(String::as_str),
            Some("blockchain")
        );
        // The OKF trust stamp names the process that drafted the page.
        let stamp = meta.generated.as_ref().expect("generated stamp");
        assert!(stamp.by.starts_with("process:visionclaw/"));
        assert!(!stamp.at.is_empty());

        // Relations are wikilink LISTS under a vocabulary-declared key — two
        // referencing pages must stay two edges, not one comma-joined target.
        assert_eq!(
            meta.extra_lists.get("related-to"),
            Some(&vec![
                "[[Consensus Layer]]".to_string(),
                "[[Bitcoin Proof-of-Work Protocol]]".to_string(),
            ])
        );

        // An unreviewed frontier concept is admitted to the ontology by its
        // `type`, and is NOT published.
        assert!(!meta.is_publishable());
    }

    #[test]
    fn slugify_matches_corpus_convention() {
        assert_eq!(slugify("Finality Mechanism"), "finality-mechanism");
        assert_eq!(slugify("3D and 4D"), "3d-and-4d");
    }

    // ── GOV-7 consistency gate ──────────────────────────────────────────────

    fn draft_with(sub_class_of: &[&str], disjoint_with: &[&str]) -> String {
        let links = |v: &[&str]| -> Vec<String> { v.iter().map(|s| format!("[[{s}]]")).collect() };
        let mut extra = std::collections::BTreeMap::new();
        extra.insert("maturity".to_string(), "draft".to_string());
        let mut extra_lists = std::collections::BTreeMap::new();
        if !sub_class_of.is_empty() {
            extra_lists.insert("is-a".to_string(), links(sub_class_of));
        }
        if !disjoint_with.is_empty() {
            extra_lists.insert("disjoint-with".to_string(), links(disjoint_with));
        }
        let meta = visionclaw_domain::vault::PageMeta {
            page_type: Some("Class".to_string()),
            resource: Some("urn:ngm:class:test".to_string()),
            status: Some("draft".to_string()),
            title: Some("Test".to_string()),
            extra,
            extra_lists,
            ..Default::default()
        };
        visionclaw_domain::vault::render_page(&meta, "# Test\n")
    }

    #[test]
    fn parse_draft_axioms_reads_class_and_subclass() {
        // A real elevation draft (subClassOf: []) parses to its class, no axioms.
        let c = FrontierCandidate {
            label: "finality mechanism".into(),
            degree: 3,
            domain: "blockchain".into(),
            referenced_by: vec!["Consensus Layer".into()],
        };
        let (_, draft) = draft_class_page(&c);
        let (classes, axioms) = parse_draft_axioms(&draft);
        assert_eq!(classes.len(), 1, "the drafted class is declared");
        assert_eq!(classes[0].iri, "urn:ngm:class:finality-mechanism");
        assert!(
            axioms.is_empty(),
            "template draft has no subclass/disjoint relations"
        );

        // A draft with a subClassOf yields a SubClassOf axiom.
        let (_, axioms) = parse_draft_axioms(&draft_with(&["urn:test:A"], &[]));
        assert_eq!(axioms.len(), 1);
        assert_eq!(axioms[0].axiom_type, AxiomType::SubClassOf);
        assert_eq!(axioms[0].subject, "urn:ngm:class:test");
        assert_eq!(axioms[0].object, "urn:test:A");
    }

    /// GOV-7: a drafted class that is a subclass of two base-ontology classes
    /// declared disjoint is INCONSISTENT — the gate must surface it (blocks).
    #[test]
    fn gate_blocks_draft_inconsistent_with_base() {
        // Base ontology: A and B are disjoint.
        let base_classes = vec![
            OwlClass {
                iri: "urn:test:A".into(),
                ..Default::default()
            },
            OwlClass {
                iri: "urn:test:B".into(),
                ..Default::default()
            },
        ];
        let base_axioms = vec![OwlAxiom {
            id: None,
            axiom_type: AxiomType::DisjointWith,
            subject: "urn:test:A".into(),
            object: "urn:test:B".into(),
            annotations: HashMap::new(),
        }];
        // Draft: test ⊑ A and test ⊑ B → test collapses to owl:Nothing.
        let (draft_classes, draft_axioms) =
            parse_draft_axioms(&draft_with(&["urn:test:A", "urn:test:B"], &[]));

        let mut classes = base_classes.clone();
        classes.extend(draft_classes.clone());
        let mut axioms = base_axioms.clone();
        axioms.extend(draft_axioms.clone());
        let outcome = WhelkInferenceEngine::check_axiom_set(&classes, &axioms);
        assert!(
            !outcome.consistent,
            "disjoint parents ⇒ inconsistent: {outcome:?}"
        );

        // The SAME draft against a base WITHOUT the disjointness is consistent.
        let mut classes2 = base_classes;
        classes2.extend(draft_classes);
        let outcome2 = WhelkInferenceEngine::check_axiom_set(&classes2, &draft_axioms);
        assert!(
            outcome2.consistent,
            "no disjointness ⇒ consistent: {outcome2:?}"
        );
    }

    /// GOV-7 fail-closed: no base source ⇒ the gate is UNAVAILABLE and blocks
    /// the approval (returns Err) rather than passing it through advisorily.
    #[tokio::test]
    async fn gate_unavailable_fails_closed() {
        let draft = &draft_with(&[], &[]);
        let res = run_consistency_gate(None, draft).await;
        assert!(res.is_err(), "None source must fail closed");
        assert!(res.unwrap_err().contains("unavailable"));
    }

    // ── ADR-2125 item 3: the probe through the real gate, not check_axiom_set ──

    /// A base repository holding siblings `A`, `B` under `Parent`, optionally
    /// declared disjoint — the fixture `run_consistency_gate` loads.
    async fn probe_base(disjoint: bool) -> Arc<dyn OntologyRepository> {
        let class = |iri: &str| OwlClass {
            iri: iri.into(),
            ..Default::default()
        };
        let axiom = |axiom_type: AxiomType, subject: &str, object: &str| OwlAxiom {
            id: None,
            axiom_type,
            subject: subject.into(),
            object: object.into(),
            annotations: HashMap::new(),
        };
        let mut axioms = vec![
            axiom(AxiomType::SubClassOf, "urn:test:A", "urn:test:Parent"),
            axiom(AxiomType::SubClassOf, "urn:test:B", "urn:test:Parent"),
        ];
        if disjoint {
            axioms.push(axiom(AxiomType::DisjointWith, "urn:test:A", "urn:test:B"));
        }
        let repo = crate::test_helpers::MockOntologyRepository::new();
        repo.save_ontology(
            &[
                class("urn:test:Parent"),
                class("urn:test:A"),
                class("urn:test:B"),
            ],
            &[],
            &axioms,
        )
        .await
        .expect("the mock repository accepts the base");
        Arc::new(repo)
    }

    /// `P ⊑ A`, `P ⊑ B`, `A disjoint-with B` through `run_consistency_gate`
    /// must block with the `WHELK_INCONSISTENT` code naming P — the same code
    /// `vault propose` reports (ADR-2125 item 2). A probe that passes cleanly
    /// fails CI.
    #[tokio::test]
    async fn probe_gate_reports_whelk_inconsistent_for_disjoint_parents() {
        let base = probe_base(true).await;
        let draft = &draft_with(&["urn:test:A", "urn:test:B"], &[]);
        let err = run_consistency_gate(Some(base), draft)
            .await
            .expect_err("P ⊑ A, P ⊑ B, A disjoint-with B must block");
        assert!(
            err.contains("WHELK_INCONSISTENT"),
            "the gate must carry the WHELK_INCONSISTENT code: {err}"
        );
        assert!(
            err.contains("urn:ngm:class:test"),
            "the blocker must name the unsatisfiable class: {err}"
        );
    }

    /// The control: the same draft against the same base without the
    /// disjointness passes the gate.
    #[tokio::test]
    async fn probe_control_gate_passes_without_disjointness() {
        let base = probe_base(false).await;
        let draft = &draft_with(&["urn:test:A", "urn:test:B"], &[]);
        let res = run_consistency_gate(Some(base), draft).await;
        assert!(res.is_ok(), "no disjointness ⇒ consistent: {res:?}");
    }

    /// ADR-2125 item 1 on this path: a draft declaring `disjoint-with` against
    /// a class it shares no direct parent with is refused before reasoning,
    /// with the code `vault validate` uses.
    #[tokio::test]
    async fn gate_refuses_a_non_sibling_disjointness_in_the_draft() {
        let base = probe_base(false).await;
        // test ⊑ A; B is under Parent, not A, so test and B are not siblings.
        let draft = &draft_with(&["urn:test:A"], &["urn:test:B"]);
        let err = run_consistency_gate(Some(base), draft)
            .await
            .expect_err("non-siblings must be refused");
        assert!(err.contains("DISJOINT_NOT_SIBLINGS"), "{err}");
    }

    /// A sibling disjointness in the draft (test and B both under Parent) is
    /// admitted, and consistent.
    #[tokio::test]
    async fn gate_admits_a_sibling_disjointness_in_the_draft() {
        let base = probe_base(false).await;
        let draft = &draft_with(&["urn:test:Parent"], &["urn:test:B"]);
        let res = run_consistency_gate(Some(base), draft).await;
        assert!(res.is_ok(), "siblings under Parent: {res:?}");
    }

    /// Domain roots and taxonomy categories are never disjointness members,
    /// even when they would be siblings.
    #[tokio::test]
    async fn gate_refuses_disjointness_with_a_domain_root() {
        let base = probe_base(false).await;
        let draft = &draft_with(&["urn:test:Parent"], &["urn:ngm:class:robotics"]);
        let err = run_consistency_gate(Some(base), draft)
            .await
            .expect_err("a domain root is never a disjointness member");
        assert!(err.contains("DISJOINT_NOT_SIBLINGS"), "{err}");
        assert!(err.contains("domain root or taxonomy category"), "{err}");
    }

    /// A base storing full class IRIs (published or `urn:ngm:class:` form)
    /// and a draft naming the same classes by wikilink title or slug: the
    /// sibling rule must see the target's parents through one slug→IRI
    /// mapping, not by exact string equality.
    fn iri_base(parent_of_b: &str) -> Vec<OwlAxiom> {
        let axiom = |axiom_type: AxiomType, subject: &str, object: &str| OwlAxiom {
            id: None,
            axiom_type,
            subject: subject.into(),
            object: object.into(),
            annotations: HashMap::new(),
        };
        vec![
            axiom(
                AxiomType::SubClassOf,
                "https://narrativegoldmine.com/class/wheel-hub",
                "https://narrativegoldmine.com/class/vehicle-part",
            ),
            axiom(
                AxiomType::SubClassOf,
                "urn:ngm:class:brake-disc",
                parent_of_b,
            ),
        ]
    }

    #[test]
    fn sibling_rule_resolves_wikilink_titles_against_a_base_storing_iris() {
        // Draft: is-a [[Vehicle Part]], disjoint-with [[Wheel Hub]] and
        // [[brake-disc]]; all three are siblings under vehicle-part.
        let base = iri_base("https://narrativegoldmine.com/class/vehicle-part");
        let (_, da) =
            parse_draft_axioms(&draft_with(&["Vehicle Part"], &["Wheel Hub", "brake-disc"]));
        assert_eq!(
            check_draft_disjointness(&base, &da),
            Ok(()),
            "siblings named by title must be recognised as siblings"
        );
    }

    #[test]
    fn sibling_rule_still_refuses_non_siblings_named_by_title() {
        // brake-disc sits under a different parent: not a sibling.
        let base = iri_base("urn:ngm:class:friction-component");
        let (_, da) = parse_draft_axioms(&draft_with(&["Vehicle Part"], &["Brake Disc"]));
        let err = check_draft_disjointness(&base, &da).expect_err("not siblings");
        assert!(err.contains("DISJOINT_NOT_SIBLINGS"), "{err}");
    }

    #[test]
    fn draft_wikilink_titles_become_class_iris() {
        let (classes, axioms) = parse_draft_axioms(&draft_with(&["Vehicle Part"], &["Wheel Hub"]));
        assert_eq!(axioms[0].object, "urn:ngm:class:vehicle-part");
        assert_eq!(axioms[1].object, "urn:ngm:class:wheel-hub");
        assert!(classes.iter().any(|c| c.iri == "urn:ngm:class:wheel-hub"));
        // An IRI target is kept as written.
        let (_, axioms) = parse_draft_axioms(&draft_with(
            &["https://narrativegoldmine.com/class/vehicle-part"],
            &[],
        ));
        assert_eq!(
            axioms[0].object,
            "https://narrativegoldmine.com/class/vehicle-part"
        );
    }

    // ── ADR-2125: draft titles resolve against the base's stored classes ────

    /// A base class as the ingest stores it: label and preferred term are
    /// the page title, `source_file` the page path, `iri` the `resource`.
    fn stored(iri: &str, title: &str) -> OwlClass {
        OwlClass {
            iri: iri.into(),
            label: Some(title.into()),
            preferred_term: Some(title.into()),
            source_file: Some(format!("knowledge/pages/{title}.md")),
            ..Default::default()
        }
    }

    #[test]
    fn titles_whose_slug_differs_from_the_stored_resource_resolve_to_it() {
        // Two of the 430 real classes `slugify(title)` does not reproduce.
        assert_eq!(slugify("VeChain"), "vechain");
        let index = BaseClassIndex::new(&[
            stored("urn:ngm:class:ve-chain", "VeChain"),
            stored("urn:ngm:class:fig-jam", "FigJam"),
            OwlClass {
                iri: "urn:ngm:class:l2-rollup".into(),
                source_file: Some("knowledge/pages/Layer2 Rollup.md".into()),
                ..Default::default()
            },
        ]);
        assert_eq!(index.resolve("VeChain"), "urn:ngm:class:ve-chain");
        assert_eq!(
            index.resolve("vechain"),
            "urn:ngm:class:ve-chain",
            "case-insensitive"
        );
        assert_eq!(index.resolve("FigJam"), "urn:ngm:class:fig-jam");
        assert_eq!(index.resolve("fig-jam"), "urn:ngm:class:fig-jam", "by slug");
        assert_eq!(
            index.resolve("Layer2 Rollup"),
            "urn:ngm:class:l2-rollup",
            "by file stem"
        );
        // A shared title: the page whose *file* is named so wins, whatever
        // the order; the other is reached by its own file stem.
        let shared = BaseClassIndex::new(&[
            OwlClass {
                source_file: Some("knowledge/pages/bc-cryptographic-primitive.md".into()),
                ..stored(
                    "urn:ngm:class:bc-cryptographic-primitive",
                    "Cryptographic Primitive",
                )
            },
            stored(
                "urn:ngm:class:cryptographic-primitive",
                "Cryptographic Primitive",
            ),
        ]);
        assert_eq!(
            shared.resolve("Cryptographic Primitive"),
            "urn:ngm:class:cryptographic-primitive"
        );
        assert_eq!(
            shared.resolve("bc-cryptographic-primitive"),
            "urn:ngm:class:bc-cryptographic-primitive"
        );
        // A name the base does not hold is minted as before; an IRI is kept.
        assert_eq!(
            index.resolve("Brand New Thing"),
            "urn:ngm:class:brand-new-thing"
        );
        assert_eq!(
            index.resolve("urn:ngm:class:VeChain"),
            "urn:ngm:class:VeChain"
        );
        // The draft's axioms name the stored IRI.
        let (classes, axioms) =
            parse_draft_axioms_against(&draft_with(&["VeChain"], &["FigJam"]), &index);
        assert_eq!(axioms[0].object, "urn:ngm:class:ve-chain");
        assert_eq!(axioms[1].object, "urn:ngm:class:fig-jam");
        assert!(classes.iter().all(|c| c.iri != "urn:ngm:class:vechain"));
    }

    async fn vechain_base(extra: &[OwlAxiom]) -> Arc<dyn OntologyRepository> {
        let repo = crate::test_helpers::MockOntologyRepository::new();
        let axiom = |axiom_type: AxiomType, subject: &str, object: &str| OwlAxiom {
            id: None,
            axiom_type,
            subject: subject.into(),
            object: object.into(),
            annotations: HashMap::new(),
        };
        let mut axioms = vec![
            axiom(
                AxiomType::SubClassOf,
                "urn:ngm:class:ve-chain",
                "urn:ngm:class:ledger",
            ),
            axiom(
                AxiomType::SubClassOf,
                "urn:ngm:class:fig-jam",
                "urn:ngm:class:ledger",
            ),
        ];
        axioms.extend_from_slice(extra);
        repo.save_ontology(
            &[
                stored("urn:ngm:class:ve-chain", "VeChain"),
                stored("urn:ngm:class:fig-jam", "FigJam"),
                stored("urn:ngm:class:ledger", "Ledger"),
                stored("urn:ngm:class:whiteboard", "Whiteboard"),
            ],
            &[],
            &axioms,
        )
        .await
        .expect("the mock repository accepts the base");
        Arc::new(repo)
    }

    /// The sibling rule sees `VeChain`'s stored parents. Minted as
    /// `urn:ngm:class:vechain` it had none, and a true sibling pair was
    /// refused as `DISJOINT_NOT_SIBLINGS`.
    #[tokio::test]
    async fn sibling_rule_resolves_titles_whose_slug_differs_from_the_resource() {
        let draft = &draft_with(&["Ledger"], &["VeChain"]);
        let res = run_consistency_gate(Some(vechain_base(&[]).await), draft).await;
        assert!(
            res.is_ok(),
            "the draft and VeChain are siblings under Ledger: {res:?}"
        );
    }

    /// The Whelk axioms name the stored class too. `is-a [[VeChain]]` plus
    /// `is-a [[Whiteboard]]`, with `Ledger` disjoint from `Whiteboard`, is
    /// unsatisfiable — but only if `VeChain` is the stored `ve-chain ⊑ Ledger`,
    /// not a fresh parentless `vechain`.
    #[tokio::test]
    async fn whelk_gate_reasons_about_the_stored_class_a_title_names() {
        let disjoint = OwlAxiom {
            id: None,
            axiom_type: AxiomType::DisjointWith,
            subject: "urn:ngm:class:ledger".into(),
            object: "urn:ngm:class:whiteboard".into(),
            annotations: HashMap::new(),
        };
        let draft = &draft_with(&["VeChain", "Whiteboard"], &[]);
        let err = run_consistency_gate(Some(vechain_base(&[disjoint]).await), draft)
            .await
            .expect_err("draft ⊑ ve-chain ⊑ Ledger and ⊑ Whiteboard, disjoint");
        assert!(err.contains("WHELK_INCONSISTENT"), "{err}");
        assert!(err.contains("urn:ngm:class:test"), "{err}");
    }

    /// ADR-2125 item 5: delta-scoped. A base that is already inconsistent
    /// (X ⊑ A, X ⊑ B with A, B disjoint) does not block an unrelated draft.
    #[tokio::test]
    async fn gate_does_not_blame_a_draft_for_preexisting_unsatisfiability() {
        let repo = crate::test_helpers::MockOntologyRepository::new();
        let class = |iri: &str| OwlClass {
            iri: iri.into(),
            ..Default::default()
        };
        let axiom = |axiom_type: AxiomType, subject: &str, object: &str| OwlAxiom {
            id: None,
            axiom_type,
            subject: subject.into(),
            object: object.into(),
            annotations: HashMap::new(),
        };
        repo.save_ontology(
            &[
                class("urn:test:A"),
                class("urn:test:B"),
                class("urn:test:X"),
            ],
            &[],
            &[
                axiom(AxiomType::DisjointWith, "urn:test:A", "urn:test:B"),
                axiom(AxiomType::SubClassOf, "urn:test:X", "urn:test:A"),
                axiom(AxiomType::SubClassOf, "urn:test:X", "urn:test:B"),
            ],
        )
        .await
        .expect("the mock repository accepts the base");
        let base: Arc<dyn OntologyRepository> = Arc::new(repo);
        let draft = &draft_with(&["urn:test:A"], &[]);
        let res = run_consistency_gate(Some(base), draft).await;
        assert!(res.is_ok(), "X was unsatisfiable before the draft: {res:?}");
    }

    // ── GOV-2 terminal PR-state mapping ─────────────────────────────────────

    #[test]
    fn terminal_for_pr_state_maps_merge_and_abandon() {
        assert_eq!(
            terminal_for_pr_state(PrState::Merged),
            Some(("concept_elevated", "elevated"))
        );
        assert_eq!(
            terminal_for_pr_state(PrState::ClosedUnmerged),
            Some(("elevation_abandoned", "abandoned"))
        );
        assert_eq!(
            terminal_for_pr_state(PrState::Open),
            None,
            "open ⇒ keep polling"
        );
    }
}
