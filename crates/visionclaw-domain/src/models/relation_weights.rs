//! Per-relation spring weights for the force pass (ADR-2191, slice 1).
//!
//! The force kernels multiply every edge's attraction by that edge's CSR
//! weight, in LinLog (`log1p(dist) * w * s`) and in Hooke
//! (`spring_k * (dist - rest) * w * s`), and by nothing else that varies per
//! edge. A per-relation stiffness is therefore exactly a per-relation weight
//! multiplier, and needs no kernel change. This module holds the host-side
//! half of that:
//!
//! * [`RelationWeightTable`]: a small, versioned, validated TOML table mapping
//!   a relation (by predicate IRI or by edge type) to a stiffness factor, with
//!   a provenance block.
//! * [`RelationWeightTable::resolve`]: the relation resolver. It keys on
//!   `owl_property_iri` first, because `edge_type` is a coarse category that
//!   collapses distinct relations (`relatedTo` and `relatesTo` are both
//!   `associative`; `requires`, `enables` and `dependsOn` are all
//!   `dependency`). It falls back to the normalised `edge_type`, which is the
//!   fine key on the ontology path (`requires`, `depends_on`, ...), and
//!   resolves anything else to the identity factor 1.0.
//! * [`build_csr`]: the one CSR builder for the force pass. It produces the
//!   unscaled `edge_weights` every other GPU consumer reads, and, only when a
//!   table is bound, a separate `spring_weights` array for the two force-pass
//!   launches.
//!
//! # Identity guarantee
//!
//! With no table, `spring_weights` is `None` and the caller passes the
//! unscaled buffer to the force pass, as before. With a table whose factor is
//! 1.0 for an edge and whose registry policy is [`RegistryWeight::Compose`],
//! that edge's spring weight is the unscaled weight itself: the multiply is
//! skipped when the factor is exactly 1.0, so the bits match even for a
//! signalling NaN, and for every other value `x * 1.0 == x` exactly under
//! IEEE 754 round-to-nearest.
//!
//! # Example
//!
//! ```
//! use std::collections::HashMap;
//! use visionclaw_domain::models::relation_weights::{build_csr, LoadOptions, RelationWeightTable};
//! use visionclaw_domain::models::Edge;
//!
//! let table = RelationWeightTable::parse(
//!     r#"
//!     format = "visionclaw.relation-weights"
//!     version = 1
//!     [provenance]
//!     source = "hand-set"
//!     [[relation]]
//!     match = "iri"
//!     key = "http://www.w3.org/2000/01/rdf-schema#subClassOf"
//!     stiffness = 3.0
//!     "#,
//!     LoadOptions::default(),
//! )
//! .unwrap();
//!
//! let mut is_a = Edge::new(10, 20, 1.5);
//! is_a.owl_property_iri = Some("http://www.w3.org/2000/01/rdf-schema#subClassOf".into());
//! let index: HashMap<u32, usize> = [(10, 0), (20, 1)].into_iter().collect();
//!
//! let csr = build_csr(&[is_a], &index, 2, Some(&table));
//! assert_eq!(csr.edge_weights, vec![1.5, 1.5]); // every other consumer: unchanged
//! assert_eq!(csr.spring_weights, Some(vec![4.5, 4.5])); // the force pass only
//! ```

use std::collections::HashMap;
use std::fmt;

use serde::Deserialize;

use super::edge::Edge;

/// The `format` string every relation-weight table must declare.
pub const FORMAT: &str = "visionclaw.relation-weights";

/// The only table `version` this build reads.
pub const VERSION: u32 = 1;

/// Smallest accepted stiffness factor. Zero removes a relation's attraction
/// but leaves repulsion, which is bounded, so the layout stays stable.
pub const STIFFNESS_MIN: f32 = 0.0;

/// Largest accepted stiffness factor.
///
/// For one spring under an explicit integrator, scaling its stiffness by `f`
/// has the same stability effect as scaling the timestep by `sqrt(f)`, so a
/// factor of 4 at most doubles the effective step for the stiffest relation.
/// With the registry's largest weight (2.0) that caps any spring weight at
/// 8.0, one order of magnitude above today's typical 0.6 to 2.0. The per-node
/// force and velocity clamps still apply on top.
pub const STIFFNESS_MAX: f32 = 4.0;

/// Most relation rows a table may hold. Matches the semantic module's
/// relationship-table capacity (256), so one key space serves both paths.
pub const MAX_RELATIONS: usize = 256;

/// Where a table's factors came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProvenanceSource {
    /// Every factor is 1.0. Used to prove the plumbing changes nothing.
    Uniform,
    /// Chosen by a person, for a smoke test or a deliberate visual choice.
    HandSet,
    /// Fitted by an offline experiment. Refused unless
    /// [`LoadOptions::allow_learned`] is set (ADR-2174 gate).
    Learned,
}

impl fmt::Display for ProvenanceSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Uniform => "uniform",
            Self::HandSet => "hand-set",
            Self::Learned => "learned",
        })
    }
}

/// How a matched edge's existing weight combines with its table factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegistryWeight {
    /// Spring weight = existing weight x factor. The existing weight of a
    /// typed knowledge edge is the semantic registry's `strength * 2`.
    #[default]
    Compose,
    /// Spring weight = factor, for every edge that resolves to a table row
    /// (by IRI or by edge type), whatever its existing weight. Unmatched
    /// edges keep their existing weight. For a learned table that was fitted
    /// without the registry weights.
    Ignore,
}

/// Which edge field a table row matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchOn {
    /// The full predicate IRI in `Edge::owl_property_iri`.
    Iri,
    /// The `Edge::edge_type` label, compared after [`normalise_edge_type`].
    EdgeType,
}

/// How an edge's factor was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedBy {
    /// Matched a row by `owl_property_iri`.
    Iri,
    /// No IRI row matched; matched a row by normalised `edge_type`.
    EdgeType,
    /// No row matched: factor 1.0 and the existing weight is kept.
    Identity,
}

/// The factor for one edge and how it was found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resolution {
    /// The stiffness factor; 1.0 for [`ResolvedBy::Identity`].
    pub factor: f32,
    /// Which key matched.
    pub by: ResolvedBy,
}

/// The table's provenance block, validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Where the factors came from.
    pub source: ProvenanceSource,
    /// Free-text note, if given.
    pub note: Option<String>,
    /// For a learned table: the crate that produced it.
    pub producer_crate: Option<String>,
    /// For a learned table: the producing commit (40-character hex SHA).
    pub commit: Option<String>,
    /// For a learned table: where the preregistered result is recorded.
    pub preregistration_result: Option<String>,
}

/// Options that gate what a load accepts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoadOptions {
    /// Accept a `learned` table. Default false, and ADR-2191 authorises no
    /// learned table: ADR-2174's experiment did not support H1.
    pub allow_learned: bool,
}

/// Why a table was refused. Every variant leaves the feature off.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RelationTableError {
    /// The text is not valid TOML for this schema (including unknown keys).
    #[error("malformed relation-weight table: {0}")]
    Malformed(String),
    /// `format` is not [`FORMAT`].
    #[error("format is {0:?}, expected {FORMAT:?}")]
    WrongFormat(String),
    /// `version` is not [`VERSION`].
    #[error("version is {0}, this build reads version {VERSION}")]
    WrongVersion(u32),
    /// More than [`MAX_RELATIONS`] rows.
    #[error("{0} relation rows, at most {MAX_RELATIONS} allowed")]
    TooManyRelations(usize),
    /// A row's key is empty after trimming.
    #[error("relation row {0} has an empty key")]
    EmptyKey(usize),
    /// A row's stiffness is NaN or infinite.
    #[error("relation {key:?}: stiffness {value} is not finite")]
    NotFinite {
        /// The row's key.
        key: String,
        /// The offending value.
        value: f32,
    },
    /// A row's stiffness lies outside [`STIFFNESS_MIN`]..=[`STIFFNESS_MAX`].
    #[error("relation {key:?}: stiffness {value} outside [{STIFFNESS_MIN}, {STIFFNESS_MAX}]")]
    OutOfBounds {
        /// The row's key.
        key: String,
        /// The offending value.
        value: f32,
    },
    /// Two rows match the same key (edge types compared after normalising).
    #[error("relation key {0:?} appears more than once")]
    DuplicateKey(String),
    /// A `uniform` table holds a factor other than 1.0.
    #[error("provenance is uniform but relation {key:?} has stiffness {value}")]
    NotUniform {
        /// The row's key.
        key: String,
        /// The offending value.
        value: f32,
    },
    /// A `learned` table was offered without [`LoadOptions::allow_learned`].
    #[error("learned table refused: allow_learned is false (ADR-2174 gate)")]
    LearnedNotAllowed,
    /// A `learned` table lacks a required provenance field.
    #[error("learned table is missing provenance field {0:?}")]
    MissingProvenance(&'static str),
    /// A `learned` table's commit is not a 40-character hex SHA.
    #[error("learned table commit {0:?} is not a 40-character hex SHA")]
    BadCommit(String),
    /// A `learned` table did not state its registry-weight policy.
    #[error("learned table must set composition.registry_weight explicitly")]
    ImplicitComposition,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTable {
    format: String,
    version: u32,
    provenance: RawProvenance,
    #[serde(default)]
    composition: RawComposition,
    #[serde(default, rename = "relation")]
    relations: Vec<RawRelation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvenance {
    source: ProvenanceSource,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    producer_crate: Option<String>,
    #[serde(default)]
    commit: Option<String>,
    #[serde(default)]
    preregistration_result: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawComposition {
    #[serde(default)]
    registry_weight: Option<RegistryWeight>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRelation {
    #[serde(rename = "match")]
    match_on: MatchOn,
    key: String,
    stiffness: f32,
}

/// Normalise an edge-type label for matching: trim, ASCII-lowercase, and map
/// `-` and spaces to `_`, so `depends-on`, `Depends On` and `depends_on` are
/// one key.
pub fn normalise_edge_type(label: &str) -> String {
    label
        .trim()
        .chars()
        .map(|c| match c {
            '-' | ' ' => '_',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// A validated per-relation stiffness table.
#[derive(Debug, Clone, PartialEq)]
pub struct RelationWeightTable {
    provenance: Provenance,
    registry_weight: RegistryWeight,
    by_iri: HashMap<String, f32>,
    by_edge_type: HashMap<String, f32>,
}

impl RelationWeightTable {
    /// Parse and validate a table from TOML text.
    ///
    /// Validation is all-or-nothing: any error refuses the whole table.
    ///
    /// # Errors
    ///
    /// Returns a [`RelationTableError`] naming the first problem found.
    pub fn parse(text: &str, options: LoadOptions) -> Result<Self, RelationTableError> {
        let raw: RawTable =
            toml::from_str(text).map_err(|e| RelationTableError::Malformed(e.to_string()))?;
        if raw.format != FORMAT {
            return Err(RelationTableError::WrongFormat(raw.format));
        }
        if raw.version != VERSION {
            return Err(RelationTableError::WrongVersion(raw.version));
        }
        if raw.relations.len() > MAX_RELATIONS {
            return Err(RelationTableError::TooManyRelations(raw.relations.len()));
        }

        let source = raw.provenance.source;
        if source == ProvenanceSource::Learned {
            if !options.allow_learned {
                return Err(RelationTableError::LearnedNotAllowed);
            }
            let present = |v: &Option<String>| v.as_deref().is_some_and(|s| !s.trim().is_empty());
            if !present(&raw.provenance.producer_crate) {
                return Err(RelationTableError::MissingProvenance("producer_crate"));
            }
            if !present(&raw.provenance.preregistration_result) {
                return Err(RelationTableError::MissingProvenance(
                    "preregistration_result",
                ));
            }
            let commit = raw
                .provenance
                .commit
                .as_deref()
                .ok_or(RelationTableError::MissingProvenance("commit"))?;
            if commit.len() != 40 || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(RelationTableError::BadCommit(commit.to_string()));
            }
            if raw.composition.registry_weight.is_none() {
                return Err(RelationTableError::ImplicitComposition);
            }
        }

        let mut by_iri = HashMap::new();
        let mut by_edge_type = HashMap::new();
        for (row, rel) in raw.relations.into_iter().enumerate() {
            let key = rel.key.trim();
            if key.is_empty() {
                return Err(RelationTableError::EmptyKey(row));
            }
            let value = rel.stiffness;
            if !value.is_finite() {
                return Err(RelationTableError::NotFinite {
                    key: key.to_string(),
                    value,
                });
            }
            if !(STIFFNESS_MIN..=STIFFNESS_MAX).contains(&value) {
                return Err(RelationTableError::OutOfBounds {
                    key: key.to_string(),
                    value,
                });
            }
            if source == ProvenanceSource::Uniform && value != 1.0 {
                return Err(RelationTableError::NotUniform {
                    key: key.to_string(),
                    value,
                });
            }
            let (map, map_key) = match rel.match_on {
                MatchOn::Iri => (&mut by_iri, key.to_string()),
                MatchOn::EdgeType => (&mut by_edge_type, normalise_edge_type(key)),
            };
            if map.insert(map_key.clone(), value).is_some() {
                return Err(RelationTableError::DuplicateKey(map_key));
            }
        }

        Ok(Self {
            provenance: Provenance {
                source,
                note: raw.provenance.note,
                producer_crate: raw.provenance.producer_crate,
                commit: raw.provenance.commit,
                preregistration_result: raw.provenance.preregistration_result,
            },
            registry_weight: raw.composition.registry_weight.unwrap_or_default(),
            by_iri,
            by_edge_type,
        })
    }

    /// A table with no rows: every edge resolves to the identity factor.
    pub fn uniform() -> Self {
        Self {
            provenance: Provenance {
                source: ProvenanceSource::Uniform,
                note: None,
                producer_crate: None,
                commit: None,
                preregistration_result: None,
            },
            registry_weight: RegistryWeight::Compose,
            by_iri: HashMap::new(),
            by_edge_type: HashMap::new(),
        }
    }

    /// The validated provenance block.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// How matched edges combine their existing weight with the factor.
    pub fn registry_weight(&self) -> RegistryWeight {
        self.registry_weight
    }

    /// Number of rows (IRI rows plus edge-type rows).
    pub fn len(&self) -> usize {
        self.by_iri.len() + self.by_edge_type.len()
    }

    /// True when the table has no rows.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Resolve an edge's factor: `owl_property_iri` first, then normalised
    /// `edge_type`, else the identity factor 1.0.
    pub fn resolve(&self, edge: &Edge) -> Resolution {
        if let Some(&factor) = edge
            .owl_property_iri
            .as_deref()
            .and_then(|iri| self.by_iri.get(iri))
        {
            return Resolution {
                factor,
                by: ResolvedBy::Iri,
            };
        }
        if let Some(&factor) = edge
            .edge_type
            .as_deref()
            .and_then(|t| self.by_edge_type.get(&normalise_edge_type(t)))
        {
            return Resolution {
                factor,
                by: ResolvedBy::EdgeType,
            };
        }
        Resolution {
            factor: 1.0,
            by: ResolvedBy::Identity,
        }
    }

    /// The force-pass weight for one edge.
    ///
    /// Identity edges, and factor-1.0 edges under [`RegistryWeight::Compose`],
    /// return `edge.weight` unchanged, bit for bit.
    pub fn spring_weight(&self, edge: &Edge) -> (f32, ResolvedBy) {
        let r = self.resolve(edge);
        let w = match (r.by, self.registry_weight) {
            (ResolvedBy::Identity, _) => edge.weight,
            (_, RegistryWeight::Ignore) => r.factor,
            (_, RegistryWeight::Compose) if r.factor == 1.0 => edge.weight,
            (_, RegistryWeight::Compose) => edge.weight * r.factor,
        };
        (w, r.by)
    }
}

/// Per-graph counts of how edges resolved, for the bind log line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResolutionStats {
    /// Graph edges (not CSR slots) matched by IRI.
    pub by_iri: usize,
    /// Graph edges matched by edge type.
    pub by_edge_type: usize,
    /// Graph edges that kept their weight (no row matched).
    pub identity: usize,
}

/// A force-pass CSR, built once for every GPU consumer.
#[derive(Debug, Clone, PartialEq)]
pub struct CsrGraph {
    /// `num_nodes + 1` offsets into the slot arrays.
    pub row_offsets: Vec<u32>,
    /// Neighbour index per slot. Each non-self-loop edge fills two slots.
    pub col_indices: Vec<u32>,
    /// Unscaled weight per slot: the buffer label propagation, degree, SSSP,
    /// Louvain and Leiden read.
    pub edge_weights: Vec<f32>,
    /// Per-slot force-pass weight when a table is bound, else `None`.
    pub spring_weights: Option<Vec<f32>>,
    /// How the graph's edges resolved, when a table is bound.
    pub stats: Option<ResolutionStats>,
}

impl CsrGraph {
    /// Neighbour indices of node `i`.
    pub fn neighbours(&self, i: usize) -> &[u32] {
        let (a, b) = (
            self.row_offsets[i] as usize,
            self.row_offsets[i + 1] as usize,
        );
        &self.col_indices[a..b]
    }

    /// Number of CSR slots.
    pub fn num_slots(&self) -> usize {
        self.col_indices.len()
    }
}

/// Build the symmetric force-pass CSR from `edges`.
///
/// Slot order is the order the force actor has always used: per node, edges
/// in input order, each non-self-loop edge pushed into its source row and
/// then mirrored into its target row. Edges whose endpoints are not in
/// `node_indices` are skipped.
pub fn build_csr(
    edges: &[Edge],
    node_indices: &HashMap<u32, usize>,
    num_nodes: usize,
    table: Option<&RelationWeightTable>,
) -> CsrGraph {
    // (neighbour, unscaled weight, spring weight)
    let mut adjacency: Vec<Vec<(u32, f32, f32)>> = vec![Vec::new(); num_nodes];
    let mut stats = ResolutionStats::default();
    for edge in edges {
        let (Some(&src), Some(&tgt)) = (
            node_indices.get(&edge.source),
            node_indices.get(&edge.target),
        ) else {
            continue;
        };
        let spring = match table {
            Some(t) => {
                let (w, by) = t.spring_weight(edge);
                match by {
                    ResolvedBy::Iri => stats.by_iri += 1,
                    ResolvedBy::EdgeType => stats.by_edge_type += 1,
                    ResolvedBy::Identity => stats.identity += 1,
                }
                w
            }
            None => edge.weight,
        };
        adjacency[src].push((tgt as u32, edge.weight, spring));
        if src != tgt {
            adjacency[tgt].push((src as u32, edge.weight, spring));
        }
    }

    let total: usize = adjacency.iter().map(Vec::len).sum();
    let mut row_offsets = Vec::with_capacity(num_nodes + 1);
    let mut col_indices = Vec::with_capacity(total);
    let mut edge_weights = Vec::with_capacity(total);
    let mut spring_weights = table.map(|_| Vec::with_capacity(total));
    for adj in &adjacency {
        row_offsets.push(col_indices.len() as u32);
        for &(target, weight, spring) in adj {
            col_indices.push(target);
            edge_weights.push(weight);
            if let Some(s) = spring_weights.as_mut() {
                s.push(spring);
            }
        }
    }
    row_offsets.push(col_indices.len() as u32);

    CsrGraph {
        row_offsets,
        col_indices,
        edge_weights,
        spring_weights,
        stats: table.map(|_| stats),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUBCLASS: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
    const RELATED_TO: &str = "https://narrativegoldmine.com/ns/v1#relatedTo";
    const RELATES_TO: &str = "https://narrativegoldmine.com/ns/v1#relatesTo";
    const REQUIRES: &str = "https://narrativegoldmine.com/ns/v1#requires";
    const ENABLES: &str = "https://narrativegoldmine.com/ns/v1#enables";
    const DEPENDS_ON: &str = "https://narrativegoldmine.com/ns/v1#dependsOn";

    fn edge(s: u32, t: u32, w: f32, ty: Option<&str>, iri: Option<&str>) -> Edge {
        let mut e = Edge::new(s, t, w);
        e.edge_type = ty.map(str::to_string);
        e.owl_property_iri = iri.map(str::to_string);
        e
    }

    fn table(body: &str) -> RelationWeightTable {
        try_table(body, LoadOptions::default()).unwrap()
    }

    fn try_table(body: &str, opts: LoadOptions) -> Result<RelationWeightTable, RelationTableError> {
        let text = format!("format = \"{FORMAT}\"\nversion = 1\n{body}");
        RelationWeightTable::parse(&text, opts)
    }

    fn rows(rows: &[(&str, &str, f32)]) -> String {
        rows.iter()
            .map(|(m, k, s)| {
                format!("[[relation]]\nmatch = \"{m}\"\nkey = \"{k}\"\nstiffness = {s:?}\n")
            })
            .collect()
    }

    fn hand_set(r: &[(&str, &str, f32)]) -> RelationWeightTable {
        table(&format!("[provenance]\nsource = \"hand-set\"\n{}", rows(r)))
    }

    // ---- resolver -------------------------------------------------------

    #[test]
    fn relates_to_and_related_to_share_a_coarse_type_but_resolve_apart() {
        let t = hand_set(&[("iri", RELATED_TO, 0.5), ("iri", RELATES_TO, 2.0)]);
        let a = edge(1, 2, 1.0, Some("associative"), Some(RELATED_TO));
        let b = edge(1, 2, 1.0, Some("associative"), Some(RELATES_TO));
        assert_eq!(
            t.resolve(&a),
            Resolution {
                factor: 0.5,
                by: ResolvedBy::Iri
            }
        );
        assert_eq!(
            t.resolve(&b),
            Resolution {
                factor: 2.0,
                by: ResolvedBy::Iri
            }
        );
    }

    #[test]
    fn the_three_dependency_relations_resolve_apart_by_iri() {
        let t = hand_set(&[
            ("iri", REQUIRES, 1.5),
            ("iri", ENABLES, 0.75),
            ("iri", DEPENDS_ON, 3.0),
        ]);
        let got: Vec<f32> = [REQUIRES, ENABLES, DEPENDS_ON]
            .iter()
            .map(|iri| {
                t.resolve(&edge(1, 2, 1.0, Some("dependency"), Some(iri)))
                    .factor
            })
            .collect();
        assert_eq!(got, vec![1.5, 0.75, 3.0]);
    }

    #[test]
    fn an_iri_missing_from_the_table_falls_back_to_the_coarse_type() {
        let t = hand_set(&[("iri", REQUIRES, 1.5), ("edge_type", "dependency", 0.5)]);
        let e = edge(1, 2, 1.0, Some("dependency"), Some(ENABLES));
        assert_eq!(
            t.resolve(&e),
            Resolution {
                factor: 0.5,
                by: ResolvedBy::EdgeType
            }
        );
    }

    #[test]
    fn an_iri_row_wins_over_an_edge_type_row() {
        let t = hand_set(&[("iri", REQUIRES, 1.5), ("edge_type", "dependency", 0.5)]);
        let e = edge(1, 2, 1.0, Some("dependency"), Some(REQUIRES));
        assert_eq!(t.resolve(&e).by, ResolvedBy::Iri);
    }

    #[test]
    fn ontology_snake_case_types_match_after_normalising() {
        let t = hand_set(&[
            ("edge_type", "depends-on", 2.0),
            ("edge_type", "is_subclass_of", 3.0),
        ]);
        assert_eq!(
            t.resolve(&edge(1, 2, 1.0, Some("depends_on"), None)).factor,
            2.0
        );
        assert_eq!(
            t.resolve(&edge(1, 2, 1.0, Some("Is_SubClass_Of"), None))
                .factor,
            3.0
        );
    }

    #[test]
    fn untyped_and_unknown_edges_resolve_to_identity() {
        let t = hand_set(&[("iri", REQUIRES, 2.0), ("edge_type", "dependency", 2.0)]);
        let identity = Resolution {
            factor: 1.0,
            by: ResolvedBy::Identity,
        };
        assert_eq!(t.resolve(&edge(1, 2, 1.0, None, None)), identity);
        assert_eq!(
            t.resolve(&edge(1, 2, 1.0, Some("explicit_link"), None)),
            identity
        );
        assert_eq!(t.resolve(&edge(1, 2, 1.0, None, Some(SUBCLASS))), identity);
    }

    // ---- validation -----------------------------------------------------

    #[test]
    fn bounds_are_inclusive_and_enforced() {
        assert!(try_table(
            &format!(
                "[provenance]\nsource = \"hand-set\"\n{}",
                rows(&[("iri", "a", 0.0), ("iri", "b", 4.0)])
            ),
            LoadOptions::default()
        )
        .is_ok());
        for bad in [-0.01f32, 4.01, 100.0] {
            let err = try_table(
                &format!(
                    "[provenance]\nsource = \"hand-set\"\n{}",
                    rows(&[("iri", "a", bad)])
                ),
                LoadOptions::default(),
            )
            .unwrap_err();
            assert!(
                matches!(err, RelationTableError::OutOfBounds { .. }),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn non_finite_stiffness_is_refused() {
        for bad in ["nan", "inf", "-inf"] {
            let body = format!(
                "[provenance]\nsource = \"hand-set\"\n[[relation]]\nmatch = \"iri\"\nkey = \"a\"\nstiffness = {bad}\n"
            );
            let err = try_table(&body, LoadOptions::default()).unwrap_err();
            assert!(
                matches!(err, RelationTableError::NotFinite { .. }),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn duplicate_keys_are_refused_including_after_normalising() {
        let dup_iri = try_table(
            &format!(
                "[provenance]\nsource = \"hand-set\"\n{}",
                rows(&[("iri", "a", 1.0), ("iri", "a", 2.0)])
            ),
            LoadOptions::default(),
        );
        assert_eq!(
            dup_iri.unwrap_err(),
            RelationTableError::DuplicateKey("a".into())
        );
        let dup_type = try_table(
            &format!(
                "[provenance]\nsource = \"hand-set\"\n{}",
                rows(&[
                    ("edge_type", "depends-on", 1.0),
                    ("edge_type", "depends_on", 2.0)
                ])
            ),
            LoadOptions::default(),
        );
        assert_eq!(
            dup_type.unwrap_err(),
            RelationTableError::DuplicateKey("depends_on".into())
        );
    }

    #[test]
    fn the_same_text_as_iri_and_edge_type_is_not_a_duplicate() {
        let t = hand_set(&[("iri", "requires", 1.0), ("edge_type", "requires", 2.0)]);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn format_version_unknown_keys_and_garbage_are_refused() {
        let wrong_format = RelationWeightTable::parse(
            "format = \"other\"\nversion = 1\n[provenance]\nsource = \"hand-set\"\n",
            LoadOptions::default(),
        );
        assert!(matches!(
            wrong_format,
            Err(RelationTableError::WrongFormat(_))
        ));
        let wrong_version = RelationWeightTable::parse(
            &format!("format = \"{FORMAT}\"\nversion = 2\n[provenance]\nsource = \"hand-set\"\n"),
            LoadOptions::default(),
        );
        assert_eq!(wrong_version, Err(RelationTableError::WrongVersion(2)));
        let unknown = try_table(
            "[provenance]\nsource = \"hand-set\"\nspring_k = 3\n",
            LoadOptions::default(),
        );
        assert!(matches!(unknown, Err(RelationTableError::Malformed(_))));
        let garbage = RelationWeightTable::parse("{not toml", LoadOptions::default());
        assert!(matches!(garbage, Err(RelationTableError::Malformed(_))));
        let bad_source = try_table(
            "[provenance]\nsource = \"guessed\"\n",
            LoadOptions::default(),
        );
        assert!(matches!(bad_source, Err(RelationTableError::Malformed(_))));
    }

    #[test]
    fn too_many_rows_and_empty_keys_are_refused() {
        let many: Vec<(String, f32)> = (0..=MAX_RELATIONS)
            .map(|i| (format!("k{i}"), 1.0))
            .collect();
        let body: String = many
            .iter()
            .map(|(k, s)| {
                format!("[[relation]]\nmatch = \"iri\"\nkey = \"{k}\"\nstiffness = {s:?}\n")
            })
            .collect();
        let err = try_table(
            &format!("[provenance]\nsource = \"hand-set\"\n{body}"),
            LoadOptions::default(),
        );
        assert_eq!(
            err.unwrap_err(),
            RelationTableError::TooManyRelations(MAX_RELATIONS + 1)
        );
        let empty = try_table(
            &format!(
                "[provenance]\nsource = \"hand-set\"\n{}",
                rows(&[("iri", "  ", 1.0)])
            ),
            LoadOptions::default(),
        );
        assert_eq!(empty.unwrap_err(), RelationTableError::EmptyKey(0));
    }

    #[test]
    fn a_uniform_table_must_hold_only_ones() {
        let ok = try_table(
            &format!(
                "[provenance]\nsource = \"uniform\"\n{}",
                rows(&[("iri", REQUIRES, 1.0)])
            ),
            LoadOptions::default(),
        );
        assert!(ok.is_ok());
        let bad = try_table(
            &format!(
                "[provenance]\nsource = \"uniform\"\n{}",
                rows(&[("iri", REQUIRES, 1.5)])
            ),
            LoadOptions::default(),
        );
        assert!(matches!(bad, Err(RelationTableError::NotUniform { .. })));
    }

    const LEARNED_OK: &str =
        "[provenance]\nsource = \"learned\"\nproducer_crate = \"test-producer\"\n\
        commit = \"0123456789abcdef0123456789abcdef01234567\"\n\
        preregistration_result = \"test-fixture#none\"\n\
        [composition]\nregistry_weight = \"ignore\"\n";

    #[test]
    fn a_learned_table_is_refused_unless_allowed() {
        assert_eq!(
            try_table(LEARNED_OK, LoadOptions::default()).unwrap_err(),
            RelationTableError::LearnedNotAllowed
        );
        let t = try_table(
            LEARNED_OK,
            LoadOptions {
                allow_learned: true,
            },
        )
        .unwrap();
        assert_eq!(t.provenance().source, ProvenanceSource::Learned);
        assert_eq!(t.registry_weight(), RegistryWeight::Ignore);
    }

    #[test]
    fn a_learned_table_needs_full_provenance_and_an_explicit_composition() {
        let allow = LoadOptions {
            allow_learned: true,
        };
        let without = |field: &str| {
            let body: String = LEARNED_OK
                .lines()
                .filter(|l| !l.starts_with(field))
                .map(|l| format!("{l}\n"))
                .collect();
            try_table(&body, allow).unwrap_err()
        };
        assert_eq!(
            without("producer_crate"),
            RelationTableError::MissingProvenance("producer_crate")
        );
        assert_eq!(
            without("commit"),
            RelationTableError::MissingProvenance("commit")
        );
        assert_eq!(
            without("preregistration_result"),
            RelationTableError::MissingProvenance("preregistration_result")
        );
        assert_eq!(
            without("registry_weight"),
            RelationTableError::ImplicitComposition
        );
        let short = LEARNED_OK.replace("0123456789abcdef0123456789abcdef01234567", "abc123");
        assert!(matches!(
            try_table(&short, allow),
            Err(RelationTableError::BadCommit(_))
        ));
    }

    // ---- weights and CSR ------------------------------------------------

    /// Today's force-actor CSR loop, kept verbatim as the reference.
    fn legacy_csr(
        edges: &[Edge],
        idx: &HashMap<u32, usize>,
        n: usize,
    ) -> (Vec<u32>, Vec<u32>, Vec<f32>) {
        let mut adjacency_lists: Vec<Vec<(u32, f32)>> = vec![Vec::new(); n];
        for edge in edges {
            if let (Some(&src), Some(&tgt)) = (idx.get(&edge.source), idx.get(&edge.target)) {
                adjacency_lists[src].push((tgt as u32, edge.weight));
                if src != tgt {
                    adjacency_lists[tgt].push((src as u32, edge.weight));
                }
            }
        }
        let mut row_offsets = vec![0u32; n + 1];
        let mut col_indices = Vec::new();
        let mut edge_weights = Vec::new();
        let mut edge_count = 0u32;
        for (i, adj) in adjacency_lists.iter().enumerate() {
            row_offsets[i] = edge_count;
            for &(target, weight) in adj {
                col_indices.push(target);
                edge_weights.push(weight);
                edge_count += 1;
            }
        }
        row_offsets[n] = edge_count;
        (row_offsets, col_indices, edge_weights)
    }

    /// A mixed edge set: registry-style typed knowledge edges (`strength * 2`),
    /// collapsed relations, ontology snake_case types, wikilinks, domain
    /// membership, agent edges, a self-loop, a dangling edge, and awkward
    /// float values (-0.0, a subnormal, a large value, a signalling NaN).
    fn mixed() -> (Vec<Edge>, HashMap<u32, usize>, usize) {
        let snan = f32::from_bits(0x7f80_0001);
        let edges = vec![
            edge(1, 2, 0.9 * 2.0, Some("hierarchical"), Some(SUBCLASS)),
            edge(2, 3, 0.7 * 2.0, Some("dependency"), Some(REQUIRES)),
            edge(3, 4, 0.7 * 2.0, Some("dependency"), Some(ENABLES)),
            edge(4, 5, 0.7 * 2.0, Some("dependency"), Some(DEPENDS_ON)),
            edge(5, 6, 0.3 * 2.0, Some("associative"), Some(RELATED_TO)),
            edge(6, 1, 0.3 * 2.0, Some("associative"), Some(RELATES_TO)),
            edge(7, 8, 1.0, Some("is_subclass_of"), None),
            edge(8, 9, 1.0, Some("depends_on"), None),
            edge(9, 1, 1.0, Some("explicit_link"), None),
            edge(10, 2, 1.5, Some("domain_member"), None),
            edge(11, 3, 0.25, Some("agent_collaboration"), None),
            edge(4, 4, 1.0, Some("hierarchical"), Some(SUBCLASS)),
            edge(5, 999, 1.0, Some("dependency"), Some(REQUIRES)),
            edge(1, 7, -0.0, None, None),
            edge(2, 8, f32::from_bits(1), None, None),
            edge(3, 9, 3.0e38, None, None),
            edge(6, 10, snan, Some("explicit_link"), None),
            edge(7, 11, 0.1, None, Some("https://example.org/unknown#rel")),
        ];
        let idx: HashMap<u32, usize> = (1..=11u32).map(|id| (id, (id - 1) as usize)).collect();
        (edges, idx, 11)
    }

    fn bits(v: &[f32]) -> Vec<u32> {
        v.iter().map(|x| x.to_bits()).collect()
    }

    #[test]
    fn with_no_table_the_csr_is_bit_identical_to_the_legacy_loop_and_has_no_spring_buffer() {
        let (edges, idx, n) = mixed();
        let (ro, ci, ew) = legacy_csr(&edges, &idx, n);
        let csr = build_csr(&edges, &idx, n, None);
        assert_eq!(csr.row_offsets, ro);
        assert_eq!(csr.col_indices, ci);
        assert_eq!(bits(&csr.edge_weights), bits(&ew));
        assert_eq!(csr.spring_weights, None);
        assert_eq!(csr.stats, None);
    }

    #[test]
    fn a_uniform_table_gives_spring_weights_bit_identical_to_the_legacy_weights() {
        let (edges, idx, n) = mixed();
        let (_, _, ew) = legacy_csr(&edges, &idx, n);
        let empty = RelationWeightTable::uniform();
        let explicit = try_table(
            &format!(
                "[provenance]\nsource = \"uniform\"\n{}",
                rows(&[
                    ("iri", SUBCLASS, 1.0),
                    ("iri", REQUIRES, 1.0),
                    ("iri", RELATED_TO, 1.0),
                    ("edge_type", "explicit_link", 1.0),
                    ("edge_type", "depends_on", 1.0),
                ])
            ),
            LoadOptions::default(),
        )
        .unwrap();
        for t in [&empty, &explicit] {
            let csr = build_csr(&edges, &idx, n, Some(t));
            assert_eq!(bits(&csr.edge_weights), bits(&ew));
            assert_eq!(bits(csr.spring_weights.as_ref().unwrap()), bits(&ew));
        }
    }

    #[test]
    fn a_hand_set_table_scales_only_spring_weights_and_only_matched_relations() {
        let (edges, idx, n) = mixed();
        let (_, _, ew) = legacy_csr(&edges, &idx, n);
        let t = hand_set(&[("iri", SUBCLASS, 4.0), ("iri", RELATED_TO, 0.25)]);
        let csr = build_csr(&edges, &idx, n, Some(&t));
        assert_eq!(
            bits(&csr.edge_weights),
            bits(&ew),
            "unscaled buffer must not move"
        );
        let spring = csr.spring_weights.unwrap();
        let mut scaled = 0;
        for i in 0..csr.col_indices.len() {
            if spring[i].to_bits() != ew[i].to_bits() {
                scaled += 1;
            }
        }
        // subClassOf 1->2 (2 slots) + self-loop 4->4 (1 slot) + relatedTo 5->6 (2 slots)
        assert_eq!(scaled, 5);
        // slot 0 of node 0 (id 1) is the 1->2 subClassOf edge
        assert_eq!(spring[0], 0.9 * 2.0 * 4.0);
        let stats = csr.stats.unwrap();
        assert_eq!(
            stats,
            ResolutionStats {
                by_iri: 3,
                by_edge_type: 0,
                identity: 14
            }
        );
    }

    #[test]
    fn ignore_mode_replaces_the_weight_of_matched_edges_only() {
        let t = table(&format!(
            "[provenance]\nsource = \"hand-set\"\n[composition]\nregistry_weight = \"ignore\"\n{}",
            rows(&[("iri", SUBCLASS, 3.0), ("edge_type", "dependency", 1.0)])
        ));
        let is_a = edge(1, 2, 1.8, Some("hierarchical"), Some(SUBCLASS));
        let dep = edge(1, 2, 1.4, Some("dependency"), Some(REQUIRES));
        let link = edge(1, 2, 1.0, Some("explicit_link"), None);
        assert_eq!(t.spring_weight(&is_a), (3.0, ResolvedBy::Iri));
        assert_eq!(t.spring_weight(&dep), (1.0, ResolvedBy::EdgeType));
        assert_eq!(t.spring_weight(&link), (1.0, ResolvedBy::Identity));
        let odd = edge(1, 2, 0.6, None, None);
        assert_eq!(t.spring_weight(&odd).0.to_bits(), 0.6f32.to_bits());
    }

    #[test]
    fn neighbours_match_the_legacy_adjacency() {
        let (edges, idx, n) = mixed();
        let (ro, ci, _) = legacy_csr(&edges, &idx, n);
        let csr = build_csr(&edges, &idx, n, None);
        for i in 0..n {
            assert_eq!(
                csr.neighbours(i),
                &ci[ro[i] as usize..ro[i + 1] as usize],
                "node {i}"
            );
        }
        // id 4 (index 3): mirror of 3->4, then 4->5, then the self-loop once.
        assert_eq!(csr.neighbours(3), &[2, 4, 3]);
        assert_eq!(csr.num_slots(), csr.edge_weights.len());
    }

    #[test]
    fn the_shipped_example_tables_parse() {
        let hand = include_str!("../../../../docs/relation-weights/hand-set-two-relations.toml");
        let t = RelationWeightTable::parse(hand, LoadOptions::default()).unwrap();
        assert_eq!(t.provenance().source, ProvenanceSource::HandSet);
        assert_eq!(t.len(), 2);
        let uni = include_str!("../../../../docs/relation-weights/uniform.toml");
        let u = RelationWeightTable::parse(uni, LoadOptions::default()).unwrap();
        assert_eq!(u.provenance().source, ProvenanceSource::Uniform);
    }
}
