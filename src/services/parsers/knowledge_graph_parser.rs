// src/services/parsers/knowledge_graph_parser.rs
//! Knowledge Graph Parser
//!
//! Parses markdown files marked with `public:: true` to extract:
//! - Nodes (pages, concepts)
//! - Edges (links, relationships)
//! - Metadata (properties, tags)
//!
//! Supports both VisionClaw v2 (IRI-first) and v4 (OntologyBlock) page
//! formats with automatic format detection. See `PageFormat` enum.

use crate::models::edge::Edge;
use crate::models::graph::GraphData;
use crate::models::metadata::{MaturityLevel, MetadataStore, PhysicalityCode, RoleCode};
use crate::models::node::Node;
use crate::services::parsers::visibility::{classify_visibility, Visibility};
use crate::utils::socket_flow_messages::BinaryNodeData;
use log::{debug, info};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// Detected page format — drives IRI computation and metadata extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFormat {
    /// v2 ontology: line 1 starts with `iri::` AND contains `rdf-type:: owl:Class`.
    V2Ontology,
    /// v2 note (workingGraph): has `public:: true` but NO `iri::` on line 1.
    V2Note,
    /// v4 ontology: contains `### OntologyBlock`.
    V4Ontology,
    /// Plain page: `public:: true` without any ontology markers.
    Plain,
}

impl PageFormat {
    /// Detect page format from raw content.
    pub fn detect(content: &str) -> Self {
        let first_line = content.lines().next().unwrap_or("");
        let has_iri_line1 = first_line.starts_with("iri::");
        let has_rdf_type_owl_class = content
            .lines()
            .any(|l| {
                let t = l.trim();
                t.starts_with("rdf-type::") && t.contains("owl:Class")
            });
        let has_ontology_block = content.contains("### OntologyBlock");

        if has_iri_line1 && has_rdf_type_owl_class {
            PageFormat::V2Ontology
        } else if has_ontology_block {
            PageFormat::V4Ontology
        } else if has_iri_line1 {
            // Has IRI but no rdf-type owl:Class — treat as v2 note variant.
            PageFormat::V2Note
        } else {
            // No IRI on line 1, no OntologyBlock — plain page or workingGraph note.
            PageFormat::Plain
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PageFormat::V2Ontology => "v2_ontology",
            PageFormat::V2Note => "v2_note",
            PageFormat::V4Ontology => "v4_ontology",
            PageFormat::Plain => "plain",
        }
    }
}

/// Which graph a file was sourced from. Stored in node metadata so
/// downstream consumers (Neo4j projection, API layer) can distinguish
/// ontology pages from user notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphSource {
    /// mainKnowledgeGraph — curated ontology pages.
    MainKnowledgeGraph,
    /// workingGraph — user notes and drafts.
    WorkingGraph,
}

impl GraphSource {
    pub fn as_str(self) -> &'static str {
        match self {
            GraphSource::MainKnowledgeGraph => "mainKnowledgeGraph",
            GraphSource::WorkingGraph => "workingGraph",
        }
    }
}

impl Default for GraphSource {
    fn default() -> Self {
        GraphSource::MainKnowledgeGraph
    }
}

/// Extract a Logseq flat property (`key:: value`) from page content.
/// Returns the first match. Only matches line-anchored (non-indented)
/// properties so bullet-level properties are not picked up by accident.
fn extract_property<'a>(content: &'a str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('-') || trimmed.starts_with('#') {
            // Past page-properties block — stop scanning.
            break;
        }
        if let Some(rest) = trimmed.strip_prefix(key) {
            if let Some(value) = rest.strip_prefix("::") {
                let v = value.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Extract wikilinks from relationship property lines.
///
/// Relationship properties are indented bullet items like:
///   `  - is-subclass-of:: [[Target1]], [[Target2]]`
///
/// This function scans the full content for known relationship keys and
/// extracts all `[[...]]` targets from their values. The set of
/// relationship keys follows the v2 PAGE-FORMAT.md spec.
fn extract_relationship_wikilinks(content: &str) -> Vec<String> {
    static RELATIONSHIP_KEYS: &[&str] = &[
        "is-subclass-of",
        "has-part",
        "is-part-of",
        "requires",
        "enables",
        "implements",
        "bridges-to",
        "depends-on",
        "belongs-to-domain",
        "implemented-in-layer",
        "sources",
    ];

    let link_re = regex::Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]")
        .expect("invalid wikilink regex");
    let mut out = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches('-').trim();
        for &key in RELATIONSHIP_KEYS {
            if let Some(rest) = trimmed.strip_prefix(key) {
                if rest.starts_with("::") {
                    let value_part = &rest[2..];
                    for cap in link_re.captures_iter(value_part) {
                        if let Some(m) = cap.get(1) {
                            let target = m.as_str().trim().to_string();
                            if !target.is_empty() {
                                out.push(target);
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// A single source-of-truth file offered to the two-pass parser.
///
/// `name` is the filename (e.g. `PageName.md`). `path` is the repo-relative
/// path used to compute the canonical IRI (ADR-050 §"Canonical IRI"). If
/// the caller only has a flat filename, pass it as `path` as well — the
/// canonical IRI stays deterministic for that input.
#[derive(Debug, Clone)]
pub struct FileBundle {
    pub name: String,
    pub path: String,
    pub content: String,
    /// Which graph this file belongs to. Defaults to `MainKnowledgeGraph`.
    pub graph_source: GraphSource,
}

impl FileBundle {
    pub fn new(name: impl Into<String>, path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            content: content.into(),
            graph_source: GraphSource::default(),
        }
    }

    /// Construct a FileBundle with an explicit graph source.
    pub fn new_with_source(
        name: impl Into<String>,
        path: impl Into<String>,
        content: impl Into<String>,
        graph_source: GraphSource,
    ) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            content: content.into(),
            graph_source,
        }
    }
}

/// A draft of a `:KGNode` ready to be projected into Neo4j.
///
/// Carries the legacy `Node` payload plus the sovereign-model fields
/// (`visibility` / `owner_pubkey` / `pod_url`). `opaque_id` is deliberately
/// computed at query time per ADR-050 §"Opaque ID construction" — it is
/// `None` here.
///
/// TODO(sibling: schema-and-binary): once the sibling agent lands the
/// `KGNode` struct rename with these fields baked in, collapse this draft
/// back into `Node` / `KGNode` and drop the wrapper.
#[derive(Debug, Clone)]
pub struct KGNodeDraft {
    pub node: Node,
    pub canonical_iri: String,
    pub visibility: Visibility,
    pub owner_pubkey: Option<String>,
    pub opaque_id: Option<String>,
    pub pod_url: Option<String>,
    /// `true` when this node is a stub — i.e. referenced by a wikilink but
    /// the source page was not present in the ingest batch. Stubs carry
    /// no label, no metadata_id, no pod_url.
    pub is_stub: bool,
}

/// Output of the two-pass parser.
#[derive(Debug, Clone, Default)]
pub struct ParseOutput {
    pub nodes: Vec<KGNodeDraft>,
    pub edges: Vec<Edge>,
    /// The UUID assigned to this ingest run. Every `WikilinkRef` edge in
    /// `edges` carries this id under `metadata["last_seen_run_id"]`. The
    /// orphan-retraction background job scans for stale run ids.
    pub run_id: String,
}

/// Internal page metadata gathered in Pass 1.
#[derive(Debug, Clone)]
struct PageMeta {
    title: String,
    canonical_iri: String,
    relative_path: String,
    raw_content: String,
    wikilinks: Vec<String>,
    /// Detected page format (v2 ontology, v2 note, v4 ontology, plain).
    format: PageFormat,
    /// Which graph this page was sourced from.
    graph_source: GraphSource,
}

/// Knowledge graph parser with position preservation support
pub struct KnowledgeGraphParser {
    /// Existing positions from database (node_id -> (x, y, z))
    existing_positions: Option<HashMap<u32, (f32, f32, f32)>>,
    /// Owner pubkey injected by the ingest pipeline (ADR-050). `None` is
    /// allowed for the legacy single-file `parse()` path and for tests
    /// that predate the sovereign model.
    owner_pubkey: Option<String>,
}

impl KnowledgeGraphParser {
    pub fn new() -> Self {
        Self {
            existing_positions: None,
            owner_pubkey: None,
        }
    }

    /// Construct a parser bound to a specific owner pubkey. Required for
    /// the sovereign two-pass `parse_bundle` entry point so that canonical
    /// IRIs and Pod URLs are stamped with the correct owner namespace.
    pub fn new_with_owner(owner_pubkey: impl Into<String>) -> Self {
        Self {
            existing_positions: None,
            owner_pubkey: Some(owner_pubkey.into()),
        }
    }

    /// Replace or set the owner pubkey. Useful when the ingest pipeline
    /// reuses a single parser instance across users.
    pub fn set_owner_pubkey(&mut self, owner_pubkey: impl Into<String>) {
        self.owner_pubkey = Some(owner_pubkey.into());
    }

    /// Current owner pubkey, if any.
    pub fn owner_pubkey(&self) -> Option<&str> {
        self.owner_pubkey.as_deref()
    }

    /// Create parser with existing positions from database
    /// These positions will be used instead of generating random ones
    pub fn with_positions(existing_positions: HashMap<u32, (f32, f32, f32)>) -> Self {
        Self {
            existing_positions: Some(existing_positions),
            owner_pubkey: None,
        }
    }

    /// Set existing positions for position preservation
    pub fn set_positions(&mut self, positions: HashMap<u32, (f32, f32, f32)>) {
        self.existing_positions = Some(positions);
    }

    /// Two-pass parse — the sovereign-model entry point (ADR-050 / ADR-051).
    ///
    /// Pass 1 walks every file, classifies visibility, extracts wikilinks,
    /// and records the adjacency map keyed by canonical IRI. Pass 2 turns
    /// every adjacency entry into a `KGNodeDraft` and synthesises a
    /// private stub for every wikilink target that is NOT present in the
    /// batch. Every wikilink edge carries `last_seen_run_id` so the
    /// orphan-retraction background job can prune stale references.
    ///
    /// Gated by the `VISIBILITY_CLASSIFICATION` env flag. When the flag
    /// is unset or `false`, the caller must fall back to the legacy
    /// single-pass `parse()` entry point instead.
    pub fn parse_bundle(&self, files: &[FileBundle]) -> Result<ParseOutput, String> {
        let owner = self
            .owner_pubkey
            .as_deref()
            .ok_or_else(|| {
                "KnowledgeGraphParser::parse_bundle requires owner_pubkey — \
                 construct with new_with_owner(...)"
                    .to_string()
            })?;

        let run_id = generate_run_id();
        info!(
            "🧭 Two-pass parse: owner={} files={} run_id={}",
            short_pubkey(owner),
            files.len(),
            run_id
        );

        // ------------------------------------------------------------------
        // PASS 1 — build the adjacency map.
        // ------------------------------------------------------------------
        let mut adjacency: HashMap<String, PageMeta> = HashMap::new();
        // Title → canonical_iri. Used to resolve `[[Page Name]]` against
        // pages that exist in the batch before falling back to a stub.
        let mut title_index: HashMap<String, String> = HashMap::new();

        for file in files {
            let meta = self.extract_page_meta(owner, file);
            title_index.insert(meta.title.clone(), meta.canonical_iri.clone());
            // Also index by the slug form so `[[my_page]]` resolves when a
            // user wrote `[[My Page]]` originally and vice versa.
            title_index.insert(slugify_title(&meta.title), meta.canonical_iri.clone());
            adjacency.insert(meta.canonical_iri.clone(), meta);
        }

        debug!(
            "Pass 1 complete: {} pages indexed, {} title aliases",
            adjacency.len(),
            title_index.len()
        );

        // ------------------------------------------------------------------
        // PASS 2 — classify, build nodes, synthesise stubs, emit edges.
        // ------------------------------------------------------------------
        let mut nodes: Vec<KGNodeDraft> = Vec::with_capacity(adjacency.len());
        let mut edges: Vec<Edge> = Vec::new();
        let mut emitted_iris: HashSet<String> = HashSet::new();
        // Dedup wikilink edges per (source_iri, target_iri) pair — a page
        // may reference the same target several times.
        let mut edge_seen: HashSet<(String, String)> = HashSet::new();

        for (iri, meta) in &adjacency {
            // This page always becomes a KGNode.
            let visibility = classify_visibility(&meta.raw_content);
            let source_node = self.build_kg_node(meta, visibility, owner);
            let source_id = source_node.node.id;
            nodes.push(source_node);
            emitted_iris.insert(iri.clone());

            for wikilink in &meta.wikilinks {
                let target_iri = resolve_wikilink_to_iri(wikilink, &title_index, owner);

                // If the target isn't an already-indexed page AND we haven't
                // already emitted a stub for it in this batch, emit a stub.
                if !adjacency.contains_key(&target_iri)
                    && !emitted_iris.contains(&target_iri)
                {
                    let stub = self.build_private_stub(&target_iri, wikilink, owner);
                    emitted_iris.insert(target_iri.clone());
                    nodes.push(stub);
                }

                let target_id = deterministic_id_from_iri(&target_iri);

                // Don't emit self-loops (a page linking to itself).
                if target_id == source_id {
                    continue;
                }
                // Dedup identical (source, target) pairs in this run.
                let key = (iri.clone(), target_iri.clone());
                if !edge_seen.insert(key) {
                    continue;
                }

                edges.push(build_wikilink_ref_edge(source_id, target_id, wikilink, &run_id));
            }
        }

        info!(
            "Pass 2 complete: {} nodes ({} stubs), {} wikilink edges",
            nodes.len(),
            nodes.iter().filter(|n| n.is_stub).count(),
            edges.len()
        );

        Ok(ParseOutput {
            nodes,
            edges,
            run_id,
        })
    }

    /// Pass-1 extraction: pull the page title, its wikilinks, the canonical
    /// IRI, and keep the raw content around for the Pass-2 visibility
    /// classifier.
    ///
    /// For v2 pages the `iri::` property IS the canonical IRI — we use it
    /// directly instead of computing a hash-based IRI from the file path.
    /// For v4 / plain pages we fall back to the legacy computation.
    #[allow(deprecated)] // Calls local canonical_iri (kept for byte-identical column values).
    fn extract_page_meta(&self, owner: &str, file: &FileBundle) -> PageMeta {
        let title = file
            .name
            .strip_suffix(".md")
            .unwrap_or(&file.name)
            .to_string();

        let format = PageFormat::detect(&file.content);

        // Canonical IRI: v2 pages carry it as a property; legacy pages compute it.
        let canonical_iri = match format {
            PageFormat::V2Ontology | PageFormat::V2Note => {
                extract_property(&file.content, "iri")
                    .unwrap_or_else(|| canonical_iri(owner, &file.path))
            }
            _ => canonical_iri(owner, &file.path),
        };

        // Body wikilinks (from markdown content, excluding OntologyBlock in v4).
        let mut wikilinks = extract_wikilink_titles(&file.content);

        // v2 pages: also extract wikilinks from structured relationship properties
        // (is-subclass-of, has-part, etc.) which use `[[Target]]` syntax.
        if matches!(format, PageFormat::V2Ontology | PageFormat::V2Note) {
            let rel_links = extract_relationship_wikilinks(&file.content);
            // Merge without duplicates — body extraction may already have them.
            // Collect owned strings into the set to avoid borrow conflicts.
            let existing: HashSet<String> = wikilinks.iter().cloned().collect();
            for link in rel_links {
                if !existing.contains(&link) {
                    wikilinks.push(link);
                }
            }
        }

        PageMeta {
            title,
            canonical_iri,
            relative_path: file.path.clone(),
            raw_content: file.content.clone(),
            wikilinks,
            format,
            graph_source: file.graph_source,
        }
    }

    /// Build a `KGNodeDraft` for an indexed page (public or private).
    fn build_kg_node(
        &self,
        meta: &PageMeta,
        visibility: Visibility,
        owner: &str,
    ) -> KGNodeDraft {
        // Reuse the legacy create_page_node logic for all the metadata /
        // ontology / position plumbing — we're not rewriting that.
        let mut node = self.create_page_node(&meta.title, &meta.raw_content);

        // Stamp the node's id from the canonical IRI so the identity is
        // stable across ingest runs (the legacy hash-of-filename fallback
        // would drift the moment the page path changes).
        let id = deterministic_id_from_iri(&meta.canonical_iri);
        node.id = id;
        node.data.node_id = id;

        // Surface the canonical IRI on the node metadata map so the
        // downstream Cypher projection can MERGE against it without a
        // second lookup.
        node.metadata
            .insert("canonical_iri".into(), meta.canonical_iri.clone());
        node.metadata
            .insert("visibility".into(), visibility.as_str().into());
        node.metadata
            .insert("owner_pubkey".into(), owner.to_string());

        // Page format and graph source — allows downstream consumers to
        // distinguish v2 ontology pages from plain notes.
        node.metadata
            .insert("page_format".into(), meta.format.as_str().into());
        node.metadata
            .insert("graph_source".into(), meta.graph_source.as_str().into());

        // v2-specific metadata extraction: pull structured properties that
        // the v2 PAGE-FORMAT.md spec defines at page level.
        if matches!(meta.format, PageFormat::V2Ontology | PageFormat::V2Note) {
            self.extract_v2_metadata(&meta.raw_content, &mut node.metadata);
        }

        // Stamp first-class Node fields that depend on FileBundle context.
        node.canonical_iri = Some(meta.canonical_iri.clone());
        node.graph_source = Some(meta.graph_source.as_str().to_string());

        let pod_url = pod_url_for(owner, &meta.relative_path, visibility);
        node.metadata.insert("pod_url".into(), pod_url.clone());

        KGNodeDraft {
            node,
            canonical_iri: meta.canonical_iri.clone(),
            visibility,
            owner_pubkey: Some(owner.to_string()),
            // opaque_id is computed at query-time per ADR-050 §"Opaque ID
            // construction" (HMAC with a rotating server-session salt).
            // The parser never stamps it — that's the projection layer's
            // responsibility.
            opaque_id: None,
            pod_url: Some(pod_url),
            is_stub: false,
        }
    }

    /// Build a private stub for a wikilink target that was not present in
    /// the ingest batch. Stubs carry no label, no metadata_id, no pod_url
    /// — they're placeholders that become fully-fledged nodes the next
    /// time the owner syncs a file whose canonical IRI matches.
    fn build_private_stub(
        &self,
        target_iri: &str,
        wikilink_text: &str,
        owner: &str,
    ) -> KGNodeDraft {
        let id = deterministic_id_from_iri(target_iri);
        let (x, y, z) = self.get_position(id);

        let mut metadata = HashMap::new();
        metadata.insert("type".into(), "kg_stub".into());
        metadata.insert("canonical_iri".into(), target_iri.to_string());
        metadata.insert(
            "visibility".into(),
            Visibility::Private.as_str().into(),
        );
        metadata.insert("owner_pubkey".into(), owner.to_string());
        // Preserve the authoring intent for the retraction job so it can
        // surface a useful audit trail if the stub is later pruned.
        metadata.insert("stub_source_wikilink".into(), wikilink_text.to_string());

        let data = BinaryNodeData {
            node_id: id,
            x,
            y,
            z,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
        };

        let mut node = Node {
            id,
            // metadata_id is empty for stubs — we only know the opaque IRI.
            metadata_id: String::new(),
            // Use the wikilink text as the visible label. The original "anonymous
            // API must never see one" privacy concern is handled at a different
            // layer (ADR-050 H2 bit-29 opacification at the binary encoder for
            // non-owners), NOT by writing an empty label to Neo4j. Empty labels
            // caused the client to fall back to rendering the numeric node ID,
            // which dominated the visible graph (~85% of nodes on a content-
            // sparse KG). The wikilink text is the most informative label we
            // have for an unresolved target. Anonymous viewers still get the
            // opacification on the wire path; the label only reaches owners.
            label: wikilink_text.to_string(),
            data,
            metadata,
            file_size: 0,
            node_type: Some("kg_stub".to_string()),
            color: Some("#6B7280".to_string()),
            size: Some(0.6),
            weight: Some(0.5),
            group: None,
            user_data: None,
            mass: Some(1.0),
            x: Some(x),
            y: Some(y),
            z: Some(z),
            vx: Some(0.0),
            vy: Some(0.0),
            vz: Some(0.0),
            owl_class_iri: None,
            visibility: Visibility::Private,
            owner_pubkey: Some(owner.to_string()),
            opaque_id: None,
            pod_url: None,
            canonical_iri: None,
            visionclaw_uri: None,
            rdf_type: None,
            same_as: None,
            domain: None,
            content_hash: None,
            quality_score: None,
            authority_score: None,
            preferred_term: None,
            graph_source: None,
            kind_id: None,
        };
        node.ensure_kind_id();

        KGNodeDraft {
            node,
            canonical_iri: target_iri.to_string(),
            visibility: Visibility::Private,
            owner_pubkey: Some(owner.to_string()),
            opaque_id: None,
            // A stub has no Pod content until the owner authors it.
            pod_url: None,
            is_stub: true,
        }
    }

    /// Extract v2-specific metadata properties from page content and insert
    /// them into the node metadata map. Only called for `V2Ontology` or
    /// `V2Note` format pages.
    ///
    /// These properties are defined in PAGE-FORMAT.md and sit at the page
    /// level (flat, non-indented). The generic property regex in
    /// `create_page_node` already captures most of them, but this method
    /// ensures the key names used downstream are consistent (snake_case)
    /// and that v2-specific keys that don't exist in v4 are always present.
    fn extract_v2_metadata(&self, content: &str, metadata: &mut HashMap<String, String>) {
        // Map of v2 property key → metadata key used downstream.
        static V2_KEYS: &[(&str, &str)] = &[
            ("domain", "domain"),
            ("quality-score", "quality_score"),
            ("authority-score", "authority_score"),
            ("content-hash", "content_hash"),
            ("preferred-term", "preferred_term"),
            ("rdf-type", "rdf_type"),
            ("same-as", "same_as"),
            ("status", "status"),
            ("iri", "iri"),
            ("uri", "uri"),
            ("context", "context"),
            ("legacy-term-id", "legacy_term_id"),
            ("version", "version"),
        ];

        for &(prop_key, meta_key) in V2_KEYS {
            if let Some(value) = extract_property(content, prop_key) {
                let value = if meta_key == "domain" {
                    value.to_lowercase()
                } else {
                    value
                };
                // Don't overwrite if already set by the generic property regex
                // in create_page_node (which uses the original kebab-case key).
                metadata.entry(meta_key.to_string()).or_insert(value);
            }
        }
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

    
    pub fn parse(&self, content: &str, filename: &str) -> Result<GraphData, String> {
        info!("Parsing knowledge graph file: {}", filename);

        
        let page_name = filename.strip_suffix(".md").unwrap_or(filename).to_string();

        
        let nodes = vec![self.create_page_node(&page_name, content)];
        let mut id_to_metadata = HashMap::new();
        id_to_metadata.insert(nodes[0].id.to_string(), page_name.clone());

        
        
        
        // Wikilink edges-only: create Edge objects for [[WikiLinks]] without
        // inflating the node count. Only edges are emitted; target nodes are NOT
        // created here. Edges whose target doesn't exist as a page node will
        // still be stored — the Neo4j MERGE will create stubs or the edge will
        // dangle harmlessly until the target page is synced.
        // Strip OntologyBlock before extracting wikilinks so taxonomy class
        // references inside ### OntologyBlock do not become wikilink edges.
        let content_for_wikilinks = Self::strip_ontology_block(content);
        let wikilink_edges = self.extract_wikilink_edges(content_for_wikilinks, &nodes[0].id);

        let metadata = self.extract_metadata_store(content, &page_name);

        debug!(
            "Parsed {}: {} nodes, {} wikilink edges",
            filename,
            nodes.len(),
            wikilink_edges.len(),
        );

        Ok(GraphData {
            nodes,
            edges: wikilink_edges,
            metadata,
            id_to_metadata,
        })
    }

    /// Create a page node, preserving existing position if available
    fn create_page_node(&self, page_name: &str, content: &str) -> Node {
        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), "page".to_string());
        metadata.insert("source_file".to_string(), format!("{}.md", page_name));
        metadata.insert("public".to_string(), "true".to_string());

        // Extract Logseq-style `key:: value` properties from the page and fold
        // them into node metadata so they reach Neo4j. Previously these were
        // parsed into a local HashMap in extract_metadata_store() but discarded
        // before returning, leaving source_domain/term-id/owl:class NULL on
        // every KGNode in the graph.
        //
        // Lowercase `source-domain` / `domain` values so the downstream domain
        // filter (`graph_types.rs::classify_node_population` and
        // `neo4j_adapter.rs::domain_to_color`) matches regardless of authoring
        // case.
        let prop_re = regex::Regex::new(r"(?m)^\s*-?\s*([a-zA-Z][a-zA-Z0-9_:\-]*)::\s*(.+)$")
            .expect("invalid property regex");
        let mut owl_class_iri: Option<String> = None;
        for cap in prop_re.captures_iter(content) {
            let (Some(k), Some(v)) = (cap.get(1), cap.get(2)) else { continue };
            let key = k.as_str();
            let mut value = v.as_str().trim().to_string();
            if matches!(key, "source-domain" | "domain" | "source_domain") {
                value = value.to_lowercase();
            }
            if key == "owl:class" {
                owl_class_iri = Some(value.clone());
            }
            if matches!(key, "type" | "source_file" | "public") {
                // Don't overwrite fields we set explicitly above.
                continue;
            }
            metadata.entry(key.to_string()).or_insert(value);
        }

        let tags = self.extract_tags(content);
        if !tags.is_empty() {
            metadata.insert("tags".to_string(), tags.join(", "));
        }

        // Derive integer OWL codes for the CUDA semantic-forces kernel.
        // We read from the metadata HashMap we just populated so the property
        // regex only runs once.  The `maturity` key is checked first, then
        // `status` as a fallback (some older pages use `status::` instead).
        let physicality = PhysicalityCode::from_logseq(
            metadata.get("owl:physicality").map(|s| s.as_str()).unwrap_or(""),
        );
        let role = RoleCode::from_logseq(
            metadata.get("owl:role").map(|s| s.as_str()).unwrap_or(""),
        );
        let maturity = MaturityLevel::from_logseq(
            metadata
                .get("maturity")
                .or_else(|| metadata.get("status"))
                .map(|s| s.as_str())
                .unwrap_or(""),
        );

        // Persist the integer codes into the node metadata HashMap so they
        // propagate to Neo4j node properties and are available to the kernel.
        metadata.insert("physicality_code".into(), physicality.as_i32().to_string());
        metadata.insert("role_code".into(), role.as_i32().to_string());
        metadata.insert("maturity_level".into(), maturity.as_i32().to_string());

        let id = self.page_name_to_id(page_name);

        // Use existing position or generate random (position preservation)
        let (x, y, z) = self.get_position(id);
        let data = BinaryNodeData {
            node_id: id,
            x,
            y,
            z,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
        };

        // Display label: prefer preferred-term over the raw filename slug.
        // Files named "AI-0424-confidential-computing.md" carry
        // `preferred-term:: Confidential Computing` — use the human name
        // for display, keep the slug as metadata_id for stable identity.
        let display_label = metadata
            .get("preferred-term")
            .cloned()
            .unwrap_or_else(|| page_name.to_string());

        // Promote v2 properties from the metadata HashMap to first-class
        // Node fields so they serialize to JSON / propagate to Neo4j
        // without downstream consumers needing to dig into the generic map.
        let preferred_term_field = metadata.get("preferred-term").cloned();
        let domain_field = metadata.get("domain")
            .or_else(|| metadata.get("source-domain"))
            .or_else(|| metadata.get("source_domain"))
            .cloned()
            .map(|d| d.to_lowercase());
        let quality_score_field = metadata.get("quality-score")
            .and_then(|v| v.parse::<f32>().ok());
        let authority_score_field = metadata.get("authority-score")
            .and_then(|v| v.parse::<f32>().ok());
        let content_hash_field = metadata.get("content-hash").cloned();
        let rdf_type_field = metadata.get("rdf-type").cloned();
        let same_as_field = metadata.get("same-as").cloned();
        let visionclaw_uri_field = metadata.get("uri").cloned();
        // canonical_iri and graph_source are stamped by build_kg_node, not here.

        Node {
            id,
            metadata_id: page_name.to_string(),
            label: display_label,
            data,
            metadata,
            file_size: 0,
            node_type: Some("page".to_string()),
            color: Some("#4A90E2".to_string()),
            size: Some(1.0),
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
            // If the page carries `owl:class:: bc:SomeClass` (or similar) that
            // declaration is now promoted to the node's first-class owl_class_iri
            // field so ontology enrichment and GPU semantic-force IRI lookup can
            // actually find the node.
            owl_class_iri,
            visibility: crate::models::node::Visibility::Public,
            owner_pubkey: None,
            opaque_id: None,
            pod_url: None,
            // v2 first-class fields — populated from parsed properties above.
            // canonical_iri and graph_source are set by build_kg_node after
            // create_page_node returns (they depend on FileBundle context).
            canonical_iri: None,
            visionclaw_uri: visionclaw_uri_field,
            rdf_type: rdf_type_field,
            same_as: same_as_field,
            domain: domain_field,
            content_hash: content_hash_field,
            quality_score: quality_score_field,
            authority_score: authority_score_field,
            preferred_term: preferred_term_field,
            graph_source: None,
            kind_id: graph_cognition_core::NodeKind::from_legacy_type("page")
                .map(|k| k.kind_id()),
        }
    }

    /// Strip everything from `### OntologyBlock` to end of string.
    /// OntologyBlock sections contain taxonomy class references like
    /// `[[Domain Concept]]` that must not become wikilink edges.
    fn strip_ontology_block(content: &str) -> &str {
        if let Some(pos) = content.find("### OntologyBlock") {
            &content[..pos]
        } else {
            content
        }
    }

    /// Extract wikilink edges only — no new nodes created.
    /// Returns Edge objects for each [[WikiLink]] found in content.
    /// Deduplicates by target to avoid multiple edges to the same page.
    /// OntologyBlock sections are stripped before scanning so taxonomy class
    /// references do not become wikilink edges.
    fn extract_wikilink_edges(&self, content: &str, source_id: &u32) -> Vec<Edge> {
        let mut edges = Vec::new();
        let mut seen_targets = std::collections::HashSet::new();

        // Strip OntologyBlock section so taxonomy references don't become edges
        let content = Self::strip_ontology_block(content);

        let link_pattern = regex::Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]")
            .expect("Invalid regex pattern");

        for cap in link_pattern.captures_iter(content) {
            if let Some(link_match) = cap.get(1) {
                let target_page = link_match.as_str().trim().to_string();
                let target_id = self.page_name_to_id(&target_page);

                // Skip self-loops and duplicates
                if target_id == *source_id || !seen_targets.insert(target_id) {
                    continue;
                }

                // Carry the wikilink text on the edge so the Neo4j writer can
                // label the auto-created stub on the target side. Without this,
                // the edge-write `MERGE (t:KGNode {id})` ON CREATE branch only
                // sets `node_type='kg_stub'` and never gets a label, so the
                // client falls back to rendering the numeric node ID.
                let mut metadata = HashMap::new();
                metadata.insert("target_wikilink".to_string(), target_page.clone());

                edges.push(Edge {
                    id: format!("{}_{}", source_id, target_id),
                    source: *source_id,
                    target: target_id,
                    weight: 1.0,
                    edge_type: Some("explicit_link".to_string()),
                    metadata: Some(metadata),
                    owl_property_iri: None,
                });
            }
        }

        edges
    }

    /// Extract links from content, preserving existing positions (legacy — creates nodes)
    #[allow(dead_code)]
    fn extract_links(&self, content: &str, source_id: &u32) -> (Vec<Node>, Vec<Edge>) {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let link_pattern = regex::Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]").expect("Invalid regex pattern");

        for cap in link_pattern.captures_iter(content) {
            if let Some(link_match) = cap.get(1) {
                let target_page = link_match.as_str().trim().to_string();
                let target_id = self.page_name_to_id(&target_page);

                let mut metadata = HashMap::new();
                metadata.insert("type".to_string(), "linked_page".to_string());

                // Use existing position or generate random (position preservation)
                let (x, y, z) = self.get_position(target_id);
                let data = BinaryNodeData {
                    node_id: target_id,
                    x,
                    y,
                    z,
                    vx: 0.0,
                    vy: 0.0,
                    vz: 0.0,
                };

                nodes.push(Node {
                    id: target_id,
                    metadata_id: target_page.clone(),
                    label: target_page.clone(),
                    data,
                    metadata,
                    file_size: 0,
                    node_type: Some("linked_page".to_string()),
                    color: Some("#7C3AED".to_string()),
                    size: Some(0.8),
                    weight: Some(0.8),
                    group: None,
                    user_data: None,
                    mass: Some(1.0),
                    x: Some(data.x),
                    y: Some(data.y),
                    z: Some(data.z),
                    vx: Some(0.0),
                    vy: Some(0.0),
                    vz: Some(0.0),
                    owl_class_iri: None,
                    visibility: crate::models::node::Visibility::Public,
                    owner_pubkey: None,
                    opaque_id: None,
                    pod_url: None,
                    canonical_iri: None,
                    visionclaw_uri: None,
                    rdf_type: None,
                    same_as: None,
                    domain: None,
                    content_hash: None,
                    quality_score: None,
                    authority_score: None,
                    preferred_term: None,
                    graph_source: None,
                    kind_id: graph_cognition_core::NodeKind::from_legacy_type("linked_page")
                        .map(|k| k.kind_id()),
                });

                edges.push(Edge {
                    id: format!("{}_{}", source_id, target_id),
                    source: *source_id,
                    target: target_id,
                    weight: 1.0,
                    edge_type: Some("link".to_string()),
                    metadata: Some(HashMap::new()),
                    owl_property_iri: None,
                });
            }
        }

        (nodes, edges)
    }

    /// Extract a MetadataStore of parsed Logseq properties keyed by page name.
    ///
    /// Historically this function parsed every `key:: value` pair in the content
    /// into a local HashMap and then returned an empty MetadataStore — the
    /// properties HashMap was never used. All ontology metadata (term-id,
    /// source-domain, owl:class, preferred-term, etc.) was silently discarded
    /// here before Neo4j ever saw it.
    ///
    /// Now we populate a Metadata record with the fields the downstream schema
    /// expects, so FileMetadata rows actually carry source attribution.
    fn extract_metadata_store(&self, content: &str, page_name: &str) -> MetadataStore {
        let mut store = MetadataStore::new();

        let prop_re = regex::Regex::new(r"(?m)^\s*-?\s*([a-zA-Z][a-zA-Z0-9_:\-]*)::\s*(.+)$")
            .expect("invalid property regex");

        let mut m = crate::models::metadata::Metadata::default();
        m.file_name = format!("{}.md", page_name);
        m.node_id = self.page_name_to_id(page_name).to_string();

        for cap in prop_re.captures_iter(content) {
            let (Some(k), Some(v)) = (cap.get(1), cap.get(2)) else { continue };
            let key = k.as_str();
            let value = v.as_str().trim().to_string();
            match key {
                "term-id" => m.term_id = Some(value),
                "preferred-term" => m.preferred_term = Some(value),
                "source-domain" | "domain" | "source_domain" =>
                    m.source_domain = Some(value.to_lowercase()),
                "status" | "ontology-status" => m.ontology_status = Some(value),
                "owl:class" => m.owl_class = Some(value),
                "owl:physicality" => m.owl_physicality = Some(value),
                "owl:role" => m.owl_role = Some(value),
                "quality-score" => m.quality_score = value.parse().ok(),
                "authority-score" => m.authority_score = value.parse().ok(),
                "maturity" => m.maturity = Some(value),
                "definition" => m.definition = Some(value),
                "belongsToDomain" | "belongs-to-domain" =>
                    m.belongs_to_domain.push(value),
                "is-subclass-of" => m.is_subclass_of.push(value),
                _ => { /* preserved via node.metadata HashMap in create_page_node */ }
            }
        }

        store.insert(page_name.to_string(), m);
        store
    }

    
    fn extract_tags(&self, content: &str) -> Vec<String> {
        let mut tags = Vec::new();

        
        let tag_pattern =
            regex::Regex::new(r"#([a-zA-Z0-9_-]+)|tag::\s*#?([a-zA-Z0-9_-]+)").expect("Invalid regex pattern");

        for cap in tag_pattern.captures_iter(content) {
            if let Some(tag) = cap.get(1).or_else(|| cap.get(2)) {
                tags.push(tag.as_str().to_string());
            }
        }

        tags.dedup();
        tags
    }

    
    /// Produce a stable u32 node ID from a page name.
    ///
    /// Uses BLAKE3 instead of `DefaultHasher` so the output is identical across
    /// Rust versions (P1-24).
    pub fn page_name_to_id(&self, page_name: &str) -> u32 {
        let hash = blake3::hash(page_name.as_bytes());
        let bytes = hash.as_bytes();
        // Take the first 4 bytes as a little-endian u32, mask to [1, u32::MAX].
        let raw = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        (raw & 0xFFFF_FFFE) + 1
    }
}

impl Default for KnowledgeGraphParser {
    fn default() -> Self {
        Self::new()
    }
}

// ----------------------------------------------------------------------------
// Sovereign-model free helpers (ADR-050 / ADR-051).
//
// These are intentionally module-free functions so tests can exercise them
// without constructing a parser. Keep them pure.
// ----------------------------------------------------------------------------

/// Canonical IRI scheme per ADR-050 §"Canonical IRI":
/// `visionclaw:owner:{owner_pubkey}/kg/{sha256(relative_path)}`.
///
/// Deterministic across runs; rename-proof identity is a non-goal — a
/// `page.md` → `folder/page.md` move is a new IRI, which matches Logseq's
/// filename-as-title semantics.
///
/// **Deprecated**: new code should use [`crate::uri::mint_owned_kg`] which
/// produces the 12-hex API alias form. This local function survives so
/// existing rows stay byte-identical (PRD-006 §5.1). NOTE: this variant
/// uses RAW HEX pubkey, diverging from `crate::utils::canonical_iri` which
/// uses bech32 npub — both forms persist in the live database.
#[deprecated(
    since = "0.2.0",
    note = "use crate::uri::mint_owned_kg for new code; this fn preserves \
            existing canonical_iri column values (raw-hex pubkey form)"
)]
pub fn canonical_iri(owner_pubkey: &str, relative_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(relative_path.as_bytes());
    let path_hash = hex_encode(&hasher.finalize());
    format!("visionclaw:owner:{}/kg/{}", owner_pubkey, path_hash)
}

/// Deterministic `u32` id derived from a canonical IRI. Stable across
/// runs and across parser instances. Avoids the legacy filename-hash
/// fallback which drifted on rename.
///
/// Reserves 0 as a sentinel (maps the hash into `[1, u32::MAX]`).
pub fn deterministic_id_from_iri(iri: &str) -> u32 {
    let mut hasher = Sha256::new();
    hasher.update(iri.as_bytes());
    let digest = hasher.finalize();
    let v = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    // Clear bit 29 — ADR-050 reserves it as the on-wire opacity flag and
    // the parser MUST NOT pre-stamp it. The protocol serialiser sets bit
    // 29 when writing a private node.
    let v = v & !0x2000_0000;
    // Avoid the sentinel 0.
    if v == 0 {
        1
    } else {
        v
    }
}

/// Resolve a `[[Wikilink]]` target to a canonical IRI.
///
/// Resolution order:
/// 1. Exact title match against indexed pages (`[[My Page]]` when a page
///    titled `My Page` is in the batch).
/// 2. Slug match (`[[my_page]]` resolves to `My Page` via its slug form).
/// 3. Fallback: synthesise a canonical IRI from the wikilink text treated
///    as a relative path (`{slug}.md`). This is what happens for a
///    target not present in the batch — the next sync that includes the
///    page's real path will produce the same IRI and upgrade the stub.
///
/// Never silently drops the wikilink — an unknown target always yields a
/// deterministic stub IRI.
#[allow(deprecated)] // Calls local canonical_iri (kept for byte-identical column values).
pub fn resolve_wikilink_to_iri(
    wikilink: &str,
    title_index: &HashMap<String, String>,
    owner_pubkey: &str,
) -> String {
    let cleaned = wikilink.trim();

    if let Some(iri) = title_index.get(cleaned) {
        return iri.clone();
    }
    let slug = slugify_title(cleaned);
    if let Some(iri) = title_index.get(&slug) {
        return iri.clone();
    }

    // Synthetic fallback — treat the slug as the relative path. A later
    // ingest run that includes the real page file will hash to the same
    // IRI (iff the authored path matches `{slug}.md`) and MERGE upgrades
    // the stub; otherwise the stub remains until the retraction job
    // prunes it.
    let synthetic_path = format!("{}.md", slug);
    canonical_iri(owner_pubkey, &synthetic_path)
}

/// Logseq slug: lowercase, spaces → underscores, non-[a-z0-9_-] dropped.
/// Intentionally simple — Logseq's own slugifier is equally permissive.
pub fn slugify_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for ch in title.chars() {
        if ch == ' ' {
            out.push('_');
        } else if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch.to_ascii_lowercase());
        }
        // Drop everything else (punctuation, emoji, etc.).
    }
    out
}

/// Extract `[[Wikilink]]` targets from a page body. Handles the pipe
/// alias form `[[Target|Display]]` — we keep `Target`.
fn extract_wikilink_titles(content: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]")
        .expect("invalid wikilink regex");
    let mut out = Vec::new();
    for cap in re.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            let target = m.as_str().trim().to_string();
            if !target.is_empty() {
                out.push(target);
            }
        }
    }
    out
}

/// Pod URL builder per ADR-051 §"Publish saga" step 2: owner-namespaced
/// container (`public/` vs `private/`) with the slug as the resource name.
pub fn pod_url_for(owner: &str, relative_path: &str, visibility: Visibility) -> String {
    let base = std::env::var("POD_BASE_URL")
        .unwrap_or_else(|_| "https://pods.visionclaw.org".to_string());
    let container = match visibility {
        Visibility::Public => "public",
        Visibility::Private => "private",
    };
    // Use the file's stem as the slug (`folder/Page.md` → `folder/page`).
    // Strip the .md extension so the Pod resource name follows the
    // convention used by ADR-051 §"Publish saga" (./{container}/kg/{slug}).
    let slug_path = relative_path
        .strip_suffix(".md")
        .unwrap_or(relative_path)
        .to_string();
    let slug = slugify_title(&slug_path);
    format!("{}/{}/{}/kg/{}", base, owner, container, slug)
}

/// Build a `WikilinkRef` edge carrying the current ingest-run id in its
/// metadata map (ADR-051 §"Orphan retraction"). Stored under
/// `last_seen_run_id` so the background scanner can detect stale edges.
///
/// Also carries the verbatim wikilink text under `target_wikilink` so that
/// when the Neo4j writer's MERGE on the target id auto-creates a stub
/// (because no matching node was supplied in this batch), the ON CREATE
/// branch has a human-readable label to set instead of leaving the stub
/// unlabelled — which causes the client to render the numeric node id.
fn build_wikilink_ref_edge(source_id: u32, target_id: u32, wikilink_text: &str, run_id: &str) -> Edge {
    let mut metadata = HashMap::new();
    metadata.insert("last_seen_run_id".to_string(), run_id.to_string());
    metadata.insert(
        "neo4j_relationship".to_string(),
        "WikilinkRef".to_string(),
    );
    metadata.insert("target_wikilink".to_string(), wikilink_text.to_string());

    Edge {
        // Keep the id format stable with the legacy extract_wikilink_edges
        // path so downstream dedup keys remain consistent.
        id: format!("{}_{}", source_id, target_id),
        source: source_id,
        target: target_id,
        weight: 1.0,
        edge_type: Some("WikilinkRef".to_string()),
        owl_property_iri: None,
        metadata: Some(metadata),
    }
}

/// Check the `VISIBILITY_CLASSIFICATION` env flag. Callers (ingest
/// pipeline) use this to choose between `parse_bundle` and legacy
/// `parse`.
pub fn visibility_classification_enabled() -> bool {
    std::env::var("VISIBILITY_CLASSIFICATION")
        .map(|v| matches!(v.as_str(), "true" | "1" | "yes"))
        .unwrap_or(false)
}

/// Generate a fresh run id (UUIDv4). Kept free so tests can assert edges
/// carry a stable value via stubbing if they want to.
fn generate_run_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Short-form pubkey for logs only — never used in cryptographic contexts.
fn short_pubkey(pubkey: &str) -> String {
    if pubkey.len() <= 12 {
        pubkey.to_string()
    } else {
        format!("{}…{}", &pubkey[..6], &pubkey[pubkey.len() - 4..])
    }
}

/// Hex encoder — avoids pulling in `hex` crate for a dozen bytes.
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
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
