//! `data/graph/*` — the NGG1 graph tiers, ported from
//! `pipeline/emit_graph_tiers.py` (ADR-NG-001 §2).
//!
//! Five artefacts replace the 39 MB `WebVOWL` monolith for the explorer's physics
//! worker:
//!
//! | file | tier | contents |
//! |---|---|---|
//! | `overview.json` | T0 | 6 domain roots + 34 categories, member counts, baked force-layout positions |
//! | `domain-<slug>.bin` ×6 | T1 | every class of one domain; full backbone; relations capped per node at top-8 by target degree |
//! | `full.bin` | — | the uncapped whole graph, CSR |
//! | `stats.json` | — | pipeline-derived truth (DDD INV-5) |
//! | `bridges.json` | — | the cross-category memberships the binary format cannot carry |
//!
//! Two things here are easy to get subtly wrong and are therefore pinned:
//!
//! * **Category inheritance walks the whole ancestry, breadth-first.** Reading
//!   direct parents only once mislabelled 4,033 of 7,457 classes as
//!   uncategorised. Nearest ancestor wins; parents are visited in declared
//!   order so the choice is deterministic, because the `.bin` tiers are
//!   byte-compared.
//! * **`bridges.json` exists because the node record holds one `u16` category.**
//!   1,397 classes carry more than one parent; the tiers keep the nearest
//!   category and the rest would be lost silently. Recording them keeps the
//!   lattice visible in the published data.

// Node ids, domain ids and category ids are u16/u32 by the frozen NGG1 record;
// the corpus is four orders of magnitude below any of those ceilings.
#![allow(clippy::cast_possible_truncation)]

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{json, Value};

use super::ngg1::{
    bake_positions, build_csr, pack, Node, CATEGORY_NONE, DOMAIN_NONE, EDGE_RELATION,
    EDGE_SUBCLASS, FLAG_BRIDGE, FLAG_CATEGORY_ROOT, FLAG_DOMAIN_ROOT, FLAG_HAS_PAGE,
    FLAG_INDIVIDUAL,
};
use super::webvowl::remap_iri;
use crate::model::{ClassRecord, Corpus, EntityType, RELATION_KEYS};

/// Pipeline identity carried into `overview.json` and `stats.json`.
pub const PIPELINE_VERSION: &str = "ng-1.0.0";
/// `did:nostr` provenance for the published corpus.
pub const ATTRIBUTED_TO: &str = "did:nostr:jjohare";
const ATTRIBUTED_LABEL: &str = "DreamLab AI";
const CORPUS_NATURE: &str = "synthetic-ai-generated-human-directed";
const CORPUS_DESCRIPTION: &str = "Mostly AI-generated synthetic content, produced under human direction, by design — this corpus exists to exercise and demonstrate the VisionFlow pipeline and VisionClaw engine on a medium-scale ontology (~8.1k classes, ~100k relations). Provenance (did:nostr, generatedAtTime, URNs) attests traceable generation under human direction, not human authorship.";
const NGM_NS_BASE: &str = "https://narrativegoldmine.com/ns";

/// On-screen node ceiling for a domain tier.
pub const MAX_NODES: usize = 1500;
/// Per-node `objectProperty` ship cap for domain tiers.
pub const RELATION_TOPK: usize = 8;

/// The six domains, in the order that fixes their ids.
pub const DOMAIN_SLUGS: &[&str] = &[
    "artificial-intelligence",
    "blockchain",
    "spatial-computing",
    "robotics",
    "distributed-collaboration",
    "infrastructure",
];

const DOMAIN_LABELS: &[(&str, &str)] = &[
    ("artificial-intelligence", "Artificial Intelligence"),
    ("blockchain", "Blockchain"),
    ("spatial-computing", "Spatial Computing"),
    ("robotics", "Robotics"),
    ("distributed-collaboration", "Distributed Collaboration"),
    ("infrastructure", "Infrastructure"),
];

/// Short-form and legacy domain vocabulary folded onto a canonical domain.
///
/// This is a *rendering* decision the graph tiers have always made; the
/// vocabulary deliberately leaves the `ai` / `artificial-intelligence` split
/// unresolved in the corpus itself, because normalising it is a content change
/// that belongs in the governance loop.
const DOMAIN_ALIASES: &[(&str, &str)] = &[
    ("ai", "artificial-intelligence"),
    ("machine-learning", "artificial-intelligence"),
    ("metaverse", "spatial-computing"),
    ("distributed-systems", "infrastructure"),
    ("supply-chain", "infrastructure"),
    ("data", "infrastructure"),
    ("governance", "infrastructure"),
    ("security", "infrastructure"),
    ("standards", "infrastructure"),
    ("finance", "blockchain"),
];

/// `(slug, label, domain id)` — position is the category id.
pub const CATEGORY_ORDER: &[(&str, &str, u16)] = &[
    ("ai-technique", "AI Technique", 0),
    ("ai-model-architecture", "AI Model Architecture", 0),
    ("ai-application", "AI Application", 0),
    ("ai-governance-and-ethics", "AI Governance and Ethics", 0),
    ("cat-ai-infrastructure", "AI Infrastructure", 0),
    ("ai-research-area", "AI Research Area", 0),
    ("bc-protocol-and-consensus", "Protocol and Consensus", 1),
    ("bc-cryptographic-primitive", "Cryptographic Primitive", 1),
    ("bc-token-and-asset", "Token and Asset", 1),
    ("bc-defi-and-economics", "DeFi and Economics", 1),
    ("bc-network-component", "Network Component", 1),
    (
        "bc-governance-and-regulation",
        "Governance and Regulation",
        1,
    ),
    ("sc-display-and-rendering", "Display and Rendering", 2),
    ("sc-interaction", "Interaction Technology", 2),
    ("sc-content-and-assets", "Content and Assets", 2),
    ("sc-platform-and-environment", "Platform and Environment", 2),
    (
        "sc-standards-and-interop",
        "Standards and Interoperability",
        2,
    ),
    ("sc-governance-and-safety", "Governance and Safety", 2),
    ("robo-perception", "Perception and Sensing", 3),
    ("robo-actuation-and-control", "Actuation and Control", 3),
    ("robo-robot-type", "Robot Type", 3),
    ("robo-navigation-and-planning", "Navigation and Planning", 3),
    ("robo-safety-and-standards", "Safety and Standards", 3),
    ("robo-human-robot-interaction", "Human-Robot Interaction", 3),
    ("dc-communication", "Communication Technology", 4),
    ("dc-workspace-tools", "Workspace Tools", 4),
    ("dc-telepresence", "Telepresence", 4),
    ("dc-protocol-and-infra", "Protocol and Infrastructure", 4),
    ("infra-computing-and-cloud", "Computing and Cloud", 5),
    ("infra-network-and-comms", "Network and Communication", 5),
    ("infra-security-and-identity", "Security and Identity", 5),
    ("infra-data-management", "Data Management", 5),
    ("infra-legal-and-regulatory", "Legal and Regulatory", 5),
    ("infra-software-engineering", "Software Engineering", 5),
];

/// Maximum ancestry depth the category resolver walks.
const MAX_DEPTH: usize = 12;

fn domain_index(slug: &str) -> Option<u16> {
    DOMAIN_SLUGS
        .iter()
        .position(|s| *s == slug)
        .map(|i| i as u16)
}

fn category_index(slug: &str) -> Option<u16> {
    CATEGORY_ORDER
        .iter()
        .position(|(s, _, _)| *s == slug)
        .map(|i| i as u16)
}

/// `_slug_of`: a colon-bearing IRI with no slash splits on the last colon,
/// anything else on the last slash.
#[must_use]
pub fn slug_of(iri: &str) -> String {
    if iri.contains(':') && !iri.contains('/') {
        iri.rsplit(':').next().unwrap_or(iri).to_owned()
    } else if iri.contains('/') {
        iri.rsplit('/').next().unwrap_or(iri).to_owned()
    } else {
        iri.to_owned()
    }
}

fn resolve_domain(domain: &str) -> u16 {
    let d = domain.trim().to_lowercase();
    let canonical = DOMAIN_ALIASES
        .iter()
        .find(|(from, _)| *from == d)
        .map_or(d.as_str(), |(_, to)| *to);
    domain_index(canonical).unwrap_or(DOMAIN_NONE)
}

/// A class bridging more than one taxonomy category or domain.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Bridge {
    /// The class's canonical IRI.
    pub iri: String,
    /// Its label.
    pub label: String,
    /// Category ids it reaches, in discovery order.
    pub categories: Vec<u16>,
    /// Domain ids it reaches, in discovery order.
    pub domains: Vec<u16>,
    /// The parent labels, as authored.
    pub parents: Vec<String>,
}

/// The resolved graph plus the statistics inputs.
#[derive(Debug, Default)]
pub struct GraphModel {
    /// Nodes in canonical-IRI order; local index equals `gid` in `full.bin`.
    pub nodes: Vec<Node>,
    /// Uncapped `(src_gid, tgt_gid, edge_type)` triples.
    pub edges: Vec<(u32, u32, u8)>,
    declared_backbone: usize,
    declared_relations: usize,
    resolvable_backbone: usize,
    resolvable_relations: usize,
    pages_public: usize,
    classes: usize,
    individuals: usize,
    uncategorised: usize,
    domainless: usize,
    multi_parent: usize,
    /// The memberships the binary format cannot carry.
    pub bridges: Vec<Bridge>,
}

/// Resolve a class's category by walking `is-a` / `instance-of` ancestry,
/// breadth-first so the nearest category ancestor wins.
struct CategoryResolver {
    parents_of: HashMap<String, Vec<String>>,
    memo: HashMap<String, u16>,
}

impl CategoryResolver {
    fn new(public: &[&ClassRecord]) -> Self {
        let parents_of = public
            .iter()
            .map(|r| {
                let parents = r
                    .sub_class_of
                    .iter()
                    .chain(&r.instance_of)
                    .map(|p| slug_of(&p.iri))
                    .collect();
                (slug_of(&r.iri), parents)
            })
            .collect();
        Self {
            parents_of,
            memo: HashMap::new(),
        }
    }

    fn resolve(&mut self, slug: &str) -> u16 {
        if let Some(hit) = self.memo.get(slug) {
            return *hit;
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        seen.insert(slug.to_owned());
        let mut frontier: Vec<String> = self.parents_of.get(slug).cloned().unwrap_or_default();
        let mut found = CATEGORY_NONE;
        let mut depth = 0;
        while !frontier.is_empty() && depth < MAX_DEPTH {
            let mut next: Vec<String> = Vec::new();
            for parent in &frontier {
                if let Some(cid) = category_index(parent) {
                    found = cid;
                    break;
                }
                if !seen.insert(parent.clone()) {
                    continue;
                }
                if let Some(grand) = self.parents_of.get(parent) {
                    next.extend(grand.iter().cloned());
                }
            }
            if found != CATEGORY_NONE {
                break;
            }
            frontier = next;
            depth += 1;
        }
        self.memo.insert(slug.to_owned(), found);
        found
    }
}

/// Resolve the corpus into nodes and typed edges.
///
/// Mirrors `WebVOWL`'s declared-target filter: an edge ships only when its target
/// is itself a declared public node.
#[must_use]
#[allow(clippy::too_many_lines)] // One faithful port of one Python function.
pub fn build_model(corpus: &Corpus) -> GraphModel {
    let public: Vec<&ClassRecord> = corpus
        .records
        .iter()
        .filter(|r| r.public && r.has_ontology && !r.iri.is_empty())
        .collect();

    // The reading-unit count: DISTINCT public source pages, not OWL entities.
    let pages_public = corpus
        .records
        .iter()
        .filter(|r| r.public)
        .map(|r| {
            if r.page_iri.is_empty() {
                r.page_id.clone()
            } else {
                r.page_iri.clone()
            }
        })
        .collect::<BTreeSet<_>>()
        .len();

    let mut resolver = CategoryResolver::new(&public);
    let domain_by_slug: HashMap<String, u16> = public
        .iter()
        .map(|r| (slug_of(&r.iri), resolve_domain(&r.domain)))
        .collect();

    // 1. Nodes, keyed by canonical IRI, first definition winning.
    let mut seen_canon: BTreeSet<String> = BTreeSet::new();
    let mut staged: Vec<(Node, &ClassRecord)> = Vec::new();
    for record in &public {
        let canon = remap_iri(&record.iri);
        if !seen_canon.insert(canon.clone()) {
            continue;
        }
        let own_slug = slug_of(&record.iri);
        let is_domain_root = DOMAIN_SLUGS.contains(&own_slug.as_str());
        let is_category_root = category_index(&own_slug).is_some();
        let category_id = if let Some(cid) = category_index(&own_slug) {
            cid
        } else if is_domain_root {
            CATEGORY_NONE
        } else {
            resolver.resolve(&own_slug)
        };
        let individual = record.entity_type == EntityType::Individual;
        let mut flags = FLAG_HAS_PAGE;
        if is_domain_root {
            flags |= FLAG_DOMAIN_ROOT;
        }
        if is_category_root {
            flags |= FLAG_CATEGORY_ROOT;
        }
        if individual {
            flags |= FLAG_INDIVIDUAL;
        }
        staged.push((
            Node {
                gid: 0,
                label: if record.label.is_empty() {
                    own_slug.clone()
                } else {
                    record.label.clone()
                },
                iri: canon,
                individual,
                domain_id: resolve_domain(&record.domain),
                category_id,
                flags,
                degree: 0,
                x: 0.0,
                y: 0.0,
                is_domain_root,
                is_category_root,
            },
            record,
        ));
    }

    // 2. Stable gid = index in canonical-IRI sort order.
    staged.sort_by(|a, b| a.0.iri.cmp(&b.0.iri));
    for (i, (node, _)) in staged.iter_mut().enumerate() {
        node.gid = i as u32;
    }
    let gid_by_canon: HashMap<String, u32> =
        staged.iter().map(|(n, _)| (n.iri.clone(), n.gid)).collect();

    // 3. Edges, declared then resolvable.
    let mut edge_set: BTreeSet<(u32, u32, u8)> = BTreeSet::new();
    let mut edges: Vec<(u32, u32, u8)> = Vec::new();
    let (mut declared_backbone, mut declared_relations) = (0usize, 0usize);
    let (mut resolvable_backbone, mut resolvable_relations) = (0usize, 0usize);
    let mut bridge_flags: BTreeSet<u32> = BTreeSet::new();

    for (node, record) in &staged {
        let backbone: Vec<_> =
            if record.entity_type == EntityType::Individual && !record.instance_of.is_empty() {
                record.instance_of.iter().collect()
            } else {
                record.sub_class_of.iter().collect()
            };
        for r in backbone {
            declared_backbone += 1;
            if let Some(&tgt) = gid_by_canon.get(&remap_iri(&r.iri)) {
                if tgt != node.gid && edge_set.insert((node.gid, tgt, EDGE_SUBCLASS)) {
                    edges.push((node.gid, tgt, EDGE_SUBCLASS));
                    resolvable_backbone += 1;
                }
            }
        }
        let mut has_bridge = false;
        for (fm_key, json_key) in RELATION_KEYS {
            for r in record.relation(json_key) {
                declared_relations += 1;
                if let Some(&tgt) = gid_by_canon.get(&remap_iri(&r.iri)) {
                    if tgt == node.gid {
                        continue;
                    }
                    let fresh = edge_set.insert((node.gid, tgt, EDGE_RELATION));
                    if fresh {
                        edges.push((node.gid, tgt, EDGE_RELATION));
                        resolvable_relations += 1;
                    }
                    if *fm_key == "bridges-to" {
                        has_bridge = true;
                    }
                }
            }
        }
        if has_bridge {
            bridge_flags.insert(node.gid);
        }
    }

    let mut nodes: Vec<Node> = staged.iter().map(|(n, _)| n.clone()).collect();
    for n in &mut nodes {
        if bridge_flags.contains(&n.gid) {
            n.flags |= FLAG_BRIDGE;
        }
    }

    // 4. Full-graph incident degree, the ranking key.
    let mut degree = vec![0u32; nodes.len()];
    for (src, tgt, _) in &edges {
        degree[*src as usize] += 1;
        degree[*tgt as usize] += 1;
    }
    for n in &mut nodes {
        n.degree = degree[n.gid as usize];
    }

    let classes = nodes.iter().filter(|n| !n.individual).count();
    let individuals = nodes.iter().filter(|n| n.individual).count();
    let uncategorised = nodes
        .iter()
        .filter(|n| n.category_id == CATEGORY_NONE && !n.is_domain_root && !n.is_category_root)
        .count();
    let domainless = nodes.iter().filter(|n| n.domain_id == DOMAIN_NONE).count();

    // 5. Bridging.
    let mut bridges = Vec::new();
    for record in &public {
        if record.sub_class_of.len() < 2 {
            continue;
        }
        let (mut cats, mut doms): (Vec<u16>, Vec<u16>) = (Vec::new(), Vec::new());
        for parent in &record.sub_class_of {
            let ps = slug_of(&parent.iri);
            let cid = category_index(&ps).unwrap_or_else(|| resolver.resolve(&ps));
            if cid != CATEGORY_NONE && !cats.contains(&cid) {
                cats.push(cid);
            }
            if let Some(&did) = domain_by_slug.get(&ps) {
                if did != DOMAIN_NONE && !doms.contains(&did) {
                    doms.push(did);
                }
            }
        }
        if cats.len() > 1 || doms.len() > 1 {
            bridges.push(Bridge {
                iri: remap_iri(&record.iri),
                label: record.label.clone(),
                categories: cats,
                domains: doms,
                parents: record
                    .sub_class_of
                    .iter()
                    .map(|r| r.label.clone())
                    .collect(),
            });
        }
    }

    GraphModel {
        nodes,
        edges,
        declared_backbone,
        declared_relations,
        resolvable_backbone,
        resolvable_relations,
        pages_public,
        classes,
        individuals,
        uncategorised,
        domainless,
        multi_parent: public.iter().filter(|r| r.sub_class_of.len() > 1).count(),
        bridges,
    }
}

/// Per-scope statistics for `stats.json`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScopeStats {
    nodes: usize,
    backbone: usize,
    relations: usize,
    shipped: usize,
    #[serde(rename = "relationsCapped")]
    relations_capped: usize,
    bytes: usize,
    #[serde(rename = "nodesTruncated", skip_serializing_if = "Option::is_none")]
    nodes_truncated: Option<usize>,
}

/// Order one source node's out-edges: backbone whole and target-sorted,
/// relations by descending target degree, capped at `topk`.
fn order_source_edges(
    src: u32,
    out_edges: &[(u32, u8)],
    degree_of: &dyn Fn(u32) -> u32,
    topk: Option<usize>,
) -> (Vec<(u32, u32, u8)>, usize) {
    let mut backbone: Vec<u32> = out_edges
        .iter()
        .filter(|(_, t)| *t == EDGE_SUBCLASS)
        .map(|(tgt, _)| *tgt)
        .collect();
    backbone.sort_unstable();
    let mut relations: Vec<u32> = out_edges
        .iter()
        .filter(|(_, t)| *t == EDGE_RELATION)
        .map(|(tgt, _)| *tgt)
        .collect();
    relations.sort_by_key(|tgt| (std::cmp::Reverse(degree_of(*tgt)), *tgt));
    let dropped = topk.map_or(0, |k| relations.len().saturating_sub(k));
    if let Some(k) = topk {
        relations.truncate(k);
    }
    let mut out: Vec<(u32, u32, u8)> = backbone
        .into_iter()
        .map(|t| (src, t, EDGE_SUBCLASS))
        .collect();
    out.extend(relations.into_iter().map(|t| (src, t, EDGE_RELATION)));
    (out, dropped)
}

/// Build one tier's NGG1 bytes and scope statistics from a node subset.
#[must_use]
pub fn build_tier(
    nodes: &[Node],
    full_edges: &[(u32, u32, u8)],
    topk: Option<usize>,
) -> (Vec<u8>, ScopeStats) {
    let local_of: HashMap<u32, u32> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.gid, i as u32))
        .collect();
    let degree_by_gid: HashMap<u32, u32> = nodes.iter().map(|n| (n.gid, n.degree)).collect();

    let mut out_by_src: BTreeMap<u32, Vec<(u32, u8)>> = BTreeMap::new();
    for (src_gid, tgt_gid, et) in full_edges {
        let (Some(&si), Some(&ti)) = (local_of.get(src_gid), local_of.get(tgt_gid)) else {
            continue;
        };
        out_by_src.entry(si).or_default().push((ti, *et));
    }

    let degree_of = |local: u32| -> u32 {
        nodes
            .get(local as usize)
            .and_then(|n| degree_by_gid.get(&n.gid).copied())
            .unwrap_or(0)
    };

    let mut ordered: Vec<(u32, u32, u8)> = Vec::new();
    let mut relations_capped = 0usize;
    for si in 0..nodes.len() as u32 {
        let Some(oe) = out_by_src.get(&si) else {
            continue;
        };
        let (triples, dropped) = order_source_edges(si, oe, &degree_of, topk);
        ordered.extend(triples);
        relations_capped += dropped;
    }

    let csr = build_csr(nodes.len(), &ordered);
    let data = pack(nodes, &csr);
    let backbone = bytes_equal_to(&csr.edge_type, EDGE_SUBCLASS);
    let relations = bytes_equal_to(&csr.edge_type, EDGE_RELATION);
    let stats = ScopeStats {
        nodes: nodes.len(),
        backbone,
        relations,
        shipped: csr.edge_type.len(),
        relations_capped,
        bytes: data.len(),
        nodes_truncated: None,
    };
    (data, stats)
}

/// One emitted graph-tier file.
#[derive(Debug, Clone)]
pub struct TierFile {
    /// Path relative to `data/graph/`.
    pub name: String,
    /// File bytes.
    pub bytes: Vec<u8>,
}

/// The result of emitting every tier.
#[derive(Debug)]
pub struct Tiers {
    /// Every file, in emission order.
    pub files: Vec<TierFile>,
    /// Node count of the full graph.
    pub nodes: usize,
    /// Resolvable edge count of the full graph.
    pub edges: usize,
}

/// Count the entries of a byte slice equal to `needle`.
///
/// `clippy::naive_bytecount` would have this pull in the `bytecount` crate.
/// The slice is one edge-type byte per shipped edge and is counted twice per
/// tier; a SIMD popcount dependency is not worth it.
#[allow(clippy::naive_bytecount)]
fn bytes_equal_to(bytes: &[u8], needle: u8) -> usize {
    bytes.iter().filter(|b| **b == needle).count()
}

/// The display label for a domain slug.
fn domain_label(slug: &str) -> &'static str {
    DOMAIN_LABELS
        .iter()
        .find(|(s, _)| *s == slug)
        .map_or("", |(_, l)| *l)
}

/// Build `overview.json`.
#[must_use]
#[allow(clippy::too_many_lines)] // One faithful port of one Python function.
pub fn build_overview(model: &GraphModel, generated_at: &str) -> Value {
    let ndom = DOMAIN_SLUGS.len();
    let ncat = CATEGORY_ORDER.len();

    let mut domain_members = vec![0usize; ndom];
    let mut category_members = vec![0usize; ncat];
    for n in &model.nodes {
        if (n.domain_id as usize) < ndom {
            domain_members[n.domain_id as usize] += 1;
        }
        if n.category_id != CATEGORY_NONE && !n.is_category_root {
            category_members[n.category_id as usize] += 1;
        }
    }

    let mut layout_edges: Vec<(usize, usize)> = CATEGORY_ORDER
        .iter()
        .enumerate()
        .map(|(ci, (_, _, dom))| (ndom + ci, *dom as usize))
        .collect();

    // Bridge edges, aggregated into weighted category pairs so 313 bridging
    // classes become a readable number of lines rather than 313 overlapping.
    let mut bridge_weight: BTreeMap<(u16, u16), usize> = BTreeMap::new();
    for b in &model.bridges {
        let mut cats = b.categories.clone();
        cats.sort_unstable();
        for i in 0..cats.len() {
            for j in (i + 1)..cats.len() {
                *bridge_weight.entry((cats[i], cats[j])).or_insert(0) += 1;
            }
        }
    }
    let bridge_pairs: Vec<((u16, u16), usize)> =
        bridge_weight.iter().map(|(k, v)| (*k, *v)).collect();
    layout_edges.extend(
        bridge_pairs
            .iter()
            .map(|((a, b), _)| (ndom + *a as usize, ndom + *b as usize)),
    );

    let pos = super::ngg1::force_layout(ndom + ncat, &layout_edges, 200, 42);

    let mut domain_root: BTreeMap<u16, &Node> = BTreeMap::new();
    let mut category_root: BTreeMap<u16, &Node> = BTreeMap::new();
    for n in &model.nodes {
        if n.is_domain_root && (n.domain_id as usize) < ndom {
            domain_root.entry(n.domain_id).or_insert(n);
        }
        if n.is_category_root && n.category_id != CATEGORY_NONE {
            category_root.entry(n.category_id).or_insert(n);
        }
    }

    let label_of = domain_label;

    let domains: Vec<Value> = DOMAIN_SLUGS
        .iter()
        .enumerate()
        .map(|(di, slug)| {
            let cat_count = CATEGORY_ORDER
                .iter()
                .filter(|(_, _, d)| *d as usize == di)
                .count();
            json!({
                "id": di, "slug": slug, "label": label_of(slug),
                "x": pos[di].0, "y": pos[di].1,
                "memberCount": domain_members[di], "categoryCount": cat_count,
            })
        })
        .collect();
    let categories: Vec<Value> = CATEGORY_ORDER
        .iter()
        .enumerate()
        .map(|(ci, (slug, label, dom))| {
            json!({
                "id": ci, "slug": slug, "label": label, "domain": dom,
                "x": pos[ndom + ci].0, "y": pos[ndom + ci].1,
                "memberCount": category_members[ci],
            })
        })
        .collect();

    // Node order is frozen: 6 domains at 0..5, then 34 categories at 6..39, so
    // edge indices and the baked positions align.
    let mut nodes: Vec<Value> = Vec::with_capacity(ndom + ncat);
    for (di, slug) in DOMAIN_SLUGS.iter().enumerate() {
        let root = domain_root.get(&(di as u16));
        nodes.push(json!({
            "id": di,
            "label": root.map_or_else(|| label_of(slug).to_owned(), |r| r.label.clone()),
            "iri": root.map_or_else(
                || format!("{NGM_NS_BASE}/domain/{slug}"), |r| r.iri.clone()),
            "domain": di,
            "degree": domain_members[di],
            "flags": FLAG_DOMAIN_ROOT | if root.is_some() { FLAG_HAS_PAGE } else { 0 },
            "x": pos[di].0, "y": pos[di].1,
        }));
    }
    for (ci, (slug, label, dom)) in CATEGORY_ORDER.iter().enumerate() {
        let root = category_root.get(&(ci as u16));
        nodes.push(json!({
            "id": ndom + ci,
            "label": root.map_or_else(|| (*label).to_owned(), |r| r.label.clone()),
            "iri": root.map_or_else(
                || format!("{NGM_NS_BASE}/category/{slug}"), |r| r.iri.clone()),
            "domain": dom,
            "category": ci,
            "degree": category_members[ci],
            "flags": FLAG_CATEGORY_ROOT | if root.is_some() { FLAG_HAS_PAGE } else { 0 },
            "x": pos[ndom + ci].0, "y": pos[ndom + ci].1,
        }));
    }

    // Backbone edges first so an index-sensitive reader sees them unmoved.
    let mut edges: Vec<Value> = CATEGORY_ORDER
        .iter()
        .enumerate()
        .map(|(ci, (_, _, dom))| {
            json!({
                "source": ndom + ci, "target": dom, "type": EDGE_SUBCLASS,
            })
        })
        .collect();
    edges.extend(bridge_pairs.iter().map(|((a, b), w)| {
        json!({
            "source": ndom + *a as usize,
            "target": ndom + *b as usize,
            "type": EDGE_RELATION,
            "weight": w,
        })
    }));

    json!({
        "version": super::ngg1::VERSION,
        "pipelineVersion": PIPELINE_VERSION,
        "generatedAt": generated_at,
        "attributedTo": ATTRIBUTED_TO,
        "provenance": {
            "did": ATTRIBUTED_TO,
            "label": ATTRIBUTED_LABEL,
            "corpusNature": CORPUS_NATURE,
        },
        "taxonomy": CATEGORY_ORDER.iter().map(|(_, l, _)| *l).collect::<Vec<_>>(),
        "nodes": nodes,
        "edges": edges,
        "domains": domains,
        "categories": categories,
    })
}

/// Emit every graph-tier artefact.
///
/// `generated_at` is supplied so the emitter stays pure and the golden test can
/// pin the date.
#[must_use]
#[allow(clippy::too_many_lines)] // One faithful port of one Python function.
pub fn emit(corpus: &Corpus, generated_at: &str) -> Tiers {
    let mut model = build_model(corpus);
    let mut files = Vec::new();

    let overview = build_overview(&model, generated_at);
    files.push(TierFile {
        name: "overview.json".to_owned(),
        bytes: vault_core::json::to_default_ascii(&overview)
            .unwrap_or_default()
            .into_bytes(),
    });

    // Insertion-ordered: `full` first, then the six domains in DOMAIN_SLUGS
    // order. A sorted map would reorder them and stats.json is byte-compared.
    let mut scope_stats: indexmap::IndexMap<String, ScopeStats> = indexmap::IndexMap::new();

    bake_positions(&mut model.nodes);
    let (full_bytes, full_scope) = build_tier(&model.nodes, &model.edges, None);
    files.push(TierFile {
        name: "full.bin".to_owned(),
        bytes: full_bytes,
    });
    scope_stats.insert("full".to_owned(), full_scope);

    for (di, slug) in DOMAIN_SLUGS.iter().enumerate() {
        let want = di as u16;
        let mut dnodes: Vec<Node> = model
            .nodes
            .iter()
            .filter(|n| n.domain_id == want)
            .cloned()
            .collect();
        dnodes.sort_by_key(|n| n.gid);
        let mut truncated = 0usize;
        if dnodes.len() > MAX_NODES {
            // Structural anchors are kept unconditionally; the rest fill the
            // remaining slots by descending degree.
            let (roots, rest): (Vec<Node>, Vec<Node>) = dnodes
                .iter()
                .cloned()
                .partition(|n| n.is_domain_root || n.is_category_root);
            let mut rest = rest;
            rest.sort_by_key(|n| (std::cmp::Reverse(n.degree), n.gid));
            rest.truncate(MAX_NODES.saturating_sub(roots.len()));
            truncated = dnodes.len() - roots.len() - rest.len();
            dnodes = roots.into_iter().chain(rest).collect();
            dnodes.sort_by_key(|n| n.gid);
        }
        bake_positions(&mut dnodes);
        let (data, mut dscope) = build_tier(&dnodes, &model.edges, Some(RELATION_TOPK));
        dscope.nodes_truncated = Some(truncated);
        files.push(TierFile {
            name: format!("domain-{slug}.bin"),
            bytes: data,
        });
        scope_stats.insert(format!("domain-{slug}"), dscope);
    }

    let stats = json!({
        "pipelineVersion": PIPELINE_VERSION,
        "attributedTo": ATTRIBUTED_TO,
        "provenance": {
            "did": ATTRIBUTED_TO,
            "label": ATTRIBUTED_LABEL,
            "corpusNature": CORPUS_NATURE,
        },
        "corpus": { "nature": "synthetic", "description": CORPUS_DESCRIPTION },
        "datasetDate": generated_at,
        "pages": model.pages_public,
        "classes": model.classes,
        "individuals": model.individuals,
        "nodes": model.nodes.len(),
        "domains": DOMAIN_SLUGS.len(),
        "categories": CATEGORY_ORDER.len(),
        "uncategorised": model.uncategorised,
        "domainless": model.domainless,
        "bridging": {
            "multiParent": model.multi_parent,
            "crossCategory": model.bridges.iter().filter(|b| b.categories.len() > 1).count(),
            "crossDomain": model.bridges.iter().filter(|b| b.domains.len() > 1).count(),
        },
        "edges": {
            "declared": model.declared_backbone + model.declared_relations,
            "declaredBackbone": model.declared_backbone,
            "declaredRelations": model.declared_relations,
            "resolvable": model.resolvable_backbone + model.resolvable_relations,
            "backbone": model.resolvable_backbone,
            "relations": model.resolvable_relations,
        },
        "scopes": scope_stats,
    });
    files.push(TierFile {
        name: "stats.json".to_owned(),
        bytes: vault_core::json::to_indented(&stats)
            .unwrap_or_default()
            .into_bytes(),
    });

    let bridges = json!({
        "pipelineVersion": PIPELINE_VERSION,
        "note": "Classes bridging more than one taxonomy category or domain. Overlap is a design property of this corpus. The NGG1 node record carries a single u16 category (FORMAT-NGG1 3), so tiers keep the nearest one; the full membership is here. Indices match overview.json.",
        "count": model.bridges.len(),
        "bridges": model.bridges,
    });
    files.push(TierFile {
        name: "bridges.json".to_owned(),
        bytes: vault_core::json::to_indented(&bridges)
            .unwrap_or_default()
            .into_bytes(),
    });

    Tiers {
        nodes: model.nodes.len(),
        edges: model.edges.len(),
        files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Ref;
    use indexmap::IndexMap;

    fn record(id: &str, domain: &str) -> ClassRecord {
        ClassRecord {
            page_id: id.to_owned(),
            slug: vault_core::slug::slugify(id),
            title: id.to_owned(),
            public: true,
            page_iri: format!("urn:visionflow:page:{}", vault_core::slug::slugify(id)),
            iri: format!("urn:ngm:class:{}", vault_core::slug::slugify(id)),
            label: id.to_owned(),
            entity_type: EntityType::Class,
            domain: domain.to_owned(),
            definition: String::new(),
            maturity: "established".into(),
            quality: 0.5,
            legacy_term_id: String::new(),
            sub_class_of: Vec::new(),
            instance_of: Vec::new(),
            relations: IndexMap::new(),
            links: Vec::new(),
            body: String::new(),
            has_ontology: true,
        }
    }

    fn parent(slug: &str) -> Ref {
        Ref {
            iri: format!("urn:ngm:class:{slug}"),
            label: slug.to_owned(),
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
    fn the_taxonomy_has_six_domains_and_thirty_four_categories() {
        assert_eq!(DOMAIN_SLUGS.len(), 6);
        assert_eq!(CATEGORY_ORDER.len(), 34);
        // Category ids are contiguous per domain, in DOMAIN_SLUGS order.
        let mut last = 0u16;
        for (_, _, dom) in CATEGORY_ORDER {
            assert!(*dom >= last, "categories must be grouped by domain");
            last = *dom;
        }
    }

    #[test]
    fn domain_aliases_fold_onto_the_canonical_six() {
        assert_eq!(resolve_domain("ai"), 0);
        assert_eq!(resolve_domain("machine-learning"), 0);
        assert_eq!(resolve_domain("metaverse"), 2);
        assert_eq!(resolve_domain("finance"), 1);
        assert_eq!(resolve_domain("blockchain"), 1);
        assert_eq!(resolve_domain("nonsense"), DOMAIN_NONE);
    }

    #[test]
    fn slug_of_splits_on_colon_or_slash() {
        assert_eq!(slug_of("urn:ngm:class:knowledge-graph"), "knowledge-graph");
        assert_eq!(slug_of("https://narrativegoldmine.com/class/x"), "x");
        assert_eq!(slug_of("bare"), "bare");
    }

    #[test]
    fn a_category_is_inherited_through_the_whole_ancestry() {
        // Leaf -> Middle -> ai-technique (a category root).
        let mut leaf = record("Leaf", "ai");
        leaf.sub_class_of = vec![parent("middle")];
        let mut middle = record("Middle", "ai");
        middle.sub_class_of = vec![parent("ai-technique")];
        let root = record("ai-technique", "ai");
        let model = build_model(&corpus_of(vec![leaf, middle, root]));
        let leaf_node = model.nodes.iter().find(|n| n.label == "Leaf").unwrap();
        assert_eq!(
            leaf_node.category_id,
            category_index("ai-technique").unwrap(),
            "a two-hop ancestor must still supply the category"
        );
    }

    #[test]
    fn the_nearest_category_ancestor_wins() {
        let mut leaf = record("Leaf", "ai");
        // Direct parent is a category; the grandparent is a different one.
        leaf.sub_class_of = vec![parent("ai-application")];
        let mut mid = record("ai-application", "ai");
        mid.sub_class_of = vec![parent("ai-technique")];
        let model = build_model(&corpus_of(vec![leaf, mid, record("ai-technique", "ai")]));
        let leaf_node = model.nodes.iter().find(|n| n.label == "Leaf").unwrap();
        assert_eq!(
            leaf_node.category_id,
            category_index("ai-application").unwrap()
        );
    }

    #[test]
    fn a_cycle_in_the_ancestry_terminates() {
        let mut a = record("A", "ai");
        a.sub_class_of = vec![parent("b")];
        let mut b = record("B", "ai");
        b.sub_class_of = vec![parent("a")];
        let model = build_model(&corpus_of(vec![a, b]));
        assert_eq!(model.nodes.len(), 2);
        assert!(model.nodes.iter().all(|n| n.category_id == CATEGORY_NONE));
    }

    #[test]
    fn gids_follow_canonical_iri_order() {
        let model = build_model(&corpus_of(vec![
            record("Zebra", "ai"),
            record("Apple", "ai"),
        ]));
        assert_eq!(model.nodes[0].label, "Apple");
        assert_eq!(model.nodes[0].gid, 0);
        assert_eq!(model.nodes[1].label, "Zebra");
    }

    #[test]
    fn only_edges_to_declared_nodes_are_resolvable() {
        let mut a = record("A", "ai");
        a.sub_class_of = vec![parent("b"), parent("nowhere")];
        let model = build_model(&corpus_of(vec![a, record("B", "ai")]));
        assert_eq!(model.declared_backbone, 2);
        assert_eq!(model.resolvable_backbone, 1);
        assert_eq!(model.edges.len(), 1);
    }

    #[test]
    fn a_bridges_to_edge_sets_the_bridge_flag() {
        let mut a = record("A", "ai");
        a.relations.insert("bridgesTo", vec![parent("b")]);
        let model = build_model(&corpus_of(vec![a, record("B", "ai")]));
        let node = model.nodes.iter().find(|n| n.label == "A").unwrap();
        assert_eq!(node.flags & FLAG_BRIDGE, FLAG_BRIDGE);
    }

    #[test]
    fn roots_carry_their_structural_flags() {
        let model = build_model(&corpus_of(vec![
            record("artificial-intelligence", "ai"),
            record("ai-technique", "ai"),
        ]));
        let domain = model.nodes.iter().find(|n| n.is_domain_root).unwrap();
        assert_eq!(domain.flags & FLAG_DOMAIN_ROOT, FLAG_DOMAIN_ROOT);
        assert_eq!(domain.category_id, CATEGORY_NONE);
        let cat = model.nodes.iter().find(|n| n.is_category_root).unwrap();
        assert_eq!(cat.flags & FLAG_CATEGORY_ROOT, FLAG_CATEGORY_ROOT);
    }

    #[test]
    fn an_individual_is_flagged_and_uses_instance_of_as_its_backbone() {
        let mut ind = record("Instance", "ai");
        ind.entity_type = EntityType::Individual;
        ind.instance_of = vec![parent("a")];
        ind.sub_class_of = vec![parent("nowhere")];
        let model = build_model(&corpus_of(vec![ind, record("A", "ai")]));
        let node = model.nodes.iter().find(|n| n.label == "Instance").unwrap();
        assert_eq!(node.flags & FLAG_INDIVIDUAL, FLAG_INDIVIDUAL);
        assert_eq!(model.individuals, 1);
        assert_eq!(model.resolvable_backbone, 1, "instance-of, not is-a");
    }

    #[test]
    fn relations_are_capped_by_descending_target_degree() {
        // A has 10 relation targets; the tier keeps the 8 highest-degree.
        let mut a = record("A", "ai");
        let targets: Vec<Ref> = (0..10).map(|i| parent(&format!("t{i:02}"))).collect();
        a.relations.insert("requires", targets);
        let mut records = vec![a];
        for i in 0..10 {
            let mut t = record(&format!("T{i:02}"), "ai");
            // Give low-numbered targets more degree via extra inbound edges.
            if i < 8 {
                t.relations.insert("uses", vec![parent("a")]);
            }
            records.push(t);
        }
        let model = build_model(&corpus_of(records));
        let (_, scope) = build_tier(&model.nodes, &model.edges, Some(RELATION_TOPK));
        assert!(scope.relations_capped > 0, "the cap must bite");
        let (_, uncapped) = build_tier(&model.nodes, &model.edges, None);
        assert_eq!(uncapped.relations_capped, 0);
        assert!(uncapped.relations > scope.relations);
    }

    #[test]
    fn backbone_edges_precede_relations_within_a_source() {
        let degree = |_: u32| 1u32;
        let (ordered, dropped) = order_source_edges(
            0,
            &[(5, EDGE_RELATION), (3, EDGE_SUBCLASS), (1, EDGE_SUBCLASS)],
            &degree,
            None,
        );
        assert_eq!(dropped, 0);
        assert_eq!(
            ordered,
            vec![
                (0, 1, EDGE_SUBCLASS),
                (0, 3, EDGE_SUBCLASS),
                (0, 5, EDGE_RELATION)
            ]
        );
    }

    #[test]
    fn bridges_record_multi_category_membership() {
        let mut a = record("A", "ai");
        a.sub_class_of = vec![parent("ai-technique"), parent("bc-token-and-asset")];
        let model = build_model(&corpus_of(vec![
            a,
            record("ai-technique", "ai"),
            record("bc-token-and-asset", "blockchain"),
        ]));
        assert_eq!(model.bridges.len(), 1);
        assert_eq!(model.bridges[0].categories.len(), 2);
        assert_eq!(model.bridges[0].domains.len(), 2);
        assert_eq!(model.multi_parent, 1);
    }

    #[test]
    fn a_single_parent_class_is_not_a_bridge() {
        let mut a = record("A", "ai");
        a.sub_class_of = vec![parent("ai-technique")];
        let model = build_model(&corpus_of(vec![a, record("ai-technique", "ai")]));
        assert!(model.bridges.is_empty());
    }

    #[test]
    fn the_overview_has_forty_nodes_in_the_frozen_order() {
        let model = build_model(&corpus_of(vec![record("A", "ai")]));
        let ov = build_overview(&model, "2026-09-22");
        let nodes = ov["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 40);
        assert_eq!(nodes[0]["id"], 0);
        assert_eq!(nodes[0]["label"], "Artificial Intelligence");
        assert_eq!(nodes[6]["id"], 6);
        assert_eq!(nodes[6]["label"], "AI Technique");
        assert_eq!(ov["taxonomy"].as_array().unwrap().len(), 34);
        // 34 backbone edges, category -> domain.
        assert_eq!(ov["edges"].as_array().unwrap().len(), 34);
        assert_eq!(ov["attributedTo"], ATTRIBUTED_TO);
    }

    #[test]
    fn an_authored_root_page_supplies_the_overview_label_and_iri() {
        let mut root = record("artificial-intelligence", "ai");
        root.label = "Artificial Intelligence (authored)".into();
        let model = build_model(&corpus_of(vec![root]));
        let ov = build_overview(&model, "2026-09-22");
        assert_eq!(
            ov["nodes"][0]["label"],
            "Artificial Intelligence (authored)"
        );
        assert_eq!(
            ov["nodes"][0]["iri"],
            "https://narrativegoldmine.com/class/artificial-intelligence"
        );
        assert_eq!(ov["nodes"][0]["flags"], FLAG_DOMAIN_ROOT | FLAG_HAS_PAGE);
    }

    #[test]
    fn a_domain_with_no_authored_root_gets_a_synthetic_iri() {
        let model = build_model(&corpus_of(vec![record("A", "ai")]));
        let ov = build_overview(&model, "2026-09-22");
        assert_eq!(
            ov["nodes"][0]["iri"],
            "https://narrativegoldmine.com/ns/domain/artificial-intelligence"
        );
        assert_eq!(ov["nodes"][0]["flags"], FLAG_DOMAIN_ROOT);
    }

    #[test]
    fn emit_produces_every_contract_c3_graph_file() {
        let tiers = emit(&corpus_of(vec![record("A", "ai")]), "2026-09-22");
        let names: Vec<&str> = tiers.files.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"overview.json"));
        assert!(names.contains(&"full.bin"));
        assert!(names.contains(&"stats.json"));
        assert!(names.contains(&"bridges.json"));
        for slug in DOMAIN_SLUGS {
            assert!(
                names.contains(&format!("domain-{slug}.bin").as_str()),
                "{slug}"
            );
        }
        assert_eq!(tiers.files.len(), 4 + DOMAIN_SLUGS.len());
    }

    #[test]
    fn the_bin_tiers_carry_the_ngg1_magic() {
        let tiers = emit(&corpus_of(vec![record("A", "ai")]), "2026-09-22");
        let is_bin = |name: &str| std::path::Path::new(name).extension() == Some("bin".as_ref());
        for f in tiers.files.iter().filter(|f| is_bin(&f.name)) {
            assert_eq!(&f.bytes[0..4], super::super::ngg1::MAGIC, "{}", f.name);
        }
    }

    #[test]
    fn emission_is_byte_stable() {
        let corpus = corpus_of(vec![record("A", "ai"), record("B", "blockchain")]);
        let a = emit(&corpus, "2026-09-22");
        let b = emit(&corpus, "2026-09-22");
        for (x, y) in a.files.iter().zip(&b.files) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.bytes, y.bytes, "{}", x.name);
        }
    }

    #[test]
    fn stats_separates_declared_from_resolvable() {
        let mut a = record("A", "ai");
        a.sub_class_of = vec![parent("b"), parent("nowhere")];
        let tiers = emit(&corpus_of(vec![a, record("B", "ai")]), "2026-09-22");
        let stats: Value = serde_json::from_slice(
            &tiers
                .files
                .iter()
                .find(|f| f.name == "stats.json")
                .unwrap()
                .bytes,
        )
        .unwrap();
        assert_eq!(stats["edges"]["declaredBackbone"], 2);
        assert_eq!(stats["edges"]["backbone"], 1);
        assert_eq!(stats["pages"], 2);
        assert_eq!(stats["classes"], 2);
        assert_eq!(stats["scopes"]["full"]["nodes"], 2);
    }
}
