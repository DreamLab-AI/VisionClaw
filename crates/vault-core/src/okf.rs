//! OKF v0.2 lifecycle and trust vocabulary (contract C1 `okf:`).
//!
//! Five frontmatter keys carry the whole governance model:
//!
//! | key | meaning |
//! |---|---|
//! | `status` | [`Status`] — `draft`, `stable` or `deprecated` |
//! | `stale_after` | the date past which the claim must be re-verified |
//! | `generated` | one [`Stamp`] — who produced the page and when |
//! | `verified` | a list of [`Stamp`]s — who attested it and when |
//! | `sources` | a list of [`Source`]s — where the content came from |
//!
//! An [`Actor`] is always prefix-qualified, so a human signature can never be
//! confused with a process stamp: `human:<npub>`, `did:nostr:<64 hex>`,
//! `process:<name>/<version>`, `agent:<producer>/<version>`.
//!
//! Vocabulary 3 (ADR-VG-011) names the principal who is answerable in `by` as a
//! `did:nostr:` key and the instrument that did the work in `via`
//! (`agent:`/`process:`); vocabulary 2 wrote `human:<npub>`. Both read here.
//! [`crate::roster::Roster`] resolves either spelling to one principal and
//! decides which principals are human.
//!
//! Nothing is dropped without a word: a stamp or source that cannot be read
//! stays out of the typed fields but is listed in [`OkfBlock::problems`].

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_yaml::Value as Yaml;

use crate::frontmatter::{Frontmatter, Wikilink};

/// The lifecycle state of a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Authored or machine-proposed, not yet attested.
    Draft,
    /// Attested and current.
    Stable,
    /// Withdrawn; kept for provenance, never served as current.
    Deprecated,
}

impl Status {
    /// The frontmatter spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Stable => "stable",
            Self::Deprecated => "deprecated",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Status {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "draft" => Ok(Self::Draft),
            "stable" => Ok(Self::Stable),
            "deprecated" => Ok(Self::Deprecated),
            other => Err(format!(
                "unknown status {other:?}; expected draft, stable or deprecated"
            )),
        }
    }
}

/// Who did something: a human, a deterministic process, or an agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Actor {
    /// A person, identified by their Nostr `npub` (vocabulary 2), or by a name
    /// the roster knows (`human:jjohare`).
    Human(String),
    /// A principal key: the 64 lowercase hex characters of `did:nostr:<hex>`
    /// (vocabulary 3, ADR-VG-011). Whether it is a person is the roster's
    /// answer ([`crate::roster::Roster::is_human`]), not the spelling's.
    Did(String),
    /// A deterministic program: name and version.
    Process {
        /// The program's name, e.g. `vault`.
        name: String,
        /// Its version, e.g. `1.0`.
        version: String,
    },
    /// A model-driven agent: producer and version.
    Agent {
        /// The producing system, e.g. `visionclaw`.
        producer: String,
        /// Its version.
        version: String,
    },
}

impl Actor {
    /// The canonical frontmatter spelling.
    #[must_use]
    pub fn as_string(&self) -> String {
        match self {
            Self::Human(npub) => format!("human:{npub}"),
            Self::Did(hex) => format!("{}{hex}", crate::principal::DID_NOSTR),
            Self::Process { name, version } => format!("process:{name}/{version}"),
            Self::Agent { producer, version } => format!("agent:{producer}/{version}"),
        }
    }

    /// `true` when the spelling itself says this actor is a person — the only
    /// kind whose attestation satisfies the human gate (decision Q6). A
    /// `did:nostr:` actor is **not** human by spelling; ask
    /// [`crate::roster::Roster::is_human`], which follows the roster's `kind`.
    #[must_use]
    pub fn is_human(&self) -> bool {
        matches!(self, Self::Human(_))
    }

    /// The principal key this actor spells, as `did:nostr:<hex>`: itself for a
    /// [`Actor::Did`], the decoded key for a `human:<npub>`. `None` for an
    /// instrument, and for a `human:` name that is not an npub.
    ///
    /// ```
    /// use vault_core::Actor;
    /// let npub: Actor = "human:npub16kv2h3hdalphuyzj8tkycq8cm9n54ymer23558ds80afkc9e7frqwrawzj".parse().unwrap();
    /// let did: Actor = "did:nostr:d598abc6edefc37e10523aec4c00f8d9674a93791aa34a1db03bfa9b60b9f246".parse().unwrap();
    /// assert_eq!(npub.principal_did(), did.principal_did());
    /// assert_eq!("human:jjohare".parse::<Actor>().unwrap().principal_did(), None);
    /// ```
    #[must_use]
    pub fn principal_did(&self) -> Option<String> {
        match self {
            Self::Did(hex) => Some(format!("{}{hex}", crate::principal::DID_NOSTR)),
            Self::Human(npub) => crate::principal::npub_to_hex(npub)
                .ok()
                .map(|hex| format!("{}{hex}", crate::principal::DID_NOSTR)),
            Self::Process { .. } | Self::Agent { .. } => None,
        }
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_string())
    }
}

impl From<Actor> for String {
    fn from(a: Actor) -> Self {
        a.as_string()
    }
}

impl TryFrom<String> for Actor {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl FromStr for Actor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if let Some(npub) = s.strip_prefix("human:") {
            if npub.is_empty() {
                return Err("human actor has no npub".into());
            }
            return Ok(Self::Human(npub.to_owned()));
        }
        if let Some(rest) = s.strip_prefix("did:") {
            let Some(body) = rest.strip_prefix("nostr:") else {
                return Err(format!("DID method of {s:?} is not nostr"));
            };
            if !crate::principal::is_hex64(body) {
                return Err(format!(
                    "{s:?} must be did:nostr: followed by 64 lowercase hex characters"
                ));
            }
            return Ok(Self::Did(body.to_owned()));
        }
        let split_versioned = |rest: &str, kind: &str| -> Result<(String, String), String> {
            rest.rsplit_once('/')
                .filter(|(n, v)| !n.is_empty() && !v.is_empty())
                .map(|(n, v)| (n.to_owned(), v.to_owned()))
                .ok_or_else(|| format!("{kind} actor {rest:?} must be <name>/<version>"))
        };
        if let Some(rest) = s.strip_prefix("process:") {
            let (name, version) = split_versioned(rest, "process")?;
            return Ok(Self::Process { name, version });
        }
        if let Some(rest) = s.strip_prefix("agent:") {
            let (producer, version) = split_versioned(rest, "agent")?;
            return Ok(Self::Agent { producer, version });
        }
        Err(format!(
            "actor {s:?} must start with human:, did:nostr:, process: or agent:"
        ))
    }
}

/// A `{ by: <actor>, at: <datetime> }` pair — the shape of both `generated`
/// and each entry of `verified`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    /// Who.
    pub by: Actor,
    /// When, as an ISO-8601 instant.
    pub at: String,
    /// Review scope, content binding and future metadata preserved verbatim.
    #[serde(flatten)]
    pub extensions: std::collections::BTreeMap<String, Yaml>,
}

impl Stamp {
    /// Build a stamp.
    #[must_use]
    pub fn new(by: Actor, at: impl Into<String>) -> Self {
        Self {
            by,
            at: at.into(),
            extensions: std::collections::BTreeMap::default(),
        }
    }

    /// Render as an inline YAML mapping value.
    #[must_use]
    pub fn to_yaml(&self) -> Yaml {
        let mut m = serde_yaml::Mapping::new();
        m.insert(Yaml::String("by".into()), Yaml::String(self.by.as_string()));
        m.insert(Yaml::String("at".into()), Yaml::String(self.at.clone()));
        for (key, value) in &self.extensions {
            if key != "by" && key != "at" {
                m.insert(Yaml::String(key.clone()), value.clone());
            }
        }
        Yaml::Mapping(m)
    }

    /// The instrument that did the work (`via`, ADR-VG-011 §1), when the stamp
    /// names one and it parses as an `agent:` or `process:` actor. The value is
    /// kept verbatim in [`Stamp::extensions`] either way.
    #[must_use]
    pub fn via(&self) -> Option<Actor> {
        self.extensions
            .get("via")
            .and_then(Yaml::as_str)
            .and_then(|s| s.parse().ok())
    }

    /// Read a stamp from a YAML mapping, returning `None` when either field is
    /// missing or the actor is unreadable. [`Stamp::read`] says which.
    #[must_use]
    pub fn from_yaml(v: &Yaml) -> Option<Self> {
        Self::read(v).ok()
    }

    /// Read a stamp from a YAML mapping.
    ///
    /// # Errors
    /// Why the value is not a stamp: not a mapping, no `by`, no `at`, or a `by`
    /// that is not a readable actor (a `did:nostr:` with a non-hex body, an
    /// unknown prefix). The caller reports it; it is never swallowed.
    pub fn read(v: &Yaml) -> Result<Self, String> {
        let m = v
            .as_mapping()
            .ok_or_else(|| "a stamp is a mapping with `by` and `at`".to_owned())?;
        let by = m
            .get(Yaml::String("by".into()))
            .and_then(Yaml::as_str)
            .ok_or_else(|| "no `by`".to_owned())?
            .parse()?;
        let at = m
            .get(Yaml::String("at".into()))
            .and_then(Yaml::as_str)
            .ok_or_else(|| "no `at`".to_owned())?
            .to_owned();
        let extensions = m
            .iter()
            .filter_map(|(key, value)| {
                let key = key.as_str()?;
                (!matches!(key, "by" | "at")).then(|| (key.to_owned(), value.clone()))
            })
            .collect();
        Ok(Self { by, at, extensions })
    }
}

/// A `sources:` entry — the provenance link that absorbed `elevatedFrom`.
///
/// Vocabulary 3 (ADR-VG-010 §5) types each source: `rel` says what the link is
/// (`origin`, `episode`, `evidence`, `derived-from`, `standard`, `reference`),
/// `at` dates the claim, and `locator` says what inside the target is cited,
/// including a `sha256-v1:` pin. A reference source may be id-only. Every
/// field, and every field this type does not model, survives a read and a
/// write unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// A short local identifier, e.g. `origin`.
    pub id: String,
    /// The source, usually a wikilink into `working/`; absent on an id-only
    /// reference source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// The relation type, as written. Absent reads as `reference`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rel: Option<String>,
    /// The claim or publication date, as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// What inside the target is cited (a page anchor, an `fp:` fingerprint, a
    /// `sha256-v1:` pin), as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    /// Every other field, preserved verbatim.
    #[serde(flatten)]
    pub extensions: std::collections::BTreeMap<String, Yaml>,
}

impl Source {
    /// A source with the two original fields and nothing else.
    #[must_use]
    pub fn new(id: impl Into<String>, resource: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            resource: Some(resource.into()),
            rel: None,
            at: None,
            locator: None,
            extensions: std::collections::BTreeMap::default(),
        }
    }

    /// The wikilink target, when `resource` is a `[[…]]` reference.
    #[must_use]
    pub fn link(&self) -> Option<Wikilink> {
        self.resource.as_deref().and_then(Wikilink::parse)
    }

    /// The relation type; `reference` when none is written.
    #[must_use]
    pub fn rel_or_default(&self) -> &str {
        self.rel.as_deref().unwrap_or("reference")
    }

    /// Render as a YAML mapping.
    #[must_use]
    pub fn to_yaml(&self) -> Yaml {
        let mut m = serde_yaml::Mapping::new();
        let mut put = |k: &str, v: &str| m.insert(Yaml::String(k.into()), Yaml::String(v.into()));
        put("id", &self.id);
        if let Some(v) = &self.resource {
            put("resource", v);
        }
        if let Some(v) = &self.rel {
            put("rel", v);
        }
        if let Some(v) = &self.at {
            put("at", v);
        }
        if let Some(v) = &self.locator {
            put("locator", v);
        }
        for (key, value) in &self.extensions {
            m.insert(Yaml::String(key.clone()), value.clone());
        }
        Yaml::Mapping(m)
    }

    /// Read a source from a YAML mapping; `None` when it has no `id`.
    #[must_use]
    pub fn from_yaml(v: &Yaml) -> Option<Self> {
        Self::read(v).ok()
    }

    /// Read a source from a YAML mapping.
    ///
    /// # Errors
    /// Why the value is not a source: not a mapping, or no textual `id`.
    pub fn read(v: &Yaml) -> Result<Self, String> {
        let m = v
            .as_mapping()
            .ok_or_else(|| "a source is a mapping with an `id`".to_owned())?;
        let text = |k: &str| {
            m.get(Yaml::String(k.into()))
                .and_then(Yaml::as_str)
                .map(str::to_owned)
        };
        let id = text("id").ok_or_else(|| "no `id`".to_owned())?;
        let extensions = m
            .iter()
            .filter_map(|(key, value)| {
                let key = key.as_str()?;
                (!matches!(key, "id" | "resource" | "rel" | "at" | "locator"))
                    .then(|| (key.to_owned(), value.clone()))
            })
            .collect();
        Ok(Self {
            id,
            resource: text("resource"),
            rel: text("rel"),
            at: text("at"),
            locator: text("locator"),
            extensions,
        })
    }
}

/// A header value that could not be read as the type it is meant to be. The
/// reader keeps it out of the typed fields and reports it here, so nothing is
/// dropped silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Which value: `generated`, `verified[2]`, `sources[0]`.
    pub field: String,
    /// Why it is unreadable.
    pub reason: String,
}

/// The OKF block projected out of a page's frontmatter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OkfBlock {
    /// `type` — the OKF type name, e.g. `Class` or `Note`.
    pub type_name: Option<String>,
    /// `resource` — the stable IRI.
    pub resource: Option<String>,
    /// `status`, when it parses.
    pub status: Option<Status>,
    /// The raw `status` text, kept so validation can complain precisely.
    pub status_raw: Option<String>,
    /// `stale_after`, as written.
    pub stale_after: Option<String>,
    /// `generated`, when it parses.
    pub generated: Option<Stamp>,
    /// `verified` — zero or more attestations.
    pub verified: Vec<Stamp>,
    /// `sources` — zero or more provenance links.
    pub sources: Vec<Source>,
    /// Every `generated`, `verified` or `sources` value that was present and
    /// could not be read (an unparseable `by`, a source without an `id`). Empty
    /// on a clean page. `vault validate` reports each; none is dropped silently.
    pub problems: Vec<Problem>,
}

impl OkfBlock {
    /// Project the OKF keys out of a frontmatter mapping. Never fails: an
    /// unparseable value simply does not appear in the typed field, which is
    /// what lets `vault validate` report it rather than crash on it.
    #[must_use]
    pub fn from_frontmatter(fm: &Frontmatter) -> Self {
        let seq = |key: &str| -> Vec<Yaml> {
            match fm.get(key) {
                Some(Yaml::Sequence(items)) => items.clone(),
                Some(other) => vec![other.clone()],
                None => Vec::new(),
            }
        };
        let status_raw = fm.text("status");
        let mut problems = Vec::new();
        let mut note = |field: String, reason: String| problems.push(Problem { field, reason });
        let generated = fm.get("generated").map(Stamp::read);
        if let Some(Err(reason)) = &generated {
            note("generated".to_owned(), reason.clone());
        }
        let mut verified = Vec::new();
        for (i, item) in seq("verified").iter().enumerate() {
            match Stamp::read(item) {
                Ok(stamp) => verified.push(stamp),
                Err(reason) => note(format!("verified[{i}]"), reason),
            }
        }
        let mut sources = Vec::new();
        for (i, item) in seq("sources").iter().enumerate() {
            match Source::read(item) {
                Ok(source) => sources.push(source),
                Err(reason) => note(format!("sources[{i}]"), reason),
            }
        }
        Self {
            type_name: fm.text("type"),
            resource: fm.text("resource"),
            status: status_raw.as_deref().and_then(|s| s.parse().ok()),
            status_raw,
            stale_after: fm.text("stale_after"),
            generated: generated.and_then(|r| r.ok()),
            verified,
            sources,
            problems,
        }
    }

    /// `true` when at least one `verified` entry was signed by a human — the
    /// predicate the promotion state machine calls `human-reviewed`.
    #[must_use]
    pub fn is_human_verified(&self) -> bool {
        self.verified.iter().any(|s| s.by.is_human())
    }

    /// [`OkfBlock::is_human_verified`], also counting a `did:nostr:` entry the
    /// roster resolves to a principal of kind human (ruling B8: human
    /// certification follows the roster kind, so a human's `verified` survives
    /// the move from `human:<npub>` to `did:nostr:`).
    #[must_use]
    pub fn is_human_verified_with(&self, roster: &crate::roster::Roster) -> bool {
        self.verified.iter().any(|s| roster.is_human(&s.by))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_round_trips_every_prefix() {
        for s in [
            "human:npub1abc",
            "process:vault/1.0",
            "agent:visionclaw/2.3",
        ] {
            let a: Actor = s.parse().unwrap();
            assert_eq!(a.as_string(), s);
        }
    }

    #[test]
    fn a_bare_actor_is_rejected() {
        assert!("vault/1.0".parse::<Actor>().is_err());
        assert!("process:novversion".parse::<Actor>().is_err());
        assert!("human:".parse::<Actor>().is_err());
    }

    #[test]
    fn only_human_actors_satisfy_the_gate() {
        assert!(Actor::Human("npub1".into()).is_human());
        assert!(!Actor::Process {
            name: "vault".into(),
            version: "1.0".into()
        }
        .is_human());
    }

    #[test]
    fn projects_the_okf_block_out_of_frontmatter() {
        let fm = Frontmatter::parse(concat!(
            "type: Class\n",
            "resource: urn:ngm:class:knowledge-graph\n",
            "status: stable\n",
            "stale_after: 2026-10-06\n",
            "generated: { by: process:vault/1.0, at: 2026-09-22T00:00:00Z }\n",
            "verified:\n",
            "  - { by: human:npub1abc, at: 2026-09-22T00:00:00Z }\n",
            "sources:\n",
            "  - { id: origin, resource: \"[[working/Notes]]\" }\n",
        ))
        .unwrap();
        let okf = OkfBlock::from_frontmatter(&fm);
        assert_eq!(okf.status, Some(Status::Stable));
        assert_eq!(okf.type_name.as_deref(), Some("Class"));
        assert!(okf.is_human_verified());
        assert_eq!(okf.sources[0].link().unwrap().target, "working/Notes");
        assert_eq!(okf.generated.unwrap().by.as_string(), "process:vault/1.0");
    }

    #[test]
    fn an_unparseable_status_is_kept_raw_for_reporting() {
        let fm = Frontmatter::parse("status: half-baked\n").unwrap();
        let okf = OkfBlock::from_frontmatter(&fm);
        assert!(okf.status.is_none());
        assert_eq!(okf.status_raw.as_deref(), Some("half-baked"));
    }

    #[test]
    fn stamp_yaml_round_trips() {
        let s = Stamp::new(Actor::Human("npub1".into()), "2026-09-22T00:00:00Z");
        assert_eq!(Stamp::from_yaml(&s.to_yaml()).unwrap(), s);
    }

    const B41: &str = "b41654017f6850b13857d19d8ae0e3f88f1365600cab0321e8101c9e92682f7a";

    #[test]
    fn a_did_nostr_actor_round_trips_and_demands_a_hex_body() {
        let s = format!("did:nostr:{B41}");
        let a: Actor = s.parse().unwrap();
        assert_eq!(a, Actor::Did(B41.into()));
        assert_eq!(a.as_string(), s);
        for bad in [
            "did:nostr:not-hex",
            "did:nostr:",
            "did:web:example.org",
            &format!("did:nostr:{}", B41.to_uppercase()),
        ] {
            assert!(bad.parse::<Actor>().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_did_actor_is_human_only_through_the_roster() {
        let did: Actor = format!("did:nostr:{B41}").parse().unwrap();
        assert!(!did.is_human(), "the spelling alone does not say");
        let roster = crate::roster::Roster::from_yaml_str(&format!(
            "principals:\n  - did: did:nostr:{B41}\n    label: jjohare\n    kind: human\n"
        ));
        assert!(roster.is_human(&did));
    }

    #[test]
    fn a_v3_stamp_keeps_via_and_everything_else() {
        let fm = Frontmatter::parse(&format!(
            "generated: {{ by: did:nostr:{B41}, via: agent:enrichment-swarm/6, at: 2026-11-01T00:00:00Z, receipt: r1 }}\n"
        ))
        .unwrap();
        let okf = OkfBlock::from_frontmatter(&fm);
        let stamp = okf.generated.unwrap();
        assert_eq!(stamp.by, Actor::Did(B41.into()));
        assert_eq!(stamp.via().unwrap().as_string(), "agent:enrichment-swarm/6");
        assert_eq!(Stamp::from_yaml(&stamp.to_yaml()).unwrap(), stamp);
        assert!(stamp.extensions.contains_key("receipt"));
        assert!(okf.problems.is_empty());
    }

    #[test]
    fn an_unreadable_stamp_is_surfaced_not_dropped() {
        let fm = Frontmatter::parse(&format!(
            "generated: {{ by: 'did:nostr:not-hex', at: 2026-11-01T00:00:00Z }}\nverified:\n  - {{ by: did:nostr:{B41}, at: 2026-11-02T00:00:00Z }}\n  - {{ by: 'who:knows', at: 2026-11-02T00:00:00Z }}\n  - {{ by: human:npub1x }}\nsources:\n  - {{ resource: '[[X]]' }}\n"
        ))
        .unwrap();
        let okf = OkfBlock::from_frontmatter(&fm);
        assert!(okf.generated.is_none());
        assert_eq!(okf.verified.len(), 1);
        let fields: Vec<&str> = okf.problems.iter().map(|p| p.field.as_str()).collect();
        assert_eq!(
            fields,
            ["generated", "verified[1]", "verified[2]", "sources[0]"]
        );
        assert!(
            okf.problems[0].reason.contains("64 lowercase hex"),
            "{:?}",
            okf.problems[0]
        );
    }

    #[test]
    fn typed_sources_survive_a_round_trip() {
        let fm = Frontmatter::parse(concat!(
            "sources:\n",
            "  - { id: origin }\n",
            "  - { id: e, resource: '[[Ep]]', rel: evidence, at: 2026-11-01, locator: 'fp:abc', note: kept }\n",
            "  - { id: d, resource: '[[Brief]]', rel: derived-from, locator: 'sha256-v1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' }\n",
        ))
        .unwrap();
        let okf = OkfBlock::from_frontmatter(&fm);
        assert_eq!(okf.sources.len(), 3, "an id-only source is legal");
        assert_eq!(okf.sources[0].resource, None);
        assert_eq!(okf.sources[0].rel_or_default(), "reference");
        let e = &okf.sources[1];
        assert_eq!(
            (e.rel.as_deref(), e.locator.as_deref()),
            (Some("evidence"), Some("fp:abc"))
        );
        assert!(e.extensions.contains_key("note"));
        for s in &okf.sources {
            assert_eq!(&Source::from_yaml(&s.to_yaml()).unwrap(), s);
        }
        assert!(okf.sources[2]
            .locator
            .as_deref()
            .unwrap()
            .starts_with("sha256-v1:"));
    }

    #[test]
    fn human_certification_survives_the_move_to_did_nostr() {
        let fm = Frontmatter::parse(&format!(
            "verified:\n  - {{ by: did:nostr:{B41}, at: 2026-11-02T00:00:00Z }}\n"
        ))
        .unwrap();
        let okf = OkfBlock::from_frontmatter(&fm);
        let roster = crate::roster::Roster::from_yaml_str(&format!(
            "principals:\n  - did: did:nostr:{B41}\n    label: jjohare\n    kind: human\n"
        ));
        assert!(
            !okf.is_human_verified(),
            "no roster: a did is not known to be human"
        );
        assert!(okf.is_human_verified_with(&roster));
        let org = crate::roster::Roster::from_yaml_str(&format!(
            "principals:\n  - did: did:nostr:{B41}\n    label: acme\n    kind: organisation\n"
        ));
        assert!(!okf.is_human_verified_with(&org));
    }
}
