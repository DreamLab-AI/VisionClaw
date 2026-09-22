//! YAML frontmatter: the only place ontology lives after the migration (Q4).
//!
//! A page is `---` / YAML mapping / `---` / body. The mapping is kept as a
//! [`serde_yaml::Mapping`], which is insertion-ordered, so a round-trip through
//! [`Frontmatter::parse`] and [`Frontmatter::render`] preserves the author's
//! key order rather than alphabetising it.
//!
//! Typed accessors ([`Frontmatter::text`], [`Frontmatter::wikilinks`], …)
//! implement the C1 property model: scalars are text / number / bool / date,
//! relations are **lists of wikilinks**, and everything else is a list of
//! strings.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_yaml::Value as Yaml;

/// A resolved `[[Target]]` or `[[Target|Alias]]` reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Wikilink {
    /// The link target: a page id (vault-relative path without `.md`).
    pub target: String,
    /// The display alias after `|`, when the author wrote one.
    pub alias: Option<String>,
}

impl Wikilink {
    /// A link with no alias.
    #[must_use]
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            alias: None,
        }
    }

    /// The text a reader sees: the alias if present, else the target.
    #[must_use]
    pub fn label(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.target)
    }

    /// Parse one `[[…]]` token. Returns `None` if `raw` is not a wikilink.
    ///
    /// ```
    /// # use vault_core::frontmatter::Wikilink;
    /// let w = Wikilink::parse("[[Ontology|the ontology]]").unwrap();
    /// assert_eq!(w.target, "Ontology");
    /// assert_eq!(w.label(), "the ontology");
    /// assert!(Wikilink::parse("Ontology").is_none());
    /// ```
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let inner = raw.trim().strip_prefix("[[")?.strip_suffix("]]")?;
        let (target, alias) = match inner.split_once('|') {
            Some((t, a)) => (t.trim(), Some(a.trim().to_owned())),
            None => (inner.trim(), None),
        };
        if target.is_empty() {
            return None;
        }
        Some(Self {
            target: target.to_owned(),
            alias,
        })
    }
}

impl fmt::Display for Wikilink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.alias {
            Some(a) => write!(f, "[[{}|{a}]]", self.target),
            None => write!(f, "[[{}]]", self.target),
        }
    }
}

/// A page's YAML frontmatter mapping, insertion-ordered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frontmatter {
    /// The raw mapping. Prefer the typed accessors; this is public so
    /// `vault edit` and `vault migrate` can rewrite keys in place.
    pub map: serde_yaml::Mapping,
}

/// The result of splitting a page file into frontmatter and body.
#[derive(Debug, Clone)]
pub struct Split<'a> {
    /// The raw YAML between the `---` fences, or `None` when the page has no
    /// frontmatter block at all.
    pub yaml: Option<&'a str>,
    /// Everything after the closing fence (or the whole file when there is no
    /// frontmatter).
    pub body: &'a str,
    /// The **1-based file line** the body starts on.
    ///
    /// Without it a body offset cannot be turned back into a file position, and
    /// a diagnostic that says `line 1845` when the file means `line 1854` sends
    /// the reader to the wrong place. `1` when there is no frontmatter.
    pub body_line: usize,
}

/// Split `text` into its frontmatter block and body without parsing the YAML.
///
/// The opening fence must be the very first line; this is Obsidian's rule and
/// it is what keeps a `---` horizontal rule in the body from being mistaken
/// for a frontmatter delimiter.
///
/// ```
/// # use vault_core::frontmatter::split;
/// let s = split("---\npublic: true\n---\n# Title\n");
/// assert_eq!(s.yaml.unwrap().trim(), "public: true");
/// assert_eq!(s.body, "# Title\n");
/// // `---` is line 1, `public: true` line 2, `---` line 3, so the body is line 4.
/// assert_eq!(s.body_line, 4);
/// assert_eq!(split("# No frontmatter\n").body_line, 1);
/// ```
#[must_use]
pub fn split(text: &str) -> Split<'_> {
    let stripped = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = stripped
        .strip_prefix("---\n")
        .or_else(|| stripped.strip_prefix("---\r\n"))
    else {
        return Split {
            yaml: None,
            body: stripped,
            body_line: 1,
        };
    };
    for (idx, line) in rest.match_indices('\n') {
        let line_start = rest[..idx].rfind('\n').map_or(0, |p| p + 1);
        let candidate = rest[line_start..idx].trim_end_matches('\r');
        if candidate == "---" {
            let _ = line;
            return Split {
                yaml: Some(&rest[..line_start]),
                body: &rest[idx + 1..],
                // The opening `---`, the YAML lines, and the closing `---`.
                body_line: 2 + rest[..idx].matches('\n').count() + 1,
            };
        }
    }
    // A closing fence on the final line with no trailing newline.
    let line_start = rest.rfind('\n').map_or(0, |p| p + 1);
    if rest[line_start..].trim_end_matches('\r') == "---" {
        return Split {
            yaml: Some(&rest[..line_start]),
            body: "",
            body_line: 2 + rest[..line_start].matches('\n').count() + 1,
        };
    }
    Split {
        yaml: None,
        body: stripped,
        body_line: 1,
    }
}

impl Frontmatter {
    /// Parse a YAML mapping.
    ///
    /// # Errors
    /// Returns the `serde_yaml` error when `yaml` is not a mapping or is
    /// syntactically invalid.
    pub fn parse(yaml: &str) -> serde_yaml::Result<Self> {
        if yaml.trim().is_empty() {
            return Ok(Self::default());
        }
        let value: Yaml = serde_yaml::from_str(yaml)?;
        match value {
            Yaml::Mapping(map) => Ok(Self { map }),
            Yaml::Null => Ok(Self::default()),
            other => Err(serde::de::Error::custom(format!(
                "frontmatter must be a mapping, found {}",
                type_name(&other)
            ))),
        }
    }

    /// Render back to a `---`-fenced block, or the empty string when there are
    /// no keys.
    ///
    /// # Errors
    /// Returns the `serde_yaml` error if a value cannot be represented.
    pub fn render(&self) -> serde_yaml::Result<String> {
        if self.map.is_empty() {
            return Ok(String::new());
        }
        let body = serde_yaml::to_string(&Yaml::Mapping(self.map.clone()))?;
        Ok(format!("---\n{body}---\n"))
    }

    /// `true` when the key is present (even if its value is null).
    #[must_use]
    pub fn has(&self, key: &str) -> bool {
        self.map.contains_key(Yaml::String(key.to_owned()))
    }

    /// Borrow the raw value under `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Yaml> {
        self.map.get(Yaml::String(key.to_owned()))
    }

    /// Insert or replace `key`. Insertion order is preserved for new keys.
    pub fn set(&mut self, key: impl Into<String>, value: Yaml) {
        self.map.insert(Yaml::String(key.into()), value);
    }

    /// Remove `key`, returning its previous value.
    pub fn remove(&mut self, key: &str) -> Option<Yaml> {
        self.map.remove(Yaml::String(key.to_owned()))
    }

    /// The scalar under `key` rendered as text (numbers and booleans included).
    #[must_use]
    pub fn text(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Yaml::String(s) => Some(s.clone()),
            Yaml::Number(n) => Some(n.to_string()),
            Yaml::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    /// The boolean under `key`. Accepts a real YAML bool or the strings
    /// `"true"` / `"false"` (case-insensitive), which is how Logseq wrote them.
    #[must_use]
    pub fn boolean(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Yaml::Bool(b) => Some(*b),
            Yaml::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    /// The number under `key`, coercing a numeric string.
    #[must_use]
    pub fn number(&self, key: &str) -> Option<f64> {
        match self.get(key)? {
            Yaml::Number(n) => n.as_f64(),
            Yaml::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// The value under `key` as a list of strings. A bare scalar is treated as
    /// a one-element list, matching Obsidian's list-or-scalar tolerance.
    #[must_use]
    pub fn strings(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            Some(Yaml::Sequence(items)) => items.iter().filter_map(yaml_to_string).collect(),
            Some(other) => yaml_to_string(other).into_iter().collect(),
            None => Vec::new(),
        }
    }

    /// The value under `key` as a list of wikilinks, skipping any entry that is
    /// not `[[…]]`.
    ///
    /// ```
    /// # use vault_core::frontmatter::Frontmatter;
    /// let fm = Frontmatter::parse("requires: [\"[[Ontology]]\", \"plain\"]").unwrap();
    /// let links = fm.wikilinks("requires");
    /// assert_eq!(links.len(), 1);
    /// assert_eq!(links[0].target, "Ontology");
    /// ```
    #[must_use]
    pub fn wikilinks(&self, key: &str) -> Vec<Wikilink> {
        self.strings(key)
            .iter()
            .filter_map(|s| Wikilink::parse(s))
            .collect()
    }

    /// Every key, in author order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.map.keys().filter_map(Yaml::as_str)
    }

    /// Reorder the keys into [`CANONICAL_ORDER`], alphabetically thereafter.
    ///
    /// Makes serialisation **independent of which input supplied each key**,
    /// which is what makes `vault migrate` idempotent on byte equality. Before
    /// this, a first pass ordered by the `json-ld` fence and a second (with the
    /// fences gone) by the page's own frontmatter; the content was identical and
    /// the bytes were not, so a no-op run still rewrote every file and "did this
    /// change anything?" could not be answered from the diff.
    ///
    /// It also makes the corpus diff-stable in git: a key added today lands in
    /// the same place as the same key added next year.
    ///
    /// ```
    /// # use vault_core::frontmatter::Frontmatter;
    /// let mut fm = Frontmatter::parse("topic: x\nstatus: stable\ntype: Class\n").unwrap();
    /// fm.sort_canonical();
    /// assert_eq!(fm.keys().collect::<Vec<_>>(), vec!["type", "status", "topic"]);
    /// ```
    pub fn sort_canonical(&mut self) {
        let rank = |key: &str| {
            CANONICAL_ORDER
                .iter()
                .position(|k| *k == key)
                .unwrap_or(CANONICAL_ORDER.len())
        };
        let mut entries: Vec<(Yaml, Yaml)> = std::mem::take(&mut self.map).into_iter().collect();
        // Rank first, then the key itself: unknown keys land after the known
        // ones in a stable alphabetical block rather than in arrival order.
        entries.sort_by(|(a, _), (b, _)| {
            let (ka, kb) = (a.as_str().unwrap_or(""), b.as_str().unwrap_or(""));
            rank(ka).cmp(&rank(kb)).then_with(|| ka.cmp(kb))
        });
        self.map = entries.into_iter().collect();
    }
}

/// The corpus's canonical frontmatter key order.
///
/// Identity first, then the OKF trust block, then provenance, then the ontology
/// relations, then `working/`'s episodic extensions. Anything absent from this
/// list sorts alphabetically after it, so an undeclared key is stable rather
/// than positioned by accident.
///
/// This is presentation only — no reader depends on key order — but a *stable*
/// presentation is what makes an 8,457-file diff reviewable.
pub const CANONICAL_ORDER: &[&str] = &[
    // identity
    "type",
    "title",
    "label",
    "aliases",
    "slug",
    "resource",
    "page_resource",
    "legacy-term-id",
    // the OKF trust block
    "public",
    "status",
    "domain",
    "maturity",
    "quality",
    "authority",
    "version",
    // provenance
    "generated",
    "sources",
    "governance-case",
    "verified",
    // content
    "definition",
    "gloss",
    // working/ episodic extensions
    "topic",
    "episodes",
    "assertions",
    "promotion-status",
    // the link graph: curated links, then the taxonomy, then everything else
    "links",
    "is-a",
    "instance-of",
    "same-as",
];

fn yaml_to_string(v: &Yaml) -> Option<String> {
    match v {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn type_name(v: &Yaml) -> &'static str {
    match v {
        Yaml::Null => "null",
        Yaml::Bool(_) => "a boolean",
        Yaml::Number(_) => "a number",
        Yaml::String(_) => "a string",
        Yaml::Sequence(_) => "a sequence",
        Yaml::Mapping(_) => "a mapping",
        Yaml::Tagged(_) => "a tagged value",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_frontmatter_from_body() {
        let s = split("---\na: 1\nb: 2\n---\nbody line\n");
        assert_eq!(s.yaml.unwrap(), "a: 1\nb: 2\n");
        assert_eq!(s.body, "body line\n");
    }

    #[test]
    fn a_horizontal_rule_in_the_body_is_not_a_fence() {
        let s = split("# Title\n\n---\n\nmore\n");
        assert!(s.yaml.is_none());
        assert_eq!(s.body, "# Title\n\n---\n\nmore\n");
    }

    #[test]
    fn handles_a_closing_fence_with_no_trailing_newline() {
        let s = split("---\na: 1\n---");
        assert_eq!(s.yaml.unwrap(), "a: 1\n");
        assert_eq!(s.body, "");
    }

    #[test]
    fn round_trips_key_order() {
        let fm = Frontmatter::parse("zeta: 1\nalpha: 2\n").unwrap();
        assert_eq!(fm.keys().collect::<Vec<_>>(), vec!["zeta", "alpha"]);
        let rendered = fm.render().unwrap();
        assert!(rendered.starts_with("---\nzeta: 1\nalpha: 2\n"));
    }

    #[test]
    fn typed_accessors_coerce_logseq_spellings() {
        let fm = Frontmatter::parse("public: \"true\"\nquality: \"0.35\"\n").unwrap();
        assert_eq!(fm.boolean("public"), Some(true));
        assert!((fm.number("quality").unwrap() - 0.35).abs() < f64::EPSILON);
    }

    #[test]
    fn a_scalar_reads_as_a_one_element_list() {
        let fm = Frontmatter::parse("aliases: KnowledgeGraph\n").unwrap();
        assert_eq!(fm.strings("aliases"), vec!["KnowledgeGraph".to_owned()]);
    }

    #[test]
    fn wikilink_display_round_trips() {
        let w = Wikilink::parse("[[A|B]]").unwrap();
        assert_eq!(w.to_string(), "[[A|B]]");
        assert_eq!(Wikilink::new("A").to_string(), "[[A]]");
    }

    #[test]
    fn a_non_mapping_frontmatter_is_rejected() {
        assert!(Frontmatter::parse("- just\n- a list\n").is_err());
    }
}
