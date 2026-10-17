//! graphicle graph → `codescape-model` nodes and edges.
//!
//! graphicle's code nodes carry no `type` attribute (only documents carry
//! `node_kind`), so the kind is read from `type` when present and otherwise
//! derived from structure: the file node by graphicle's own label rule,
//! methods as targets of `method` edges (or `.name()` labels), functions by
//! their `name()` label, classes as owners of methods. Spans arrive as a start
//! line (`L12`) or a range (`L12-L30`); a missing end is the line before the
//! next symbol in the file that the node does not contain.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use codescape_model::{
    lod, CodeEdge, CodeNode, Confidence, GraphSnapshot, IdInterner, InternError, Lang, NodeKind,
    Relation, Span, Tier,
};

use crate::engine::{attr_str, is_file_node, node_key, Graph};

/// Parse graphicle's `source_location` (`L12` or `L12-L30`).
pub(crate) fn parse_location(loc: &str) -> Option<(u32, Option<u32>)> {
    let rest = loc.strip_prefix('L')?;
    let (start, end) = match rest.split_once('-') {
        Some((a, b)) => (a, Some(b.trim_start_matches('L'))),
        None => (rest, None),
    };
    let start: u32 = start.parse().ok().filter(|&s| s > 0)?;
    let end = end
        .and_then(|e| e.parse::<u32>().ok())
        .filter(|&e| e >= start);
    Some((start, end))
}

/// Confidence from graphicle's upper-case strings, case-insensitively.
pub(crate) fn confidence(s: &str) -> Confidence {
    if s.eq_ignore_ascii_case("inferred") {
        Confidence::Inferred
    } else if s.eq_ignore_ascii_case("ambiguous") {
        Confidence::Ambiguous
    } else {
        Confidence::Extracted
    }
}

/// Display label: graphicle's `.run()` / `start()` become `run` / `start`.
pub(crate) fn display_label(raw: &str) -> &str {
    let s = raw.strip_prefix('.').unwrap_or(raw);
    s.strip_suffix("()").unwrap_or(s)
}

fn is_constant_name(name: &str) -> bool {
    name.len() > 1
        && name.chars().any(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Kind of graphicle node `i`.
fn kind_of(graph: &Graph, i: usize, lang: Lang) -> NodeKind {
    let a = &graph.nodes()[i].attributes;
    let ty = attr_str(a, "type");
    if !ty.is_empty() {
        return NodeKind::from_graphicle(ty);
    }
    // Non-code records (`doc_ref` citations, `rationale` comments, concepts…)
    // keep graphicle's own class instead of being guessed from their label.
    match attr_str(a, "file_type") {
        "" | "code" => {}
        "document" | "paper" | "image" | "video" => return NodeKind::Document,
        other => return NodeKind::from_graphicle(other),
    }
    if attr_str(a, "source_file").is_empty() {
        return NodeKind::External;
    }
    if is_file_node(a) {
        return NodeKind::File;
    }
    let node_kind = attr_str(a, "node_kind");
    if !node_kind.is_empty() {
        return NodeKind::from_graphicle(node_kind);
    }
    let label = attr_str(a, "label");
    let is_member = graph
        .in_edges(i)
        .iter()
        .any(|(_, _, e)| attr_str(&e.attributes, "relation") == "method");
    if is_member || (label.starts_with('.') && label.ends_with("()")) {
        return NodeKind::Method;
    }
    if label.ends_with("()") {
        return NodeKind::Function;
    }
    let out = graph.out_edges(i);
    if out
        .iter()
        .any(|(_, _, e)| attr_str(&e.attributes, "relation") == "case_of")
    {
        return NodeKind::Enum;
    }
    if is_constant_name(label) {
        return NodeKind::Constant;
    }
    let owns_methods = out
        .iter()
        .any(|(_, _, e)| attr_str(&e.attributes, "relation") == "method");
    let callable_class = a
        .get("_callable_class")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    match lang {
        Lang::Rust | Lang::Go => NodeKind::Struct,
        _ if owns_methods || callable_class => NodeKind::Class,
        _ if label.chars().next().is_some_and(char::is_uppercase) => NodeKind::Class,
        _ => NodeKind::Variable,
    }
}

/// Map graphicle nodes at `indices` (whole files at a time, so spans can be
/// derived) into fresh `CodeNode`s with ids from `interner`. Parent, tier,
/// cluster and analytics are filled in later by [`assemble`].
pub(crate) fn map_nodes(
    graph: &Graph,
    indices: &[usize],
    interner: &mut IdInterner,
    loc: &HashMap<String, u32>,
) -> Result<Vec<CodeNode>, InternError> {
    let mut nodes = Vec::with_capacity(indices.len());
    // Explicit range ends, and the structural owner (class of a method) of each node.
    let mut explicit_end: Vec<Option<u32>> = Vec::with_capacity(indices.len());
    let mut owner: HashMap<usize, usize> = HashMap::new();
    for &i in indices {
        let n = &graph.nodes()[i];
        let a = &n.attributes;
        let key = node_key(&n.id);
        let id = interner.intern(&key)?;
        let source = attr_str(a, "source_file");
        let path: Option<Arc<str>> = (!source.is_empty()).then(|| Arc::from(source));
        let lang = path.as_deref().map_or(Lang::Other, Lang::from_path);
        let kind = kind_of(graph, i, lang);
        let raw_label = attr_str(a, "label");
        let label: Arc<str> = Arc::from(if kind == NodeKind::File {
            raw_label
        } else {
            display_label(raw_label)
        });
        let file_loc = path
            .as_deref()
            .and_then(|p| loc.get(p))
            .copied()
            .unwrap_or(0);
        let (span, end) = match (&kind, parse_location(attr_str(a, "source_location"))) {
            (NodeKind::File, _) => (
                Some(Span {
                    start_line: 1,
                    end_line: file_loc.max(1),
                }),
                Some(file_loc.max(1)),
            ),
            (_, Some((start, end))) => (
                Some(Span {
                    start_line: start,
                    end_line: end.unwrap_or(start),
                }),
                end,
            ),
            (_, None) => (None, None),
        };
        if kind != NodeKind::File {
            for (u, _, e) in graph.in_edges(i) {
                let rel = attr_str(&e.attributes, "relation");
                let ua = &graph.nodes()[u].attributes;
                if matches!(rel, "method" | "contains")
                    && attr_str(ua, "source_file") == source
                    && !is_file_node(ua)
                {
                    owner.insert(i, u);
                    break;
                }
            }
        }
        explicit_end.push(end);
        nodes.push(CodeNode {
            id,
            key: Arc::from(key),
            kind: kind.clone(),
            lang,
            label,
            path,
            span,
            parent: 0,
            community: 0,
            cluster: 0,
            importance: 0.0,
            tier: Tier::L2,
            size: if kind == NodeKind::File { file_loc } else { 0 },
        });
    }
    derive_span_ends(indices, &mut nodes, &explicit_end, &owner, loc);
    Ok(nodes)
}

/// Fill missing span ends: the line before the next symbol of the same file
/// that is not a descendant of the node, else the file's last line.
fn derive_span_ends(
    indices: &[usize],
    nodes: &mut [CodeNode],
    explicit_end: &[Option<u32>],
    owner: &HashMap<usize, usize>,
    loc: &HashMap<String, u32>,
) {
    let mut by_file: HashMap<Arc<str>, Vec<usize>> = HashMap::new();
    for (k, n) in nodes.iter().enumerate() {
        if n.kind != NodeKind::File && n.span.is_some() && explicit_end[k].is_none() {
            if let Some(p) = &n.path {
                by_file.entry(p.clone()).or_default().push(k);
            }
        }
    }
    // Starts of every symbol of each file, explicit ones included, for the "next symbol" scan.
    let mut starts_by_file: HashMap<Arc<str>, Vec<(u32, usize)>> = HashMap::new();
    for (k, n) in nodes.iter().enumerate() {
        if let (Some(p), Some(s), false) = (&n.path, n.span, n.kind == NodeKind::File) {
            if by_file.contains_key(p) {
                starts_by_file
                    .entry(p.clone())
                    .or_default()
                    .push((s.start_line, indices[k]));
            }
        }
    }
    let is_descendant = |candidate: usize, ancestor: usize| {
        let mut cur = candidate;
        for _ in 0..32 {
            match owner.get(&cur) {
                Some(&up) if up == ancestor => return true,
                Some(&up) => cur = up,
                None => return false,
            }
        }
        false
    };
    for (path, members) in by_file {
        let mut starts = starts_by_file.remove(&path).unwrap_or_default();
        starts.sort_unstable();
        let last = loc.get(&*path).copied().unwrap_or(0);
        for k in members {
            let me = indices[k];
            let Some(span) = nodes[k].span.as_mut() else {
                continue;
            };
            let from = starts.partition_point(|&(s, _)| s <= span.start_line);
            let next = starts[from..]
                .iter()
                .find(|&&(_, gi)| !is_descendant(gi, me))
                .map(|&(s, _)| s - 1);
            span.end_line = next.unwrap_or(last).max(span.start_line);
        }
    }
}

/// Map every graphicle edge whose endpoints have ids in `ids` (by graphicle index).
pub(crate) fn map_edges(graph: &Graph, ids: &[u32]) -> Vec<CodeEdge> {
    let mut out = Vec::with_capacity(graph.edge_count());
    for (u, v, e) in graph.iter_edges() {
        let (s, t) = (ids[u], ids[v]);
        if s == 0 || t == 0 || s == t {
            continue;
        }
        let rel = attr_str(&e.attributes, "relation");
        out.push(CodeEdge {
            source: s,
            target: t,
            relation: Relation::from_graphicle(if rel.is_empty() { "related" } else { rel }),
            weight: 1.0,
            confidence: confidence(attr_str(&e.attributes, "confidence")),
        });
    }
    out
}

/// The structural part of a snapshot built from a graphicle graph.
pub(crate) struct Assembly {
    /// Nodes (tiers, parents and clusters set) and edges.
    pub(crate) snapshot: GraphSnapshot,
    /// Ids that were mapped fresh rather than carried over from the previous snapshot.
    pub(crate) fresh: HashSet<u32>,
}

/// Build nodes and edges from `graph`.
///
/// With `prev` = `(snapshot, touched paths)`, nodes of untouched files are
/// carried over from the previous snapshot unchanged (community and
/// importance included) and only nodes of touched files, plus nodes the
/// previous snapshot did not have, are mapped fresh. Fresh nodes that existed
/// before keep their previous community and importance. Directory aggregates
/// are minted again by [`lod::build_tiers`] (same interned ids), and edges are
/// re-read from the graph in full, which is linear and cheap.
pub(crate) fn assemble(
    graph: &Graph,
    interner: &mut IdInterner,
    prev: Option<(&GraphSnapshot, &BTreeSet<String>)>,
    loc: &HashMap<String, u32>,
) -> Result<Assembly, InternError> {
    let n = graph.node_count();
    let prev_index = prev.map(|(s, _)| s.index()).unwrap_or_default();
    let mut ids = Vec::with_capacity(n);
    let mut carried: Vec<Option<CodeNode>> = Vec::with_capacity(n);
    let mut fresh_indices = Vec::new();
    for (i, node) in graph.nodes().iter().enumerate() {
        let id = match node.id.as_str() {
            Some(key) => interner.intern(key)?,
            None => interner.intern(&node_key(&node.id))?,
        };
        ids.push(id);
        let source = attr_str(&node.attributes, "source_file");
        let reuse = prev.and_then(|(snap, touched)| {
            if touched.contains(source) {
                return None;
            }
            prev_index
                .get(&id)
                .map(|&k| &snap.nodes[k])
                .filter(|old| old.kind != NodeKind::Directory)
        });
        match reuse {
            Some(old) => carried.push(Some(old.clone())),
            None => {
                carried.push(None);
                fresh_indices.push(i);
            }
        }
    }
    let mut fresh_nodes = map_nodes(graph, &fresh_indices, interner, loc)?.into_iter();
    let mut fresh = HashSet::with_capacity(fresh_indices.len());
    let mut nodes = Vec::with_capacity(n);
    for slot in carried {
        let node = match slot {
            Some(node) => node,
            None => {
                let mut node = fresh_nodes.next().expect("one mapped node per fresh index");
                if let Some(old) = prev_index
                    .get(&node.id)
                    .and_then(|&k| prev.map(|(s, _)| &s.nodes[k]))
                {
                    node.community = old.community;
                    node.importance = old.importance;
                }
                fresh.insert(node.id);
                node
            }
        };
        nodes.push(node);
    }

    // Structure: symbol → file by path, file → top-level directory by build_tiers.
    let mut file_of: HashMap<Arc<str>, u32> = HashMap::new();
    for node in &nodes {
        if node.kind == NodeKind::File {
            if let Some(p) = &node.path {
                file_of.entry(p.clone()).or_insert(node.id);
            }
        }
    }
    for node in nodes.iter_mut() {
        node.cluster = 0;
        node.parent = match (&node.kind, &node.path) {
            (NodeKind::File, _) => 0,
            (_, Some(p)) => file_of
                .get(p)
                .copied()
                .filter(|&f| f != node.id)
                .unwrap_or(0),
            _ => 0,
        };
    }
    let mut edges = map_edges(graph, &ids);
    let contains: HashSet<(u32, u32)> = edges
        .iter()
        .filter(|e| e.relation == Relation::Contains)
        .map(|e| (e.source, e.target))
        .collect();
    for node in &nodes {
        if node.parent != 0 && !contains.contains(&(node.parent, node.id)) {
            edges.push(CodeEdge {
                source: node.parent,
                target: node.id,
                relation: Relation::Contains,
                weight: 1.0,
                confidence: Confidence::Extracted,
            });
        }
    }
    let mut snapshot = GraphSnapshot {
        nodes,
        edges,
        ..Default::default()
    };
    lod::build_tiers(&mut snapshot, interner)?;
    Ok(Assembly { snapshot, fresh })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_parse_both_forms() {
        assert_eq!(parse_location("L12"), Some((12, None)));
        assert_eq!(parse_location("L3-L9"), Some((3, Some(9))));
        assert_eq!(
            parse_location("L9-L3"),
            Some((9, None)),
            "inverted range drops the end"
        );
        assert_eq!(parse_location("L0"), None);
        assert_eq!(parse_location("12"), None);
    }

    #[test]
    fn confidence_and_labels() {
        assert_eq!(confidence("EXTRACTED"), Confidence::Extracted);
        assert_eq!(confidence("INFERRED"), Confidence::Inferred);
        assert_eq!(confidence("ambiguous"), Confidence::Ambiguous);
        assert_eq!(confidence(""), Confidence::Extracted);
        assert_eq!(display_label(".run()"), "run");
        assert_eq!(display_label("start()"), "start");
        assert_eq!(display_label("Engine"), "Engine");
        assert!(is_constant_name("MAX_SIZE"));
        assert!(!is_constant_name("Engine"));
    }

    #[test]
    fn kinds_follow_graphicle_records() {
        use serde_json::{json, Map, Value};
        let attrs = |pairs: &[(&str, Value)]| -> Map<String, Value> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect()
        };
        let mut g = graphicle::graph::Graph::new(true, false);
        let code = |label: &str| {
            attrs(&[
                ("label", json!(label)),
                ("file_type", json!("code")),
                ("source_file", json!("web/a.ts")),
                ("source_location", json!("L2")),
            ])
        };
        g.node("web_a", code("a.ts")).unwrap();
        g.node("web_a_run", code("run()")).unwrap();
        g.node("web_a_box", code("Box")).unwrap();
        g.node("web_a_box_open", code(".open()")).unwrap();
        g.node("web_a_max", code("MAX_SIZE")).unwrap();
        g.node(
            "docref_adr_0010",
            attrs(&[
                ("label", json!("ADR-0010")),
                ("file_type", json!("doc_ref")),
                ("source_file", json!("web/a.ts")),
            ]),
        )
        .unwrap();
        g.node("serde", attrs(&[("label", json!("serde"))]))
            .unwrap();
        g.node(
            "typed",
            attrs(&[
                ("label", json!("T")),
                ("type", json!("trait")),
                ("source_file", json!("web/a.ts")),
            ]),
        )
        .unwrap();
        g.relate("web_a_box", "web_a_box_open", "method", Map::new())
            .unwrap();
        let kind = |id: &str| kind_of(&g, g.index_of(id).unwrap(), Lang::TypeScript);
        assert_eq!(kind("web_a"), NodeKind::File);
        assert_eq!(kind("web_a_run"), NodeKind::Function);
        assert_eq!(kind("web_a_box"), NodeKind::Class);
        assert_eq!(kind("web_a_box_open"), NodeKind::Method);
        assert_eq!(kind("web_a_max"), NodeKind::Constant);
        assert_eq!(kind("docref_adr_0010"), NodeKind::Other("doc_ref".into()));
        assert_eq!(kind("serde"), NodeKind::External);
        assert_eq!(
            kind("typed"),
            NodeKind::Interface,
            "an explicit `type` wins"
        );
    }
}
