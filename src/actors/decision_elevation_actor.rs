//! DecisionElevationActor — the write-half actor of ADR-050 (decision elevation).
//!
//! The mirror image of [`crate::actors::elevation_actor::ElevationActor`] for
//! `dl:DecisionRecord` instances. It receives a just-recorded SIGNIFICANT
//! decision from the governed write door (via the [`DecisionElevationSink`] seam,
//! implemented here by [`ActorElevationSink`]), drafts the decision as a corpus
//! page, opens a `DecisionElevation` broker case (ACSP `ActionRequest`, kind
//! 31402) on the forum governance panel, and — on a human `approve` (kind 31403)
//! — PRs the page into the corpus via [`GitHubPRService`] and tracks the PR to a
//! terminal git state (the GOV-2 merge poll). No auto-PR: the broker human gate
//! is mandatory — a decision page is only PR'd after an `approve` decision.
//!
//! Deliberately leaner than `ElevationActor`: decisions are ABox `prov:Activity`
//! individuals that add no TBox axioms, so there is NO EL++ consistency gate
//! (an approved decision page cannot make the classified graph inconsistent).
//! Boot is env-gated exactly like `ElevationActor` (dev/staging default ON,
//! production opt-in; needs `FORUM_RELAY_URL` + a panel secret to publish).
//!
//! ## Durable case state (ADR-2101)
//!
//! Open cases are NOT in-process state. Every lifecycle transition — case
//! opened, broker decided, PR opened, terminal git state — is written through
//! [`DecisionElevationStore`], the same `data/enrichment.sqlite3` authority the
//! sibling `ElevationActor` uses, and the in-memory maps are a cache of it. At
//! boot [`plan_reconciliation`] reloads every non-terminal case and either
//! resumes it (re-arm the merge poll, re-open a PR whose approval crashed
//! before it was created, re-arm a case still awaiting a human) or times it out
//! past [`OPEN_CASE_TTL`] with a kind-31404 receipt. This closes the ADR-2006
//! gap where a crash silently lost an open governance decision.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use actix::prelude::*;
use log::{error, info, warn};
use serde_json::json;

use crate::adapters::decision_elevation_store::{
    status as case_status, DecisionCase, DecisionElevationStore,
};
use crate::adapters::sqlite_enrichment_repository::{status_for_outcome, StoredDecision};
use crate::services::acsp::events::{
    ActionDef, ActionStyle, FieldDef, FieldType, LayoutHint, PanelDefinition, PanelSchema,
};
use crate::services::acsp::{
    build_action_request, build_case_status_update, build_panel_definition, build_panel_state,
    AcspClient, ActionPriority, ActionRequest, CaseCategory, CaseDecision, CaseSpec, SubjectKind,
};
use crate::services::decision_elevation::{
    decision_slug, draft_decision_page, DecisionElevationSink, ElevatedDecision,
};
use crate::services::github_pr_service::{GitHubPRService, PrState};
use crate::types::ontology_tools::AgentContext;

/// NIP-33 panel id (the `d` tag) for the decision-elevation control surface.
const PANEL_ID: &str = "vc-decision-elevation";
/// Case-id namespace; the decision subscription filters on this prefix.
const CASE_PREFIX: &str = "vc-decelev-";
/// How many decision-elevation cases may be open at once (bounds broker volume).
const MAX_OPEN_CASES: usize = 16;
/// GOV-2 merge-poll cadence: how often opened elevation PRs are checked for a
/// terminal git state (merged → `decision_elevated`, closed → abandoned).
const PR_POLL_INTERVAL: Duration = Duration::from_secs(120);
/// How long a case may sit awaiting a human decision before boot reconciliation
/// times it out with a receipt. Cases already tracking an open PR are exempt —
/// a long-lived PR is legitimate work the GOV-2 poll owns.
const OPEN_CASE_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);
/// Kind-31404 status published when reconciliation times a stale case out.
const EXPIRED_STATUS: &str = "decision_elevation_expired";
/// How long reconciliation waits between attempts while the ACSP client is
/// still connecting (timeout receipts need a live publisher).
const RECONCILE_RETRY: Duration = Duration::from_secs(2);
/// Cap on those retries (~30s) — after it, reconciliation runs anyway and the
/// status writes land even though the receipts cannot publish.
const RECONCILE_MAX_ATTEMPTS: u8 = 15;

/// Unix milliseconds now — the local receipt time on a durable decision row.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Unix seconds now. Used for case-open stamps and the TTL comparison.
fn now_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// The elevation trigger: a significant decision to route into the corpus.
#[derive(Message)]
#[rtype(result = "()")]
struct ElevateDecision(ElevatedDecision);

#[derive(Message)]
#[rtype(result = "()")]
struct Decision(CaseDecision);

/// Poll tracked elevation PRs for a terminal git state (GOV-2).
#[derive(Message)]
#[rtype(result = "()")]
struct PollPrs;

/// Boot reconciliation: reload durable open cases and resume or time them out.
#[derive(Message)]
#[rtype(result = "()")]
struct Reconcile;

/// An opened decision-elevation PR being tracked to its terminal state (GOV-2).
#[derive(Debug, Clone)]
struct TrackedPr {
    pr_url: String,
    decision_urn: String,
}

#[derive(Debug, Clone)]
struct PendingCase {
    decision_urn: String,
    file_path: String,
    draft: String,
    summary: String,
}

pub struct DecisionElevationActor {
    acsp: Option<Arc<AcspClient>>,
    panel_secret: String,
    forum_relay_url: String,
    /// Open broker cases awaiting a human decision, keyed by case id.
    pending: HashMap<String, PendingCase>,
    /// GOV-2: elevation PRs opened on approve, tracked to their terminal git state.
    elevating: HashMap<String, TrackedPr>,
    /// Decision URNs already cased this session (idempotent trigger skip-list).
    seen: HashSet<String>,
    elevated_count: u32,
    rejected_count: u32,
    last_pr_url: Option<String>,
    /// ADR-2101 durable case-state authority. `None` only when the store could
    /// not be opened — the actor then degrades to the old in-process behaviour
    /// rather than dropping decisions on the floor (fail-open, ADR-050).
    store: Option<Arc<DecisionElevationStore>>,
    /// Reconciliation runs exactly once per process.
    reconciled: bool,
    reconcile_attempts: u8,
}

/// Pure production check (mirrors `elevation_actor::is_production_from`) so the
/// dev-default-ON / prod-opt-in gate is unit-testable without env mutation.
fn is_production_from(app_env: Option<String>, node_env: Option<String>) -> bool {
    let is_prod = |v: &Option<String>| {
        v.as_deref()
            .map(|s| s.eq_ignore_ascii_case("production"))
            .unwrap_or(false)
    };
    is_prod(&app_env) || is_prod(&node_env)
}

fn is_production_env() -> bool {
    is_production_from(
        std::env::var("APP_ENV").ok(),
        std::env::var("NODE_ENV").ok(),
    )
}

impl DecisionElevationActor {
    /// Construct if enabled + configured. `DECISION_ELEVATION_ENABLED` defaults
    /// ON in dev/staging and opt-in in production (an explicit value wins), and
    /// publishing additionally requires `FORUM_RELAY_URL` + a panel secret —
    /// `None` means the write-half is dormant for this profile/config (the
    /// governed decision write door then simply never sees a live sink).
    pub fn new() -> Option<Self> {
        let enabled = match std::env::var("DECISION_ELEVATION_ENABLED") {
            Ok(v) => v == "1" || v.eq_ignore_ascii_case("true"),
            Err(_) => !is_production_env(),
        };
        if !enabled {
            return None;
        }
        let forum_relay_url = std::env::var("FORUM_RELAY_URL").ok()?;
        let panel_secret = std::env::var("ACSP_PANEL_NOSTR_PRIVKEY")
            .or_else(|_| std::env::var("VISIONCLAW_NOSTR_PRIVKEY"))
            .ok()?;
        Some(Self {
            acsp: None,
            panel_secret,
            forum_relay_url,
            pending: HashMap::new(),
            elevating: HashMap::new(),
            seen: HashSet::new(),
            elevated_count: 0,
            rejected_count: 0,
            last_pr_url: None,
            store: None,
            reconciled: false,
            reconcile_attempts: 0,
        })
    }

    /// Inject an already-open durable store (wiring seam for `AppState` and the
    /// tests). Without it the actor opens its own connection to the same file at
    /// boot — the actor starts before `AppState` publishes any handle.
    pub fn with_store(mut self, store: Arc<DecisionElevationStore>) -> Self {
        self.store = Some(store);
        self
    }

    fn panel_definition() -> PanelDefinition {
        PanelDefinition {
            title: "Decision Elevation".into(),
            description: "Significant decision records proposed for the public corpus. \
                          Approve to commit the decision page as a PR so the next sync \
                          re-derives it into the asserted graph (ADR-050)."
                .into(),
            version: "1.0.0".into(),
            schema: PanelSchema::ActionInbox,
            fields: vec![
                FieldDef {
                    name: "summary".into(),
                    field_type: FieldType::String,
                    label: "Decision".into(),
                },
                FieldDef {
                    name: "decision_urn".into(),
                    field_type: FieldType::String,
                    label: "Decision URN".into(),
                },
                FieldDef {
                    name: "causal_edges".into(),
                    field_type: FieldType::Json,
                    label: "Causal edges".into(),
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
                    label: "Publish".into(),
                    style: ActionStyle::Primary,
                },
                ActionDef {
                    id: "reject".into(),
                    label: "Keep runtime-only".into(),
                    style: ActionStyle::Secondary,
                },
            ],
            layout: LayoutHint::InboxTable,
            capabilities: vec![],
            refresh_secs: 60,
        }
    }

    fn state_snapshot(&self) -> serde_json::Value {
        json!({
            "open_cases": self.pending.len(),
            "elevated": self.elevated_count,
            "rejected": self.rejected_count,
            "awaiting_merge": self.elevating.len(),
            "last_pr_url": self.last_pr_url,
            "durable": self.store.is_some(),
        })
    }

    /// Persist a broker decision (when there is one), then — for an approval —
    /// open the corpus PR and stamp it durably BEFORE the in-memory tracking
    /// insert. Sequencing both writes inside one future is what makes the
    /// status transitions deterministic: `record_decision` (→ `approved`) can
    /// never land after `mark_elevating` (→ `elevating`) and undo it.
    ///
    /// `elevate: None` records a decision with no corpus write (reject / amend /
    /// delegate); `record: None` resumes a PR whose approval was already
    /// persisted by a previous process (boot reconciliation).
    fn spawn_decision_outcome(
        &mut self,
        case_id: String,
        record: Option<StoredDecision>,
        elevate: Option<PendingCase>,
        ctx: &mut Context<Self>,
    ) {
        let store = self.store.clone();
        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                if let (Some(store), Some(record)) = (store.as_ref(), record.as_ref()) {
                    match store.record_decision(record).await {
                        Ok(id) => info!(
                            "[DecisionElevation] decision row {id} persisted for {case_id} (outcome '{}', event {:?})",
                            record.outcome, record.decision_event_id
                        ),
                        Err(e) => {
                            error!("[DecisionElevation] decision persist FAILED for {case_id}: {e}; external write refused, reconcile the open case");
                            return None;
                        },
                    }
                }
                if record.is_some() && store.is_none() {
                    error!("[DecisionElevation] no durable store for {case_id}; external write refused");
                    return None;
                }
                let case = elevate?;
                let durable_store = store.as_ref()?;
                match durable_store.claim_application(&case_id).await {
                    Ok(true) => {},
                    Ok(false) => {
                        warn!("[DecisionElevation] case {case_id} is not an unclaimed approval; PR refused");
                        return None;
                    },
                    Err(e) => {
                        error!("[DecisionElevation] cannot persist application claim for {case_id}: {e}; PR refused");
                        return None;
                    }
                }
                let agent_id = format!(
                    "decision-{}",
                    decision_slug(&case.decision_urn, &case.summary)
                );
                let pr = GitHubPRService::new();
                let agent_ctx = AgentContext {
                    agent_id,
                    agent_type: "decision-elevation".into(),
                    task_description: format!(
                        "ADR-050 broker-approved elevation of decision {}",
                        case.decision_urn
                    ),
                    session_id: None,
                    confidence: 0.5,
                    user_id: "acsp-governance".into(),
                };
                let title = format!(
                    "feat(decision): elevate decision '{}' to corpus",
                    case.summary
                );
                let body = format!(
                    "Corpus page for decision record **{}**\n\n\
                     URN: `{}`\n\n\
                     Approved via the forum governance panel (ADR-050 decision \
                     elevation). The next `force_full` sync re-derives this decision \
                     into `urn:ngm:graph:ontology:assert`; signed attribution stays in \
                     the provenance graph.\n\n🤖 Generated by Claude Code",
                    case.summary, case.decision_urn
                );
                match pr
                    .create_ontology_pr(&case.file_path, &case.draft, &title, &body, &agent_ctx)
                    .await
                {
                    Ok(url) => {
                        // Durable BEFORE in-memory: a crash here resumes from
                        // `elevating` with the PR url intact.
                        if let Some(store) = store.as_ref() {
                            if let Err(e) = store.mark_elevating(&case_id, &url).await {
                                warn!("[DecisionElevation] PR stamp persist failed for {case_id}: {e}; the merge poll will not resume after a restart");
                            }
                        }
                        Some((case_id, case.decision_urn, url))
                    }
                    Err(e) => {
                        // Outcome may be unknown after an upstream timeout. Keep
                        // `applying` for manual reconciliation, never blindly retry.
                        error!("[DecisionElevation] PR creation failed for {case_id}: {e}");
                        None
                    }
                }
            })
            .map(|opened, act, ctx| {
                if let Some((case_id, decision_urn, url)) = opened {
                    act.elevated_count += 1;
                    act.last_pr_url = Some(url.clone());
                    act.elevating.insert(
                        case_id.clone(),
                        TrackedPr {
                            pr_url: url.clone(),
                            decision_urn,
                        },
                    );
                    info!("[DecisionElevation] PR created: {url} (tracking case {case_id} for merge → decision_elevated)");
                }
                act.publish_state(ctx);
            }),
        );
    }

    /// Apply a reconciliation plan to the live actor: re-arm the in-memory
    /// caches from durable state, resume interrupted work, and publish a
    /// timeout receipt for anything stale.
    fn apply_reconciliation(&mut self, plan: Vec<ReconcileAction>, ctx: &mut Context<Self>) {
        let (mut tracking, mut resumed_pr, mut pending, mut expired) = (0, 0, 0, 0);
        for action in plan {
            match action {
                ReconcileAction::ResumeTracking(case) => {
                    let Some(pr_url) = case.pr_url.clone() else {
                        continue;
                    };
                    self.seen.insert(case.decision_urn.clone());
                    self.elevating.insert(
                        case.case_id.clone(),
                        TrackedPr {
                            pr_url,
                            decision_urn: case.decision_urn,
                        },
                    );
                    tracking += 1;
                }
                ReconcileAction::ResumePr(case) => {
                    self.seen.insert(case.decision_urn.clone());
                    warn!(
                        "[DecisionElevation] case {} was approved but has no recorded PR — re-opening the corpus PR",
                        case.case_id
                    );
                    let case_id = case.case_id.clone();
                    self.spawn_decision_outcome(case_id, None, Some(pending_from(&case)), ctx);
                    resumed_pr += 1;
                }
                ReconcileAction::ResumePending(case) => {
                    self.seen.insert(case.decision_urn.clone());
                    self.pending
                        .insert(case.case_id.clone(), pending_from(&case));
                    pending += 1;
                }
                ReconcileAction::Expire(case) => {
                    self.spawn_expiry(case, ctx);
                    expired += 1;
                }
            }
        }
        info!(
            "[DecisionElevation] reconciliation complete: {tracking} PR(s) re-armed, {resumed_pr} approval(s) resumed, {pending} case(s) still awaiting a human, {expired} timed out"
        );
        self.publish_state(ctx);
    }

    /// Time a stale case out: a kind-31404 receipt on the panel (so the forum
    /// shows why it vanished) plus the terminal durable status. The receipt is
    /// best-effort; the status write is not conditional on it.
    fn spawn_expiry(&mut self, case: DecisionCase, ctx: &mut Context<Self>) {
        let store = self.store.clone();
        let acsp = self.acsp.clone();
        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                let case_id = case.case_id;
                warn!(
                    "[DecisionElevation] case {case_id} for decision {} timed out unanswered after {}s — expiring with a receipt",
                    case.decision_urn,
                    OPEN_CASE_TTL.as_secs()
                );
                if let Some(acsp) = acsp.as_ref() {
                    if let Err(e) = acsp
                        .publish(&build_case_status_update(
                            PANEL_ID,
                            &case_id,
                            EXPIRED_STATUS,
                            case.pr_url.as_deref().unwrap_or(""),
                        ))
                        .await
                    {
                        warn!("[DecisionElevation] expiry receipt publish failed for {case_id}: {e}");
                    }
                }
                if let Some(store) = store.as_ref() {
                    if let Err(e) = store.mark_terminal(&case_id, case_status::EXPIRED).await {
                        warn!("[DecisionElevation] expiry status persist failed for {case_id}: {e}");
                    }
                }
            })
            .map(|_, _, _| ()),
        );
    }

    fn publish_state(&self, ctx: &mut Context<Self>) {
        let Some(acsp) = self.acsp.clone() else {
            return;
        };
        let state = self.state_snapshot();
        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                if let Err(e) = acsp.publish(&build_panel_state(PANEL_ID, &state)).await {
                    warn!("[DecisionElevation] panel state publish failed: {e}");
                }
            })
            .map(|_, _, _| ()),
        );
    }
}

impl Actor for DecisionElevationActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!(
            "[DecisionElevation] actor starting (panel '{PANEL_ID}', case prefix '{CASE_PREFIX}')"
        );
        let secret = self.panel_secret.clone();
        let relay = self.forum_relay_url.clone();
        let addr = ctx.address();

        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                match AcspClient::connect(&secret, &relay).await {
                    Ok(client) => {
                        let client = Arc::new(client);
                        let def = build_panel_definition(PANEL_ID, &Self::panel_definition());
                        if let Err(e) = client.publish(&def).await {
                            warn!("[DecisionElevation] panel definition publish failed: {e}");
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
                        error!("[DecisionElevation] ACSP connect failed: {e}; actor idle");
                        None
                    }
                }
            })
            .map(|client, act, _ctx| {
                act.acsp = client;
            }),
        );

        // ADR-2101: attach the durable case-state authority, then reconcile
        // whatever the previous process left open. Opening its own connection to
        // `data/enrichment.sqlite3` is safe (WAL, single-writer serialised by
        // tokio-rusqlite) and keeps the actor bootable before `AppState`.
        if self.store.is_some() {
            ctx.address().do_send(Reconcile);
        } else {
            let db_path = DecisionElevationStore::default_db_path();
            ctx.spawn(
                actix::fut::wrap_future::<_, Self>(async move {
                    match DecisionElevationStore::open(&db_path).await {
                        Ok(store) => Some(Arc::new(store)),
                        Err(e) => {
                            error!("[DecisionElevation] durable case store unavailable at {}: {e}; running WITHOUT restart recovery", db_path.display());
                            None
                        }
                    }
                })
                .map(|store, act, ctx| {
                    if store.is_some() {
                        act.store = store;
                        info!("[DecisionElevation] durable case store attached (ADR-2101); reconciling open cases");
                        ctx.address().do_send(Reconcile);
                    }
                }),
            );
        }

        // GOV-2: poll opened elevation PRs for a terminal git state.
        if GitHubPRService::has_github_token() {
            info!("[DecisionElevation] GOV-2 merge poll armed (every {}s) — merged PRs fire decision_elevated", PR_POLL_INTERVAL.as_secs());
        } else {
            warn!("[DecisionElevation] GOV-2 DEGRADED: no GitHub token (PRIVATE_REPO_GITHUB_PAT) — elevation PRs cannot be opened and merge polling cannot resolve; decision_elevated will never fire until a token is configured");
        }
        ctx.run_interval(PR_POLL_INTERVAL, |_act, ctx| {
            ctx.address().do_send(PollPrs);
        });
    }
}

impl Handler<ElevateDecision> for DecisionElevationActor {
    type Result = ();

    fn handle(&mut self, ElevateDecision(dec): ElevateDecision, ctx: &mut Self::Context) {
        // Idempotent + bounded: skip a decision already cased this session, and
        // never exceed the open-case budget (fail-open — dropping is safe: the
        // decision is durable at runtime and the read-half re-derives elevated ones).
        if self.seen.contains(&dec.decision_urn) {
            return;
        }
        if self.pending.len() >= MAX_OPEN_CASES {
            warn!(
                "[DecisionElevation] open-case budget ({MAX_OPEN_CASES}) reached; decision {} stays runtime-only until a case frees",
                dec.decision_urn
            );
            return;
        }
        let Some(acsp) = self.acsp.clone() else {
            // Not yet connected (or connect failed) — fail-open, drop the trigger.
            return;
        };

        let (file_path, draft) = draft_decision_page(&dec);
        let slug = decision_slug(&dec.decision_urn, &dec.input.summary);
        let case_id = format!("{CASE_PREFIX}{slug}");
        let summary = if dec.input.summary.trim().is_empty() {
            dec.decision_urn.clone()
        } else {
            dec.input.summary.clone()
        };

        let causal: Vec<String> = dec
            .input
            .caused
            .iter()
            .chain(dec.input.precedent_for.iter())
            .chain(dec.input.influenced.iter())
            .cloned()
            .collect();
        let priority = if dec.acsp_approved {
            ActionPriority::High
        } else {
            ActionPriority::Medium
        };
        let reasoning = format!(
            "Significant decision (caused {} / precedent {} / influenced {}{}) proposed for the \
             public corpus so the force_full assert-graph rebuild re-derives it (ADR-050).",
            dec.input.caused.len(),
            dec.input.precedent_for.len(),
            dec.input.influenced.len(),
            if dec.input.proposal_urn.is_some() {
                "; governed a graph mutation"
            } else {
                ""
            },
        );
        let spec = CaseSpec {
            case_id: case_id.clone(),
            title: format!("Elevate decision: {summary}"),
            priority,
            category: CaseCategory::DecisionElevation,
            subject_kind: SubjectKind::AutomationProposal,
            subject_id: dec.decision_urn.clone(),
            request: ActionRequest {
                fields: json!({
                    "summary": summary,
                    "decision_urn": dec.decision_urn,
                    "causal_edges": causal,
                    "file_path": file_path.clone(),
                    "operation": { "kind": "corpus-pr", "file_path": file_path.clone(), "draft": draft.clone() },
                    "acsp_approved": dec.acsp_approved,
                }),
                reasoning: Some(reasoning),
                context_url: None,
            },
        };

        let pending = PendingCase {
            decision_urn: dec.decision_urn.clone(),
            file_path,
            draft,
            summary,
        };

        // ADR-2101 durable-first: the case row exists before the forum ever
        // sees the request, so no crash window can lose an open case. A publish
        // that then fails closes the row out — nobody can answer a case that was
        // never published, so leaving it open would strand it for ever.
        let record = DecisionCase::opening(
            case_id.clone(),
            pending.decision_urn.clone(),
            pending.file_path.clone(),
            pending.draft.clone(),
            pending.summary.clone(),
            now_s(),
        );
        let store = self.store.clone();
        let persist_id = case_id.clone();

        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                if let Some(store) = store.as_ref() {
                    if let Err(e) = store.open_case(&record).await {
                        return Err(format!("durable case persist failed for {persist_id}: {e}; request refused"));
                    }
                }
                if store.is_none() {
                    return Err("durable case store unavailable; request refused".into());
                }
                let published = acsp.publish(&build_action_request(&spec)).await.map(|_| ());
                if published.is_err() {
                    if let Some(store) = store.as_ref() {
                        if let Err(e) = store
                            .mark_terminal(&persist_id, case_status::PUBLISH_FAILED)
                            .await
                        {
                            warn!("[DecisionElevation] publish-failure receipt persist failed for {persist_id}: {e}");
                        }
                    }
                }
                published
            })
            .map(move |result, act, ctx| match result {
                Ok(()) => {
                    info!(
                        "[DecisionElevation] opened case {case_id} for decision {}",
                        pending.decision_urn
                    );
                    act.seen.insert(pending.decision_urn.clone());
                    act.pending.insert(case_id, pending);
                    act.publish_state(ctx);
                }
                Err(e) => warn!("[DecisionElevation] case publish failed: {e}"),
            }),
        );
    }
}

impl Handler<Decision> for DecisionElevationActor {
    type Result = ();

    fn handle(&mut self, Decision(d): Decision, ctx: &mut Self::Context) {
        let Some(case) = self.pending.remove(&d.case_id) else {
            return; // replayed / foreign decision
        };
        info!(
            "[DecisionElevation] case {} decided '{}' by {} — {}",
            d.case_id, d.action, d.responder_pubkey, d.reasoning
        );

        // ADR-2101/ADR-2006: the broker decision is durable, correlated on the
        // signed 31403 event id, and its transaction moves the case status in
        // the same commit — the decision row and the case can never disagree.
        let record = decision_record(&d);
        let approved = d.action.as_str() == "approve";
        if !approved {
            // reject / amend / delegate — the decision stays runtime-only.
            self.rejected_count += 1;
        }
        self.spawn_decision_outcome(
            d.case_id.clone(),
            Some(record),
            approved.then_some(case),
            ctx,
        );
    }
}

/// Build the durable [`StoredDecision`] for a decision-elevation case from the
/// forum [`CaseDecision`] (kind-31403), mirroring
/// [`crate::actors::elevation_actor`]'s `decision_record`. Correlation is on the
/// SIGNED event id — `(case_id, action, responder_pubkey)` is not unique, so a
/// replayed 31403 or an admin answering twice the same way would collide. A
/// decision carrying no event id (locally minted) falls back to the tuple plus
/// `local`. `writeback_committed` stays `false`: the elevation "commit" is the
/// merged corpus PR, tracked separately by the GOV-2 poll.
fn decision_record(d: &CaseDecision) -> StoredDecision {
    let attributed = crate::uri::is_pubkey_hex(&d.responder_pubkey);
    let owner_did = if attributed {
        crate::uri::did_nostr(&d.responder_pubkey).ok()
    } else {
        None
    };
    let correlation = if d.event_id.is_empty() {
        format!(
            "decelev-decide:{}:{}:{}:local",
            d.case_id, d.action, d.responder_pubkey
        )
    } else {
        format!("decelev-decide:{}:{}", d.case_id, d.event_id)
    };
    StoredDecision {
        case_id: d.case_id.clone(),
        outcome: d.action.clone(),
        attributed,
        broker_pubkey: Some(d.responder_pubkey.clone()),
        reasoning: Some(d.reasoning.clone()),
        writeback_triggered: status_for_outcome(&d.action) == "approved",
        writeback_committed: false,
        activity_urn: crate::uri::execution(&correlation),
        proposal_urn: None,
        owner_did,
        decided_at_ms: now_ms(),
        decision_event_id: (!d.event_id.is_empty()).then(|| d.event_id.clone()),
        decision_created_at_s: (d.created_at > 0).then_some(d.created_at as i64),
    }
}

/// Rehydrate the in-process case shape from a durable row.
fn pending_from(case: &DecisionCase) -> PendingCase {
    PendingCase {
        decision_urn: case.decision_urn.clone(),
        file_path: case.file_path.clone(),
        draft: case.draft.clone(),
        summary: case.summary.clone(),
    }
}

/// What boot reconciliation must do with one durable open case.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReconcileAction {
    /// A corpus PR is open — re-arm the GOV-2 merge poll for it.
    ResumeTracking(DecisionCase),
    /// Approved, but no PR is recorded: the previous process died between the
    /// decision and the PR. Re-open it (the draft is durable).
    ResumePr(DecisionCase),
    /// Still awaiting a human and within the TTL — re-arm the pending case so a
    /// late kind-31403 is still matched.
    ResumePending(DecisionCase),
    /// Unanswered past the TTL — close it out with a receipt.
    Expire(DecisionCase),
}

/// Pure reconciliation policy: decide what happens to each durable open case.
///
/// Kept free of I/O so every branch — including the TTL boundary — is unit
/// testable without a relay, a GitHub token or an actor system. A case tracking
/// an open PR is NEVER expired: a long-lived PR is legitimate work the merge
/// poll owns, and expiring it would fabricate a terminal state git disagrees
/// with. Terminal rows are filtered out before this point, and skipped
/// defensively here too.
fn plan_reconciliation(cases: Vec<DecisionCase>, now_s: i64, ttl_s: i64) -> Vec<ReconcileAction> {
    cases
        .into_iter()
        .filter(|c| !case_status::is_terminal(&c.status))
        .filter_map(|case| {
            let tracking = case.status == case_status::ELEVATING && case.pr_url.is_some();
            let stale = now_s.saturating_sub(case.opened_at_s) > ttl_s;
            match case.status.as_str() {
                _ if tracking => Some(ReconcileAction::ResumeTracking(case)),
                case_status::APPLYING => {
                    warn!("[DecisionElevation] case {} has an uncertain PR application; reconcile upstream before retry", case.case_id);
                    None
                }
                _ if stale => Some(ReconcileAction::Expire(case)),
                // `elevating` with no PR url can only be a partially written
                // row; treat it like an approval whose PR never landed.
                case_status::APPROVED | case_status::ELEVATING => {
                    Some(ReconcileAction::ResumePr(case))
                }
                case_status::PENDING => Some(ReconcileAction::ResumePending(case)),
                other => {
                    warn!("[DecisionElevation] reconciliation skipping case {} in unknown status '{other}'", case.case_id);
                    None
                }
            }
        })
        .collect()
}

impl Handler<Reconcile> for DecisionElevationActor {
    type Result = ();

    fn handle(&mut self, _msg: Reconcile, ctx: &mut Self::Context) {
        if self.reconciled {
            return;
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        // Timeout receipts need a live publisher; wait briefly for the ACSP
        // connect rather than expiring cases silently. Bounded — after the cap
        // the status writes still land, only the receipts are missed.
        if self.acsp.is_none() && self.reconcile_attempts < RECONCILE_MAX_ATTEMPTS {
            self.reconcile_attempts += 1;
            ctx.run_later(RECONCILE_RETRY, |_act, ctx| {
                ctx.address().do_send(Reconcile);
            });
            return;
        }
        self.reconciled = true;

        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move { store.open_cases().await }).map(
                |result, act, ctx| match result {
                    Ok(cases) => {
                        if cases.is_empty() {
                            info!("[DecisionElevation] reconciliation: no open cases carried over");
                            return;
                        }
                        info!(
                            "[DecisionElevation] reconciliation: {} open case(s) recovered from durable state",
                            cases.len()
                        );
                        let plan =
                            plan_reconciliation(cases, now_s(), OPEN_CASE_TTL.as_secs() as i64);
                        act.apply_reconciliation(plan, ctx);
                    }
                    Err(e) => error!(
                        "[DecisionElevation] reconciliation read FAILED: {e}; open cases from the previous process are NOT recovered"
                    ),
                },
            ),
        );
    }
}

/// GOV-2: map a terminal PR git state to `(31404 status)`. `Open` ⇒ keep polling.
fn terminal_for_pr_state(state: PrState) -> Option<&'static str> {
    match state {
        PrState::Merged => Some("decision_elevated"),
        PrState::ClosedUnmerged => Some("decision_abandoned"),
        PrState::Open => None,
    }
}

/// Map the published kind-31404 status to the durable terminal case status.
fn terminal_case_status(status: &str) -> &'static str {
    if status == "decision_elevated" {
        case_status::PUBLISHED
    } else {
        case_status::ABANDONED
    }
}

impl Handler<PollPrs> for DecisionElevationActor {
    type Result = ();

    fn handle(&mut self, _msg: PollPrs, ctx: &mut Self::Context) {
        if self.elevating.is_empty() {
            return;
        }
        if !GitHubPRService::has_github_token() {
            warn!(
                "[DecisionElevation] GOV-2 merge poll DEGRADED: {} PR(s) tracked but no GitHub token; decision_elevated cannot fire until PRIVATE_REPO_GITHUB_PAT is configured",
                self.elevating.len()
            );
            return;
        }
        let Some(acsp) = self.acsp.clone() else {
            return;
        };
        let tracked: Vec<(String, TrackedPr)> = self
            .elevating
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let pr = GitHubPRService::new();
        let store = self.store.clone();

        ctx.spawn(
            actix::fut::wrap_future::<_, Self>(async move {
                let mut resolved: Vec<String> = Vec::new();
                for (case_id, tracked) in tracked {
                    match pr.pr_state(&tracked.pr_url).await {
                        Ok(state) => {
                            let Some(status) = terminal_for_pr_state(state) else {
                                continue; // still open — keep tracking
                            };
                            if let Err(e) = acsp
                                .publish(&build_case_status_update(
                                    PANEL_ID,
                                    &case_id,
                                    status,
                                    &tracked.pr_url,
                                ))
                                .await
                            {
                                warn!("[DecisionElevation] GOV-2 31404 publish failed for {case_id}: {e}");
                            }
                            // ADR-2101: the durable terminal status is written
                            // independently of the publish above — the two facts
                            // (forum visibility vs durable case authority) fail
                            // separately, mirroring the ElevationActor. The case
                            // is only dropped from the in-memory tracking map
                            // once it is durably terminal, so a persist failure
                            // retries on the next poll instead of vanishing.
                            let store_status = terminal_case_status(status);
                            if let Some(store) = store.as_ref() {
                                if let Err(e) = store.mark_terminal(&case_id, store_status).await {
                                    warn!("[DecisionElevation] GOV-2 terminal status persist failed for {case_id}: {e}; retrying next poll");
                                    continue;
                                }
                            }
                            info!(
                                "[DecisionElevation] case {case_id} terminal: {status} for decision {} ({})",
                                tracked.decision_urn, tracked.pr_url
                            );
                            resolved.push(case_id);
                        }
                        Err(e) => {
                            warn!("[DecisionElevation] GOV-2 PR state poll failed for {case_id}: {e}");
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

/// Adapter making a live [`DecisionElevationActor`] address satisfy the
/// [`DecisionElevationSink`] the governed write door calls. `elevate` is
/// fire-and-forget: `do_send` never blocks, and a dead mailbox is reported as
/// `Err` (the caller logs and ignores — ADR-050 fail-open).
pub struct ActorElevationSink {
    addr: Addr<DecisionElevationActor>,
}

impl ActorElevationSink {
    pub fn new(addr: Addr<DecisionElevationActor>) -> Self {
        Self { addr }
    }
}

impl DecisionElevationSink for ActorElevationSink {
    fn elevate(&self, decision: ElevatedDecision) -> Result<(), String> {
        self.addr
            .try_send(ElevateDecision(decision))
            .map_err(|e| format!("decision-elevation mailbox send failed: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncertain_pr_application_never_replays_or_expires_as_if_unapplied() {
        let c = case("uncertain", "applying", None, NOW - TTL - 1);
        assert!(plan_reconciliation(vec![c], NOW, TTL).is_empty());
    }

    #[test]
    fn production_gate_defaults_dev_on_prod_off() {
        assert!(!is_production_from(None, None));
        assert!(is_production_from(Some("production".into()), None));
        assert!(is_production_from(None, Some("Production".into())));
        assert!(!is_production_from(Some("staging".into()), None));
    }

    #[test]
    fn terminal_pr_state_maps_merge_and_abandon() {
        assert_eq!(
            terminal_for_pr_state(PrState::Merged),
            Some("decision_elevated")
        );
        assert_eq!(
            terminal_for_pr_state(PrState::ClosedUnmerged),
            Some("decision_abandoned")
        );
        assert_eq!(terminal_for_pr_state(PrState::Open), None);
    }

    // -- ADR-2101 durable case state ------------------------------------

    const TTL: i64 = 14 * 24 * 60 * 60;
    const NOW: i64 = 1_800_000_000;

    fn case(id: &str, status: &str, pr_url: Option<&str>, opened_at_s: i64) -> DecisionCase {
        DecisionCase {
            case_id: format!("{CASE_PREFIX}{id}"),
            decision_urn: format!("urn:ngm:kg:decision:{id}"),
            file_path: format!("decisions/{id}.md"),
            draft: "# decision page".into(),
            summary: "a significant decision".into(),
            status: status.into(),
            pr_url: pr_url.map(str::to_string),
            opened_at_s,
        }
    }

    #[test]
    fn terminal_case_status_maps_the_31404_status() {
        assert_eq!(
            terminal_case_status("decision_elevated"),
            case_status::PUBLISHED
        );
        assert_eq!(
            terminal_case_status("decision_abandoned"),
            case_status::ABANDONED
        );
    }

    #[test]
    fn pending_from_rehydrates_the_in_process_case() {
        let c = case("alpha", case_status::PENDING, None, NOW);
        let p = pending_from(&c);
        assert_eq!(p.decision_urn, c.decision_urn);
        assert_eq!(p.file_path, c.file_path);
        assert_eq!(p.draft, c.draft);
        assert_eq!(p.summary, c.summary);
    }

    #[test]
    fn reconciliation_resumes_a_tracked_pr() {
        let c = case(
            "tracked",
            case_status::ELEVATING,
            Some("https://github.com/o/r/pull/3"),
            NOW - 10,
        );
        assert_eq!(
            plan_reconciliation(vec![c.clone()], NOW, TTL),
            vec![ReconcileAction::ResumeTracking(c)]
        );
    }

    #[test]
    fn a_tracked_pr_is_never_expired_however_old() {
        // A long-lived PR is legitimate work the merge poll owns; expiring it
        // would fabricate a terminal state git disagrees with.
        let c = case(
            "ancient",
            case_status::ELEVATING,
            Some("https://github.com/o/r/pull/4"),
            NOW - TTL * 10,
        );
        assert_eq!(
            plan_reconciliation(vec![c.clone()], NOW, TTL),
            vec![ReconcileAction::ResumeTracking(c)]
        );
    }

    #[test]
    fn reconciliation_reopens_an_approval_that_never_got_a_pr() {
        // The crash window the finding named: approved, PR creation lost.
        let c = case("approved", case_status::APPROVED, None, NOW - 60);
        assert_eq!(
            plan_reconciliation(vec![c.clone()], NOW, TTL),
            vec![ReconcileAction::ResumePr(c)]
        );
        // A half-written `elevating` row with no PR url is the same situation.
        let half = case("half", case_status::ELEVATING, None, NOW - 60);
        assert_eq!(
            plan_reconciliation(vec![half.clone()], NOW, TTL),
            vec![ReconcileAction::ResumePr(half)]
        );
    }

    #[test]
    fn reconciliation_rearms_a_case_still_awaiting_a_human() {
        let c = case("waiting", case_status::PENDING, None, NOW - TTL + 1);
        assert_eq!(
            plan_reconciliation(vec![c.clone()], NOW, TTL),
            vec![ReconcileAction::ResumePending(c)]
        );
    }

    #[test]
    fn reconciliation_expires_stale_unanswered_cases() {
        let pending = case("stale", case_status::PENDING, None, NOW - TTL - 1);
        let approved = case("stuck", case_status::APPROVED, None, NOW - TTL - 1);
        assert_eq!(
            plan_reconciliation(vec![pending.clone(), approved.clone()], NOW, TTL),
            vec![
                ReconcileAction::Expire(pending),
                ReconcileAction::Expire(approved)
            ]
        );
    }

    #[test]
    fn reconciliation_skips_terminal_and_unknown_rows() {
        let plan = plan_reconciliation(
            vec![
                case("done", case_status::PUBLISHED, None, NOW),
                case("gone", case_status::ABANDONED, None, NOW),
                case("no", case_status::REJECTED, None, NOW),
                case("weird", "not-a-status", None, NOW),
            ],
            NOW,
            TTL,
        );
        assert!(plan.is_empty(), "nothing terminal or unknown is resumed");
    }

    #[test]
    fn decision_record_correlates_on_the_signed_event_id() {
        let d = CaseDecision {
            case_id: "vc-decelev-x".into(),
            action: "approve".into(),
            reasoning: "ship it".into(),
            responder_pubkey: "f".repeat(64),
            event_id: "abc123".into(),
            created_at: 1_700_000_000,
        };
        let r = decision_record(&d);
        assert_eq!(r.case_id, "vc-decelev-x");
        assert_eq!(r.outcome, "approve");
        assert!(r.writeback_triggered, "approve triggers the corpus write");
        assert!(
            !r.writeback_committed,
            "the merge is what commits, not this"
        );
        assert_eq!(r.decision_event_id.as_deref(), Some("abc123"));
        assert_eq!(r.decision_created_at_s, Some(1_700_000_000));

        // The activity URN content-addresses the correlation string, so assert
        // the property that matters rather than its text: the SIGNED EVENT ID is
        // what distinguishes two decisions. Same event id ⇒ same URN (so a
        // re-delivery is recognisable); different event id ⇒ different URN (so an
        // admin answering the same case twice the same way no longer collides —
        // exactly what the tuple-derived identifier could not manage, ADR-2006).
        assert!(
            r.activity_urn.starts_with("urn:visionclaw:execution:"),
            "{}",
            r.activity_urn
        );
        assert_eq!(
            r.activity_urn,
            decision_record(&d).activity_urn,
            "correlation must be stable for a re-delivered event"
        );
        let mut resent = d.clone();
        resent.event_id = "def456".into();
        assert_ne!(
            r.activity_urn,
            decision_record(&resent).activity_urn,
            "a second signed decision on the same case must not collide"
        );
    }

    #[test]
    fn decision_record_falls_back_for_a_locally_minted_decision() {
        let d = CaseDecision {
            case_id: "vc-decelev-y".into(),
            action: "reject".into(),
            reasoning: "no".into(),
            responder_pubkey: "not-hex".into(),
            event_id: String::new(),
            created_at: 0,
        };
        let r = decision_record(&d);
        assert!(!r.attributed, "a non-hex responder is unattributed");
        assert!(r.owner_did.is_none());
        assert!(!r.writeback_triggered);
        assert!(r.decision_event_id.is_none());
        assert!(r.decision_created_at_s.is_none());
        // No signed event to correlate on: the record falls back to the tuple
        // plus `local`, which still separates distinct outcomes.
        assert!(
            r.activity_urn.starts_with("urn:visionclaw:execution:"),
            "{}",
            r.activity_urn
        );
        let mut amended = d.clone();
        amended.action = "amend".into();
        assert_ne!(r.activity_urn, decision_record(&amended).activity_urn);
    }

    /// Integration-style: a real temp SQLite file carries open cases across a
    /// process boundary and the planner resumes exactly the right work.
    #[tokio::test]
    async fn open_cases_survive_a_restart_and_plan_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("enrichment.sqlite3");
        let opened_at = now_s();
        {
            let store = DecisionElevationStore::open(&path).await.unwrap();
            for (id, _s) in [("waiting", ()), ("tracked", ()), ("done", ())] {
                store
                    .open_case(&DecisionCase::opening(
                        format!("{CASE_PREFIX}{id}"),
                        format!("urn:ngm:kg:decision:{id}"),
                        format!("decisions/{id}.md"),
                        "# decision page",
                        "a significant decision",
                        opened_at,
                    ))
                    .await
                    .unwrap();
            }
            store
                .mark_elevating(
                    &format!("{CASE_PREFIX}tracked"),
                    "https://github.com/o/r/pull/11",
                )
                .await
                .unwrap();
            store
                .mark_terminal(&format!("{CASE_PREFIX}done"), case_status::PUBLISHED)
                .await
                .unwrap();
        } // every handle dropped — the process "crashes"

        let store = DecisionElevationStore::open(&path).await.unwrap();
        let mut plan = plan_reconciliation(store.open_cases().await.unwrap(), now_s(), TTL);
        plan.sort_by_key(|a| match a {
            ReconcileAction::ResumeTracking(c)
            | ReconcileAction::ResumePr(c)
            | ReconcileAction::ResumePending(c)
            | ReconcileAction::Expire(c) => c.case_id.clone(),
        });

        assert_eq!(plan.len(), 2, "the published case is not resumed: {plan:?}");
        match &plan[0] {
            ReconcileAction::ResumeTracking(c) => {
                assert_eq!(c.case_id, format!("{CASE_PREFIX}tracked"));
                assert_eq!(c.pr_url.as_deref(), Some("https://github.com/o/r/pull/11"));
            }
            other => panic!("expected the tracked PR to resume, got {other:?}"),
        }
        match &plan[1] {
            ReconcileAction::ResumePending(c) => {
                assert_eq!(c.case_id, format!("{CASE_PREFIX}waiting"));
                assert_eq!(c.draft, "# decision page", "the draft survives a restart");
            }
            other => panic!("expected the unanswered case to re-arm, got {other:?}"),
        }
    }

    /// The same durable store, wound past the TTL, times the case out instead.
    #[tokio::test]
    async fn a_stale_stored_case_plans_an_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("enrichment.sqlite3");
        let store = DecisionElevationStore::open(&path).await.unwrap();
        store
            .open_case(&DecisionCase::opening(
                format!("{CASE_PREFIX}stale"),
                "urn:ngm:kg:decision:stale",
                "decisions/stale.md",
                "# decision page",
                "a significant decision",
                now_s() - TTL - 1,
            ))
            .await
            .unwrap();

        let plan = plan_reconciliation(store.open_cases().await.unwrap(), now_s(), TTL);
        assert!(
            matches!(plan.as_slice(), [ReconcileAction::Expire(c)] if c.case_id == format!("{CASE_PREFIX}stale")),
            "{plan:?}"
        );
    }
}
