//! Agent Control Surface Protocol (ACSP) wire types and event builders.
//!
//! Serde-exact mirror of the consumer structs in
//! `nostr-rust-forum/crates/nostr-bbs-core/src/governance.rs`: content keys are
//! **snake_case**, enum values are **kebab-case** (panel vocabulary) or
//! **snake_case** (priority labels), and every event is NIP-33
//! parameterised-replaceable addressed by a non-empty `["d", id]` first tag.
//! An unknown enum value or camelCase key fails the consumer parse and the
//! panel silently never renders — the round-trip tests below lock the shapes.
//!
//! Builders emit **unsigned** events (`kind`, `created_at`, `tags`, `content`);
//! the [`super::client::AcspClient`] signs and publishes them. Reference
//! producer: agentbox `mcp/servers/nostr-bridge.js`; schema doc:
//! `docs/agentbox-docs/developer/agent-control-surface-panels.md`.

use serde::{Deserialize, Serialize};

// Single source of truth: the governance kinds are defined by `nostr-bbs-core`
// (`u64` there; ACSP builders use `u16`, so each is narrowed in a const
// context, which the compiler checks cannot truncate for 31400..=31405).
use nostr_bbs_core::governance as gov;

pub const KIND_PANEL_DEFINITION: u16 = gov::KIND_PANEL_DEFINITION as u16;
pub const KIND_PANEL_STATE: u16 = gov::KIND_PANEL_STATE as u16;
pub const KIND_ACTION_REQUEST: u16 = gov::KIND_ACTION_REQUEST as u16;
pub const KIND_ACTION_RESPONSE: u16 = gov::KIND_ACTION_RESPONSE as u16;
pub const KIND_PANEL_UPDATE: u16 = gov::KIND_PANEL_UPDATE as u16;
pub const KIND_PANEL_RETIRED: u16 = gov::KIND_PANEL_RETIRED as u16;

// ── Panel vocabulary (kebab-case on the wire) ───────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PanelSchema {
    ActionInbox,
    Dashboard,
    ConfigForm,
    StatusBoard,
    ChatBridge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PanelCapability {
    BulkAction,
    Filter,
    Search,
    Sort,
    Export,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FieldType {
    String,
    Int,
    Float,
    Bool,
    Json,
    Enum,
    Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldDef {
    pub name: String,
    pub field_type: FieldType,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ActionStyle {
    Primary,
    Secondary,
    Destructive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionDef {
    pub id: String,
    pub label: String,
    pub style: ActionStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutHint {
    InboxTable,
    Kanban,
    CardGrid,
    SplitDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PanelDefinition {
    pub title: String,
    pub description: String,
    pub version: String,
    pub schema: PanelSchema,
    pub fields: Vec<FieldDef>,
    pub actions: Vec<ActionDef>,
    pub layout: LayoutHint,
    pub capabilities: Vec<PanelCapability>,
    pub refresh_secs: u32,
}

// ── Action request / response content ───────────────────────────────────────

/// Case priority. Travels as a **tag label** (`["priority", "high"]`), never
/// in content (consumer rejects it there).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionPriority {
    Critical,
    High,
    Medium,
    Low,
}

impl ActionPriority {
    pub fn as_tag_value(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// Kind-31402 content. `reasoning`/`context_url` serialise as explicit `null`
/// when absent (the reference builder does the same).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionRequest {
    pub fields: serde_json::Value,
    pub reasoning: Option<String>,
    pub context_url: Option<String>,
}

/// Kind-31403 content — published only by human admins via the forum UI;
/// agents parse it when subscribing for answers. `action` is `approve`,
/// `reject`, or another `DecisionOutcome` discriminant (`amend`, `delegate`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionResponse {
    pub action: String,
    pub reasoning: String,
}

// ── Broker-case projection tags (31402) ─────────────────────────────────────

/// Relay-side `CaseCategory` (snake_case tag values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseCategory {
    ContributorMeshShare,
    WorkflowReview,
    PolicyException,
    TrustAlert,
    ManualSubmission,
    KnowledgeEnrichment,
    /// ADR-050: elevate a significant decision record into the public corpus so
    /// the `force_full` assert-graph rebuild re-derives it (the inverse of
    /// `KnowledgeEnrichment` for `dl:DecisionRecord` instances). Carries no
    /// share-state ladder, like `KnowledgeEnrichment`.
    DecisionElevation,
}

impl CaseCategory {
    pub fn as_tag_value(self) -> &'static str {
        match self {
            Self::ContributorMeshShare => "contributor_mesh_share",
            Self::WorkflowReview => "workflow_review",
            Self::PolicyException => "policy_exception",
            Self::TrustAlert => "trust_alert",
            Self::ManualSubmission => "manual_submission",
            Self::KnowledgeEnrichment => "knowledge_enrichment",
            Self::DecisionElevation => "decision_elevation",
        }
    }
}

/// Relay-side `SubjectKind` (snake_case tag values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectKind {
    WorkArtifact,
    SkillPackage,
    AutomationProposal,
    PolicyException,
    Opaque,
}

impl SubjectKind {
    pub fn as_tag_value(self) -> &'static str {
        match self {
            Self::WorkArtifact => "work_artifact",
            Self::SkillPackage => "skill_package",
            Self::AutomationProposal => "automation_proposal",
            Self::PolicyException => "policy_exception",
            Self::Opaque => "opaque",
        }
    }
}

// ── Unsigned event builders ─────────────────────────────────────────────────

/// An unsigned ACSP event: kind + tags + content, ready for the client to
/// sign. The `["d", id]` tag is always first per the protocol invariant.
#[derive(Debug, Clone, PartialEq)]
pub struct UnsignedAcspEvent {
    pub kind: u16,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

/// Build a kind-31400 PanelDefinition event.
pub fn build_panel_definition(panel_id: &str, def: &PanelDefinition) -> UnsignedAcspEvent {
    UnsignedAcspEvent {
        kind: KIND_PANEL_DEFINITION,
        tags: vec![vec!["d".into(), panel_id.into()]],
        content: serde_json::to_string(def).expect("PanelDefinition serialises"),
    }
}

/// Build a kind-31401 full PanelState snapshot.
pub fn build_panel_state(panel_id: &str, state: &serde_json::Value) -> UnsignedAcspEvent {
    UnsignedAcspEvent {
        kind: KIND_PANEL_STATE,
        tags: vec![vec!["d".into(), panel_id.into()]],
        content: state.to_string(),
    }
}

/// Build a kind-31404 incremental state diff (shallow-merged by the consumer).
pub fn build_panel_update(panel_id: &str, diff: &serde_json::Value) -> UnsignedAcspEvent {
    UnsignedAcspEvent {
        kind: KIND_PANEL_UPDATE,
        tags: vec![vec!["d".into(), panel_id.into()]],
        content: diff.to_string(),
    }
}

/// Build a kind-31404 **CaseStatusUpdate** — a terminal broker-case lifecycle
/// transition (GOV-2 / ADR-130 Decision 2). Emitted when an elevation PR reaches
/// a terminal git state: `status = "concept_elevated"` on merge, or
/// `"elevation_abandoned"` on close-without-merge. The content is the shallow-
/// merge object the consumer folds into the panel's last 31401 snapshot (the
/// 31404 PanelUpdate contract: top-level object, `["d", panel_id]`), carrying
/// `{case_id, status, pr_url}` so the governance panel shows which case reached
/// which terminal state and links its PR. The `d` tag is the **panel id** (not
/// the case id) because the consumer keys 31404 merges by panel, per the schema
/// doc; the case is identified inside the content.
pub fn build_case_status_update(
    panel_id: &str,
    case_id: &str,
    status: &str,
    pr_url: &str,
) -> UnsignedAcspEvent {
    let content = serde_json::json!({
        "case_id": case_id,
        "status": status,
        "pr_url": pr_url,
    });
    UnsignedAcspEvent {
        kind: KIND_PANEL_UPDATE,
        tags: vec![vec!["d".into(), panel_id.into()]],
        content: content.to_string(),
    }
}

/// Build a kind-31405 panel retirement.
pub fn build_panel_retired(panel_id: &str) -> UnsignedAcspEvent {
    UnsignedAcspEvent {
        kind: KIND_PANEL_RETIRED,
        tags: vec![vec!["d".into(), panel_id.into()]],
        content: "{}".into(),
    }
}

/// Everything a kind-31402 broker case needs. The d-tag is the case id and
/// becomes `broker_cases.id`; title/category/subject/priority are projected
/// into the governance inbox without content parsing.
#[derive(Debug, Clone)]
pub struct CaseSpec {
    pub case_id: String,
    pub title: String,
    pub priority: ActionPriority,
    pub category: CaseCategory,
    pub subject_kind: SubjectKind,
    pub subject_id: String,
    pub request: ActionRequest,
}

/// Build a kind-31403 ActionResponse (decision) event for `case_id`.
///
/// Normally a human admin publishes 31403 from the forum UI; VisionClaw uses
/// this to **project** an operator/bridge REST decision back to the forum so the
/// decision is visible in the forum's `broker_decisions` (ADR-130 Decision 2 /
/// gap-close item 2). The d-tag is the case id (the 31402 it answers); content
/// is the `{action, reasoning}` shape the consumer and [`ActionResponse`] parse.
pub fn build_action_response(case_id: &str, action: &str, reasoning: &str) -> UnsignedAcspEvent {
    let content = ActionResponse {
        action: action.to_string(),
        reasoning: reasoning.to_string(),
    };
    UnsignedAcspEvent {
        kind: KIND_ACTION_RESPONSE,
        tags: vec![vec!["d".into(), case_id.into()]],
        content: serde_json::to_string(&content).expect("ActionResponse serialises"),
    }
}

/// Build a kind-31402 ActionRequest (broker case) event.
pub fn build_action_request(spec: &CaseSpec) -> UnsignedAcspEvent {
    UnsignedAcspEvent {
        kind: KIND_ACTION_REQUEST,
        tags: vec![
            vec!["d".into(), spec.case_id.clone()],
            vec!["priority".into(), spec.priority.as_tag_value().into()],
            vec!["category".into(), spec.category.as_tag_value().into()],
            vec![
                "subject-kind".into(),
                spec.subject_kind.as_tag_value().into(),
            ],
            vec!["subject-id".into(), spec.subject_id.clone()],
            vec!["title".into(), spec.title.clone()],
        ],
        content: serde_json::to_string(&spec.request).expect("ActionRequest serialises"),
    }
}

/// Extract the first value of a named tag.
pub fn extract_tag<'a>(tags: &'a [Vec<String>], name: &str) -> Option<&'a str> {
    tags.iter()
        .find(|t| t.first().map(String::as_str) == Some(name))
        .and_then(|t| t.get(1))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The governance kinds are a wire contract with the forum and agentbox:
    /// fail loudly if the re-exported values ever drift from 31400-31405.
    #[test]
    fn governance_kinds_are_31400_to_31405() {
        assert_eq!(
            [
                KIND_PANEL_DEFINITION,
                KIND_PANEL_STATE,
                KIND_ACTION_REQUEST,
                KIND_ACTION_RESPONSE,
                KIND_PANEL_UPDATE,
                KIND_PANEL_RETIRED,
            ],
            [31400u16, 31401, 31402, 31403, 31404, 31405]
        );
    }

    /// The example panel from the architecture doc must serialise with
    /// kebab-case enums and snake_case keys — the exact strings the consumer
    /// strict-serde expects.
    #[test]
    fn panel_definition_wire_shape_matches_consumer() {
        let def = PanelDefinition {
            title: "VisionClaw Graph Health".into(),
            description: "Physics convergence and sync status".into(),
            version: "1.0.0".into(),
            schema: PanelSchema::StatusBoard,
            fields: vec![FieldDef {
                name: "iteration".into(),
                field_type: FieldType::Int,
                label: "Physics iteration".into(),
            }],
            actions: vec![ActionDef {
                id: "force-resync".into(),
                label: "Force resync".into(),
                style: ActionStyle::Destructive,
            }],
            layout: LayoutHint::CardGrid,
            capabilities: vec![PanelCapability::Filter],
            refresh_secs: 30,
        };
        let json = serde_json::to_value(&def).unwrap();
        assert_eq!(json["schema"], "status-board");
        assert_eq!(json["layout"], "card-grid");
        assert_eq!(json["capabilities"][0], "filter");
        assert_eq!(json["fields"][0]["field_type"], "int");
        assert_eq!(json["actions"][0]["style"], "destructive");
        assert_eq!(json["refresh_secs"], 30);
        // Round-trip survives.
        let back: PanelDefinition = serde_json::from_value(json).unwrap();
        assert_eq!(back, def);
    }

    /// Content/tag split for 31402: priority is a tag, never content; the
    /// content struct emits explicit nulls for absent optionals.
    #[test]
    fn action_request_split_matches_doc_example() {
        let spec = CaseSpec {
            case_id: "case-42".into(),
            title: "Review artifact".into(),
            priority: ActionPriority::High,
            category: CaseCategory::WorkflowReview,
            subject_kind: SubjectKind::WorkArtifact,
            subject_id: "art-1".into(),
            request: ActionRequest {
                fields: serde_json::json!({"entity": "urn:agentbox:bead:abc"}),
                reasoning: Some("needs human sign-off".into()),
                context_url: None,
            },
        };
        let ev = build_action_request(&spec);
        assert_eq!(ev.kind, 31402);
        assert_eq!(ev.tags[0], vec!["d".to_string(), "case-42".to_string()]);
        assert_eq!(extract_tag(&ev.tags, "priority"), Some("high"));
        assert_eq!(extract_tag(&ev.tags, "category"), Some("workflow_review"));
        assert_eq!(extract_tag(&ev.tags, "subject-kind"), Some("work_artifact"));
        assert_eq!(extract_tag(&ev.tags, "title"), Some("Review artifact"));

        let content: serde_json::Value = serde_json::from_str(&ev.content).unwrap();
        assert_eq!(content["fields"]["entity"], "urn:agentbox:bead:abc");
        assert_eq!(content["reasoning"], "needs human sign-off");
        assert!(content["context_url"].is_null(), "explicit null required");
        assert!(
            content.get("priority").is_none(),
            "priority must NOT appear in content"
        );
    }

    #[test]
    fn action_response_parses_forum_shape() {
        let raw = r#"{"action":"approve","reasoning":"Checked the axioms against the corpus; the draft holds."}"#;
        let resp: ActionResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.action, "approve");
    }

    /// GOV-2: the 31404 CaseStatusUpdate is a valid PanelUpdate — top-level
    /// object content, `["d", panel_id]` — carrying the terminal case status.
    #[test]
    fn case_status_update_wire_shape() {
        let ev = build_case_status_update(
            "vc-elevation",
            "vc-elev-finality-mechanism",
            "concept_elevated",
            "https://github.com/o/r/pull/42",
        );
        assert_eq!(ev.kind, KIND_PANEL_UPDATE);
        assert_eq!(
            ev.tags[0],
            vec!["d".to_string(), "vc-elevation".to_string()]
        );
        let content: serde_json::Value = serde_json::from_str(&ev.content).unwrap();
        assert!(
            content.is_object(),
            "31404 content must be a shallow-merge object"
        );
        assert_eq!(content["case_id"], "vc-elev-finality-mechanism");
        assert_eq!(content["status"], "concept_elevated");
        assert_eq!(content["pr_url"], "https://github.com/o/r/pull/42");
    }

    #[test]
    fn knowledge_enrichment_category_tag_value() {
        assert_eq!(
            CaseCategory::KnowledgeEnrichment.as_tag_value(),
            "knowledge_enrichment"
        );
    }

    #[test]
    fn d_tag_is_always_first_and_non_empty() {
        for ev in [
            build_panel_definition(
                "p1",
                &PanelDefinition {
                    title: "t".into(),
                    description: "d".into(),
                    version: "1.0.0".into(),
                    schema: PanelSchema::ActionInbox,
                    fields: vec![],
                    actions: vec![],
                    layout: LayoutHint::InboxTable,
                    capabilities: vec![],
                    refresh_secs: 30,
                },
            ),
            build_panel_state("p1", &serde_json::json!({"x": 1})),
            build_panel_update("p1", &serde_json::json!({"x": 2})),
            build_panel_retired("p1"),
        ] {
            assert_eq!(ev.tags[0][0], "d");
            assert!(!ev.tags[0][1].is_empty());
        }
    }
}

/// Reconciliation contract between the ACSP wire vocabulary (this module) and
/// the cherry-picked broker domain kernel ([`crate::domain::broker`], ADR-130
/// Decision 2). One vocabulary must flow from the kernel through the ACSP
/// producer to the forum consumer: the kernel's `CaseCategory` / `SubjectKind`
/// serde forms are byte-identical to the tag values this module emits, and an
/// ACSP [`ActionResponse`] parses into a kernel `DecisionOutcome`. If these
/// drift, a case the kernel decides would project to a tag the consumer parses
/// differently (or not at all) — the DDD invariant-7 failure this locks against.
#[cfg(test)]
mod broker_kernel_reconciliation {
    use super::*;
    use crate::domain::broker::{
        broker_case::{CaseCategory as KernelCaseCategory, SubjectKind as KernelSubjectKind},
        DecisionOutcome,
    };

    /// The kernel `CaseCategory` snake_case serde form equals the ACSP producer
    /// tag value for every variant.
    #[test]
    fn case_category_vocabulary_is_single_sourced() {
        let pairs = [
            (
                KernelCaseCategory::ContributorMeshShare,
                CaseCategory::ContributorMeshShare,
            ),
            (
                KernelCaseCategory::WorkflowReview,
                CaseCategory::WorkflowReview,
            ),
            (
                KernelCaseCategory::PolicyException,
                CaseCategory::PolicyException,
            ),
            (KernelCaseCategory::TrustAlert, CaseCategory::TrustAlert),
            (
                KernelCaseCategory::ManualSubmission,
                CaseCategory::ManualSubmission,
            ),
            (
                KernelCaseCategory::KnowledgeEnrichment,
                CaseCategory::KnowledgeEnrichment,
            ),
            (
                KernelCaseCategory::DecisionElevation,
                CaseCategory::DecisionElevation,
            ),
        ];
        for (kernel, acsp) in pairs {
            let kernel_wire = serde_json::to_value(&kernel).unwrap();
            assert_eq!(
                kernel_wire.as_str().unwrap(),
                acsp.as_tag_value(),
                "kernel serde vs ACSP tag drifted for {kernel:?}"
            );
        }
    }

    /// The kernel `SubjectKind` snake_case serde form equals the ACSP producer
    /// tag value for every variant.
    #[test]
    fn subject_kind_vocabulary_is_single_sourced() {
        let pairs = [
            (KernelSubjectKind::WorkArtifact, SubjectKind::WorkArtifact),
            (KernelSubjectKind::SkillPackage, SubjectKind::SkillPackage),
            (
                KernelSubjectKind::AutomationProposal,
                SubjectKind::AutomationProposal,
            ),
            (
                KernelSubjectKind::PolicyException,
                SubjectKind::PolicyException,
            ),
            (KernelSubjectKind::Opaque, SubjectKind::Opaque),
        ];
        for (kernel, acsp) in pairs {
            let kernel_wire = serde_json::to_value(&kernel).unwrap();
            assert_eq!(
                kernel_wire.as_str().unwrap(),
                acsp.as_tag_value(),
                "kernel serde vs ACSP tag drifted for {kernel:?}"
            );
        }
    }

    /// An ACSP kind-31403 `ActionResponse` (the forum human-decision content)
    /// parses into a kernel `DecisionOutcome` via `from_action`.
    #[test]
    fn action_response_parses_into_kernel_outcome() {
        let raw = r#"{"action":"approve","reasoning":"Checked the axioms against the corpus; the draft holds."}"#;
        let resp: ActionResponse = serde_json::from_str(raw).unwrap();
        let outcome = DecisionOutcome::from_action(&resp.action, None)
            .expect("approve maps to a kernel outcome");
        assert_eq!(outcome, DecisionOutcome::Approve);
        assert_eq!(outcome.action_str(), "approve");
    }

    /// The 31402 `CaseSpec` category/subject tags a case queued from a kernel
    /// `KnowledgeEnrichment` case would carry match the kernel serde form, so a
    /// case built by the kernel projects to the exact tags the consumer expects.
    #[test]
    fn knowledge_enrichment_case_projects_to_matching_tags() {
        let spec = CaseSpec {
            case_id: "case-1".into(),
            title: "Elevate concept".into(),
            priority: ActionPriority::Medium,
            category: CaseCategory::KnowledgeEnrichment,
            subject_kind: SubjectKind::WorkArtifact,
            subject_id: "urn:visionclaw:concept:bc:x".into(),
            request: ActionRequest {
                fields: serde_json::json!({}),
                reasoning: None,
                context_url: None,
            },
        };
        let ev = build_action_request(&spec);
        assert_eq!(
            extract_tag(&ev.tags, "category"),
            Some(
                serde_json::to_value(KernelCaseCategory::KnowledgeEnrichment)
                    .unwrap()
                    .as_str()
                    .unwrap()
            )
        );
        assert_eq!(
            extract_tag(&ev.tags, "subject-kind"),
            Some(
                serde_json::to_value(KernelSubjectKind::WorkArtifact)
                    .unwrap()
                    .as_str()
                    .unwrap()
            )
        );
    }
}

/// Producer-side contract: everything VisionClaw builds is run through the
/// forum crate's own parser and validator (`nostr-bbs-core`), so a contract
/// change in the forum fails here rather than in production. The consumer-side
/// golden files only prove VisionClaw can *read* the forum's events; these prove
/// the forum can read ours.
///
/// The pinned crate exposes `validate_governance_event` (kind range plus a
/// non-empty `d` tag), typed content structs, and `check_rationale`. It has no
/// single "validate this 31402/31403" entry point, so content rules are
/// asserted by deserialising into the crate's own types and calling
/// `check_rationale` for the 31403 human-rationale rule.
#[cfg(test)]
mod forum_contract {
    use super::*;
    use nostr_bbs_core::governance::{
        self as forum, check_rationale, validate_governance_event, RationaleError, RiskTier,
    };
    use nostr_bbs_core::NostrEvent;

    /// Wrap an unsigned builder output in the forum's event type. Id and
    /// signature are irrelevant to the governance validator.
    fn as_forum_event(ev: &UnsignedAcspEvent) -> NostrEvent {
        NostrEvent {
            id: "0".repeat(64),
            pubkey: "1".repeat(64),
            created_at: 1_760_000_000,
            kind: u64::from(ev.kind),
            tags: ev.tags.clone(),
            content: ev.content.clone(),
            sig: "2".repeat(128),
        }
    }

    fn assert_forum_accepts(ev: &UnsignedAcspEvent) -> NostrEvent {
        let forum_ev = as_forum_event(ev);
        validate_governance_event(&forum_ev)
            .unwrap_or_else(|e| panic!("forum validator rejected kind {}: {e}", ev.kind));
        assert!(forum::is_governance_kind(forum_ev.kind));
        assert_eq!(
            forum::extract_d_tag(&forum_ev.tags),
            Some(ev.tags[0][1].as_str())
        );
        forum_ev
    }

    fn sample_panel() -> PanelDefinition {
        PanelDefinition {
            title: "Knowledge Elevation".into(),
            description: "Frontier concepts proposed for formalisation".into(),
            version: "1.0.0".into(),
            schema: PanelSchema::ActionInbox,
            fields: vec![FieldDef {
                name: "name".into(),
                field_type: FieldType::String,
                label: "Proposed class".into(),
            }],
            actions: vec![
                ActionDef {
                    id: "approve".into(),
                    label: "Elevate".into(),
                    style: ActionStyle::Primary,
                },
                ActionDef {
                    id: "reject".into(),
                    label: "Reject".into(),
                    style: ActionStyle::Destructive,
                },
            ],
            layout: LayoutHint::InboxTable,
            capabilities: vec![PanelCapability::Filter, PanelCapability::Sort],
            refresh_secs: 30,
        }
    }

    fn sample_spec(category: CaseCategory, subject: SubjectKind) -> CaseSpec {
        CaseSpec {
            case_id: "vc-elev-finality-mechanism".into(),
            title: "Elevate finality mechanism".into(),
            priority: ActionPriority::High,
            category,
            subject_kind: subject,
            subject_id: "urn:visionclaw:concept:finality".into(),
            request: ActionRequest {
                fields: serde_json::json!({"name": "Finality", "domain": "bc"}),
                reasoning: Some("Referenced by three classes".into()),
                context_url: Some("https://example.org/ctx".into()),
            },
        }
    }

    #[test]
    fn kinds_match_the_forum_constants() {
        assert_eq!(
            u64::from(KIND_PANEL_DEFINITION),
            forum::KIND_PANEL_DEFINITION
        );
        assert_eq!(u64::from(KIND_PANEL_STATE), forum::KIND_PANEL_STATE);
        assert_eq!(u64::from(KIND_ACTION_REQUEST), forum::KIND_ACTION_REQUEST);
        assert_eq!(u64::from(KIND_ACTION_RESPONSE), forum::KIND_ACTION_RESPONSE);
        assert_eq!(u64::from(KIND_PANEL_UPDATE), forum::KIND_PANEL_UPDATE);
        assert_eq!(u64::from(KIND_PANEL_RETIRED), forum::KIND_PANEL_RETIRED);
    }

    #[test]
    fn panel_definition_31400_parses_as_forum_type() {
        let def = sample_panel();
        let ev = assert_forum_accepts(&build_panel_definition("vc-elevation", &def));
        assert_eq!(ev.kind, 31400);
        let parsed: forum::PanelDefinition =
            serde_json::from_str(&ev.content).expect("forum PanelDefinition parses our content");
        assert_eq!(parsed.title, def.title);
        assert_eq!(parsed.schema, forum::PanelSchema::ActionInbox);
        assert_eq!(parsed.layout, forum::LayoutHint::InboxTable);
        assert_eq!(parsed.actions.len(), 2);
        assert_eq!(parsed.refresh_secs, 30);
    }

    #[test]
    fn every_panel_vocabulary_value_we_emit_parses_in_the_forum() {
        for schema in [
            PanelSchema::ActionInbox,
            PanelSchema::Dashboard,
            PanelSchema::ConfigForm,
            PanelSchema::StatusBoard,
            PanelSchema::ChatBridge,
        ] {
            for layout in [
                LayoutHint::InboxTable,
                LayoutHint::Kanban,
                LayoutHint::CardGrid,
                LayoutHint::SplitDetail,
            ] {
                let mut def = sample_panel();
                def.schema = schema.clone();
                def.layout = layout;
                def.capabilities = vec![
                    PanelCapability::BulkAction,
                    PanelCapability::Filter,
                    PanelCapability::Search,
                    PanelCapability::Sort,
                    PanelCapability::Export,
                ];
                def.fields = [
                    FieldType::String,
                    FieldType::Int,
                    FieldType::Float,
                    FieldType::Bool,
                    FieldType::Json,
                    FieldType::Enum,
                    FieldType::Timestamp,
                ]
                .into_iter()
                .map(|field_type| FieldDef {
                    name: "f".into(),
                    field_type,
                    label: "F".into(),
                })
                .collect();
                let ev = build_panel_definition("p", &def);
                serde_json::from_str::<forum::PanelDefinition>(&ev.content)
                    .expect("forum parses every vocabulary value");
            }
        }
    }

    #[test]
    fn panel_state_31401_update_31404_and_retired_31405_are_valid() {
        let state = assert_forum_accepts(&build_panel_state(
            "vc-elevation",
            &serde_json::json!({"cases": [], "pending": 0}),
        ));
        assert_eq!(state.kind, 31401);
        assert!(serde_json::from_str::<serde_json::Value>(&state.content)
            .unwrap()
            .is_object());

        let update = assert_forum_accepts(&build_panel_update(
            "vc-elevation",
            &serde_json::json!({"pending": 1}),
        ));
        assert_eq!(update.kind, 31404);

        let status = assert_forum_accepts(&build_case_status_update(
            "vc-elevation",
            "vc-elev-x",
            "concept_elevated",
            "https://github.com/o/r/pull/1",
        ));
        assert_eq!(status.kind, 31404);
        let merge: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&status.content).expect("31404 content is a merge object");
        assert_eq!(merge["case_id"], "vc-elev-x");

        let retired = assert_forum_accepts(&build_panel_retired("vc-elevation"));
        assert_eq!(retired.kind, 31405);
    }

    #[test]
    fn action_request_31402_parses_and_tags_are_forum_vocabulary() {
        for (category, subject) in [
            (CaseCategory::KnowledgeEnrichment, SubjectKind::WorkArtifact),
            (CaseCategory::DecisionElevation, SubjectKind::Opaque),
            (
                CaseCategory::ManualSubmission,
                SubjectKind::AutomationProposal,
            ),
            (CaseCategory::WorkflowReview, SubjectKind::SkillPackage),
            (CaseCategory::PolicyException, SubjectKind::PolicyException),
        ] {
            let spec = sample_spec(category, subject);
            let ev = assert_forum_accepts(&build_action_request(&spec));
            assert_eq!(ev.kind, 31402);
            let parsed: forum::ActionRequest =
                serde_json::from_str(&ev.content).expect("forum ActionRequest parses");
            assert_eq!(parsed.fields["name"], "Finality");
            assert_eq!(
                parsed.reasoning.as_deref(),
                Some("Referenced by three classes")
            );
            // Priority is a tag label and must parse as the forum's enum.
            let priority = forum::extract_tag(&ev.tags, "priority").unwrap();
            let p: forum::ActionPriority =
                serde_json::from_value(serde_json::Value::String(priority.into()))
                    .expect("priority tag is a forum ActionPriority");
            assert_eq!(p, forum::ActionPriority::High);
            // We declare no tier or task properties; the forum must read that
            // as "absent", not as an error.
            assert!(parsed.risk_tier.is_none() && parsed.task_properties.is_none());
        }
    }

    #[test]
    fn every_priority_we_emit_parses_in_the_forum() {
        for p in [
            ActionPriority::Critical,
            ActionPriority::High,
            ActionPriority::Medium,
            ActionPriority::Low,
        ] {
            serde_json::from_value::<forum::ActionPriority>(serde_json::Value::String(
                p.as_tag_value().into(),
            ))
            .expect("priority label parses");
        }
    }

    #[test]
    fn voice_request_31402_is_valid() {
        let ev = crate::services::voice_intent_client::build_voice_action_request(
            "restart the crawler",
            "did:nostr:abcd",
        );
        let forum_ev = assert_forum_accepts(&ev);
        assert_eq!(forum_ev.kind, 31402);
        serde_json::from_str::<forum::ActionRequest>(&forum_ev.content).unwrap();
    }

    #[test]
    fn action_response_31403_parses_as_forum_type() {
        let reasoning = "Checked the axioms against the corpus; the draft holds.";
        let ev = assert_forum_accepts(&build_action_response(
            "vc-elev-finality-mechanism",
            "approve",
            reasoning,
        ));
        assert_eq!(ev.kind, 31403);
        let parsed: forum::ActionResponse =
            serde_json::from_str(&ev.content).expect("forum ActionResponse parses");
        assert_eq!(parsed.action, "approve");
        assert_eq!(parsed.reasoning, reasoning);
        // The d-tag must be the case id of the 31402 it answers.
        assert_eq!(
            forum::extract_d_tag(&ev.tags),
            Some("vc-elev-finality-mechanism")
        );
    }

    /// FR2.2: at a High or Critical effective tier a human decision needs a
    /// rationale of at least `MIN_HUMAN_RATIONALE_CHARS`. A representative
    /// rationale built by VisionClaw passes at every tier.
    #[test]
    fn action_response_31403_with_a_real_rationale_passes_every_tier() {
        let ev = build_action_response(
            "c1",
            "approve",
            "Checked the axioms against the corpus; the draft holds.",
        );
        let parsed: forum::ActionResponse = serde_json::from_str(&ev.content).unwrap();
        for tier in [
            RiskTier::Low,
            RiskTier::Medium,
            RiskTier::High,
            RiskTier::Critical,
        ] {
            assert_eq!(
                check_rationale(tier, &parsed.action, Some(&parsed.reasoning)),
                Ok(()),
                "tier {tier:?}"
            );
        }
    }

    /// The forum refuses a short rationale at a consequential tier; the builder
    /// passes the text through verbatim, so the rule bites for short input.
    /// This documents that the check runs over VisionClaw's output.
    #[test]
    fn action_response_31403_short_rationale_is_refused_at_high_tier() {
        let ev = build_action_response("c1", "approve", "ok");
        let parsed: forum::ActionResponse = serde_json::from_str(&ev.content).unwrap();
        assert_eq!(
            check_rationale(RiskTier::High, &parsed.action, Some(&parsed.reasoning)),
            Err(RationaleError::TooShort { chars: 2 })
        );
        assert_eq!(
            check_rationale(RiskTier::Medium, &parsed.action, Some(&parsed.reasoning)),
            Ok(())
        );
    }

    /// Every action word VisionClaw's kernel projection can emit is one the
    /// forum treats as a human decision needing a rationale (or knowingly not).
    #[test]
    fn projected_action_vocabulary_is_known_to_the_forum() {
        for action in ["approve", "reject", "amend", "delegate"] {
            assert!(forum::outcome_requires_rationale(action), "{action}");
        }
    }

    /// KNOWN CONTRACT VIOLATION (reported, not fixed): the decide path projects
    /// `record.reasoning.unwrap_or_default()`, so a decision recorded without
    /// reasoning is published as a 31403 with an empty rationale. At a High or
    /// Critical effective tier the forum relay refuses that. The builder must
    /// not invent a rationale (the forum forbids filling one in), so the fix
    /// belongs at the decide route (require reasoning when the case tier is
    /// High+), not here. This test pins the current behaviour and will fail,
    /// prompting a revisit, once the producer stops emitting empty rationales.
    #[test]
    fn known_failure_empty_rationale_projection_is_refused_at_high_tier() {
        let ev = build_action_response("c1", "approve", "");
        let parsed: forum::ActionResponse = serde_json::from_str(&ev.content).unwrap();
        assert_eq!(
            check_rationale(RiskTier::High, &parsed.action, Some(&parsed.reasoning)),
            Err(RationaleError::Missing)
        );
    }
}
