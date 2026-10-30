//! Code → concept link index (ADR-2164 D4, D5): the pure half of the
//! cross-volume link layer.
//!
//! Codescape's `initialGraphLoad` carries `metadata.ontology_refs` on annotated
//! code nodes (`<iri>#L<line> …`); VisionClaw's carries `metadata.class_iri`,
//! `metadata.page_iri` and the top-level `owl_class_iri` on knowledge and
//! ontology nodes. Each `BackendConnection` builds its own table from its own
//! topology frame; [`join`] matches the two on the IRI and yields one
//! [`LinkRecord`] per target node. Nothing here is persisted or sent, and no
//! server ever talks to the other (ADR-2164 D4, ADR-2143 I1).
//!
//! No gdext types: `binary_protocol.rs` wraps these for GDScript.
//!
//! ```
//! use std::collections::HashMap;
//! use visionclaw_xr_gdext::link_index::{join, parse_ontology_refs, CodeRef};
//!
//! let code = vec![CodeRef {
//!     key: "src_rank".into(),
//!     id: 7,
//!     path: "src/rank.rs".into(),
//!     is_file: true,
//!     refs: parse_ontology_refs("urn:ngm:class:bm25#L12"),
//! }];
//! let iri_ids = HashMap::from([("urn:ngm:class:bm25".to_string(), vec![42])]);
//! let out = join(&code, 3, &iri_ids, 5, |_| 1);
//! assert_eq!(out.records.len(), 1);
//! assert_eq!(out.records[0].target.volume, "ontology");
//! assert_eq!(out.records[0].provenance.generations, [3, 5]);
//! ```

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde_json::Value;

/// Connection id of the Codescape stream (contract §1).
pub const CODE_CONN: &str = "codescape";
/// Connection id of the VisionClaw stream (contract §1).
pub const VC_CONN: &str = "visionclaw";
/// Volume ids (contract §1).
pub const VOLUME_CODESCAPE: &str = "codescape";
/// Knowledge volume (class code 0).
pub const VOLUME_KNOWLEDGE: &str = "knowledge";
/// Ontology volume (class code 1).
pub const VOLUME_ONTOLOGY: &str = "ontology";
/// Link kind of a code annotation.
pub const KIND_ANNOTATES: &str = "annotates";
/// Provenance `via` of a code annotation.
pub const VIA_CODESCAPE_ANNOTATION: &str = "codescape-annotation";

const IRI_PREFIXES: &[&str] = &["urn:ngm:class:", "urn:visionflow:page:"];

/// The IRI shape an annotation may carry (ADR-2164 D1), checked again here so a
/// malformed ref from any server is skipped rather than joined.
pub fn is_annotation_iri(iri: &str) -> bool {
    IRI_PREFIXES.iter().any(|p| {
        iri.strip_prefix(p).is_some_and(|local| {
            (1..=128).contains(&local.len())
                && local
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    })
}

/// The IRIs a VisionClaw node answers to: `metadata.class_iri`,
/// `metadata.page_iri` and the top-level `owl_class_iri`, in that order,
/// non-empty and de-duplicated.
pub fn extract_iris(node: &Value) -> Vec<String> {
    let meta = node.get("metadata");
    let cands = [
        meta.and_then(|m| m.get("class_iri")),
        meta.and_then(|m| m.get("page_iri")),
        node.get("owl_class_iri"),
    ];
    let mut out: Vec<String> = Vec::new();
    for v in cands.into_iter().flatten() {
        if let Some(s) = v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            if !out.iter().any(|o| o == s) {
                out.push(s.to_string());
            }
        }
    }
    out
}

/// Parse `metadata.ontology_refs` (`"<iri>#L<n> …"`) into `(iri, n)` pairs.
/// Tokens without `#L`, with a malformed IRI, or with a line that is not a
/// positive integer are skipped.
pub fn parse_ontology_refs(s: &str) -> Vec<(String, u32)> {
    s.split_whitespace()
        .filter_map(|tok| {
            let (iri, line) = tok.rsplit_once("#L")?;
            let line: u32 = line.parse().ok().filter(|&n| n > 0)?;
            is_annotation_iri(iri).then(|| (iri.to_string(), line))
        })
        .collect()
}

/// One annotated Codescape node from the current topology frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRef {
    /// `metadata_id`: Codescape's stable interned key.
    pub key: String,
    /// Wire id, valid for the frame's generation only.
    pub id: u32,
    /// `metadata.source_file`.
    pub path: String,
    /// `node_type == "file"`: carries the roll-up of its symbols.
    pub is_file: bool,
    /// Parsed `metadata.ontology_refs`.
    pub refs: Vec<(String, u32)>,
}

/// The [`CodeRef`] of one Codescape `initialGraphLoad` node, or `None` when it
/// carries no well-formed ref.
pub fn code_ref_from_node(node: &Value) -> Option<CodeRef> {
    let refs = parse_ontology_refs(node.get("metadata")?.get("ontology_refs")?.as_str()?);
    if refs.is_empty() {
        return None;
    }
    Some(CodeRef {
        key: node
            .get("metadata_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        id: u32::try_from(node.get("id")?.as_u64()?).ok()?,
        path: node
            .get("metadata")
            .and_then(|m| m.get("source_file"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        is_file: node.get("node_type").and_then(Value::as_str) == Some("file"),
        refs,
    })
}

/// `iri → [wire ids]` over VisionClaw nodes (one IRI may sit on several).
/// Ids within each list are sorted and unique.
pub fn iri_table<'a>(nodes: impl IntoIterator<Item = &'a Value>) -> HashMap<String, Vec<u32>> {
    let mut t: HashMap<String, Vec<u32>> = HashMap::new();
    for n in nodes {
        let Some(id) = n
            .get("id")
            .and_then(Value::as_u64)
            .and_then(|i| u32::try_from(i).ok())
        else {
            continue;
        };
        for iri in extract_iris(n) {
            t.entry(iri).or_default().push(id);
        }
    }
    for ids in t.values_mut() {
        ids.sort_unstable();
        ids.dedup();
    }
    t
}

/// One end of a link (ADR-2164 D5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Endpoint {
    /// ADR-2158 volume id.
    pub volume: String,
    /// Connection id.
    pub conn: String,
    /// Stable identity: Codescape `metadata_id`, or the VisionClaw IRI.
    pub key: String,
    /// Wire id, valid only for the generation in the provenance.
    pub id: u32,
}

/// Where a link came from (ADR-2164 D5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    /// `codescape-annotation`.
    pub via: String,
    /// Codescape repository label. Empty: the XR stream does not carry it yet.
    pub repo: String,
    /// File holding the annotation.
    pub path: String,
    /// 1-based line of the annotation.
    pub line: u32,
    /// Codescape revision. `None`: the XR stream does not carry it yet.
    pub revision: Option<u64>,
    /// `[source generation, target generation]`; a record is valid only while
    /// both connections are still on these.
    pub generations: [i64; 2],
}

/// A cross-volume link record (ADR-2164 D5). Client render state only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkRecord {
    /// The code node.
    pub source: Endpoint,
    /// The knowledge or ontology node.
    pub target: Endpoint,
    /// Always [`KIND_ANNOTATES`] here.
    pub kind: String,
    /// Origin of the claim.
    pub provenance: Provenance,
}

/// Output of [`join`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JoinResult {
    /// One record per (distinct annotation, target node), sorted by
    /// (target key, source key, target id, path, line).
    pub records: Vec<LinkRecord>,
    /// Distinct annotations that found no knowledge or ontology node.
    pub unresolved: usize,
}

/// The volume of a VisionClaw node by class code: `0` knowledge, `1` ontology.
/// Agents (`2`) and plain nodes (`3`) are not link targets.
pub fn volume_for_class(class_code: u8) -> Option<&'static str> {
    match class_code {
        0 => Some(VOLUME_KNOWLEDGE),
        1 => Some(VOLUME_ONTOLOGY),
        _ => None,
    }
}

/// Join Codescape refs against the VisionClaw IRI table.
///
/// - One annotation reaches the client on its symbol node and again in its
///   file's roll-up; refs are de-duplicated on (iri, path, line) and the
///   symbol's copy wins (lowest id among equals, for determinism).
/// - One record per target id: an IRI on two nodes gives two records.
/// - An annotation whose IRI is absent, or present only on nodes that are not
///   knowledge or ontology (`class_of` other than 0 or 1), counts once in
///   [`JoinResult::unresolved`] and yields no record.
pub fn join(
    code: &[CodeRef],
    code_gen: i64,
    iri_ids: &HashMap<String, Vec<u32>>,
    vc_gen: i64,
    class_of: impl Fn(u32) -> u8,
) -> JoinResult {
    // (iri, path, line) → the code node that carries it.
    let mut chosen: BTreeMap<(&str, &str, u32), &CodeRef> = BTreeMap::new();
    for c in code {
        for (iri, line) in &c.refs {
            let k = (iri.as_str(), c.path.as_str(), *line);
            match chosen.get(&k) {
                Some(prev) if (prev.is_file, prev.id) <= (c.is_file, c.id) => {}
                _ => {
                    chosen.insert(k, c);
                }
            }
        }
    }
    let mut out = JoinResult::default();
    for ((iri, path, line), c) in chosen {
        let mut hit = false;
        for &tid in iri_ids.get(iri).map_or(&[][..], Vec::as_slice) {
            let Some(volume) = volume_for_class(class_of(tid)) else {
                continue;
            };
            hit = true;
            out.records.push(LinkRecord {
                source: Endpoint {
                    volume: VOLUME_CODESCAPE.into(),
                    conn: CODE_CONN.into(),
                    key: c.key.clone(),
                    id: c.id,
                },
                target: Endpoint {
                    volume: volume.into(),
                    conn: VC_CONN.into(),
                    key: iri.to_string(),
                    id: tid,
                },
                kind: KIND_ANNOTATES.into(),
                provenance: Provenance {
                    via: VIA_CODESCAPE_ANNOTATION.into(),
                    repo: String::new(),
                    path: path.to_string(),
                    line,
                    revision: None,
                    generations: [code_gen, vc_gen],
                },
            });
        }
        if !hit {
            out.unresolved += 1;
        }
    }
    out.records.sort_by(|a, b| {
        (
            &a.target.key,
            &a.source.key,
            a.target.id,
            &a.provenance.path,
            a.provenance.line,
        )
            .cmp(&(
                &b.target.key,
                &b.source.key,
                b.target.id,
                &b.provenance.path,
                b.provenance.line,
            ))
    });
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const BM25: &str = "urn:ngm:class:bm25";
    const SPARQL: &str = "urn:ngm:class:sparql";
    const PAGE: &str = "urn:visionflow:page:comfy-ui";

    #[test]
    fn extract_iris_reads_three_keys_and_dedups() {
        let n = json!({
            "id": 1,
            "owl_class_iri": BM25,
            "metadata": { "class_iri": BM25, "page_iri": PAGE, "other_iri": SPARQL }
        });
        assert_eq!(extract_iris(&n), vec![BM25.to_string(), PAGE.to_string()]);
        let n = json!({ "id": 2, "owl_class_iri": SPARQL, "metadata": { "class_iri": "", "page_iri": 5 } });
        assert_eq!(extract_iris(&n), vec![SPARQL.to_string()]);
        assert!(extract_iris(&json!({ "id": 3 })).is_empty());
    }

    #[test]
    fn parse_ontology_refs_skips_malformed_tokens() {
        let s = format!(
            "{BM25}#L4 {SPARQL}#L0 {PAGE}#L12 urn:ngm:class:Bad#L3 {BM25} #L9 {BM25}#Lx https://x/y#L2 {SPARQL}#L7"
        );
        assert_eq!(
            parse_ontology_refs(&s),
            vec![(BM25.into(), 4), (PAGE.into(), 12), (SPARQL.into(), 7)]
        );
        assert!(parse_ontology_refs("").is_empty());
    }

    fn cref(key: &str, id: u32, path: &str, is_file: bool, refs: &[(&str, u32)]) -> CodeRef {
        CodeRef {
            key: key.into(),
            id,
            path: path.into(),
            is_file,
            refs: refs.iter().map(|(i, l)| (i.to_string(), *l)).collect(),
        }
    }

    #[test]
    fn join_yields_exact_records_and_counts_unresolved() {
        let code = vec![
            // File node with its roll-up: the symbol's BM25#L4 and its own SPARQL#L1.
            cref("src_a", 10, "src/a.rs", true, &[(BM25, 4), (SPARQL, 1)]),
            // The symbol carrying BM25#L4: it wins over the file's copy.
            cref("src_a_rank", 11, "src/a.rs", false, &[(BM25, 4)]),
            // An IRI VisionClaw does not have.
            cref(
                "src_b",
                20,
                "src/b.rs",
                true,
                &[("urn:ngm:class:absent", 2)],
            ),
            // An IRI that sits only on an agent node.
            cref("src_c", 30, "src/c.rs", true, &[(PAGE, 9)]),
        ];
        let iri_ids = HashMap::from([
            (BM25.to_string(), vec![100]),
            (SPARQL.to_string(), vec![200, 201]),
            (PAGE.to_string(), vec![300]),
        ]);
        let class_of = |id: u32| match id {
            100 | 200 => 1,
            201 => 0,
            300 => 2,
            _ => 3,
        };
        let out = join(&code, 4, &iri_ids, 9, class_of);
        let got: Vec<(&str, u32, &str, u32, &str, &str, u32)> = out
            .records
            .iter()
            .map(|r| {
                (
                    r.source.key.as_str(),
                    r.source.id,
                    r.target.key.as_str(),
                    r.target.id,
                    r.target.volume.as_str(),
                    r.provenance.path.as_str(),
                    r.provenance.line,
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                ("src_a_rank", 11, BM25, 100, "ontology", "src/a.rs", 4),
                ("src_a", 10, SPARQL, 200, "ontology", "src/a.rs", 1),
                ("src_a", 10, SPARQL, 201, "knowledge", "src/a.rs", 1),
            ]
        );
        assert_eq!(out.unresolved, 2, "absent IRI and agent-only IRI");
        for r in &out.records {
            assert_eq!(r.kind, KIND_ANNOTATES);
            assert_eq!(r.source.volume, VOLUME_CODESCAPE);
            assert_eq!(r.source.conn, CODE_CONN);
            assert_eq!(r.target.conn, VC_CONN);
            assert_eq!(r.provenance.via, VIA_CODESCAPE_ANNOTATION);
            assert_eq!(r.provenance.generations, [4, 9]);
        }
    }

    #[test]
    fn symbol_beats_file_whatever_the_order() {
        let iri_ids = HashMap::from([(BM25.to_string(), vec![1])]);
        let file = cref("f", 5, "a.rs", true, &[(BM25, 3)]);
        let sym = cref("s", 9, "a.rs", false, &[(BM25, 3)]);
        for code in [
            vec![file.clone(), sym.clone()],
            vec![sym.clone(), file.clone()],
        ] {
            let out = join(&code, 0, &iri_ids, 0, |_| 0);
            assert_eq!(out.records.len(), 1);
            assert_eq!(out.records[0].source.key, "s");
        }
        // Same IRI and path on another line is a distinct annotation.
        let other = cref("f", 5, "a.rs", true, &[(BM25, 3), (BM25, 8)]);
        assert_eq!(join(&[other, sym], 0, &iri_ids, 0, |_| 0).records.len(), 2);
    }

    #[test]
    fn one_iri_on_two_nodes_gives_two_records() {
        let code = vec![cref("s", 1, "a.rs", false, &[(PAGE, 2)])];
        let iri_ids = HashMap::from([(PAGE.to_string(), vec![70, 71])]);
        let out = join(&code, 1, &iri_ids, 1, |_| 0);
        assert_eq!(
            out.records.iter().map(|r| r.target.id).collect::<Vec<_>>(),
            vec![70, 71]
        );
        assert_eq!(out.unresolved, 0);
    }

    #[test]
    fn class_codes_map_to_volumes() {
        assert_eq!(volume_for_class(0), Some(VOLUME_KNOWLEDGE));
        assert_eq!(volume_for_class(1), Some(VOLUME_ONTOLOGY));
        assert_eq!(volume_for_class(2), None);
        assert_eq!(volume_for_class(3), None);
    }

    #[test]
    fn frames_build_both_tables() {
        let cs = json!({ "type": "initialGraphLoad", "nodes": [
            { "id": 4, "metadata_id": "src_a", "node_type": "file",
              "metadata": { "source_file": "src/a.rs", "ontology_refs": format!("{BM25}#L4") } },
            { "id": 5, "metadata_id": "src_b", "node_type": "file",
              "metadata": { "source_file": "src/b.rs" } },
            { "id": 6, "metadata_id": "src_a_f", "node_type": "function",
              "metadata": { "source_file": "src/a.rs", "ontology_refs": "garbage" } },
        ]});
        let refs: Vec<CodeRef> = cs["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(code_ref_from_node)
            .collect();
        assert_eq!(refs, vec![cref("src_a", 4, "src/a.rs", true, &[(BM25, 4)])]);

        let vc = json!({ "nodes": [
            { "id": 8, "metadata": { "class_iri": BM25, "page_iri": PAGE } },
            { "id": 9, "metadata": { "page_iri": PAGE } },
            { "id": 9, "metadata": { "page_iri": PAGE } },
            { "metadata": { "class_iri": SPARQL } },
        ]});
        let t = iri_table(vc["nodes"].as_array().unwrap());
        assert_eq!(t[BM25], vec![8]);
        assert_eq!(t[PAGE], vec![8, 9]);
        assert!(!t.contains_key(SPARQL), "a node without an id is skipped");
    }

    #[test]
    fn records_serialise_in_the_d5_shape() {
        let code = vec![cref("s", 1, "a.rs", false, &[(BM25, 2)])];
        let out = join(
            &code,
            3,
            &HashMap::from([(BM25.to_string(), vec![7])]),
            4,
            |_| 1,
        );
        let v = serde_json::to_value(&out.records[0]).unwrap();
        assert_eq!(
            v,
            json!({
                "source": { "volume": "codescape", "conn": "codescape", "key": "s", "id": 1 },
                "target": { "volume": "ontology", "conn": "visionclaw", "key": BM25, "id": 7 },
                "kind": "annotates",
                "provenance": { "via": "codescape-annotation", "repo": "", "path": "a.rs",
                                "line": 2, "revision": null, "generations": [3, 4] }
            })
        );
    }

    /// Real-data join (report evidence). Run with
    /// `LINK_INDEX_CS=<codescape initialGraphLoad or ontology-refs JSON>`
    /// `LINK_INDEX_VC=<visionclaw initialGraphLoad or /api/graph/data JSON>`
    /// `[LINK_INDEX_CAP=3000] cargo test --lib link_index -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs captured frames in LINK_INDEX_CS / LINK_INDEX_VC"]
    fn real_data_join() {
        let load = |var: &str| -> Value {
            let p = std::env::var(var).unwrap_or_else(|_| panic!("{var} unset"));
            serde_json::from_slice(&std::fs::read(&p).expect("readable")).expect("json")
        };
        let cs = load("LINK_INDEX_CS");
        let code: Vec<CodeRef> = if let Some(nodes) = cs.get("nodes").and_then(Value::as_array) {
            nodes.iter().filter_map(code_ref_from_node).collect()
        } else {
            // The route's `[{key, id, path, line, iri}]`, one CodeRef per row.
            cs.as_array()
                .expect("route array")
                .iter()
                .map(|r| CodeRef {
                    key: r["key"].as_str().unwrap_or("").into(),
                    id: r["id"].as_u64().unwrap_or(0) as u32,
                    path: r["path"].as_str().unwrap_or("").into(),
                    is_file: false,
                    refs: vec![(
                        r["iri"].as_str().unwrap_or("").into(),
                        r["line"].as_u64().unwrap_or(0) as u32,
                    )],
                })
                .collect()
        };
        let vc = load("LINK_INDEX_VC");
        let mut nodes: Vec<&Value> = vc
            .get("nodes")
            .or_else(|| vc.get("data").and_then(|d| d.get("nodes")))
            .and_then(Value::as_array)
            .expect("nodes")
            .iter()
            .collect();
        if let Some(cap) = std::env::var("LINK_INDEX_CAP")
            .ok()
            .and_then(|c| c.parse().ok())
        {
            let q = |n: &Value| {
                let m = &n["metadata"]["quality_score"];
                m.as_f64()
                    .or_else(|| m.as_str().and_then(|s| s.parse().ok()))
                    .unwrap_or(0.0)
            };
            nodes.sort_by(|a, b| q(b).total_cmp(&q(a)));
            nodes.truncate(cap);
        }
        let classes: HashMap<u32, u8> = nodes
            .iter()
            .filter_map(|n| {
                let id = n["id"].as_u64()? as u32;
                // Wire class codes come from id flag bits; from JSON, the node type
                // stands in: `ontology_node` → ontology, anything else → knowledge.
                let ty = ["node_type", "nodeType", "type"]
                    .iter()
                    .find_map(|k| n[*k].as_str())
                    .unwrap_or("");
                let ont = ty.starts_with("ontology") || ty.starts_with("owl");
                Some((id, if ont { 1 } else { 0 }))
            })
            .collect();
        let t = iri_table(nodes.iter().copied());
        let out = join(&code, 1, &t, 1, |id| classes.get(&id).copied().unwrap_or(3));
        let distinct: std::collections::BTreeSet<(&str, &str, u32)> = out
            .records
            .iter()
            .map(|r| {
                (
                    r.target.key.as_str(),
                    r.provenance.path.as_str(),
                    r.provenance.line,
                )
            })
            .collect();
        println!(
            "code refs: {} nodes; vc nodes: {}; records: {}; distinct annotations resolved: {}; unresolved: {}",
            code.len(),
            nodes.len(),
            out.records.len(),
            distinct.len(),
            out.unresolved
        );
        for r in &out.records {
            println!(
                "  {}:{} {} -> {} #{} ({})",
                r.provenance.path,
                r.provenance.line,
                r.source.key,
                r.target.key,
                r.target.id,
                r.target.volume
            );
        }
    }
}
