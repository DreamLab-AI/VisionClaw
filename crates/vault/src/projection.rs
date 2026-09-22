//! The public build boundary — a port of `pipeline/public_projection.py`.
//!
//! This is the security-relevant half of the build. Three things happen here
//! and all three must keep happening:
//!
//! 1. **Input census** ([`inspect_inputs`]). Every markdown file under
//!    `pages/` is accounted for before the parser gets a chance to silently
//!    drop it. A `public` value that is not a real boolean is refusal, not
//!    coercion — "true" the string never becomes publication permission.
//! 2. **Identity collision check.** If one identity (page id, slug or IRI) is
//!    claimed by both a public and a private page, the build stops. Otherwise a
//!    rename could leak a private page through a public IRI.
//! 3. **Reference redaction** ([`project`]). Filtering precedes the closure, so
//!    a private intermediate class never enters inferred ancestry; and private
//!    *names* occurring in prose are replaced with `[private]`, not just typed
//!    edges dropped.
//!
//! Pages under a dot-directory or an unpublished namespace are force-marked
//! private and carried into the marker set, so an incoming link cannot make a
//! held page public.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use indexmap::IndexMap;
use regex::Regex;
use vault_core::frontmatter;
use vault_core::page::is_unpublished;

use crate::model::{ClassRecord, Corpus, Ref};

/// An input could not safely cross the public build boundary.
#[derive(Debug, thiserror::Error)]
pub enum PublicationBlocked {
    /// The census found inputs it cannot classify.
    #[error("public build refused input census: {0}")]
    Census(String),
    /// One identity is claimed by both a public and a private page.
    #[error("public/private identity collision on {0}; resolve in the authoring source")]
    Collision(String),
}

/// The `census.json` document.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Census {
    /// Schema version of the census policy.
    pub policy_version: u32,
    /// Pages with `public: true`.
    pub public_pages: usize,
    /// Pages with `public: false` or no flag.
    pub non_public_pages: usize,
    /// Always zero: a rejected input raises instead of being counted.
    pub rejected: usize,
    /// Files skipped by directory rule or missing frontmatter.
    pub excluded: usize,
    /// Files that produced a page.
    pub parsed: usize,
    /// Every raw counter, so the arithmetic is auditable.
    pub counts: BTreeMap<String, usize>,
    /// `true` when every input file is accounted for exactly once.
    pub balanced: bool,
}

/// Walk `pages_dir` and account for every markdown file.
///
/// # Errors
/// [`PublicationBlocked::Census`] when any file has an invalid or conflicting
/// publication flag, or cannot be read.
pub fn inspect_inputs(pages_dir: impl AsRef<Path>) -> Result<Census, PublicationBlocked> {
    let pages_dir = pages_dir.as_ref();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut bump = |k: &str| *counts.entry(k.to_owned()).or_insert(0) += 1;

    let mut all: Vec<std::path::PathBuf> = walkdir::WalkDir::new(pages_dir)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    all.sort();

    for path in all {
        bump("input_files");
        let rel = path.strip_prefix(pages_dir).unwrap_or(&path);
        let parts: Vec<String> = rel
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if parts.iter().any(|p| p.starts_with('.')) || is_unpublished(rel) {
            bump("excluded_directory");
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            bump("invalid_input");
            continue;
        };
        let split = frontmatter::split(&text);
        let Some(yaml) = split.yaml else {
            bump("excluded_no_frontmatter");
            continue;
        };
        let Ok(fm) = frontmatter::Frontmatter::parse(yaml) else {
            bump("invalid_input");
            continue;
        };
        // Never coerce a string or a number into publication permission: only a
        // real YAML boolean counts, and anything else refuses the build.
        match fm.get("public") {
            Some(serde_yaml::Value::Bool(true)) => bump("included_public"),
            None | Some(serde_yaml::Value::Bool(false)) => bump("included_private"),
            Some(_) => bump("invalid_publication_flag"),
        }
    }

    let invalid: usize = counts
        .iter()
        .filter(|(k, _)| k.starts_with("invalid_") || k.starts_with("conflicting_"))
        .map(|(_, v)| *v)
        .sum();
    if invalid > 0 {
        return Err(PublicationBlocked::Census(
            serde_json::to_string(&counts).unwrap_or_default(),
        ));
    }

    let get = |k: &str| counts.get(k).copied().unwrap_or(0);
    let input_files = get("input_files");
    let accounted: usize = counts
        .iter()
        .filter(|(k, _)| k.as_str() != "input_files")
        .map(|(_, v)| *v)
        .sum();

    Ok(Census {
        policy_version: 1,
        public_pages: get("included_public"),
        non_public_pages: get("included_private"),
        rejected: 0,
        excluded: get("excluded_directory") + get("excluded_no_frontmatter"),
        parsed: get("included_public") + get("included_private"),
        balanced: input_files == accounted,
        counts,
    })
}

/// Identities belonging to **held** pages, so an incoming link cannot publish
/// one.
///
/// A held page is a page that exists, is loaded by the vault, is reachable by
/// `validate`, `find` and `migrate`, and is deliberately never published: an
/// [`vault_core::page::UNPUBLISHED_DIRS`] folder. Its identities are collected
/// here so a link from a public page cannot drag it into the bundle.
///
/// A **dot-directory is not a held page** — `.deleted/` and `.trash/` are
/// tombstones, `.obsidian/` is editor state, and none of them is part of the
/// corpus at all (the vault walk reports them as skips). Collecting a
/// tombstone's identity blocked the live page that replaced it: the
/// `.deleted/Recurrent-Neural-Network.md` tombstone and the public
/// `Recurrent Neural Network.md` slugify to the same thing, and the build
/// refused the whole bundle over a page that had been deleted on purpose.
/// Deletion is not a publication hold.
///
/// # Errors
/// [`PublicationBlocked::Census`] when the tree cannot be walked.
pub fn excluded_identities(pages_dir: impl AsRef<Path>) -> Result<HashSet<String>, std::io::Error> {
    let pages_dir = pages_dir.as_ref();
    let mut out = HashSet::new();
    for entry in walkdir::WalkDir::new(pages_dir)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|e| e.to_str()) != Some("md")
        {
            continue;
        }
        let rel = entry.path().strip_prefix(pages_dir).unwrap_or(entry.path());
        if !is_unpublished(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let id = rel.with_extension("").to_string_lossy().replace('\\', "/");
        let Ok(page) = vault_core::page::Page::parse(entry.path(), rel, &id, &text) else {
            continue;
        };
        out.insert(id);
        out.insert(page.slug());
        if let Some(r) = page.frontmatter.text("resource") {
            out.insert(r);
        }
        out.insert(page.title());
    }
    Ok(out)
}

/// A page's identity values: its id, slug, page IRI and class IRI.
fn identity(record: &ClassRecord) -> Vec<String> {
    [
        record.page_id.clone(),
        record.slug.clone(),
        record.page_iri.clone(),
        record.iri.clone(),
    ]
    .into_iter()
    .filter(|v| !v.is_empty())
    .collect()
}

/// The redactor: a compiled alternation over every private marker plus the
/// word-boundary rule Python expressed with look-around.
struct Redactor {
    pattern: Option<Regex>,
    markers: HashSet<String>,
}

impl Redactor {
    fn new(markers: HashSet<String>) -> Self {
        let mut sorted: Vec<&String> = markers.iter().filter(|m| !m.is_empty()).collect();
        // Longest first, then lexicographic, so the alternation is
        // deterministic and never matches a prefix of a longer marker.
        sorted.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        let pattern = if sorted.is_empty() {
            None
        } else {
            let alt: Vec<String> = sorted.iter().map(|m| regex::escape(m)).collect();
            Regex::new(&alt.join("|")).ok()
        };
        Self { pattern, markers }
    }

    /// `\w` or `-` — the characters Python's look-around forbade on either side.
    fn is_boundary_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_' || c == '-'
    }

    fn redact(&self, text: &str) -> String {
        let Some(re) = &self.pattern else {
            return text.to_owned();
        };
        let mut out = String::with_capacity(text.len());
        let mut pos = 0usize;
        while pos <= text.len() {
            let Some(m) = re.find_at(text, pos) else {
                break;
            };
            let before_ok = text[..m.start()]
                .chars()
                .next_back()
                .is_none_or(|c| !Self::is_boundary_char(c));
            let after_ok = text[m.end()..]
                .chars()
                .next()
                .is_none_or(|c| !Self::is_boundary_char(c));
            if before_ok && after_ok {
                out.push_str(&text[pos..m.start()]);
                out.push_str("[private]");
                pos = m.end();
            } else {
                // Not a real boundary match: advance one character and retry,
                // so a longer marker starting later is still found.
                let step = text[m.start()..].chars().next().map_or(1, char::len_utf8);
                out.push_str(&text[pos..m.start() + step]);
                pos = m.start() + step;
            }
        }
        out.push_str(&text[pos.min(text.len())..]);
        out
    }

    fn is_private_ref(&self, r: &Ref) -> bool {
        self.markers.contains(&r.iri)
    }

    fn clean_refs(&self, refs: &[Ref]) -> Vec<Ref> {
        refs.iter()
            .filter(|r| !self.is_private_ref(r))
            .map(|r| Ref {
                iri: self.redact(&r.iri),
                label: self.redact(&r.label),
            })
            .collect()
    }

    fn clean_record(&self, r: &ClassRecord) -> ClassRecord {
        let mut relations: IndexMap<&'static str, Vec<Ref>> = IndexMap::new();
        for (key, refs) in &r.relations {
            let cleaned = self.clean_refs(refs);
            if !cleaned.is_empty() {
                relations.insert(key, cleaned);
            }
        }
        ClassRecord {
            page_id: r.page_id.clone(),
            slug: self.redact(&r.slug),
            title: self.redact(&r.title),
            public: r.public,
            page_iri: self.redact(&r.page_iri),
            iri: self.redact(&r.iri),
            label: self.redact(&r.label),
            entity_type: r.entity_type,
            domain: self.redact(&r.domain),
            definition: self.redact(&r.definition),
            maturity: self.redact(&r.maturity),
            quality: r.quality,
            legacy_term_id: self.redact(&r.legacy_term_id),
            sub_class_of: self.clean_refs(&r.sub_class_of),
            instance_of: self.clean_refs(&r.instance_of),
            relations,
            links: self.clean_refs(&r.links),
            body: self.redact(&r.body),
            has_ontology: r.has_ontology,
        }
    }
}

/// HTTP projections of the URN namespaces, so a redacted URN cannot reappear
/// as a resolvable web IRI.
const URN_TO_HTTP: &[(&str, &str)] = &[
    ("urn:ngm:class:", "https://narrativegoldmine.com/class/"),
    (
        "urn:ngm:individual:",
        "https://narrativegoldmine.com/individual/",
    ),
    (
        "urn:visionflow:owl:class:",
        "https://narrativegoldmine.com/class/",
    ),
    (
        "urn:visionflow:page:",
        "https://narrativegoldmine.com/page/",
    ),
    (
        "urn:visionflow:linked:",
        "https://narrativegoldmine.com/linked/",
    ),
];

/// Project `corpus` onto the public set, dropping and redacting every private
/// reference.
///
/// `extra_private` carries identities from pages the walk excluded entirely.
///
/// # Errors
/// [`PublicationBlocked::Collision`] when an identity is claimed by both a
/// public and a private page.
#[allow(clippy::implicit_hasher)] // The build always uses the default hasher.
pub fn project(
    corpus: &Corpus,
    extra_private: &HashSet<String>,
) -> Result<Corpus, PublicationBlocked> {
    let (public, private): (Vec<&ClassRecord>, Vec<&ClassRecord>) =
        corpus.records.iter().partition(|r| r.public);

    let public_ids: HashSet<String> = public.iter().flat_map(|r| identity(r)).collect();
    let mut private_ids: HashSet<String> = private.iter().flat_map(|r| identity(r)).collect();
    private_ids.extend(extra_private.iter().cloned());

    if let Some(clash) = public_ids.intersection(&private_ids).next() {
        return Err(PublicationBlocked::Collision(clash.clone()));
    }

    let public_names: HashSet<String> = public
        .iter()
        .flat_map(|r| [r.title.clone(), r.label.clone()])
        .filter(|s| !s.is_empty())
        .collect();
    let private_names: HashSet<String> = private
        .iter()
        .flat_map(|r| [r.title.clone(), r.label.clone()])
        .filter(|s| !s.is_empty())
        .collect();

    let mut markers: HashSet<String> = private_ids.clone();
    markers.extend(private_names.difference(&public_names).cloned());
    for value in &private_ids {
        for (urn, http) in URN_TO_HTTP {
            if let Some(tail) = value.strip_prefix(urn) {
                markers.insert(format!("{http}{tail}"));
            }
        }
    }
    // An identity that is also a public name must not be redacted out of the
    // public bundle; the collision check above already proved no page claims
    // both, so this only strips names that coincide.
    for name in &public_names {
        markers.remove(name);
    }

    let redactor = Redactor::new(markers);
    let records: Vec<ClassRecord> = public.iter().map(|r| redactor.clean_record(r)).collect();

    let mut by_class_slug = IndexMap::new();
    for (i, r) in records.iter().enumerate() {
        if r.has_ontology && !r.iri.is_empty() {
            by_class_slug.entry(r.class_slug()).or_insert(i);
        }
    }
    Ok(Corpus {
        records,
        by_class_slug,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntityType;

    fn record(id: &str, public: bool) -> ClassRecord {
        ClassRecord {
            page_id: id.to_owned(),
            slug: vault_core::slug::slugify(id),
            title: id.to_owned(),
            public,
            page_iri: format!("urn:visionflow:page:{}", vault_core::slug::slugify(id)),
            iri: format!("urn:ngm:class:{}", vault_core::slug::slugify(id)),
            label: id.to_owned(),
            entity_type: EntityType::Class,
            domain: String::new(),
            definition: String::new(),
            maturity: "draft".into(),
            quality: 0.0,
            legacy_term_id: String::new(),
            sub_class_of: Vec::new(),
            instance_of: Vec::new(),
            relations: IndexMap::new(),
            links: Vec::new(),
            body: String::new(),
            has_ontology: true,
        }
    }

    fn corpus_of(records: Vec<ClassRecord>) -> Corpus {
        let mut by_class_slug = IndexMap::new();
        for (i, r) in records.iter().enumerate() {
            by_class_slug.entry(r.class_slug()).or_insert(i);
        }
        Corpus {
            records,
            by_class_slug,
        }
    }

    #[test]
    fn keeps_only_public_pages() {
        let c = corpus_of(vec![record("Pub", true), record("Secret", false)]);
        let p = project(&c, &HashSet::new()).unwrap();
        assert_eq!(p.records.len(), 1);
        assert_eq!(p.records[0].page_id, "Pub");
    }

    #[test]
    fn drops_edges_that_point_at_a_private_page() {
        let mut pub_rec = record("Pub", true);
        pub_rec.sub_class_of = vec![
            Ref {
                iri: "urn:ngm:class:secret".into(),
                label: "Secret".into(),
            },
            Ref {
                iri: "urn:ngm:class:open".into(),
                label: "Open".into(),
            },
        ];
        let c = corpus_of(vec![pub_rec, record("Secret", false), record("Open", true)]);
        let p = project(&c, &HashSet::new()).unwrap();
        let parents: Vec<&str> = p.records[0]
            .sub_class_of
            .iter()
            .map(|r| r.iri.as_str())
            .collect();
        assert_eq!(parents, vec!["urn:ngm:class:open"]);
    }

    #[test]
    fn redacts_a_private_name_out_of_prose() {
        let mut pub_rec = record("Pub", true);
        pub_rec.body = "Refer to Secret Project for detail; SecretProjectX is unrelated.".into();
        let c = corpus_of(vec![pub_rec, record("Secret Project", false)]);
        let p = project(&c, &HashSet::new()).unwrap();
        assert_eq!(
            p.records[0].body,
            "Refer to [private] for detail; SecretProjectX is unrelated."
        );
    }

    #[test]
    fn a_name_that_is_also_public_is_not_redacted() {
        let mut pub_rec = record("Pub", true);
        pub_rec.body = "Shared appears here.".into();
        let c = corpus_of(vec![pub_rec, record("Shared", true), {
            let mut r = record("Other", false);
            r.title = "Shared".into();
            r.label = "Shared".into();
            r
        }]);
        let p = project(&c, &HashSet::new()).unwrap();
        assert_eq!(p.records[0].body, "Shared appears here.");
    }

    #[test]
    fn the_http_projection_of_a_private_urn_is_redacted_too() {
        let mut pub_rec = record("Pub", true);
        pub_rec.body = "See https://narrativegoldmine.com/class/secret now.".into();
        let c = corpus_of(vec![pub_rec, record("Secret", false)]);
        let p = project(&c, &HashSet::new()).unwrap();
        assert!(p.records[0].body.contains("[private]"));
    }

    #[test]
    fn an_identity_collision_blocks_the_build() {
        let mut a = record("A", true);
        let mut b = record("B", false);
        a.iri = "urn:ngm:class:same".into();
        b.iri = "urn:ngm:class:same".into();
        let err = project(&corpus_of(vec![a, b]), &HashSet::new()).unwrap_err();
        assert!(matches!(err, PublicationBlocked::Collision(_)));
    }

    #[test]
    fn excluded_identities_are_treated_as_private() {
        let mut pub_rec = record("Pub", true);
        pub_rec.links = vec![Ref {
            iri: "urn:ngm:class:held".into(),
            label: "Held".into(),
        }];
        let extra: HashSet<String> = ["urn:ngm:class:held".to_owned()].into_iter().collect();
        let p = project(&corpus_of(vec![pub_rec]), &extra).unwrap();
        assert!(p.records[0].links.is_empty());
    }

    #[test]
    fn census_counts_and_balances() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(pages.join("_misc")).unwrap();
        std::fs::write(pages.join("A.md"), "---\npublic: true\n---\n").unwrap();
        std::fs::write(pages.join("B.md"), "---\npublic: false\n---\n").unwrap();
        std::fs::write(pages.join("C.md"), "no frontmatter\n").unwrap();
        std::fs::write(pages.join("_misc/D.md"), "---\npublic: true\n---\n").unwrap();
        let census = inspect_inputs(&pages).unwrap();
        assert_eq!(census.public_pages, 1);
        assert_eq!(census.non_public_pages, 1);
        assert_eq!(census.excluded, 2);
        assert!(census.balanced);
    }

    #[test]
    fn a_string_public_flag_refuses_the_build() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(pages.join("A.md"), "---\npublic: \"true\"\n---\n").unwrap();
        assert!(matches!(
            inspect_inputs(&pages),
            Err(PublicationBlocked::Census(_))
        ));
    }
}
