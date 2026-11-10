//! Extractor for the pre-October-2026 property format of `visionGraph/knowledge/pages/*.md`.
//!
//! Relations are written as Logseq properties, `key:: [[A]], [[B]]`, or as bold labels,
//! `- **Key**: [[A]]`. Only the learner's fourteen typed keys are kept; `is-a` and
//! `is-subclass-of` share one type, as do `part-of` and `is-part-of`. Pages are identified by file
//! stem; wikilinks resolve by stem, `preferred-term`, `title`, `alias` or the local name of
//! `owl:class`, compared after lowercasing and dropping every non-alphanumeric character, so
//! `[[AIGovernancePrinciple]]` finds the page titled "AI Governance Principle".

use crate::corpus::Triple;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

/// The fourteen typed keys, by normalised spelling, mapped to their canonical name.
const KEYS: &[(&str, &str)] = &[
    ("relatedto", "related-to"),
    ("enables", "enables"),
    ("uses", "uses"),
    ("requires", "requires"),
    ("haspart", "has-part"),
    ("isa", "is-subclass-of"),
    ("issubclassof", "is-subclass-of"),
    ("supports", "supports"),
    ("bridgesto", "bridges-to"),
    ("dependson", "depends-on"),
    ("contrastswith", "contrasts-with"),
    ("implements", "implements"),
    ("standardizedby", "standardized-by"),
    ("standardisedby", "standardized-by"),
    ("partof", "part-of"),
    ("ispartof", "part-of"),
    ("implementedinlayer", "implemented-in-layer"),
];

pub fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn canonical(key: &str) -> Option<&'static str> {
    let k = norm(key);
    KEYS.iter().find(|(n, _)| *n == k).map(|(_, c)| *c)
}

/// Split a line into `(key, value)` for `key:: value` or `**Key**: value`.
fn property(line: &str) -> Option<(&str, &str)> {
    let l = line.trim_start().trim_start_matches("- ").trim_start();
    if let Some(rest) = l.strip_prefix("**") {
        let (k, v) = rest.split_once("**")?;
        return Some((k, v.trim_start().strip_prefix(':')?));
    }
    let (k, v) = l.split_once("::")?;
    // Logseq property keys never contain whitespace; prose that happens to contain `::` does.
    (!k.is_empty() && !k.contains(char::is_whitespace)).then_some((k, v))
}

fn wikilinks(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = v;
    while let Some(a) = rest.find("[[") {
        let Some(b) = rest[a + 2..].find("]]") else {
            break;
        };
        let inner = &rest[a + 2..a + 2 + b];
        let target = inner.split('|').next().unwrap_or("").trim();
        if !target.is_empty() {
            out.push(target.to_string());
        }
        rest = &rest[a + 2 + b + 2..];
    }
    out
}

pub struct Page {
    pub stem: String,
    pub names: Vec<String>,
    pub rels: Vec<(&'static str, String)>,
}

pub fn parse(stem: &str, text: &str) -> Page {
    let mut page = Page {
        stem: stem.to_string(),
        names: vec![stem.to_string()],
        rels: Vec::new(),
    };
    for line in text.lines() {
        let Some((k, v)) = property(line) else {
            continue;
        };
        match norm(k).as_str() {
            "preferredterm" | "title" => page.names.push(v.trim().to_string()),
            "alias" | "aliases" => page.names.extend(
                v.split(',')
                    .map(|a| a.trim().trim_matches(|c| c == '[' || c == ']').to_string()),
            ),
            "owlclass" => {
                if let Some(local) = v.trim().rsplit(':').next() {
                    page.names.push(local.to_string());
                }
            }
            _ => {
                if let Some(c) = canonical(k) {
                    page.rels.extend(wikilinks(v).into_iter().map(|t| (c, t)));
                }
            }
        }
    }
    page
}

/// One snapshot: pages by stem, and typed triples between them in stem space.
pub struct Snapshot {
    pub stems: Vec<String>,
    pub triples: Vec<(String, &'static str, String)>,
    pub stats: Stats,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Stats {
    pub pages: usize,
    pub link_mentions: usize,
    pub resolved: usize,
    pub self_loops: usize,
    pub name_collisions: usize,
    pub per_relation: BTreeMap<String, usize>,
}

/// Load `dir/*.md` (one level; `.deleted/` and other subdirectories are skipped).
pub fn load(dir: &Path) -> std::io::Result<Snapshot> {
    let mut paths: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
        .collect();
    paths.sort();
    let pages: Vec<Page> = paths
        .iter()
        .map(|p| {
            Ok(parse(
                &p.file_stem().unwrap().to_string_lossy(),
                &fs::read_to_string(p)?,
            ))
        })
        .collect::<std::io::Result<_>>()?;

    let mut stats = Stats {
        pages: pages.len(),
        ..Default::default()
    };
    // Stems first, so a page's own file name always wins over another page's alias.
    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, p) in pages.iter().enumerate() {
        index.insert(norm(&p.stem), i);
    }
    for (i, p) in pages.iter().enumerate() {
        for n in &p.names[1..] {
            let k = norm(n);
            match index.get(&k) {
                Some(&j) if j != i => stats.name_collisions += 1,
                Some(_) => {}
                None => {
                    index.insert(k, i);
                }
            }
        }
    }
    let mut seen = HashSet::new();
    let mut triples = Vec::new();
    for (i, p) in pages.iter().enumerate() {
        for (rel, target) in &p.rels {
            stats.link_mentions += 1;
            // Namespaced class links (`[[ai:ModelArchitecture]]`) resolve on their local name.
            let local = target.rsplit_once(':').map(|(_, l)| l);
            let Some(&j) = index
                .get(&norm(target))
                .or_else(|| local.and_then(|l| index.get(&norm(l))))
            else {
                continue;
            };
            stats.resolved += 1;
            if i == j {
                stats.self_loops += 1;
                continue;
            }
            if seen.insert((i, *rel, j)) {
                *stats.per_relation.entry(rel.to_string()).or_default() += 1;
                triples.push((p.stem.clone(), *rel, pages[j].stem.clone()));
            }
        }
    }
    Ok(Snapshot {
        stems: pages.into_iter().map(|p| p.stem).collect(),
        triples,
        stats,
    })
}

/// Map stem-space triples onto node ids, dropping any whose endpoint is not a node.
pub fn to_ids(
    triples: &[(String, &'static str, String)],
    node: &HashMap<String, u32>,
    rel: &HashMap<&'static str, u16>,
) -> Vec<Triple> {
    triples
        .iter()
        .filter_map(|(h, r, t)| {
            Some(Triple {
                head: *node.get(h)?,
                rel: *rel.get(r)?,
                tail: *node.get(t)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_property_spellings_and_merges_synonyms() {
        let p = parse(
            "AI-0001-x",
            "- preferred-term:: Thing One\n    - enables:: [[A]], [[B|label]]\n- **Is Subclass Of**: [[C]]\n- **Requires**: plain text only\n  - is-a:: [[D]]\n- source:: [[Ignored]]\n",
        );
        assert_eq!(
            p.names,
            vec!["AI-0001-x".to_string(), "Thing One".to_string()]
        );
        let got: Vec<_> = p.rels.iter().map(|(r, t)| format!("{r}>{t}")).collect();
        assert_eq!(
            got,
            vec![
                "enables>A",
                "enables>B",
                "is-subclass-of>C",
                "is-subclass-of>D"
            ]
        );
    }

    #[test]
    fn camel_case_targets_resolve_to_spaced_titles() {
        assert_eq!(
            norm("AIGovernancePrinciple"),
            norm("AI Governance Principle")
        );
        assert_eq!(canonical("Has Part"), Some("has-part"));
        assert_eq!(canonical("is-part-of"), Some("part-of"));
        assert_eq!(canonical("source"), None);
    }

    #[test]
    fn namespaced_targets_resolve_on_local_name() {
        let dir = std::env::temp_dir().join(format!("rf-property-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Model Architecture.md"), "- term-id:: X\n").unwrap();
        std::fs::write(
            dir.join("Transformer.md"),
            "- is-subclass-of:: [[ai:ModelArchitecture]]\n",
        )
        .unwrap();
        let s = load(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            s.triples,
            vec![(
                "Transformer".to_string(),
                "is-subclass-of",
                "Model Architecture".to_string()
            )]
        );
    }
}
