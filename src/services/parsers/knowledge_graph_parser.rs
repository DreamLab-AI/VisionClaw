// src/services/parsers/knowledge_graph_parser.rs
//! Knowledge Graph Parser
//!
//! Parses vault pages admitted by the ADR-2040 §V4 gate to extract:
//! - Nodes (pages, concepts)
//! - Edges (links, relationships)
//! - Metadata (properties, tags)

use crate::utils::socket_flow_messages::BinaryNodeData;
use log::{debug, info, warn};
use std::collections::HashMap;
use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::metadata::MetadataStore;
use visionclaw_domain::models::node::Node;
use visionclaw_domain::vault::{self, LinkResolution, PageMeta, VaultIndex};

/// Knowledge graph parser with position preservation support
pub struct KnowledgeGraphParser {
    /// Existing positions from database (node_id -> (x, y, z))
    existing_positions: Option<HashMap<u32, (f32, f32, f32)>>,
}

impl KnowledgeGraphParser {
    pub fn new() -> Self {
        Self {
            existing_positions: None,
        }
    }

    /// Create parser with existing positions from database
    /// These positions will be used instead of generating random ones
    pub fn with_positions(existing_positions: HashMap<u32, (f32, f32, f32)>) -> Self {
        Self {
            existing_positions: Some(existing_positions),
        }
    }

    /// Set existing positions for position preservation
    pub fn set_positions(&mut self, positions: HashMap<u32, (f32, f32, f32)>) {
        self.existing_positions = Some(positions);
    }

    /// Get position for a node ID, using existing position or generating random.
    /// Public alias `get_position_public` provided for cross-module callers
    /// that build nodes directly from canonical entities (see ADR-090 Phase B).
    pub fn get_position_public(&self, node_id: u32) -> (f32, f32, f32) {
        self.get_position(node_id)
    }

    /// Get position for a node ID, using existing position or generating random
    fn get_position(&self, node_id: u32) -> (f32, f32, f32) {
        if let Some(ref positions) = self.existing_positions {
            if let Some(&(x, y, z)) = positions.get(&node_id) {
                return (x, y, z);
            }
        }
        // Generate random position only if no existing position found
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (
            rng.gen_range(-100.0..100.0),
            rng.gen_range(-100.0..100.0),
            rng.gen_range(-100.0..100.0),
        )
    }

    /// Parse a page with no vault index: link text is hashed as written.
    pub fn parse(&self, content: &str, filename: &str) -> GraphData {
        self.parse_with_index(content, filename, None)
    }

    /// Parse a page, resolving its wikilinks against the vault.
    ///
    /// `filename` is the **vault-relative** path (`Ns/Title.md`), not a bare
    /// basename: page identity is that path per §V1, and passing a basename
    /// collapses every namespaced page onto its leaf name — which silently
    /// merged distinct pages (e.g. `ETSI_Domain_Infrastructure/Security` with
    /// the root `Security`) and orphaned every bare link to a subfolder page.
    ///
    /// `index` supplies Obsidian's bare-link resolution. `None` keeps the
    /// pre-index behaviour (link text is hashed as-is) for callers with no
    /// vault listing to hand, such as the local single-file sync.
    pub fn parse_with_index(
        &self,
        content: &str,
        filename: &str,
        index: Option<&VaultIndex>,
    ) -> GraphData {
        info!("Parsing knowledge graph file: {}", filename);

        // Page identity is the vault-relative path under `pages/` without the
        // `.md`, with `/` as the namespace separator (ADR-2040 §V1). The legacy
        // `___` and `%2F` encodings decode to `/` here. Node ids are unchanged
        // by the decode: slugify collapses any run of non-alphanumerics to a
        // single `-`, so `A___B Testing` and `A/B Testing` hash identically
        // (governing doc Invariant 4).
        let page_name = vault::page_name_from_path(filename);

        let nodes = vec![self.create_page_node(&page_name, content)];
        let mut id_to_metadata = HashMap::new();
        id_to_metadata.insert(nodes[0].id.to_string(), page_name.clone());

        // Wikilink edges-only: create Edge objects for [[WikiLinks]] without
        // inflating the node count. Only edges are emitted; target nodes are NOT
        // created here. Edges whose target doesn't exist as a page node will
        // still be stored — the Oxigraph SPARQL INSERT will create stubs or the edge will
        // dangle harmlessly until the target page is synced.
        let (wikilink_edges, ambiguous) =
            self.extract_wikilink_edges(content, &nodes[0].id, &page_name, index);
        if ambiguous > 0 {
            // Recorded for the sync log: an ambiguous basename was resolved by
            // the same-folder / sorted-order tie-break rather than uniquely.
            warn!(
                "{}: {} wikilink(s) resolved by ambiguous basename",
                filename, ambiguous
            );
        }

        let metadata = MetadataStore::new();

        debug!(
            "Parsed {}: {} nodes, {} wikilink edges",
            filename,
            nodes.len(),
            wikilink_edges.len(),
        );

        GraphData {
            nodes,
            edges: wikilink_edges,
            metadata,
            id_to_metadata,
        }
    }

    /// Create a page node, preserving existing position if available.
    ///
    /// All authored metadata comes from `visionclaw_domain::vault` (ADR-2040
    /// D4) — the page's frontmatter. The metadata KEY SET is unchanged from
    /// pre-ADR-2040 (`type`, `source_file`, `public`, `file_size`, `tags`,
    /// `source_domain`, `quality_score`, `maturity`), so the client contract is
    /// untouched; only `source_file` changes shape, to the vault-relative path
    /// with `/` namespaces (`A/B Testing.md`, was `A___B Testing.md`).
    fn create_page_node(&self, page_name: &str, content: &str) -> Node {
        let meta = vault::parse(content);

        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), "page".to_string());
        metadata.insert("source_file".to_string(), format!("{}.md", page_name));
        // Every page reaching this constructor has already passed the §V4 gate
        // in the caller, so the published flag is constant — as it was before.
        metadata.insert("public".to_string(), "true".to_string());

        // Real markdown byte-size, surfaced so the client can size nodes by content volume.
        let content_size = content.len();
        metadata.insert("file_size".to_string(), content_size.to_string());

        let tags = Self::extract_tags(&meta, content);
        if !tags.is_empty() {
            metadata.insert("tags".to_string(), tags.join(", "));
        }

        // Pages carrying a class IRI are reclassified as ontology nodes downstream
        // (graph_state_actor.classify_node treats owl_class_iri.is_some() as ontology).
        let owl_class_iri = meta.owl_class.clone();
        if let Some(ref dom) = meta.source_domain {
            metadata.insert("source_domain".to_string(), dom.clone());
        }

        // Authored quality/maturity from the frontmatter — the signals the
        // per-client quality gates filter on.
        if let Some(q) = Self::extract_quality(&meta) {
            metadata.insert("quality_score".to_string(), q.to_string());
        }
        if let Some(m) = Self::extract_maturity(&meta) {
            metadata.insert("maturity".to_string(), m);
        }

        let id = self.page_name_to_id(page_name);

        // Use existing position or generate random (position preservation)
        let (x, y, z) = self.get_position(id);
        let data: visionclaw_domain::BinaryNodeData = BinaryNodeData {
            node_id: id,
            x,
            y,
            z,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
        }
        .into();

        // Pages with owl:class metadata are surfaced as ontology nodes so the
        // dual-graph (knowledge ↔ ontology) X-axis separation control has something
        // to separate. Pages without it remain "page" (knowledge population).
        let (node_type, color) = if owl_class_iri.is_some() {
            (
                Some("ontology_node".to_string()),
                Some("#B91C7B".to_string()),
            )
        } else {
            (Some("page".to_string()), Some("#4A90E2".to_string()))
        };

        // Display label, in order: the §V2 `title`, else the identity's leaf.
        // Identity (`metadata_id`, `id`, `source_file`) is untouched by this —
        // it stays the full vault-relative path.
        //
        // A `title` that merely repeats the page's own identity is not a
        // display title — `vault-migrate` writes the vault-relative path into
        // this key on ~223 converted pages — so it is ignored. The fallback is
        // the LEAF, never the full path: Obsidian displays `Ns/Title` as
        // `Title`, and a label carrying its folder is noise the client then has
        // to strip.
        let label = meta
            .title
            .clone()
            .filter(|title| title != page_name)
            .unwrap_or_else(|| vault::identity_basename(page_name).to_string());

        Node {
            id,
            metadata_id: page_name.to_string(),
            label,
            data,
            metadata,
            file_size: content_size as u64,
            node_type,
            color,
            // Monotonic ln(bytes) mapping: long-tailed file sizes -> small visual range
            // (~0 for tiny, ~9 for 8KB, ~11 for 64KB). No magic multipliers.
            size: Some((content_size as f32).max(1.0).ln()),
            weight: Some(1.0),
            group: None,
            user_data: None,
            mass: Some(1.0),
            x: Some(data.x),
            y: Some(data.y),
            z: Some(data.z),
            vx: Some(0.0),
            vy: Some(0.0),
            vz: Some(0.0),
            owl_class_iri,
        }
    }

    /// The frontmatter `quality` scalar (vocabulary.yaml `scalars.quality`,
    /// 0–1), clamped. Published as `metadata.quality_score`, the key the
    /// per-client quality filter (`client_filter.rs`) and the client's
    /// quality-driven node visuals read.
    pub(crate) fn extract_quality(meta: &PageMeta) -> Option<f32> {
        meta.extra
            .get("quality")
            .and_then(|q| q.trim().parse::<f32>().ok())
            .filter(|q| q.is_finite())
            .map(|q| q.clamp(0.0, 1.0))
    }

    /// The frontmatter `maturity` scalar (draft / emerging / established / …)
    /// — feeds the client's min-maturity filter.
    pub(crate) fn extract_maturity(meta: &PageMeta) -> Option<String> {
        meta.extra
            .get("maturity")
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
    }

    /// Extract wikilink edges only — no new nodes created.
    /// Returns Edge objects for each [[WikiLink]] found in content.
    /// Deduplicates by target to avoid multiple edges to the same page.
    ///
    /// Targets are resolved through [`VaultIndex`] (Obsidian's rule): a bare
    /// `[[Title]]` finds the page wherever it lives in the vault, so it joins
    /// the real node instead of minting a phantom stub beside it. Returns the
    /// edges plus the number of links that resolved only via an ambiguous
    /// basename, for the sync log.
    fn extract_wikilink_edges(
        &self,
        content: &str,
        source_id: &u32,
        from_identity: &str,
        index: Option<&VaultIndex>,
    ) -> (Vec<Edge>, usize) {
        let mut edges = Vec::new();
        let mut seen_targets = std::collections::HashSet::new();
        let mut ambiguous = 0usize;

        let link_pattern =
            regex::Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]").expect("Invalid regex pattern");

        for cap in link_pattern.captures_iter(content) {
            if let Some(link_match) = cap.get(1) {
                let raw_target = link_match.as_str();
                let target_page = match index {
                    Some(index) => {
                        let resolution = index.resolve(raw_target, from_identity);
                        if let LinkResolution::Ambiguous {
                            ref chosen,
                            ref alternatives,
                        } = resolution
                        {
                            ambiguous += 1;
                            debug!(
                                "Ambiguous wikilink [[{}]] on {}: chose {} from {:?}",
                                raw_target, from_identity, chosen, alternatives
                            );
                        }
                        resolution.target().to_string()
                    }
                    // No vault listing: still decode the legacy encodings so a
                    // `[[Ns___Title]]` hashes to the same id as `Ns/Title`.
                    None => vault::normalise_link_target(raw_target),
                };
                if target_page.is_empty() {
                    continue;
                }
                let target_id = self.page_name_to_id(&target_page);

                // Skip self-loops and duplicates
                if target_id == *source_id || !seen_targets.insert(target_id) {
                    continue;
                }

                edges.push(Edge {
                    id: format!("{}_{}", source_id, target_id),
                    source: *source_id,
                    target: target_id,
                    weight: 1.0,
                    edge_type: Some("explicit_link".to_string()),
                    metadata: None,
                    owl_property_iri: None,
                });
            }
        }

        (edges, ambiguous)
    }

    /// A page's tags: the authored frontmatter `tags` property (via
    /// `PageMeta`) first, then body `#hashtags` in document order. Deduplicated across both sources — the previous `Vec::dedup`
    /// only collapsed ADJACENT repeats, so a tag recurring later in the body
    /// was emitted twice.
    fn extract_tags(meta: &PageMeta, content: &str) -> Vec<String> {
        let mut tags: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut push = |tag: String, tags: &mut Vec<String>| {
            if !tag.is_empty() && seen.insert(tag.clone()) {
                tags.push(tag);
            }
        };

        for tag in &meta.tags {
            push(tag.clone(), &mut tags);
        }

        let tag_pattern = regex::Regex::new(r"#([a-zA-Z0-9_-]+)").expect("Invalid regex pattern");
        for cap in tag_pattern.captures_iter(content) {
            if let Some(tag) = cap.get(1) {
                push(tag.as_str().to_string(), &mut tags);
            }
        }

        tags
    }

    pub fn page_name_to_id(&self, page_name: &str) -> u32 {
        // Deterministic, seeded SHA-256 derivation (ADR-100 D2 / PRD-018 WS-0),
        // replacing the previous `DefaultHasher` whose output is explicitly NOT
        // stable across Rust releases and machines — the root cause of the
        // historical node-ID collisions and broken IRI→node resolution.
        //
        // The canonical derivation slugifies internally with the same lower /
        // collapse-non-alnum rule (now diacritic-preserving via NFKD), so
        // "Camera", "camera", and the IRI local-name "camera" (from
        // urn:ngm:class:camera) all resolve to the same id — preserving the
        // cross-graph join the `@type: Page` / `@type: Class` blocks rely on.
        visionclaw_ontology::services::canonical_iri::NodeIdHasher::derive_id(
            &visionclaw_ontology::services::canonical_iri::slugify(page_name),
        )
    }

    /// Canonical slug: lowercase, collapse non-alphanumeric runs to `-`,
    /// strip leading/trailing dashes.
    pub fn slugify(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut prev_dash = true; // suppresses leading dash
        for c in s.chars() {
            if c.is_ascii_alphanumeric() {
                out.extend(c.to_lowercase());
                prev_dash = false;
            } else if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
        // Strip trailing dash
        if out.ends_with('-') {
            out.pop();
        }
        out
    }
}

impl Default for KnowledgeGraphParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_preservation() {
        let mut positions = HashMap::new();
        positions.insert(12345u32, (10.0f32, 20.0f32, 30.0f32));

        let parser = KnowledgeGraphParser::with_positions(positions);
        let pos = parser.get_position(12345);

        assert_eq!(pos, (10.0, 20.0, 30.0));
    }

    fn label_of(page_path: &str, frontmatter: &str) -> String {
        let content = format!("---\npublic: true\n{frontmatter}---\n\n# Body\n");
        KnowledgeGraphParser::new().parse(&content, page_path).nodes[0]
            .label
            .clone()
    }

    #[test]
    fn a_subfolder_page_without_a_title_is_labelled_by_its_leaf() {
        // Obsidian displays `Ns/Title` as `Title`; the full path in a label is
        // noise, and 203 converted pages hit this branch.
        assert_eq!(
            label_of("podcast-evidence/black-friday-gpt.md", ""),
            "black-friday-gpt"
        );
    }

    #[test]
    fn a_subfolder_page_with_a_title_is_labelled_by_that_title() {
        assert_eq!(
            label_of(
                "podcast-evidence/black-friday-gpt.md",
                "title: Black Friday GPT\n"
            ),
            "Black Friday GPT"
        );
    }

    #[test]
    fn a_title_that_merely_echoes_the_identity_falls_back_to_the_leaf() {
        // What `vault-migrate` actually writes today.
        assert_eq!(
            label_of(
                "podcast-evidence/black-friday-gpt.md",
                "title: podcast-evidence/black-friday-gpt\n"
            ),
            "black-friday-gpt"
        );
    }

    #[test]
    fn a_root_page_is_unchanged_by_the_leaf_rule() {
        assert_eq!(label_of("Agentic AI.md", ""), "Agentic AI");
        assert_eq!(
            label_of("Agentic AI.md", "title: Agentic Artificial Intelligence\n"),
            "Agentic Artificial Intelligence"
        );
    }

    #[test]
    fn identity_is_not_affected_by_the_label_rule() {
        let content = "---\npublic: true\n---\n\n# Body\n";
        let graph =
            KnowledgeGraphParser::new().parse(content, "podcast-evidence/black-friday-gpt.md");
        let node = &graph.nodes[0];

        assert_eq!(node.label, "black-friday-gpt");
        assert_eq!(node.metadata_id, "podcast-evidence/black-friday-gpt");
        assert_eq!(
            node.metadata.get("source_file").map(String::as_str),
            Some("podcast-evidence/black-friday-gpt.md")
        );
    }

    #[test]
    fn test_fallback_to_random() {
        let parser = KnowledgeGraphParser::new();
        let pos = parser.get_position(99999);

        // Should be within random range
        assert!(pos.0 >= -100.0 && pos.0 <= 100.0);
        assert!(pos.1 >= -100.0 && pos.1 <= 100.0);
        assert!(pos.2 >= -100.0 && pos.2 <= 100.0);
    }
}
