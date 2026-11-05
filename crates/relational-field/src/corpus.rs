//! Load the visionGraph knowledge corpus into a typed, directed multigraph.
//!
//! Each page's YAML frontmatter is read with a deliberately small line parser: the corpus
//! writes relations as `key:` followed by `- '[[Target]]'` items (or a single inline
//! `key: '[[Target]]'`), which is all this needs. A key counts as a relation when its values
//! are wikilinks; everything else (`aliases`, `domain`, `slug`, ...) is metadata.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

/// One directed, typed edge as recorded on the source page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Triple {
    pub head: u32,
    pub rel: u16,
    pub tail: u32,
}

/// The loaded graph.
pub struct Graph {
    pub titles: Vec<String>,
    /// `None` when the page has no `domain`.
    pub domain: Vec<Option<u16>>,
    pub domain_names: Vec<String>,
    pub rel_names: Vec<String>,
    pub triples: Vec<Triple>,
    pub stats: LoadStats,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct LoadStats {
    pub pages: usize,
    pub link_mentions: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub self_loops_dropped: usize,
    pub duplicates_dropped: usize,
    pub triples: usize,
    pub per_relation: BTreeMap<String, usize>,
}

struct Page {
    title: String,
    slug: Option<String>,
    stem: String,
    aliases: Vec<String>,
    domain: Option<String>,
    /// (relation key, raw wikilink target)
    rels: Vec<(String, String)>,
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('\'').and_then(|t| t.strip_suffix('\''))
        .or_else(|| s.strip_prefix('"').and_then(|t| t.strip_suffix('"')))
        .unwrap_or(s)
}

/// `[[target|label]]` -> `target`; `None` when the value is not a wikilink.
fn wikilink_target(v: &str) -> Option<String> {
    let v = unquote(v);
    let inner = v.strip_prefix("[[")?.strip_suffix("]]")?;
    let target = inner.split('|').next()?.trim();
    (!target.is_empty()).then(|| target.to_string())
}

fn parse_page(stem: &str, text: &str) -> Option<Page> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut page = Page {
        title: stem.to_string(),
        slug: None,
        stem: stem.to_string(),
        aliases: Vec::new(),
        domain: None,
        rels: Vec::new(),
    };
    let mut key = String::new();
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some(item) = line.strip_prefix("- ") {
            match key.as_str() {
                "aliases" => page.aliases.push(unquote(item).to_string()),
                _ => {
                    if let Some(t) = wikilink_target(item) {
                        page.rels.push((key.clone(), t));
                    }
                }
            }
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            continue; // nested mapping (e.g. `generated:`), not a relation
        }
        if let Some((k, v)) = line.split_once(':') {
            key = k.trim().to_string();
            let v = v.trim();
            match key.as_str() {
                "title" if !v.is_empty() => page.title = unquote(v).to_string(),
                "slug" if !v.is_empty() => page.slug = Some(unquote(v).to_string()),
                "domain" if !v.is_empty() => page.domain = Some(unquote(v).to_string()),
                _ => {
                    if let Some(t) = wikilink_target(v) {
                        page.rels.push((key.clone(), t));
                    }
                }
            }
        }
    }
    Some(page)
}

/// Read every `*.md` page directly under `dir`.
pub fn load(dir: &Path) -> std::io::Result<Graph> {
    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    entries.sort(); // deterministic node ids

    let mut pages = Vec::with_capacity(entries.len());
    for p in &entries {
        let stem = p.file_stem().unwrap().to_string_lossy().to_string();
        if let Some(page) = parse_page(&stem, &fs::read_to_string(p)?) {
            pages.push(page);
        }
    }

    // Name index: title first, then slug, alias and filename stem. First claim wins, so a
    // page's own title is never shadowed by another page's alias.
    let mut index: HashMap<String, u32> = HashMap::new();
    for (i, pg) in pages.iter().enumerate() {
        index.entry(pg.title.to_lowercase()).or_insert(i as u32);
    }
    for (i, pg) in pages.iter().enumerate() {
        let names = pg.slug.iter().chain(pg.aliases.iter()).chain(std::iter::once(&pg.stem));
        for n in names {
            index.entry(n.to_lowercase()).or_insert(i as u32);
        }
    }

    let mut domain_ids: BTreeMap<String, u16> = BTreeMap::new();
    for pg in &pages {
        if let Some(d) = &pg.domain {
            let n = domain_ids.len() as u16;
            domain_ids.entry(d.clone()).or_insert(n);
        }
    }
    let mut rel_ids: BTreeMap<String, u16> = BTreeMap::new();
    let mut stats = LoadStats { pages: pages.len(), ..Default::default() };
    let mut seen: HashSet<Triple> = HashSet::new();
    let mut triples = Vec::new();
    for (i, pg) in pages.iter().enumerate() {
        for (key, target) in &pg.rels {
            stats.link_mentions += 1;
            let Some(&j) = index.get(&target.to_lowercase()) else {
                stats.unresolved += 1;
                continue;
            };
            stats.resolved += 1;
            if j == i as u32 {
                stats.self_loops_dropped += 1;
                continue;
            }
            let n = rel_ids.len() as u16;
            let rel = *rel_ids.entry(key.clone()).or_insert(n);
            let t = Triple { head: i as u32, rel, tail: j };
            if seen.insert(t) {
                triples.push(t);
            } else {
                stats.duplicates_dropped += 1;
            }
        }
    }

    let mut rel_names = vec![String::new(); rel_ids.len()];
    for (k, v) in &rel_ids {
        rel_names[*v as usize] = k.clone();
    }
    let mut domain_names = vec![String::new(); domain_ids.len()];
    for (k, v) in &domain_ids {
        domain_names[*v as usize] = k.clone();
    }
    for t in &triples {
        *stats.per_relation.entry(rel_names[t.rel as usize].clone()).or_default() += 1;
    }
    stats.triples = triples.len();

    Ok(Graph {
        titles: pages.iter().map(|p| p.title.clone()).collect(),
        domain: pages.iter().map(|p| p.domain.as_ref().map(|d| domain_ids[d])).collect(),
        domain_names,
        rel_names,
        triples,
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relations_aliases_and_piped_links() {
        let text = "---\ntype: Class\ntitle: 3D Asset\naliases:\n- 3DAsset\nslug: 3-d-asset\n\
                    domain: spatial-computing\ngenerated:\n  by: x\nlinks:\n- '[[w3-c|W3C]]'\n\
                    is-a:\n- '[[Digital Asset]]'\nsingle: '[[Solo]]'\n---\nbody [[Ignored]]\n";
        let p = parse_page("3D Asset", text).unwrap();
        assert_eq!(p.title, "3D Asset");
        assert_eq!(p.slug.as_deref(), Some("3-d-asset"));
        assert_eq!(p.aliases, vec!["3DAsset"]);
        assert_eq!(p.domain.as_deref(), Some("spatial-computing"));
        assert_eq!(
            p.rels,
            vec![
                ("links".into(), "w3-c".into()),
                ("is-a".into(), "Digital Asset".into()),
                ("single".into(), "Solo".into()),
            ]
        );
    }

    #[test]
    fn rejects_pages_without_frontmatter() {
        assert!(parse_page("x", "no frontmatter\n").is_none());
    }
}
