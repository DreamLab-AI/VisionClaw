//! `scaffold-index.json`, `prose-index.json`, `search-index.json` and the
//! backlink index they share.
//!
//! `scaffold-index.json` is the byte-parity artefact: Loom's
//! `loom-scaffold/src/index.rs` loads it, and the migration is not allowed to
//! change what Loom sees. Everything about its emission is therefore fixed —
//! the class ordering, the `rel` key order, the "omit empty lists" rule, the
//! `q: null` for a zero quality score, the 400-character definition cap, the
//! 20-entry backlink cap, and `CPython`'s compact `ensure_ascii` JSON writer.

use std::collections::HashMap;

use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use vault_core::slug::{ref_slug, slugify};

use crate::closure::Closure;
use crate::model::{ClassRecord, Corpus, Ref};

/// Definition truncation cap, shared by the scaffold and prose indexes.
pub const DEFINITION_CAP: usize = 400;
/// Maximum backlinks carried per class in the scaffold index.
pub const BACKLINK_CAP: usize = 20;
/// Maximum characters of "Current Landscape" prose carried per class.
pub const LANDSCAPE_CAP: usize = 1500;

/// The frozen `rel` key order of `scaffold-index.json` v1. Note that it is
/// **not** the page API's order; both are contracts and neither may be
/// rearranged to match the other.
pub const SCAFFOLD_REL_ORDER: &[&str] = &[
    "hasPart",
    "requires",
    "enables",
    "dependsOn",
    "implements",
    "uses",
    "partOf",
    "relatedTo",
    "bridgesTo",
    "supports",
    "standardizedBy",
    "contrastsWith",
];

/// Map target **page** slug to the page slugs that link to it.
///
/// Keyed by page slug, not class slug: 96 pages in the corpus differ between
/// the two, and the scaffold index's `bl` field is a page-slug list.
#[must_use]
#[allow(clippy::implicit_hasher)] // The build always uses the default hasher.
pub fn backlink_index(corpus: &Corpus) -> HashMap<String, Vec<String>> {
    let slug_set: std::collections::HashSet<&str> =
        corpus.records.iter().map(|r| r.slug.as_str()).collect();
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for record in &corpus.records {
        for link in &record.links {
            let target = if link.iri.contains(':') {
                ref_slug(&link.iri)
            } else {
                slugify(&link.label)
            };
            if slug_set.contains(target.as_str()) && target != record.slug {
                out.entry(target).or_default().push(record.slug.clone());
            }
        }
    }
    out
}

/// A definition truncated to [`DEFINITION_CAP`] characters.
///
/// The one place the rule lives. The scaffold index's `d`, the prose index and
/// the OKF bundle's `description` are all derived from the page's leading
/// paragraph and must agree on the cap; three copies of `400` would not stay
/// three copies of the same number.
#[must_use]
pub fn truncate_definition(s: &str) -> String {
    truncate_chars(s, DEFINITION_CAP)
}

/// Truncate to `cap` **characters** (not bytes) — Python `str` semantics.
fn truncate_chars(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        s.to_owned()
    } else {
        s.chars().take(cap).collect()
    }
}

/// Build the `scaffold-index.json` document.
///
/// `generated` is supplied by the caller so the emitter itself is pure and the
/// golden test can pin the timestamp.
#[must_use]
#[allow(clippy::implicit_hasher)]
pub fn scaffold_index(
    corpus: &Corpus,
    closure: &Closure,
    backlinks: &HashMap<String, Vec<String>>,
    generated: &str,
) -> Value {
    let mut candidates: Vec<&ClassRecord> = corpus.public_classes().collect();
    // Python: `sorted(candidates, key=lambda p: ref_slug(p.ontology_class.iri))`
    // — a stable sort, so equal slugs keep corpus order and "first wins".
    candidates.sort_by_key(|r| r.class_slug());

    let mut classes = Map::new();
    for record in candidates {
        let slug = record.class_slug();
        if classes.contains_key(&slug) {
            continue; // first definition wins
        }

        let mut rel = Map::new();
        for key in SCAFFOLD_REL_ORDER {
            let mut targets: Vec<String> = Vec::new();
            for r in record.relation(key) {
                let t = r.slug();
                if !t.is_empty() && !targets.contains(&t) {
                    targets.push(t);
                }
            }
            if !targets.is_empty() {
                // Empty lists MUST be omitted — the v1 contract.
                rel.insert((*key).to_owned(), json!(targets));
            }
        }

        let title = if record.label.is_empty() {
            record.title.clone()
        } else {
            record.label.clone()
        };
        let mut entry = Map::new();
        entry.insert("t".into(), json!(title));
        entry.insert(
            "d".into(),
            json!(truncate_chars(&record.definition, DEFINITION_CAP)),
        );
        entry.insert("dom".into(), json!(record.domain));
        // Python's `oc.quality_score if oc.quality_score else None`: a zero
        // score is falsy and becomes null.
        entry.insert(
            "q".into(),
            if record.quality == 0.0 {
                Value::Null
            } else {
                json!(record.quality)
            },
        );
        entry.insert("m".into(), json!(record.maturity));
        entry.insert("sup".into(), json!(closure.parents_of(&slug)));
        entry.insert("isup".into(), json!(closure.inferred_of(&slug)));
        entry.insert("rel".into(), Value::Object(rel));
        entry.insert(
            "bl".into(),
            json!(backlinks
                .get(&record.slug)
                .map(|v| v.iter().take(BACKLINK_CAP).cloned().collect::<Vec<_>>())
                .unwrap_or_default()),
        );
        classes.insert(slug, Value::Object(entry));
    }

    json!({
        "version": 1,
        "generated": generated,
        "counts": { "classes": classes.len() },
        "classes": Value::Object(classes),
    })
}

/// The "Current Landscape" heading, and any heading, each either as a markdown
/// heading or as the Logseq heading bullet (`- ### Current Landscape`) the
/// corpus used before `vault repair bodies`. Both forms must match: the section
/// feeds the Loom's prose layer, and a form that stops matching empties it
/// without an error. The captured `#` run is the heading's level.
fn landscape_regexes() -> &'static (regex::Regex, regex::Regex) {
    static R: std::sync::OnceLock<(regex::Regex, regex::Regex)> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        (
            regex::Regex::new(r"(?m)^\s*(?:-\s+)?(#{2,4})\s+Current Landscape.*$")
                .expect("static landscape regex"),
            regex::Regex::new(r"(?m)^\s*(?:-\s+)?(#{1,6})\s+").expect("static heading regex"),
        )
    })
}

/// Extract the "Current Landscape" section, flattened to one line and capped.
///
/// The section runs to the next heading of the same or a higher level, so its
/// own sub-headings stay inside it.  Where a page has several, the last is used.
///
/// Bullets lose their list markers so a consumer can splice the text straight
/// into an LLM context block without carrying Logseq indentation semantics.
///
/// # Panics
/// Never: every slice index is taken from a `char_indices` boundary.
#[must_use]
pub fn extract_current_landscape(body: &str, cap: usize) -> String {
    let (heading, next) = landscape_regexes();
    // The last section: a page that has been re-researched carries the newer
    // "Current Landscape (2026)" after the older one.
    let Some(c) = heading.captures_iter(body).last() else {
        return String::new();
    };
    let level = c[1].len();
    let rest = &body[c.get(0).map_or(0, |m| m.end())..];
    let section = next
        .captures_iter(rest)
        .find(|n| n[1].len() <= level)
        .and_then(|n| n.get(0))
        .map_or(rest, |n| &rest[..n.start()]);

    let mut lines: Vec<&str> = Vec::new();
    for raw in section.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let line = line.strip_prefix('-').map_or(line, str::trim_start);
        if !line.is_empty() {
            lines.push(line);
        }
    }
    let text = lines.join(" ");
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= cap {
        return text.trim().to_owned();
    }
    // Cut on a sentence boundary where one exists in the second half.
    let head: String = chars[..cap].iter().collect();
    match head.rfind(". ") {
        Some(byte_cut) => {
            let char_cut = head[..byte_cut].chars().count() + 1;
            if char_cut > cap / 2 {
                chars[..char_cut]
                    .iter()
                    .collect::<String>()
                    .trim()
                    .to_owned()
            } else {
                head.trim().to_owned()
            }
        }
        None => head.trim().to_owned(),
    }
}

/// Build the `prose-index.json` document: the prose layer the structural
/// scaffold index deliberately truncates.
#[must_use]
pub fn prose_index(corpus: &Corpus, generated: &str) -> Value {
    let mut entries: Vec<(String, Map<String, Value>)> = Vec::new();
    for record in corpus.public() {
        if !record.has_ontology {
            continue;
        }
        let slug = if record.iri.is_empty() {
            record.slug.clone()
        } else {
            record.class_slug()
        };
        let mut entry = Map::new();
        if record.definition.chars().count() > DEFINITION_CAP {
            entry.insert("dfull".into(), json!(record.definition));
        }
        let cl = extract_current_landscape(&record.body, LANDSCAPE_CAP);
        if !cl.is_empty() {
            entry.insert("cl".into(), json!(cl));
        }
        if !entry.is_empty() {
            entries.push((slug, entry));
        }
    }
    let with_landscape = entries.iter().filter(|(_, e)| e.contains_key("cl")).count();
    let with_full = entries
        .iter()
        .filter(|(_, e)| e.contains_key("dfull"))
        .count();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut pages = Map::new();
    for (slug, entry) in entries {
        pages.insert(slug, Value::Object(entry));
    }
    json!({
        "version": 1,
        "generated": generated,
        "counts": {
            "pages": pages.len(),
            "with_landscape": with_landscape,
            "with_full_definition": with_full,
        },
        "pages": Value::Object(pages),
    })
}

/// Build the `search-index.json` array.
///
/// One documented divergence from the Python pipeline: `labels` used to be
/// `[label] + preferred-term`, read out of the fence's `vc:legacyProperties`.
/// The migration folds `preferred-term` into `aliases`, so `labels` is now
/// `[label] + aliases` with the label itself removed. Pages that carried both
/// an alias and a preferred term therefore gain entries rather than lose them.
#[must_use]
#[allow(clippy::implicit_hasher)]
pub fn search_index(corpus: &Corpus, aliases: &HashMap<String, Vec<String>>) -> Value {
    let mut index = Vec::new();
    for record in corpus.public() {
        let mut entry = Map::new();
        entry.insert("id".into(), json!(record.slug));
        entry.insert("title".into(), json!(record.title));

        if record.has_ontology {
            let mut labels = vec![record.label.clone()];
            for a in aliases.get(&record.page_id).map_or(&[][..], Vec::as_slice) {
                if a != &record.label && !labels.contains(a) {
                    labels.push(a.clone());
                }
            }
            entry.insert("domain".into(), json!(record.domain));
            entry.insert(
                "domain_name".into(),
                json!(if record.domain.is_empty() {
                    String::new()
                } else {
                    title_case(&record.domain.replace('-', " "))
                }),
            );
            entry.insert("definition".into(), json!(record.definition));
            entry.insert("entityType".into(), json!(record.entity_type.as_str()));
            entry.insert("qualityScore".into(), json!(record.quality));
            entry.insert("maturity".into(), json!(record.maturity));
            entry.insert("iri".into(), json!(record.iri));
            entry.insert("labels".into(), json!(labels));
            entry.insert(
                "is_subclass_of".into(),
                json!(record
                    .sub_class_of
                    .iter()
                    .map(|r| r.label.clone())
                    .collect::<Vec<_>>()),
            );
            entry.insert(
                "wikilinks".into(),
                json!(record
                    .links
                    .iter()
                    .take(20)
                    .map(|r| r.label.clone())
                    .collect::<Vec<_>>()),
            );
        } else {
            entry.insert("labels".into(), json!([record.title.clone()]));
        }
        index.push(Value::Object(entry));
    }
    Value::Array(index)
}

/// Python's `str.title()`: capitalise the first letter of each word, lowercase
/// the rest.
fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start_of_word = true;
    for c in s.chars() {
        if c.is_alphabetic() {
            if start_of_word {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start_of_word = false;
        } else {
            out.push(c);
            start_of_word = true;
        }
    }
    out
}

/// Relations inherited from ancestors, rendered as the page API's
/// `{id,label}` pairs.
#[must_use]
pub fn refs_to_json(refs: &[Ref]) -> Value {
    Value::Array(
        refs.iter()
            .map(|r| json!({ "id": r.iri, "label": r.label }))
            .collect(),
    )
}

/// Convert an [`IndexMap`] of inherited relations into a JSON object,
/// preserving key order.
#[must_use]
pub fn inherited_to_json(map: &IndexMap<String, Vec<Ref>>) -> Value {
    let mut out = Map::new();
    for (k, refs) in map {
        if !refs.is_empty() {
            out.insert(k.clone(), refs_to_json(refs));
        }
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntityType;

    fn record(id: &str, public: bool) -> ClassRecord {
        ClassRecord {
            page_id: id.to_owned(),
            slug: slugify(id),
            title: id.to_owned(),
            public,
            page_iri: format!("urn:visionflow:page:{}", slugify(id)),
            iri: format!("urn:ngm:class:{}", slugify(id)),
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
    fn scaffold_omits_empty_relation_lists() {
        let mut r = record("A", true);
        r.relations.insert(
            "requires",
            vec![Ref {
                iri: "urn:ngm:class:b".into(),
                label: "B".into(),
            }],
        );
        let doc = scaffold_index(
            &corpus_of(vec![r]),
            &Closure::default(),
            &HashMap::new(),
            "T",
        );
        let rel = &doc["classes"]["a"]["rel"];
        assert_eq!(rel.as_object().unwrap().len(), 1);
        assert_eq!(rel["requires"][0], "b");
    }

    #[test]
    fn scaffold_uses_the_frozen_rel_order() {
        let mut r = record("A", true);
        for key in ["contrastsWith", "requires", "hasPart"] {
            r.relations.insert(
                SCAFFOLD_REL_ORDER.iter().find(|k| **k == key).unwrap(),
                vec![Ref {
                    iri: "urn:ngm:class:x".into(),
                    label: "X".into(),
                }],
            );
        }
        let doc = scaffold_index(
            &corpus_of(vec![r]),
            &Closure::default(),
            &HashMap::new(),
            "T",
        );
        let keys: Vec<&String> = doc["classes"]["a"]["rel"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(keys, vec!["hasPart", "requires", "contrastsWith"]);
    }

    #[test]
    fn a_zero_quality_score_becomes_null() {
        let mut zero = record("A", true);
        zero.quality = 0.0;
        let mut set = record("B", true);
        set.quality = 0.35;
        let doc = scaffold_index(
            &corpus_of(vec![zero, set]),
            &Closure::default(),
            &HashMap::new(),
            "T",
        );
        assert!(doc["classes"]["a"]["q"].is_null());
        assert_eq!(doc["classes"]["b"]["q"], 0.35);
    }

    #[test]
    fn definitions_truncate_by_character_not_byte() {
        let mut r = record("A", true);
        r.definition = "\u{e9}".repeat(500);
        let doc = scaffold_index(
            &corpus_of(vec![r]),
            &Closure::default(),
            &HashMap::new(),
            "T",
        );
        assert_eq!(
            doc["classes"]["a"]["d"].as_str().unwrap().chars().count(),
            DEFINITION_CAP
        );
    }

    #[test]
    fn classes_are_ordered_by_class_slug() {
        let doc = scaffold_index(
            &corpus_of(vec![record("Zebra", true), record("Apple", true)]),
            &Closure::default(),
            &HashMap::new(),
            "T",
        );
        let keys: Vec<&String> = doc["classes"].as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["apple", "zebra"]);
        assert_eq!(doc["counts"]["classes"], 2);
    }

    #[test]
    fn backlinks_are_capped_and_keyed_by_page_slug() {
        let mut target = record("Target", true);
        target.slug = "target".into();
        let mut records = vec![target];
        for i in 0..25 {
            let mut src = record(&format!("Src{i:02}"), true);
            src.links = vec![Ref {
                iri: "urn:ngm:class:target".into(),
                label: "Target".into(),
            }];
            records.push(src);
        }
        let corpus = corpus_of(records);
        let bl = backlink_index(&corpus);
        assert_eq!(bl["target"].len(), 25);
        let doc = scaffold_index(&corpus, &Closure::default(), &bl, "T");
        assert_eq!(
            doc["classes"]["target"]["bl"].as_array().unwrap().len(),
            BACKLINK_CAP
        );
    }

    #[test]
    fn a_page_never_backlinks_to_itself() {
        let mut r = record("Self", true);
        r.links = vec![Ref {
            iri: "urn:ngm:class:self".into(),
            label: "Self".into(),
        }];
        assert!(backlink_index(&corpus_of(vec![r])).is_empty());
    }

    #[test]
    fn current_landscape_flattens_bullets_and_stops_at_the_next_heading() {
        let body = "- ### Current Landscape (2026)\n  - Widely adopted.\n  - Growing fast.\n- ### References\n  - Ignore me.\n";
        assert_eq!(
            extract_current_landscape(body, LANDSCAPE_CAP),
            "Widely adopted. Growing fast."
        );
    }

    #[test]
    fn current_landscape_reads_the_same_from_markdown_headings() {
        let logseq = "- ### Current Landscape (2026)\n  - Widely adopted.\n  - Growing fast.\n- ### References\n  - Ignore me.\n";
        let obsidian = "### Current Landscape (2026)\n\n- Widely adopted.\n- Growing fast.\n\n### References\n\n- Ignore me.\n";
        assert_eq!(
            extract_current_landscape(obsidian, LANDSCAPE_CAP),
            extract_current_landscape(logseq, LANDSCAPE_CAP)
        );
        assert_eq!(
            extract_current_landscape(&crate::bodies::convert_body(logseq), LANDSCAPE_CAP),
            "Widely adopted. Growing fast."
        );
    }

    #[test]
    fn current_landscape_keeps_its_own_sub_headings() {
        let body = "- ### Content\n  ## Current Landscape\n  Intro.\n  ### Standards\n  WCAG 2.2.\n  ## References\n  Ignore me.\n";
        assert_eq!(
            extract_current_landscape(body, LANDSCAPE_CAP),
            "Intro. ### Standards WCAG 2.2."
        );
        assert_eq!(
            extract_current_landscape(&crate::bodies::convert_body(body), LANDSCAPE_CAP),
            "Intro. ### Standards WCAG 2.2."
        );
    }

    #[test]
    fn the_newest_landscape_section_wins() {
        let body =
            "## Current Landscape (2025)\nOld.\n## Other\nx\n### Current Landscape (2026)\nNew.\n";
        assert_eq!(extract_current_landscape(body, LANDSCAPE_CAP), "New.");
    }

    #[test]
    fn a_page_without_a_landscape_section_yields_nothing() {
        assert_eq!(extract_current_landscape("- ### Other\n  - x\n", 100), "");
    }

    #[test]
    fn prose_index_omits_pages_with_nothing_to_add() {
        let mut short = record("Short", true);
        short.definition = "brief".into();
        let mut long = record("Long", true);
        long.definition = "x".repeat(DEFINITION_CAP + 1);
        let doc = prose_index(&corpus_of(vec![short, long]), "T");
        assert_eq!(doc["counts"]["pages"], 1);
        assert!(doc["pages"].get("short").is_none());
        assert!(doc["pages"]["long"]["dfull"].is_string());
    }

    #[test]
    fn search_index_carries_aliases_as_labels() {
        let r = record("Graph", true);
        let mut aliases = HashMap::new();
        aliases.insert(
            "Graph".to_owned(),
            vec!["KG".to_owned(), "Graph".to_owned()],
        );
        let doc = search_index(&corpus_of(vec![r]), &aliases);
        assert_eq!(doc[0]["labels"], json!(["Graph", "KG"]));
    }

    #[test]
    fn search_index_title_cases_the_domain_name() {
        let mut r = record("A", true);
        r.domain = "spatial-computing".into();
        let doc = search_index(&corpus_of(vec![r]), &HashMap::new());
        assert_eq!(doc[0]["domain_name"], "Spatial Computing");
    }

    #[test]
    fn a_non_ontology_page_gets_only_id_title_labels() {
        let mut r = record("Note", true);
        r.has_ontology = false;
        let doc = search_index(&corpus_of(vec![r]), &HashMap::new());
        assert_eq!(doc[0].as_object().unwrap().len(), 3);
    }
}
