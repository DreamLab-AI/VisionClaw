//! Jev option for the client command surfaces (VC-06-S1-01, VC-06-S1-02,
//! VC-34-S1-01, VC-34-S1-02).
//!
//! Pure question builders and answer interpreters plus two async entry points
//! ([`understand_command`], [`rank_palette`]) that wrap
//! [`crate::services::system_one::JevClient`]. Both entry points return
//! `Err(FallbackReason)` on every failure; the HTTP handler turns that into
//! `{"used": false, "reason": …}` and the browser keeps its pre-change
//! behaviour. Jev only *suggests*: the client assembles any settings patch
//! deterministically, the user reviews and applies it through the existing
//! authenticated `/api/settings/*` path, and palette suggestions run the
//! normal registry handler.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::services::system_one::{FallbackReason, JevClient, JevItem, Question};

/// Question-set version for the command bar fan-out (logged on fallback).
pub const COMMAND_QUESTION_SET: &str = "vc-command-v1";
/// Question-set version for palette ranking.
pub const PALETTE_QUESTION_SET: &str = "vc-palette-v1";

/// Candidate setting count ceiling (Choice allows 255; the shortlist is
/// pre-narrowed client-side, this bounds the request regardless).
pub const MAX_SETTING_CANDIDATES: usize = 60;
/// Palette entries ceiling.
pub const MAX_PALETTE_CANDIDATES: usize = 40;
/// Longest command / query text forwarded.
pub const MAX_TEXT_CHARS: usize = 500;
const MAX_FIELD_CHARS: usize = 200;

/// Typed command-bar intents (VC-34-S1-01). Typed IDs, never endpoints.
pub const INTENTS: [&str; 6] =
    ["settings_change", "graph_search", "view_save_load", "analytics", "help", "none"];
/// Action classes (VC-06-S1-02).
pub const ACTION_CLASSES: [&str; 4] =
    ["persistent_setting", "temporary_physics_action", "saved_view_action", "clarify"];
/// The key offered when no candidate setting fits.
pub const NO_SETTING: &str = "none";

/// A visible, non-secret setting offered as a candidate.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettingCandidate {
    /// Dotted settings path.
    pub path: String,
    /// Human label.
    #[serde(default)]
    pub label: String,
    /// Control type (slider, toggle, …).
    #[serde(default, rename = "type")]
    pub kind: String,
    /// Minimum for ranged settings.
    #[serde(default)]
    pub min: Option<f64>,
    /// Maximum for ranged settings.
    #[serde(default)]
    pub max: Option<f64>,
    /// Current value.
    #[serde(default)]
    pub current: Option<Value>,
    /// Public description.
    #[serde(default)]
    pub description: String,
}

/// Request body of `POST /api/jev/command`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRequest {
    /// The raw command-bar text.
    pub command: String,
    /// Shortlisted visible settings (client pre-narrows lexically).
    #[serde(default)]
    pub candidates: Vec<SettingCandidate>,
}

/// One scored suggestion.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Suggestion {
    /// Typed identifier (intent id, action class or settings path).
    pub id: String,
    /// Choice confidence in `[0,1]`.
    pub confidence: f64,
}

/// What Jev understood about a command. Every field is optional: a field is
/// present only when its question was asked, answered validly and cleared the
/// suggestion threshold.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommandUnderstanding {
    /// VC-34-S1-01 typed intent.
    pub intent: Option<Suggestion>,
    /// VC-06-S1-02 action class.
    pub action_class: Option<Suggestion>,
    /// VC-06-S1-01 target setting (never `none`; never a protected path).
    pub setting: Option<Suggestion>,
    /// VC-06-S1-01 probability that a needed value/direction is missing.
    pub value_missing: Option<f64>,
}

/// One palette entry offered for ranking.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct PaletteEntry {
    /// Registry id (echoed back; the client maps it to its own handler).
    pub id: String,
    /// Title.
    pub title: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Category name.
    #[serde(default)]
    pub category: String,
}

/// Request body of `POST /api/jev/palette`.
#[derive(Debug, Clone, Deserialize)]
pub struct PaletteRequest {
    /// The search text.
    pub query: String,
    /// Currently enabled commands only.
    #[serde(default)]
    pub commands: Vec<PaletteEntry>,
}

/// One ranked entry.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RankedCommand {
    /// Registry id.
    pub id: String,
    /// Expected relevance score (0–2).
    pub score: f64,
}

/// Ranked palette suggestions.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PaletteRanking {
    /// Entries ordered best-first. Empty when Jev judged nothing answers.
    pub ranked: Vec<RankedCommand>,
    /// `true` when Jev judged that no entry answers the query.
    pub no_match: bool,
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Settings that must never be offered to a model or patched from a text box:
/// credentials, identity and auth material. Matching is by path segment text.
pub fn is_protected_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    ["secret", "token", "password", "passwd", "apikey", "api_key", "credential", "privatekey", "private_key", "nsec", "auth", "nostr", "pubkey", "session"]
        .iter()
        .any(|w| p.contains(w))
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 120
        && path.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '[' | ']'))
}

/// Keep only well-formed, non-protected, de-duplicated candidates within the
/// ceiling. Order is preserved.
pub fn sanitise_candidates(input: &[SettingCandidate]) -> Vec<SettingCandidate> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for c in input {
        if out.len() >= MAX_SETTING_CANDIDATES {
            break;
        }
        if !valid_path(&c.path) || is_protected_path(&c.path) || c.path == NO_SETTING {
            continue;
        }
        if !seen.insert(c.path.clone()) {
            continue;
        }
        let mut c = c.clone();
        c.label = clip(&c.label, MAX_FIELD_CHARS);
        c.kind = clip(&c.kind, 32);
        c.description = clip(&c.description, MAX_FIELD_CHARS);
        if let Some(Value::String(s)) = &c.current {
            c.current = Some(Value::String(clip(s, 64)));
        }
        out.push(c);
    }
    out
}

fn intent_question() -> Question {
    Question::Choice {
        instructions: "Which kind of request is `command`, a short instruction typed into the command bar of a graph-visualisation app? Choose the single best fit. Choose `none` when the text is not a request this app can serve, is a negation of an action (for example 'do not hide the labels'), or is too vague to pick.".into(),
        criteria: vec![
            ("settings_change".into(), "The user wants to change how the view, graph or physics looks or behaves: a setting value, a toggle, a colour, a size or a force. Not for searching or for saving a view.".into()),
            ("graph_search".into(), "The user wants to find or focus on particular nodes, edges or entities in the graph by name or property.".into()),
            ("view_save_load".into(), "The user wants to save the current camera/layout view or restore a previously saved one.".into()),
            ("analytics".into(), "The user wants analytics about the graph such as clusters, centrality, counts or statistics.".into()),
            ("help".into(), "The user is asking how to do something, what a control does, or for the list of commands.".into()),
            ("none".into(), "None of the other options applies: unsupported, negated, off-topic or too vague.".into()),
        ],
    }
}

fn action_class_question() -> Question {
    Question::Choice {
        instructions: "What kind of effect does `command` ask for? `persistent_setting` changes a saved user preference that stays after reload. `temporary_physics_action` is a one-off action on the running simulation (pause, resume, reheat, re-run layout, reset the layout) that is not saved as a preference. `saved_view_action` saves or restores a named view. Choose `clarify` when the text could mean more than one of these (for example 'reset' alone, or 'slow it down' which could be a saved physics preference or a pause) or when it fits none.".into(),
        criteria: vec![
            ("persistent_setting".into(), "Change a saved preference or default value, e.g. 'set repulsion to 200', 'always show labels'.".into()),
            ("temporary_physics_action".into(), "A one-off simulation action that is not saved: pause, resume, restart or reset the layout positions only.".into()),
            ("saved_view_action".into(), "Save, name, load or restore a saved view.".into()),
            ("clarify".into(), "Ambiguous between the other options, or none of them applies.".into()),
        ],
    }
}

fn setting_question(cands: &[SettingCandidate]) -> Question {
    let mut criteria: Vec<(String, String)> = cands
        .iter()
        .map(|c| {
            let range = match (c.min, c.max) {
                (Some(a), Some(b)) => format!(" Range {a}..{b}."),
                _ => String::new(),
            };
            let desc = if c.description.is_empty() { String::new() } else { format!(" {}", c.description) };
            (c.path.clone(), format!("{} ({}).{}{}", c.label, c.kind, range, desc))
        })
        .collect();
    criteria.push((NO_SETTING.into(), "None of the listed settings is what the user wants to change.".into()));
    Question::Choice {
        instructions: "Which listed setting does `command` ask to change? Choose the one setting the text names or clearly implies. Choose `none` when no listed setting fits, when the text only asks a question, or when it does not ask for a change.".into(),
        criteria,
    }
}

fn value_missing_question() -> Question {
    Question::Noul {
        instructions: "The text `command` asks to change a setting but gives neither a concrete value (a number, an on/off state, a named option) nor a clear direction such as 'more', 'less', 'bigger' or 'smaller'. Answer yes when a needed value or direction is missing.".into(),
    }
}

/// Ask the command-bar questions enabled by flag. `Err` ⇒ no usable
/// understanding; keep the pre-change path.
pub async fn understand_command(
    client: &JevClient,
    req: &CommandRequest,
) -> Result<CommandUnderstanding, FallbackReason> {
    let want_intent = client.enabled(JevItem::CommandIntent);
    let want_class = client.enabled(JevItem::ActionClass);
    let want_setting = client.enabled(JevItem::SettingsPatch);
    let lead = if want_intent {
        JevItem::CommandIntent
    } else if want_class {
        JevItem::ActionClass
    } else if want_setting {
        JevItem::SettingsPatch
    } else {
        return Err(FallbackReason::FlagOff);
    };
    let command = clip(req.command.trim(), MAX_TEXT_CHARS);
    if command.is_empty() {
        return Err(FallbackReason::Malformed);
    }
    let cands = if want_setting { sanitise_candidates(&req.candidates) } else { Vec::new() };
    let want_setting = want_setting && !cands.is_empty();

    let mut qs: Vec<(String, Question)> = Vec::new();
    if want_intent {
        qs.push(("intent".into(), intent_question()));
    }
    if want_class {
        qs.push(("action_class".into(), action_class_question()));
    }
    if want_setting {
        qs.push(("setting".into(), setting_question(&cands)));
        qs.push(("value_missing".into(), value_missing_question()));
    }
    if qs.is_empty() {
        return Err(FallbackReason::FlagOff);
    }
    let answers = client
        .ask(lead, COMMAND_QUESTION_SET, false, json!({ "command": command }), &qs)
        .await?;

    let mut out = CommandUnderstanding::default();
    let mut any = false;
    let suggest = |c: &crate::services::system_one::ChoiceAnswer| client.accept_suggestion(c.confidence).is_ok();
    if want_intent {
        if let Ok(c) = answers.choice("intent", &INTENTS) {
            if c.choice != "none" && suggest(&c) {
                out.intent = Some(Suggestion { id: c.choice, confidence: c.confidence });
                any = true;
            }
        }
    }
    if want_class {
        if let Ok(c) = answers.choice("action_class", &ACTION_CLASSES) {
            if suggest(&c) {
                out.action_class = Some(Suggestion { id: c.choice, confidence: c.confidence });
                any = true;
            }
        }
    }
    if want_setting {
        let mut offered: Vec<&str> = cands.iter().map(|c| c.path.as_str()).collect();
        offered.push(NO_SETTING);
        if let Ok(c) = answers.choice("setting", &offered) {
            if c.choice != NO_SETTING && suggest(&c) {
                out.setting = Some(Suggestion { id: c.choice, confidence: c.confidence });
                out.value_missing = answers.noul("value_missing").ok();
                any = true;
            }
        }
    }
    if any {
        Ok(out)
    } else {
        // Nothing parsed or nothing cleared the threshold.
        Err(FallbackReason::LowConfidence)
    }
}

/// Ask Jev to rank the currently enabled palette commands for `req.query`.
pub async fn rank_palette(
    client: &JevClient,
    req: &PaletteRequest,
) -> Result<PaletteRanking, FallbackReason> {
    if !client.enabled(JevItem::PaletteSearch) {
        return Err(FallbackReason::FlagOff);
    }
    let query = clip(req.query.trim(), MAX_TEXT_CHARS);
    let entries: Vec<&PaletteEntry> = req
        .commands
        .iter()
        .filter(|e| !e.id.is_empty() && e.id.len() <= 120)
        .take(MAX_PALETTE_CANDIDATES)
        .collect();
    if query.is_empty() || entries.is_empty() {
        return Err(FallbackReason::Malformed);
    }
    let state = json!({
        "query": query,
        "commands": entries.iter().enumerate().map(|(i, e)| json!({
            "ref": format!("c{i}"),
            "title": clip(&e.title, MAX_FIELD_CHARS),
            "description": clip(&e.description, MAX_FIELD_CHARS),
            "category": clip(&e.category, 64),
        })).collect::<Vec<_>>(),
    });
    let mut qs: Vec<(String, Question)> = Vec::new();
    for i in 0..entries.len() {
        qs.push((
            format!("c{i}"),
            Question::Score {
                instructions: format!("How well does the command in `commands` whose ref is c{i} serve the intent expressed by `query`?"),
                criteria: vec![
                    "Unrelated to what the query asks for".into(),
                    "Related but would not accomplish what the query asks".into(),
                    "Directly accomplishes what the query asks".into(),
                ],
            },
        ));
    }
    qs.push((
        "any".into(),
        Question::Noul {
            instructions: "At least one command listed in `commands` accomplishes what `query` asks for.".into(),
        },
    ));
    let answers = client.ask(JevItem::PaletteSearch, PALETTE_QUESTION_SET, false, state, &qs).await?;
    let any = answers.noul("any")?;
    let mut ranked = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let s = answers.score(&format!("c{i}"), 3)?;
        if client.accept_suggestion(s.confidence).is_ok() && s.score >= 1.5 {
            ranked.push(RankedCommand { id: e.id.clone(), score: s.score });
        }
    }
    if any < 0.5 || ranked.is_empty() {
        return Ok(PaletteRanking { ranked: Vec::new(), no_match: true });
    }
    ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PaletteRanking { ranked, no_match: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::system_one::testing::*;
    use crate::services::system_one::{JevSettings, TransportError};
    use std::sync::Arc;

    fn cands() -> Vec<SettingCandidate> {
        vec![
            SettingCandidate { path: "visualisation.graphs.knowledge.physics.repelK".into(), label: "Repulsion".into(), kind: "slider".into(), min: Some(0.0), max: Some(1000.0), current: Some(json!(100)), description: "Node repulsion force".into() },
            SettingCandidate { path: "visualisation.nodes.size".into(), label: "Node size".into(), kind: "slider".into(), min: Some(0.1), max: Some(10.0), current: Some(json!(1)), description: "".into() },
            SettingCandidate { path: "auth.nostr.privateKey".into(), label: "Key".into(), kind: "text".into(), min: None, max: None, current: None, description: "".into() },
        ]
    }

    fn resp(items: Value) -> String {
        json!({"model":"m","answers":items,"usage":{"input_tokens":1,"output_tokens":1}}).to_string()
    }
    fn ch(c: &str, conf: f64) -> Value {
        json!({"type":"choice","choice":c,"confidence":conf,"probabilities":{c:1.0}})
    }

    fn client(script: Script, extra: &[(&str, &str)]) -> (JevClient, Arc<MockTransport>) {
        let t = MockTransport::new(script);
        (JevClient::with_transport(enabled_settings(extra), t.clone()), t)
    }

    fn req() -> CommandRequest {
        CommandRequest { command: "make the nodes bigger".into(), candidates: cands() }
    }

    #[test]
    fn protected_paths_never_offered() {
        let s = sanitise_candidates(&cands());
        assert_eq!(s.len(), 2);
        assert!(s.iter().all(|c| !c.path.contains("auth")));
        let bad = vec![SettingCandidate { path: "a b".into(), ..cands()[0].clone() }];
        assert!(sanitise_candidates(&bad).is_empty());
    }

    #[tokio::test]
    async fn wire_shape_fan_out() {
        let (c, t) = client(Script::Reply(200, resp(json!({
            "intent": ch("settings_change", 0.9),
            "action_class": ch("persistent_setting", 0.9),
            "setting": ch("visualisation.nodes.size", 0.8),
            "value_missing": {"type":"noul","noul":0.1}
        }))), &[]);
        let u = understand_command(&c, &req()).await.unwrap();
        let body = t.last_body().unwrap();
        let qs = &body["questions"];
        for k in ["intent", "action_class", "setting"] {
            assert!(qs[k]["criteria"].is_object(), "{k} criteria must be a map");
        }
        assert_eq!(qs["value_missing"]["type"], "noul");
        // `none` is offered, protected path is not.
        assert!(qs["setting"]["criteria"].get("none").is_some());
        assert!(qs["setting"]["criteria"].get("auth.nostr.privateKey").is_none());
        assert!(qs["intent"]["criteria"].get("none").is_some());
        assert_eq!(body["state"]["command"], "make the nodes bigger");
        // Accept path.
        assert_eq!(u.intent.unwrap().id, "settings_change");
        assert_eq!(u.action_class.unwrap().id, "persistent_setting");
        assert_eq!(u.setting.unwrap().id, "visualisation.nodes.size");
        assert!(u.value_missing.unwrap() < 0.2);
    }

    #[tokio::test]
    async fn only_enabled_items_are_asked() {
        let (c, t) = client(
            Script::Reply(200, resp(json!({"intent": ch("help", 0.9)}))),
            &[("VISIONCLAW_JEV_ACTION_CLASS", "0"), ("VISIONCLAW_JEV_SETTINGS_PATCH", "0")],
        );
        let u = understand_command(&c, &req()).await.unwrap();
        let body = t.last_body().unwrap();
        assert!(body["questions"].get("action_class").is_none());
        assert!(body["questions"].get("setting").is_none());
        assert_eq!(u.intent.unwrap().id, "help");
        assert!(u.setting.is_none());
    }

    #[tokio::test]
    async fn command_fallbacks_are_all_errors() {
        let ok = resp(json!({"intent": ch("help", 0.9), "action_class": ch("clarify", 0.9), "setting": ch("none", 0.9), "value_missing": {"type":"noul","noul":0.5}}));
        // flag off: nothing sent
        let t = MockTransport::new(Script::Reply(200, ok.clone()));
        let off = JevClient::with_transport(JevSettings::disabled(), t.clone());
        assert_eq!(understand_command(&off, &req()).await.unwrap_err(), FallbackReason::FlagOff);
        assert_eq!(t.calls(), 0);
        // no endpoint
        let t = MockTransport::new(Script::Reply(200, ok.clone()));
        let s = JevSettings::from_lookup(&|k| (k == "VISIONCLAW_JEV").then(|| "1".to_string()));
        let c = JevClient::with_transport(s, t.clone());
        assert_eq!(understand_command(&c, &req()).await.unwrap_err(), FallbackReason::NoEndpoint);
        assert_eq!(t.calls(), 0);
        for (script, want) in [
            (Script::Fail(TransportError::Timeout), FallbackReason::Timeout),
            (Script::Reply(503, "{}".into()), FallbackReason::Http(503)),
            (Script::Reply(200, "nope".into()), FallbackReason::Malformed),
            // foreign choice keys parse as malformed per question → nothing usable
            (Script::Reply(200, resp(json!({"intent": ch("rm_rf", 0.99)}))), FallbackReason::LowConfidence),
            // low confidence everywhere
            (Script::Reply(200, resp(json!({"intent": ch("help", 0.1), "action_class": ch("persistent_setting", 0.2), "setting": ch("visualisation.nodes.size", 0.3), "value_missing": {"type":"noul","noul":0.5}}))), FallbackReason::LowConfidence),
        ] {
            let (c, _) = client(script, &[]);
            assert_eq!(understand_command(&c, &req()).await.unwrap_err(), want);
        }
    }

    #[tokio::test]
    async fn none_choices_are_not_suggestions() {
        let (c, _) = client(Script::Reply(200, resp(json!({"intent": ch("none", 0.99), "action_class": ch("clarify", 0.9), "setting": ch("none", 0.99), "value_missing": {"type":"noul","noul":0.5}}))), &[]);
        let u = understand_command(&c, &req()).await.unwrap();
        assert!(u.intent.is_none() && u.setting.is_none());
        assert_eq!(u.action_class.unwrap().id, "clarify");
    }

    fn pal() -> PaletteRequest {
        PaletteRequest {
            query: "undo my last change".into(),
            commands: vec![
                PaletteEntry { id: "settings.undo".into(), title: "Undo".into(), description: "Revert settings".into(), category: "settings".into() },
                PaletteEntry { id: "view.theme.toggle".into(), title: "Toggle theme".into(), description: "".into(), category: "view".into() },
            ],
        }
    }
    fn sc(s: f64, c: f64) -> Value { json!({"type":"score","score":s,"confidence":c,"legend":{}}) }

    #[tokio::test]
    async fn palette_wire_and_accept() {
        let (c, t) = client(Script::Reply(200, resp(json!({
            "c0": sc(1.9, 0.9), "c1": sc(0.2, 0.9), "any": {"type":"noul","noul":0.95}
        }))), &[]);
        let r = rank_palette(&c, &pal()).await.unwrap();
        assert_eq!(r.ranked.len(), 1);
        assert_eq!(r.ranked[0].id, "settings.undo");
        let body = t.last_body().unwrap();
        assert!(body["questions"]["c0"]["criteria"].is_array());
        assert_eq!(body["questions"]["c0"]["criteria"].as_array().unwrap().len(), 3);
        assert_eq!(body["questions"]["any"]["type"], "noul");
        assert_eq!(body["state"]["commands"][0]["ref"], "c0");
    }

    #[tokio::test]
    async fn palette_no_match_and_fallbacks() {
        let (c, _) = client(Script::Reply(200, resp(json!({"c0": sc(1.9, 0.9), "c1": sc(1.9, 0.9), "any": {"type":"noul","noul":0.05}}))), &[]);
        let r = rank_palette(&c, &pal()).await.unwrap();
        assert!(r.no_match && r.ranked.is_empty());
        for (script, want) in [
            (Script::Fail(TransportError::Timeout), FallbackReason::Timeout),
            (Script::Reply(429, "{}".into()), FallbackReason::Http(429)),
            (Script::Reply(200, "x".into()), FallbackReason::Malformed),
            // out-of-range score is malformed
            (Script::Reply(200, resp(json!({"c0": sc(5.0, 0.9), "c1": sc(0.0, 0.9), "any": {"type":"noul","noul":0.9}}))), FallbackReason::Malformed),
        ] {
            let (c, _) = client(script, &[]);
            assert_eq!(rank_palette(&c, &pal()).await.unwrap_err(), want);
        }
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let off = JevClient::with_transport(JevSettings::disabled(), t.clone());
        assert_eq!(rank_palette(&off, &pal()).await.unwrap_err(), FallbackReason::FlagOff);
        assert_eq!(t.calls(), 0);
    }

    #[tokio::test]
    async fn palette_low_confidence_scores_are_dropped() {
        let (c, _) = client(Script::Reply(200, resp(json!({"c0": sc(1.9, 0.1), "c1": sc(1.9, 0.1), "any": {"type":"noul","noul":0.9}}))), &[]);
        let r = rank_palette(&c, &pal()).await.unwrap();
        assert!(r.ranked.is_empty() && r.no_match);
    }
}
