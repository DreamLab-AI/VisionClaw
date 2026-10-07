//! WP2 — inbound settings and filter sync on the `/wss` text channel.
//!
//! Until this module the headset only *wrote* settings (physics PUTs from the
//! HUD) and never heard about anyone else's change, so a desktop session that
//! moved the repulsion or the node filter left the headset showing stale state
//! and stale button faces. The desktop handles four text frames this module
//! mirrors (`client/src/store/websocket/textMessageHandler.ts`):
//!
//! * `settingsUpdated` (ADR-2047): `{category, updatedBy, timestamp, settings?}`.
//!   A frame older than (or equal to) the last one applied for its category is
//!   stale and dropped; the writer's own echo is dropped; `nodeFilter` carries
//!   the full filter object and is applied directly; every other category is a
//!   signal to re-read that category over REST.
//! * `filter_update_success`: the server's reply to a `filter_update`.
//! * `graphUpdated`: topology changed server-side; refetch it (debounced).
//!
//! Receipt never triggers a write. The server stays authoritative and the
//! headset never echoes a received value back (that would re-broadcast and
//! fight the editor that made the change).
//!
//! Pure Rust with no Godot types, so every rule here is unit-tested headless.

use std::collections::HashMap;

use serde_json::Value;

/// The `nodeFilter` fields XR applies. Field defaults are the desktop's
/// (`useGraphFiltering.ts`: `storeNodeFilter?.x ?? default`), because a peer's
/// `nodeFilter` frame replaces the whole object on the desktop too.
/// `minConnections` / `minMaturity` are desktop-only and never on the wire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeFilter {
    pub enabled: bool,
    pub quality_threshold: f32,
    pub authority_threshold: f32,
    pub filter_by_quality: bool,
    pub filter_by_authority: bool,
    /// `filterMode == "and"`; anything else is OR (desktop + server).
    pub mode_and: bool,
    pub include_linked_pages: bool,
}

impl Default for NodeFilter {
    fn default() -> Self {
        Self {
            enabled: false,
            quality_threshold: 0.7,
            authority_threshold: 0.5,
            filter_by_quality: true,
            filter_by_authority: false,
            mode_and: false,
            include_linked_pages: false,
        }
    }
}

/// Per-node inputs to [`NodeFilter::passes`], parsed from `initialGraphLoad`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FilterInputs {
    pub quality: Option<f32>,
    pub authority: Option<f32>,
    pub linked_page: bool,
}

fn num(v: Option<&Value>) -> Option<f32> {
    v.and_then(Value::as_f64)
        .filter(|f| f.is_finite())
        .map(|f| f as f32)
}

impl NodeFilter {
    /// Build from a `settings` object. Missing or wrongly-typed fields take the
    /// desktop default rather than failing the whole frame.
    pub fn from_settings(v: &Value) -> Self {
        let d = Self::default();
        let b = |k: &str, dflt: bool| v.get(k).and_then(Value::as_bool).unwrap_or(dflt);
        Self {
            enabled: b("enabled", d.enabled),
            quality_threshold: num(v.get("qualityThreshold")).unwrap_or(d.quality_threshold),
            authority_threshold: num(v.get("authorityThreshold")).unwrap_or(d.authority_threshold),
            filter_by_quality: b("filterByQuality", d.filter_by_quality),
            filter_by_authority: b("filterByAuthority", d.filter_by_authority),
            mode_and: v.get("filterMode").and_then(Value::as_str) == Some("and"),
            include_linked_pages: b("includeLinkedPages", d.include_linked_pages),
        }
    }

    /// Desktop `visibleNodes` predicate, minus the hierarchy-expansion and
    /// degree/maturity gates XR does not carry. `degree` feeds the desktop's
    /// quality fallback (`min(1, degree / 10)`) for an unscored node; an
    /// unscored authority falls back to `1 - depth * 0.2` with depth 0 (XR has
    /// no hierarchy map), i.e. 1.0.
    pub fn passes(&self, inp: &FilterInputs, degree: u32) -> bool {
        if !self.include_linked_pages && inp.linked_page {
            return false;
        }
        if !self.enabled {
            return true;
        }
        let quality = inp
            .quality
            .unwrap_or_else(|| (degree as f32 / 10.0).min(1.0));
        let authority = inp.authority.unwrap_or(1.0);
        let pq = !self.filter_by_quality || quality >= self.quality_threshold;
        let pa = !self.filter_by_authority || authority >= self.authority_threshold;
        if self.mode_and {
            pq && pa
        } else if self.filter_by_quality || self.filter_by_authority {
            pq || pa
        } else {
            true
        }
    }
}

/// The physics fields the headset tracks for its HUD (button faces + status
/// line). Each is `Some` only when the GET body carried it as a number, so a
/// partial or older server never zeroes a tracked value.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PhysicsView {
    pub repel_k: Option<f32>,
    pub rest_length: Option<f32>,
    pub dag_bias_k: Option<f32>,
    pub dag_level_distance: Option<f32>,
    pub plane_bias_k: Option<f32>,
    pub plane_spacing: Option<f32>,
    pub axis_compression_z: Option<f32>,
}

impl PhysicsView {
    /// Parse a `GET /api/settings/physics` body (camelCase `PhysicsSettings`).
    /// `None` when the body is not a JSON object.
    pub fn parse(body: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(body).ok()?;
        if !v.is_object() {
            return None;
        }
        Some(Self {
            repel_k: num(v.get("repelK")),
            rest_length: num(v.get("restLength")),
            dag_bias_k: num(v.get("dagBiasK")),
            dag_level_distance: num(v.get("dagLevelDistance")),
            plane_bias_k: num(v.get("planeBiasK")),
            plane_spacing: num(v.get("planeSpacing")),
            axis_compression_z: num(v.get("axisCompressionZ")),
        })
    }

    /// `(name, value)` for every present field, names as GDScript keys.
    pub fn fields(&self) -> Vec<(&'static str, f32)> {
        [
            ("repel_k", self.repel_k),
            ("rest_length", self.rest_length),
            ("dag_bias_k", self.dag_bias_k),
            ("dag_level_distance", self.dag_level_distance),
            ("plane_bias_k", self.plane_bias_k),
            ("plane_spacing", self.plane_spacing),
            ("axis_compression_z", self.axis_compression_z),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|x| (k, x)))
        .collect()
    }
}

/// Why a frame was dropped. Surfaced to GDScript for the debug log only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ignored {
    /// Not JSON, not an object, or no string `type`.
    Malformed,
    /// A text frame this module does not route (broker events, acks, …).
    NotHandled,
    MissingCategory,
    OwnEcho,
    Stale,
    /// `nodeFilter` without a `settings` object.
    MissingPayload,
}

impl Ignored {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::NotHandled => "not_handled",
            Self::MissingCategory => "missing_category",
            Self::OwnEcho => "own_echo",
            Self::Stale => "stale",
            Self::MissingPayload => "missing_payload",
        }
    }
}

/// What the scene should do with one inbound text frame.
#[derive(Debug, Clone, PartialEq)]
pub enum TextEvent {
    /// Apply this filter to the draw domain (category `nodeFilter`).
    NodeFilter(NodeFilter),
    /// Re-read this settings category over REST (`physics`, `rendering`, …).
    Refetch(String),
    /// `filter_update_success` — the server applied a per-client filter.
    FilterAck {
        enabled: Option<bool>,
    },
    /// `graphUpdated` — topology changed server-side.
    GraphUpdated {
        revision: Option<i64>,
        reason: String,
    },
    Ignored(Ignored),
}

/// ADR-2047 receive state: last applied timestamp per category plus this
/// session's own pubkey for echo suppression.
#[derive(Debug, Default)]
pub struct SettingsSync {
    last_applied: HashMap<String, i64>,
    own_pubkey: String,
}

impl SettingsSync {
    pub fn new() -> Self {
        Self::default()
    }

    /// The hex pubkey this session writes as (`updatedBy` on its own echoes).
    /// Empty disables echo suppression.
    pub fn set_own_pubkey(&mut self, hex: &str) {
        self.own_pubkey = hex.trim().to_ascii_lowercase();
    }

    pub fn last_applied(&self, category: &str) -> Option<i64> {
        self.last_applied.get(category).copied()
    }

    /// Classify and admit one text frame. `now_ms` stands in for a missing
    /// `timestamp` (desktop: `Date.now()`), so an unstamped frame is always
    /// treated as current. Never panics on any input.
    pub fn handle(&mut self, text: &str, now_ms: i64) -> TextEvent {
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return TextEvent::Ignored(Ignored::Malformed);
        };
        let Some(kind) = v.get("type").and_then(Value::as_str) else {
            return TextEvent::Ignored(Ignored::Malformed);
        };
        match kind {
            "settingsUpdated" => self.admit_settings(&v, now_ms),
            "filter_update_success" => TextEvent::FilterAck {
                enabled: v
                    .get("enabled")
                    .or_else(|| v.get("data").and_then(|d| d.get("enabled")))
                    .and_then(Value::as_bool),
            },
            "graphUpdated" => TextEvent::GraphUpdated {
                revision: v.get("revision").and_then(Value::as_i64),
                reason: v
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            },
            _ => TextEvent::Ignored(Ignored::NotHandled),
        }
    }

    fn admit_settings(&mut self, v: &Value, now_ms: i64) -> TextEvent {
        let Some(category) = v
            .get("category")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
        else {
            return TextEvent::Ignored(Ignored::MissingCategory);
        };
        if let Some(by) = v.get("updatedBy").and_then(Value::as_str) {
            if !self.own_pubkey.is_empty() && by.trim().eq_ignore_ascii_case(&self.own_pubkey) {
                return TextEvent::Ignored(Ignored::OwnEcho);
            }
        }
        let ts = v.get("timestamp").and_then(Value::as_i64).unwrap_or(now_ms);
        if let Some(&last) = self.last_applied.get(category) {
            if ts <= last {
                return TextEvent::Ignored(Ignored::Stale);
            }
        }
        if category == "nodeFilter" {
            // The desktop records the timestamp before checking the payload, so a
            // payload-less frame still advances the watermark. Mirror that.
            self.last_applied.insert(category.to_string(), ts);
            return match v.get("settings").filter(|s| s.is_object()) {
                Some(s) => TextEvent::NodeFilter(NodeFilter::from_settings(s)),
                None => TextEvent::Ignored(Ignored::MissingPayload),
            };
        }
        self.last_applied.insert(category.to_string(), ts);
        TextEvent::Refetch(category.to_string())
    }
}

/// Coalesces `graphUpdated` bursts into topology refetches the server will
/// actually serve. `requestInitialData` is dropped server-side if it arrives
/// within `INITIAL_DATA_COOLDOWN` (30 s, `position_updates.rs`) of the previous
/// one on the same connection, so a naive refetch-per-frame silently loses
/// every update after the first. This gate fires the first refetch after a
/// short debounce and holds later ones until the cooldown has elapsed, keeping
/// exactly one trailing refetch pending so the last change is never missed.
#[derive(Debug, Clone)]
pub struct RefetchGate {
    debounce_ms: i64,
    cooldown_ms: i64,
    pending_since: Option<i64>,
    last_sent: Option<i64>,
    last_revision: Option<i64>,
}

/// Server cooldown plus a 1 s margin for clock and queueing skew.
pub const REFETCH_COOLDOWN_MS: i64 = 31_000;
/// Desktop `scheduleGraphRefetch` debounce scale; bursts collapse into one.
pub const REFETCH_DEBOUNCE_MS: i64 = 1_500;

impl Default for RefetchGate {
    fn default() -> Self {
        Self::new(REFETCH_DEBOUNCE_MS, REFETCH_COOLDOWN_MS)
    }
}

impl RefetchGate {
    pub fn new(debounce_ms: i64, cooldown_ms: i64) -> Self {
        Self {
            debounce_ms,
            cooldown_ms,
            pending_since: None,
            last_sent: None,
            last_revision: None,
        }
    }

    /// Record a `graphUpdated`. A revision at or below the last one seen is a
    /// duplicate or reordered frame and is ignored; `None` revisions always count.
    /// Returns whether the update was accepted.
    pub fn note(&mut self, revision: Option<i64>, now_ms: i64) -> bool {
        if let (Some(r), Some(last)) = (revision, self.last_revision) {
            if r <= last {
                return false;
            }
        }
        if revision.is_some() {
            self.last_revision = revision;
        }
        if self.pending_since.is_none() {
            self.pending_since = Some(now_ms);
        }
        true
    }

    /// Whether a refetch should be sent now. Consumes the pending update when it
    /// returns true.
    pub fn poll(&mut self, now_ms: i64) -> bool {
        let Some(since) = self.pending_since else {
            return false;
        };
        if now_ms - since < self.debounce_ms {
            return false;
        }
        if let Some(sent) = self.last_sent {
            if now_ms - sent < self.cooldown_ms {
                return false;
            }
        }
        self.pending_since = None;
        self.last_sent = Some(now_ms);
        true
    }

    /// A reconnect gets a fresh `initialGraphLoad` and a fresh server-side
    /// cooldown, so pending work and the cooldown clock both reset. The
    /// revision watermark is kept: revisions are server-global.
    pub fn reset_connection(&mut self) {
        self.pending_since = None;
        self.last_sent = None;
    }

    pub fn is_pending(&self) -> bool {
        self.pending_since.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: &str = "aa11";
    const ME: &str = "bb22";

    fn sync() -> SettingsSync {
        let mut s = SettingsSync::new();
        s.set_own_pubkey(ME);
        s
    }

    fn settings_frame(category: &str, by: &str, ts: i64, settings: Option<&str>) -> String {
        match settings {
            Some(body) => format!(
                r#"{{"type":"settingsUpdated","category":"{category}","updatedBy":"{by}","timestamp":{ts},"settings":{body}}}"#
            ),
            None => format!(
                r#"{{"type":"settingsUpdated","category":"{category}","updatedBy":"{by}","timestamp":{ts}}}"#
            ),
        }
    }

    // --- staleness / echo / malformed -------------------------------------

    #[test]
    fn stale_timestamp_is_ignored_per_category() {
        let mut s = sync();
        assert_eq!(
            s.handle(&settings_frame("physics", PEER, 100, None), 0),
            TextEvent::Refetch("physics".into())
        );
        assert_eq!(
            s.handle(&settings_frame("physics", PEER, 100, None), 0),
            TextEvent::Ignored(Ignored::Stale),
            "equal ts is stale"
        );
        assert_eq!(
            s.handle(&settings_frame("physics", PEER, 99, None), 0),
            TextEvent::Ignored(Ignored::Stale)
        );
        // Another category has its own watermark.
        assert_eq!(
            s.handle(&settings_frame("rendering", PEER, 50, None), 0),
            TextEvent::Refetch("rendering".into())
        );
        assert_eq!(
            s.handle(&settings_frame("physics", PEER, 101, None), 0),
            TextEvent::Refetch("physics".into())
        );
        assert_eq!(s.last_applied("physics"), Some(101));
    }

    #[test]
    fn missing_category_is_ignored_and_records_nothing() {
        let mut s = sync();
        for f in [
            r#"{"type":"settingsUpdated","updatedBy":"aa","timestamp":5}"#,
            r#"{"type":"settingsUpdated","category":"","timestamp":5}"#,
            r#"{"type":"settingsUpdated","category":7,"timestamp":5}"#,
        ] {
            assert_eq!(
                s.handle(f, 0),
                TextEvent::Ignored(Ignored::MissingCategory),
                "{f}"
            );
        }
        assert!(s.last_applied("").is_none());
    }

    #[test]
    fn own_echo_is_ignored_case_insensitively_and_does_not_advance_the_watermark() {
        let mut s = sync();
        assert_eq!(
            s.handle(&settings_frame("physics", "BB22", 10, None), 0),
            TextEvent::Ignored(Ignored::OwnEcho)
        );
        assert_eq!(s.last_applied("physics"), None);
        // With no own pubkey set, nothing is an echo.
        let mut anon = SettingsSync::new();
        assert_eq!(
            anon.handle(&settings_frame("physics", ME, 10, None), 0),
            TextEvent::Refetch("physics".into())
        );
    }

    #[test]
    fn malformed_payloads_never_panic() {
        let mut s = sync();
        let junk = [
            "",
            "not json",
            "null",
            "[]",
            "42",
            r#""str""#,
            "{}",
            r#"{"type":5}"#,
            r#"{"type":"settingsUpdated","category":"nodeFilter","timestamp":"soon","settings":[1,2]}"#,
            r#"{"type":"settingsUpdated","category":"nodeFilter","timestamp":1e400}"#,
            r#"{"type":"graphUpdated","revision":"x"}"#,
            r#"{"type":"filter_update_success","data":7}"#,
            "{\"type\":\"settingsUpdated\",\"category\":\"\u{0}\"}",
        ];
        for j in junk {
            let _ = s.handle(j, 1);
        }
        assert_eq!(
            s.handle("not json", 0),
            TextEvent::Ignored(Ignored::Malformed)
        );
        assert_eq!(
            s.handle(r#"{"type":"broker:new_case"}"#, 0),
            TextEvent::Ignored(Ignored::NotHandled)
        );
    }

    #[test]
    fn unstamped_frame_uses_now_and_counts_as_current() {
        let mut s = sync();
        let f = r#"{"type":"settingsUpdated","category":"physics","updatedBy":"aa"}"#;
        assert_eq!(s.handle(f, 500), TextEvent::Refetch("physics".into()));
        assert_eq!(s.last_applied("physics"), Some(500));
        assert_eq!(s.handle(f, 400), TextEvent::Ignored(Ignored::Stale));
    }

    // --- nodeFilter mapping ------------------------------------------------

    #[test]
    fn node_filter_frame_maps_every_supported_field() {
        let mut s = sync();
        let body = r#"{"enabled":true,"qualityThreshold":0.8,"authorityThreshold":0.3,"filterByQuality":false,"filterByAuthority":true,"filterMode":"and","includeLinkedPages":true,"minConnections":4}"#;
        let ev = s.handle(&settings_frame("nodeFilter", PEER, 7, Some(body)), 0);
        assert_eq!(
            ev,
            TextEvent::NodeFilter(NodeFilter {
                enabled: true,
                quality_threshold: 0.8,
                authority_threshold: 0.3,
                filter_by_quality: false,
                filter_by_authority: true,
                mode_and: true,
                include_linked_pages: true,
            })
        );
    }

    #[test]
    fn node_filter_missing_fields_take_desktop_defaults() {
        let f = NodeFilter::from_settings(
            &serde_json::json!({"enabled": true, "qualityThreshold": "high"}),
        );
        assert_eq!(
            f,
            NodeFilter {
                enabled: true,
                ..NodeFilter::default()
            }
        );
    }

    #[test]
    fn node_filter_without_settings_is_ignored_but_advances_watermark() {
        let mut s = sync();
        assert_eq!(
            s.handle(&settings_frame("nodeFilter", PEER, 9, None), 0),
            TextEvent::Ignored(Ignored::MissingPayload)
        );
        assert_eq!(s.last_applied("nodeFilter"), Some(9));
        assert_eq!(
            s.handle(&settings_frame("nodeFilter", PEER, 9, Some("{}")), 0),
            TextEvent::Ignored(Ignored::Stale)
        );
    }

    #[test]
    fn filter_predicate_matches_the_desktop() {
        let q = |x: f32| FilterInputs {
            quality: Some(x),
            ..Default::default()
        };
        // Desktop + server quirk, pinned deliberately: in OR mode a disabled
        // authority check counts as a pass, so the default quality-only OR filter
        // admits everything (`useGraphFiltering.ts` notes OR "neutralises" it).
        let or_default = NodeFilter {
            enabled: true,
            ..NodeFilter::default()
        };
        assert!(or_default.passes(&q(0.0), 0));
        // Quality-only gating needs AND mode.
        let on = NodeFilter {
            mode_and: true,
            ..or_default
        };
        assert!(on.passes(&q(0.7), 0));
        assert!(!on.passes(&q(0.69), 0));
        // Unscored quality → min(1, degree/10).
        assert!(!on.passes(&FilterInputs::default(), 6));
        assert!(on.passes(&FilterInputs::default(), 7));
        // OR with both checks off passes everything.
        let none = NodeFilter {
            filter_by_quality: false,
            ..or_default
        };
        assert!(none.passes(&q(0.0), 0));
        // AND requires both; unscored authority is 1.0.
        let and = NodeFilter {
            filter_by_authority: true,
            authority_threshold: 0.6,
            ..on
        };
        assert!(and.passes(&q(0.9), 0));
        assert!(!and.passes(
            &FilterInputs {
                quality: Some(0.9),
                authority: Some(0.5),
                linked_page: false
            },
            0
        ));
        // OR with both checks on passes on either.
        let or = NodeFilter {
            filter_by_authority: true,
            authority_threshold: 0.6,
            ..or_default
        };
        assert!(or.passes(
            &FilterInputs {
                quality: Some(0.1),
                authority: Some(0.6),
                linked_page: false
            },
            0
        ));
        assert!(!or.passes(
            &FilterInputs {
                quality: Some(0.1),
                authority: Some(0.5),
                linked_page: false
            },
            0
        ));
        // linked_page gate applies even with the filter disabled.
        let off = NodeFilter::default();
        let stub = FilterInputs {
            linked_page: true,
            ..Default::default()
        };
        assert!(!off.passes(&stub, 50));
        assert!(NodeFilter {
            include_linked_pages: true,
            ..off
        }
        .passes(&stub, 0));
    }

    // --- filter ack / graphUpdated ----------------------------------------

    #[test]
    fn filter_ack_and_graph_updated_are_classified() {
        let mut s = sync();
        assert_eq!(
            s.handle(
                r#"{"type":"filter_update_success","enabled":true,"timestamp":1}"#,
                0
            ),
            TextEvent::FilterAck {
                enabled: Some(true)
            }
        );
        assert_eq!(
            s.handle(
                r#"{"type":"filter_update_success","data":{"visible_nodes":3}}"#,
                0
            ),
            TextEvent::FilterAck { enabled: None }
        );
        assert_eq!(
            s.handle(
                r#"{"type":"graphUpdated","revision":12,"reason":"sync"}"#,
                0
            ),
            TextEvent::GraphUpdated {
                revision: Some(12),
                reason: "sync".into()
            }
        );
    }

    // --- physics read-back -------------------------------------------------

    #[test]
    fn physics_view_takes_present_numeric_fields_only() {
        let v = PhysicsView::parse(
            r#"{"repelK":250.5,"restLength":90,"dagBiasK":0.6,"dagLevelDistance":"80","planeSpacing":null,"axisCompressionZ":0.3,"damping":0.9}"#,
        )
        .unwrap();
        assert_eq!(v.repel_k, Some(250.5));
        assert_eq!(v.rest_length, Some(90.0));
        assert_eq!(v.dag_bias_k, Some(0.6));
        assert_eq!(v.dag_level_distance, None, "string is not a number here");
        assert_eq!(v.plane_spacing, None);
        assert_eq!(v.plane_bias_k, None);
        assert_eq!(v.axis_compression_z, Some(0.3));
        assert_eq!(v.fields().len(), 4);
        assert!(PhysicsView::parse("[1]").is_none());
        assert!(PhysicsView::parse("<html>").is_none());
    }

    // --- refetch gate ------------------------------------------------------

    #[test]
    fn refetch_gate_debounces_then_respects_the_server_cooldown() {
        let mut g = RefetchGate::new(1_000, 30_000);
        assert!(!g.poll(0), "nothing pending");
        assert!(g.note(Some(1), 0));
        assert!(g.note(Some(2), 200), "burst coalesces");
        assert!(!g.poll(900), "still debouncing");
        assert!(g.poll(1_000), "first refetch fires after the debounce");
        assert!(!g.poll(1_001), "consumed");
        // An update inside the cooldown is held, not lost.
        assert!(g.note(Some(3), 5_000));
        assert!(!g.poll(20_000));
        assert!(g.is_pending());
        assert!(g.poll(31_000), "trailing refetch once the cooldown elapses");
        assert!(!g.is_pending());
    }

    #[test]
    fn refetch_gate_drops_duplicate_revisions_and_resets_on_reconnect() {
        let mut g = RefetchGate::new(0, 30_000);
        assert!(g.note(Some(5), 0));
        assert!(g.poll(0));
        assert!(!g.note(Some(5), 10), "duplicate revision");
        assert!(!g.note(Some(4), 10), "reordered older revision");
        assert!(g.note(None, 10), "unrevisioned update always counts");
        g.reset_connection();
        assert!(!g.is_pending());
        assert!(g.note(Some(6), 20));
        assert!(g.poll(20), "fresh connection, fresh cooldown");
    }
}
