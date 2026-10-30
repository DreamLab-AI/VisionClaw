//! Standards-grade RDF round-trip serialisation (WS-1 / ADR-100).
//!
//! VisionClaw's Oxigraph quad-store can be persisted and re-ingested in any of
//! the W3C-standard RDF dataset/graph syntaxes. This module is the single
//! serialise/parse surface for that, built **entirely** on oxigraph's bundled
//! `oxrdfio` re-export (`oxigraph::io::{RdfFormat, RdfParser, RdfSerializer}`).
//! No new dependency is introduced — the reuse-correct path versus pulling in a
//! second RDF stack such as `sophia`.
//!
//! ## Why not `Store::dump_to_writer`?
//!
//! `Store::dump_to_writer` iterates **every** quad in the store with no graph
//! filter. ADR-101 D5 requires the migration ledger graph
//! (`urn:ngm:graph:migrations`) — and the volatile shortest-path caches — to be
//! excluded from any data export: they are infrastructure bookkeeping, not
//! ontology/knowledge content. So we iterate the store ourselves and skip the
//! excluded graphs before feeding each quad to the serialiser.
//!
//! ## Format capability matrix (from `RdfFormat::supports_datasets`)
//!
//! | Format   | Datasets (named graphs) | Use here                  |
//! |----------|-------------------------|---------------------------|
//! | N-Quads  | yes                     | lossless full-store export |
//! | TriG     | yes                     | human-readable dataset     |
//! | JSON-LD  | yes (streaming profile) | web/interop export         |
//! | Turtle   | **no** (single graph)   | per-named-graph export     |
//!
//! Turtle cannot represent named graphs, so [`export_graph`] takes an explicit
//! graph name and serialises just that one graph's triples.

use std::io;

use oxigraph::io::{JsonLdProfile, RdfFormat, RdfParser, RdfSerializer};
use oxigraph::model::{GraphName, GraphNameRef, NamedNodeRef, Quad};
use oxigraph::store::Store;

use crate::oxigraph_ontology_repository::{GRAPH_CACHE_APSP, GRAPH_CACHE_SSSP};
use crate::sparql_migrations::GRAPH_MIGRATIONS;

/// The shared VisionClaw namespace base (ADR-100 D1). Registered as the `vc:`
/// prefix on serialiser output so Turtle/TriG/JSON-LD stay human-readable.
pub const VC_NS: &str = "https://narrativegoldmine.com/ns/v1#";

/// Named graphs that are **never** included in a data export (ADR-101 D5):
/// the migration ledger and the volatile path-distance caches. These carry
/// bookkeeping/derived state, not authored ontology or knowledge triples.
pub const EXPORT_EXCLUDED_GRAPHS: &[&str] = &[GRAPH_MIGRATIONS, GRAPH_CACHE_SSSP, GRAPH_CACHE_APSP];

/// Errors surfaced by the round-trip serialiser.
///
/// The crate does not depend on `thiserror`, so `Display`/`Error` are hand
/// written to match the existing [`crate::sparql_migrations::MigrationError`]
/// convention.
#[derive(Debug)]
pub enum SerdeError {
    /// The requested format cannot encode named graphs (e.g. Turtle) but a
    /// dataset export was requested. Use [`export_graph`] instead.
    NotADatasetFormat(&'static str),
    /// An I/O failure while writing serialised bytes.
    Io(io::Error),
    /// A failure while reading/parsing serialised bytes back into the store.
    Parse(oxigraph::io::RdfParseError),
    /// A failure while iterating the store.
    Store(oxigraph::store::StorageError),
    /// A bulk-load failure (parse + transaction) during import.
    Loader(oxigraph::store::LoaderError),
}

impl std::fmt::Display for SerdeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerdeError::NotADatasetFormat(fmt) => write!(
                f,
                "format {fmt} does not support datasets; use export_graph for a single named graph"
            ),
            SerdeError::Io(e) => write!(f, "serialise I/O error: {e}"),
            SerdeError::Parse(e) => write!(f, "parse error: {e}"),
            SerdeError::Store(e) => write!(f, "store error: {e}"),
            SerdeError::Loader(e) => write!(f, "loader error: {e}"),
        }
    }
}

impl std::error::Error for SerdeError {}

impl From<io::Error> for SerdeError {
    fn from(e: io::Error) -> Self {
        SerdeError::Io(e)
    }
}
impl From<oxigraph::io::RdfParseError> for SerdeError {
    fn from(e: oxigraph::io::RdfParseError) -> Self {
        SerdeError::Parse(e)
    }
}
impl From<oxigraph::store::StorageError> for SerdeError {
    fn from(e: oxigraph::store::StorageError) -> Self {
        SerdeError::Store(e)
    }
}
impl From<oxigraph::store::LoaderError> for SerdeError {
    fn from(e: oxigraph::store::LoaderError) -> Self {
        SerdeError::Loader(e)
    }
}

/// The canonical dataset (named-graph-aware) export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetFormat {
    /// `application/n-quads` — line-based, lossless, the safe default.
    NQuads,
    /// `application/trig` — Turtle-family, named-graph-aware, human readable.
    TriG,
    /// `application/ld+json` — JSON-LD, streaming profile (named-graph-aware).
    JsonLd,
}

impl DatasetFormat {
    fn rdf_format(self) -> RdfFormat {
        match self {
            Self::NQuads => RdfFormat::NQuads,
            Self::TriG => RdfFormat::TriG,
            Self::JsonLd => RdfFormat::JsonLd {
                // Streaming profile is required for quad-by-quad serialisation
                // without buffering the whole dataset into a JSON tree.
                profile: JsonLdProfile::Streaming.into(),
            },
        }
    }

    /// IANA media type, for `Content-Type` headers on an export endpoint.
    pub fn media_type(self) -> &'static str {
        match self {
            Self::NQuads => "application/n-quads",
            Self::TriG => "application/trig",
            Self::JsonLd => "application/ld+json",
        }
    }
}

/// Returns `true` when `graph` is one of the export-excluded bookkeeping graphs.
fn is_excluded(graph: &GraphName) -> bool {
    match graph {
        GraphName::NamedNode(n) => EXPORT_EXCLUDED_GRAPHS.contains(&n.as_str()),
        // The default (unnamed) graph holds nothing in VisionClaw's layout
        // (everything is filed under a named graph), but if it ever does it is
        // authored content, so it is exported.
        GraphName::BlankNode(_) | GraphName::DefaultGraph => false,
    }
}

/// Serialise the **whole store as a dataset**, excluding the bookkeeping graphs
/// in [`EXPORT_EXCLUDED_GRAPHS`] (ADR-101 D5). Named graphs are preserved.
///
/// Standards-grade: bytes are produced by oxigraph's `oxrdfio` serialiser, so
/// the output validates against the relevant W3C grammar.
/// @ontology urn:ngm:class:rdf
pub fn export_dataset(store: &Store, format: DatasetFormat) -> Result<Vec<u8>, SerdeError> {
    let rdf_format = format.rdf_format();
    debug_assert!(
        rdf_format.supports_datasets(),
        "DatasetFormat must map to a dataset-capable RdfFormat"
    );

    let mut serializer = RdfSerializer::from_format(rdf_format)
        .with_prefix("vc", VC_NS)
        .and_then(|s| s.with_prefix("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"))
        .and_then(|s| s.with_prefix("rdfs", "http://www.w3.org/2000/01/rdf-schema#"))
        .and_then(|s| s.with_prefix("owl", "http://www.w3.org/2002/07/owl#"))
        // Prefix registration only fails on a malformed IRI literal (all of the
        // above are compile-time constants), so this is infallible in practice.
        .unwrap_or_else(|_| RdfSerializer::from_format(rdf_format))
        .for_writer(Vec::new());

    for quad in store.iter() {
        let quad: Quad = quad?;
        if is_excluded(&quad.graph_name) {
            continue;
        }
        serializer.serialize_quad(&quad)?;
    }

    Ok(serializer.finish()?)
}

/// Serialise a **single named graph** as Turtle (or any graph format).
///
/// Turtle cannot encode named graphs, so the graph name is dropped and only the
/// triples of `graph_name` are emitted. The bookkeeping graphs are refused
/// outright to keep ADR-101 D5 consistent across both export paths.
pub fn export_graph(
    store: &Store,
    graph_name: NamedNodeRef<'_>,
    format: RdfFormat,
) -> Result<Vec<u8>, SerdeError> {
    if EXPORT_EXCLUDED_GRAPHS.contains(&graph_name.as_str()) {
        // Refuse to leak bookkeeping graphs through the per-graph path.
        return Ok(Vec::new());
    }

    let mut serializer = RdfSerializer::from_format(format)
        .with_prefix("vc", VC_NS)
        .unwrap_or_else(|_| RdfSerializer::from_format(format))
        .for_writer(Vec::new());

    let gref: GraphNameRef<'_> = graph_name.into();
    for quad in store.quads_for_pattern(None, None, None, Some(gref)) {
        let quad: Quad = quad?;
        serializer.serialize_triple(quad.as_ref())?;
    }

    Ok(serializer.finish()?)
}

/// Convenience: Turtle export of one named graph.
pub fn export_graph_turtle(
    store: &Store,
    graph_name: NamedNodeRef<'_>,
) -> Result<Vec<u8>, SerdeError> {
    export_graph(store, graph_name, RdfFormat::Turtle)
}

/// Parse `bytes` (a dataset serialisation) back into `store`, preserving named
/// graphs. The inverse of [`export_dataset`]; used for round-trip verification
/// and for restoring an exported snapshot.
pub fn import_dataset(
    store: &Store,
    format: DatasetFormat,
    bytes: &[u8],
) -> Result<(), SerdeError> {
    store.load_from_reader(RdfParser::from_format(format.rdf_format()), bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::{NamedNode, NamedNodeRef, Quad, QuadRef};

    fn ttl_iri(s: &str) -> NamedNode {
        NamedNode::new(s).unwrap()
    }

    /// Seeds a store with two authored named graphs plus a migrations-ledger
    /// quad that MUST be excluded from any export.
    fn seed_store() -> Store {
        let store = Store::new().unwrap();
        let s = ttl_iri("https://narrativegoldmine.com/ns/v1#artificial-intelligence/agent");
        let p = ttl_iri("http://www.w3.org/2000/01/rdf-schema#label");
        let lit = oxigraph::model::Literal::new_simple_literal("Café Gödel Δelta");
        // Knowledge graph triple (authored content — must survive export).
        store
            .insert(QuadRef::new(
                s.as_ref(),
                p.as_ref(),
                lit.as_ref(),
                NamedNodeRef::new("urn:ngm:graph:knowledge").unwrap(),
            ))
            .unwrap();
        // Ontology graph triple (authored content — must survive export).
        let cls = ttl_iri("https://narrativegoldmine.com/ns/v1#robotics/manipulator");
        store
            .insert(QuadRef::new(
                cls.as_ref(),
                NamedNodeRef::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#type").unwrap(),
                NamedNodeRef::new("http://www.w3.org/2002/07/owl#Class").unwrap(),
                NamedNodeRef::new("urn:ngm:graph:ontology:assert").unwrap(),
            ))
            .unwrap();
        // Migrations-ledger quad (bookkeeping — must be EXCLUDED from export).
        store
            .insert(QuadRef::new(
                NamedNodeRef::new("urn:ngm:migration:0001").unwrap(),
                NamedNodeRef::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#type").unwrap(),
                NamedNodeRef::new("urn:ngm:Migration").unwrap(),
                NamedNodeRef::new(GRAPH_MIGRATIONS).unwrap(),
            ))
            .unwrap();
        store
    }

    fn count_authored(store: &Store) -> usize {
        store
            .iter()
            .filter_map(Result::ok)
            .filter(|q| !is_excluded(&q.graph_name))
            .count()
    }

    #[test]
    fn nquads_round_trip_preserves_authored_quads_and_drops_bookkeeping() {
        let src = seed_store();
        let bytes = export_dataset(&src, DatasetFormat::NQuads).unwrap();

        // The migration ledger IRI must NOT appear in the serialised bytes.
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(
            !text.contains(GRAPH_MIGRATIONS),
            "migrations graph leaked into export"
        );
        assert!(
            !text.contains("urn:ngm:migration:0001"),
            "migration ledger subject leaked into export"
        );

        // Round-trip into a fresh store and compare the authored quad set.
        let dst = Store::new().unwrap();
        import_dataset(&dst, DatasetFormat::NQuads, &bytes).unwrap();

        let mut src_quads: Vec<Quad> = src
            .iter()
            .filter_map(Result::ok)
            .filter(|q| !is_excluded(&q.graph_name))
            .collect();
        let mut dst_quads: Vec<Quad> = dst.iter().filter_map(Result::ok).collect();
        src_quads.sort_by_key(|q| q.to_string());
        dst_quads.sort_by_key(|q| q.to_string());
        assert_eq!(
            src_quads, dst_quads,
            "authored quads not preserved across N-Quads round trip"
        );
        assert_eq!(dst_quads.len(), count_authored(&src));
    }

    #[test]
    fn trig_round_trip_preserves_named_graphs() {
        let src = seed_store();
        let bytes = export_dataset(&src, DatasetFormat::TriG).unwrap();
        let dst = Store::new().unwrap();
        import_dataset(&dst, DatasetFormat::TriG, &bytes).unwrap();
        assert_eq!(count_authored(&dst), count_authored(&src));
        // Diacritics in the rdfs:label must survive the round trip verbatim.
        let text = String::from_utf8(bytes).unwrap();
        assert!(
            text.contains("Café Gödel Δelta"),
            "diacritics not preserved"
        );
    }

    #[test]
    fn jsonld_round_trip_preserves_authored_quads() {
        let src = seed_store();
        let bytes = export_dataset(&src, DatasetFormat::JsonLd).unwrap();
        let dst = Store::new().unwrap();
        import_dataset(&dst, DatasetFormat::JsonLd, &bytes).unwrap();
        assert_eq!(count_authored(&dst), count_authored(&src));
    }

    #[test]
    fn turtle_export_of_single_graph_excludes_other_graphs() {
        let src = seed_store();
        let ttl = export_graph_turtle(
            &src,
            NamedNodeRef::new("urn:ngm:graph:ontology:assert").unwrap(),
        )
        .unwrap();
        let text = String::from_utf8(ttl).unwrap();
        // The ontology class is present; the knowledge-graph label is not.
        // (Turtle compacts the canonical IRI to `vc:robotics\/manipulator` —
        // the `/` is PN_LOCAL-escaped per the Turtle grammar, which is correct.)
        assert!(
            text.contains("manipulator") && text.contains("vc:"),
            "ontology class missing from Turtle export"
        );
        assert!(
            !text.contains("Café"),
            "other graph leaked into Turtle export"
        );
    }

    #[test]
    fn turtle_export_of_bookkeeping_graph_is_empty() {
        let src = seed_store();
        let ttl = export_graph_turtle(&src, NamedNodeRef::new(GRAPH_MIGRATIONS).unwrap()).unwrap();
        assert!(ttl.is_empty(), "bookkeeping graph must not export");
    }

    #[test]
    fn dataset_formats_report_iana_media_types() {
        assert_eq!(DatasetFormat::NQuads.media_type(), "application/n-quads");
        assert_eq!(DatasetFormat::TriG.media_type(), "application/trig");
        assert_eq!(DatasetFormat::JsonLd.media_type(), "application/ld+json");
    }
}
