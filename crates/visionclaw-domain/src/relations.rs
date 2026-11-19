//! The relation schema (ADR-2204 D1): the one definition of what a relation
//! is, loaded from `crates/visionclaw-domain/data/relations.yaml`.
//!
//! Every producer, store and client that needs a relation's label, predicate
//! mapping, weight, colour or the co-citation fan-out bound asks this module
//! instead of keeping its own table. The YAML is embedded at compile time
//! ([`RelationSchema::builtin`]) and validated on first use; the same document
//! projects to JSON ([`RelationSchema::projection`]) for the clients, pinned
//! to the YAML by a producer/consumer fixture test.
//!
//! Slice S2 is value-preserving: [`Relation::legacy_weight`] holds the weight
//! each producer gave an edge before the single weighting stage of S3.
//!
//! # Example
//!
//! ```
//! use visionclaw_domain::relations::{PredicateClass, RelationSchema};
//!
//! let schema = RelationSchema::builtin();
//! let subclass = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
//! match schema.classify(subclass) {
//!     PredicateClass::Relation(r) => assert_eq!(r.label, "hierarchical"),
//!     other => panic!("unexpected {other:?}"),
//! }
//! assert!(matches!(
//!     schema.classify("https://example.org/unknown"),
//!     PredicateClass::Unknown
//! ));
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// The embedded schema document (`crates/visionclaw-domain/data/relations.yaml`).
pub const RELATIONS_YAML: &str = include_str!("../data/relations.yaml");

/// The longest legacy weight accepted. Legacy weights were registry strength
/// (≤ 0.9) × 2, so 1.8 is reachable; the schema allows up to 2.
const MAX_LEGACY_WEIGHT: f32 = 2.0;

/// What kind of relation a label is (ADR-2204 D1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationKind {
    /// Subsumption / taxonomy: parent and child.
    Hierarchy,
    /// A named predicate between two pages.
    Typed,
    /// An authored link (a wikilink or provenance bridge).
    Link,
    /// Derived by the system: spokes, co-citation, inferred edges.
    Derived,
}

/// One relation as declared in `crates/visionclaw-domain/data/relations.yaml`, predicates expanded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    /// The label stored on an edge and sent on the wire.
    pub label: String,
    /// The relation's kind.
    pub kind: RelationKind,
    /// Full predicate IRIs that produce this relation (CURIEs expanded on
    /// load).
    #[serde(default)]
    pub predicates: Vec<String>,
    /// Conventional frontmatter keys that produce it. Documentation of the
    /// corpus vocabulary, which lives outside this repository.
    #[serde(default)]
    pub frontmatter_keys: Vec<String>,
    /// Extra spellings the clients resolve to this relation's colour.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Forward predicate → its inverse predicate, both of this relation.
    #[serde(default)]
    pub inverse_predicates: BTreeMap<String, String>,
    /// The label the inferred-edge materialiser still stamps on this relation
    /// (a known mislabel, corrected by a later slice).
    #[serde(default)]
    pub stamped_label: Option<String>,
    /// The weight a producer gave this relation's edges before ADR-2204 S3.
    #[serde(default)]
    pub legacy_weight: Option<f32>,
    /// Evidence-mode affinity `a ∈ (0,1)`: a versioned policy value (the pre-S3 registry
    /// strength), not a calibrated probability and not tuned (ADR-2204 D1/D3). Required for a
    /// relation that takes part in physics in the builtin schema; absent means the relation
    /// contributes no evidence.
    #[serde(default)]
    pub affinity: Option<f32>,
    /// Why `legacy_weight` has that value.
    #[serde(default)]
    pub legacy_weight_note: Option<String>,
    /// Whether the relation takes part in physics.
    pub physics: bool,
    /// Whether the relation is drawn by default.
    pub drawn: bool,
    /// Colour as `#RRGGBB`, when the relation has one of its own.
    #[serde(default)]
    pub colour: Option<String>,
}

/// The co-citation fan-out bound declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FanoutThreshold {
    /// Environment variable that overrides `default`.
    pub env: String,
    /// Value when the variable is unset, unparsable or below `min`.
    pub default: usize,
    /// Smallest accepted override.
    pub min: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSchema {
    version: u32,
    prefixes: BTreeMap<String, String>,
    default_colour: String,
    fanout_threshold: FanoutThreshold,
    #[serde(default)]
    ignored_predicates: Vec<String>,
    relations: Vec<Relation>,
}

/// Why a schema document was rejected.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SchemaError {
    /// The document is not valid YAML for the schema shape.
    #[error("relations schema does not parse: {0}")]
    Parse(String),
    /// Two relations share a label.
    #[error("duplicate relation label `{0}`")]
    DuplicateLabel(String),
    /// One predicate maps to more than one relation (or is also ignored).
    #[error("predicate `{predicate}` is claimed by `{first}` and `{second}`")]
    DuplicatePredicate {
        /// The expanded predicate IRI.
        predicate: String,
        /// First claimant.
        first: String,
        /// Second claimant.
        second: String,
    },
    /// A CURIE uses a prefix the document does not declare.
    #[error("unknown prefix in `{0}`")]
    UnknownPrefix(String),
    /// A weight is non-finite, not positive, or above the accepted maximum.
    #[error("relation `{label}` has an out-of-range legacy weight {weight}")]
    WeightOutOfRange {
        /// The relation.
        label: String,
        /// The offending value.
        weight: f32,
    },
    /// A relation that produces edges has no weight.
    #[error("relation `{0}` is a producer but declares no legacy_weight")]
    MissingWeight(String),
    /// An affinity is not strictly inside `(0, 1)`.
    #[error("relation `{label}` has an affinity {affinity} outside (0, 1)")]
    AffinityOutOfRange {
        /// The relation.
        label: String,
        /// The offending value.
        affinity: f32,
    },
    /// A colour is not `#RRGGBB`.
    #[error("`{0}` is not a #RRGGBB colour")]
    BadColour(String),
    /// A colour key (label or alias) is claimed twice.
    #[error("colour key `{0}` is claimed twice")]
    DuplicateColourKey(String),
    /// An inverse pair names a predicate the relation does not own.
    #[error("relation `{label}` declares an inverse involving foreign predicate `{predicate}`")]
    ForeignInverse {
        /// The relation.
        label: String,
        /// The predicate it does not own.
        predicate: String,
    },
    /// A label the sync needs for a non-predicate producer is absent.
    #[error("required producer relation `{0}` is missing")]
    MissingProducer(String),
    /// The fan-out declaration is unusable.
    #[error("fanout_threshold default {default} is below its min {min}")]
    BadFanout {
        /// Declared default.
        default: usize,
        /// Declared minimum.
        min: usize,
    },
    /// Version is not the one this loader understands.
    #[error("unsupported relations schema version {0}")]
    Version(u32),
}

/// What a predicate IRI means to the schema.
#[derive(Debug, Clone, Copy)]
pub enum PredicateClass<'a> {
    /// Produces an edge with this relation.
    Relation(&'a Relation),
    /// Known, deliberately no edge.
    Ignored,
    /// Not in the schema: quarantined by the caller, never given defaults.
    Unknown,
}

/// Non-predicate edge producers whose weight the sync reads from the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Producer {
    /// An authored `[[wikilink]]` edge.
    Wikilink,
    /// The `elevatedFrom` provenance bridge.
    ElevatedFrom,
    /// A synthetic domain root's membership spoke.
    DomainSpoke,
    /// A reasoner-entailed subclass edge.
    Inferred,
    /// One shared dangling target between two pages.
    CoCitation,
    /// An agent-to-agent sidechain payment edge.
    ChainPayment,
}

impl Producer {
    /// The schema label this producer's weight is declared under.
    pub const fn label(self) -> &'static str {
        match self {
            Producer::Wikilink => "explicit_link",
            Producer::ElevatedFrom => "elevated_from",
            Producer::DomainSpoke => "domain_member",
            Producer::Inferred => "inferred",
            Producer::CoCitation => "co_citation",
            Producer::ChainPayment => "chain_payment",
        }
    }

    const ALL: [Producer; 6] = [
        Producer::Wikilink,
        Producer::ElevatedFrom,
        Producer::DomainSpoke,
        Producer::Inferred,
        Producer::CoCitation,
        Producer::ChainPayment,
    ];
}

/// A validated relation schema.
#[derive(Debug, Clone)]
pub struct RelationSchema {
    version: u32,
    prefixes: BTreeMap<String, String>,
    default_colour: String,
    fanout: FanoutThreshold,
    ignored: HashSet<String>,
    relations: Vec<Relation>,
    by_label: HashMap<String, usize>,
    by_predicate: HashMap<String, usize>,
}

impl RelationSchema {
    /// Parse and validate a schema document.
    ///
    /// # Errors
    ///
    /// Returns a [`SchemaError`] naming the first violated rule: duplicate
    /// labels or predicates, unknown prefixes, weights outside `(0, 2]`,
    /// malformed colours, foreign inverse pairs, a producer without a weight,
    /// or a missing non-predicate producer relation.
    pub fn from_yaml(text: &str) -> Result<Self, SchemaError> {
        let raw: RawSchema =
            serde_yaml::from_str(text).map_err(|e| SchemaError::Parse(e.to_string()))?;
        if raw.version != 1 {
            return Err(SchemaError::Version(raw.version));
        }
        if raw.fanout_threshold.default < raw.fanout_threshold.min {
            return Err(SchemaError::BadFanout {
                default: raw.fanout_threshold.default,
                min: raw.fanout_threshold.min,
            });
        }
        check_colour(&raw.default_colour)?;
        let expand = |curie: &str| expand_curie(&raw.prefixes, curie);

        let mut relations = raw.relations;
        let mut by_label = HashMap::new();
        let mut by_predicate: HashMap<String, usize> = HashMap::new();
        let mut colour_keys: HashSet<String> = HashSet::new();
        let ignored: HashSet<String> = raw
            .ignored_predicates
            .iter()
            .map(|c| expand(c))
            .collect::<Result<_, _>>()?;

        for (i, rel) in relations.iter_mut().enumerate() {
            if by_label.insert(rel.label.clone(), i).is_some() {
                return Err(SchemaError::DuplicateLabel(rel.label.clone()));
            }
            rel.predicates = rel
                .predicates
                .iter()
                .map(|c| expand(c))
                .collect::<Result<_, _>>()?;
            for p in &rel.predicates {
                if ignored.contains(p) {
                    return Err(SchemaError::DuplicatePredicate {
                        predicate: p.clone(),
                        first: "ignored_predicates".into(),
                        second: rel.label.clone(),
                    });
                }
                if let Some(&j) = by_predicate.get(p) {
                    return Err(SchemaError::DuplicatePredicate {
                        predicate: p.clone(),
                        first: by_label_name(&by_label, j),
                        second: rel.label.clone(),
                    });
                }
                by_predicate.insert(p.clone(), i);
            }
            let mut inverses = BTreeMap::new();
            for (a, b) in &rel.inverse_predicates {
                let (a, b) = (expand(a)?, expand(b)?);
                for side in [&a, &b] {
                    if !rel.predicates.contains(side) {
                        return Err(SchemaError::ForeignInverse {
                            label: rel.label.clone(),
                            predicate: side.clone(),
                        });
                    }
                }
                inverses.insert(a, b);
            }
            rel.inverse_predicates = inverses;
            if let Some(w) = rel.legacy_weight {
                if !(w.is_finite() && w > 0.0 && w <= MAX_LEGACY_WEIGHT) {
                    return Err(SchemaError::WeightOutOfRange {
                        label: rel.label.clone(),
                        weight: w,
                    });
                }
            } else if !rel.predicates.is_empty() {
                return Err(SchemaError::MissingWeight(rel.label.clone()));
            }
            if let Some(a) = rel.affinity {
                if !(a.is_finite() && a > 0.0 && a < 1.0) {
                    return Err(SchemaError::AffinityOutOfRange {
                        label: rel.label.clone(),
                        affinity: a,
                    });
                }
            }
            if let Some(c) = &rel.colour {
                check_colour(c)?;
                for key in std::iter::once(&rel.label).chain(rel.aliases.iter()) {
                    if !colour_keys.insert(key.clone()) {
                        return Err(SchemaError::DuplicateColourKey(key.clone()));
                    }
                }
            }
        }

        for producer in Producer::ALL {
            let rel = by_label
                .get(producer.label())
                .map(|&i| &relations[i])
                .ok_or_else(|| SchemaError::MissingProducer(producer.label().to_string()))?;
            if rel.legacy_weight.is_none() {
                return Err(SchemaError::MissingWeight(rel.label.clone()));
            }
        }

        Ok(Self {
            version: raw.version,
            prefixes: raw.prefixes,
            default_colour: raw.default_colour,
            fanout: raw.fanout_threshold,
            ignored,
            relations,
            by_label,
            by_predicate,
        })
    }

    /// The schema embedded in this build, parsed once.
    ///
    /// # Panics
    ///
    /// Panics if the embedded `crates/visionclaw-domain/data/relations.yaml` is invalid. That is a
    /// build-time defect, covered by the `builtin_schema_is_valid` test.
    pub fn builtin() -> &'static RelationSchema {
        static SCHEMA: OnceLock<RelationSchema> = OnceLock::new();
        SCHEMA.get_or_init(|| {
            RelationSchema::from_yaml(RELATIONS_YAML).unwrap_or_else(|e| {
                panic!("embedded crates/visionclaw-domain/data/relations.yaml is invalid: {e}")
            })
        })
    }

    /// Every relation, in declaration order.
    pub fn relations(&self) -> &[Relation] {
        &self.relations
    }

    /// The relation carrying `label`, if declared.
    pub fn relation(&self, label: &str) -> Option<&Relation> {
        self.by_label.get(label).map(|&i| &self.relations[i])
    }

    /// Classify a fully expanded predicate IRI.
    pub fn classify(&self, iri: &str) -> PredicateClass<'_> {
        if let Some(&i) = self.by_predicate.get(iri) {
            PredicateClass::Relation(&self.relations[i])
        } else if self.ignored.contains(iri) {
            PredicateClass::Ignored
        } else {
            PredicateClass::Unknown
        }
    }

    /// The pre-S3 weight of a non-predicate producer. Infallible: the loader
    /// rejects a schema that lacks any [`Producer`].
    pub fn producer_weight(&self, producer: Producer) -> f32 {
        self.relation(producer.label())
            .and_then(|r| r.legacy_weight)
            .unwrap_or_else(|| unreachable!("loader guarantees producer {producer:?}"))
    }

    /// The co-citation fan-out bound: the schema default, or the environment
    /// override when it parses and is at least the declared minimum.
    pub fn fanout_threshold(&self) -> usize {
        self.fanout_from(std::env::var(&self.fanout.env).ok().as_deref())
    }

    /// Whether a shared target with `referrers` referring pages makes co-citation: at most
    /// the threshold does, more makes none (ADR-2204 D3). The one boundary every site uses.
    pub fn admits_cocitation(&self, referrers: usize) -> bool {
        self.admits_cocitation_at(referrers, self.fanout_threshold())
    }

    /// [`Self::admits_cocitation`] against an explicit `threshold`, for tests and callers that
    /// resolved it once.
    pub fn admits_cocitation_at(&self, referrers: usize, threshold: usize) -> bool {
        referrers <= threshold
    }

    /// The evidence-mode affinity of `label` (a label or alias), if it has one.
    pub fn affinity(&self, label: &str) -> Option<f32> {
        self.relation(label).and_then(|r| r.affinity)
    }

    /// [`Self::fanout_threshold`] with the override passed in, for tests.
    pub fn fanout_from(&self, env_value: Option<&str>) -> usize {
        env_value
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&n| n >= self.fanout.min)
            .unwrap_or(self.fanout.default)
    }

    /// The JSON projection the clients consume: colour per label and alias,
    /// the fallback colour, and the per-relation drawing/physics defaults.
    ///
    /// Deterministic: keys are sorted, so the committed copy can be pinned to
    /// the YAML byte for byte.
    pub fn projection(&self) -> serde_json::Value {
        let mut colours = BTreeMap::new();
        let mut relations = Vec::new();
        for rel in &self.relations {
            if let Some(c) = &rel.colour {
                for key in std::iter::once(&rel.label).chain(rel.aliases.iter()) {
                    colours.insert(key.clone(), c.clone());
                }
            }
            relations.push(serde_json::json!({
                "label": rel.label,
                "kind": rel.kind,
                "physics": rel.physics,
                "drawn": rel.drawn,
                "colour": rel.colour,
            }));
        }
        serde_json::json!({
            "schemaVersion": self.version,
            "defaultColour": self.default_colour,
            "colours": colours,
            "relations": relations,
        })
    }

    /// The prefix map the document declared.
    pub fn prefixes(&self) -> &BTreeMap<String, String> {
        &self.prefixes
    }
}

fn by_label_name(by_label: &HashMap<String, usize>, idx: usize) -> String {
    by_label
        .iter()
        .find(|(_, &i)| i == idx)
        .map(|(l, _)| l.clone())
        .unwrap_or_default()
}

fn expand_curie(prefixes: &BTreeMap<String, String>, curie: &str) -> Result<String, SchemaError> {
    if curie.starts_with("http://") || curie.starts_with("https://") || curie.starts_with("urn:") {
        return Ok(curie.to_string());
    }
    let (prefix, local) = curie
        .split_once(':')
        .ok_or_else(|| SchemaError::UnknownPrefix(curie.to_string()))?;
    prefixes
        .get(prefix)
        .map(|ns| format!("{ns}{local}"))
        .ok_or_else(|| SchemaError::UnknownPrefix(curie.to_string()))
}

fn check_colour(c: &str) -> Result<(), SchemaError> {
    let ok = c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit());
    if ok {
        Ok(())
    } else {
        Err(SchemaError::BadColour(c.to_string()))
    }
}

/// Process-wide count of quarantined (unknown) predicates.
///
/// An unknown predicate creates no edge and takes no invented defaults
/// (ADR-2204 D1). The raw IRI is kept here with a count so it is observable;
/// persisting it with the store is a later slice.
#[derive(Debug, Default)]
pub struct Quarantine {
    seen: Mutex<BTreeMap<String, u64>>,
}

impl Quarantine {
    /// The shared ledger.
    pub fn global() -> &'static Quarantine {
        static Q: OnceLock<Quarantine> = OnceLock::new();
        Q.get_or_init(Quarantine::default)
    }

    /// Record `count` uses of an unknown predicate. Returns `true` the first
    /// time this predicate is seen, so the caller can warn once per predicate.
    pub fn record(&self, predicate: &str, count: u64) -> bool {
        let mut seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        let entry = seen.entry(predicate.to_string()).or_insert(0);
        let first = *entry == 0;
        *entry += count;
        first
    }

    /// A copy of the ledger: raw predicate IRI → count.
    pub fn snapshot(&self) -> BTreeMap<String, u64> {
        self.seen.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Total quarantined uses across all predicates.
    pub fn total(&self) -> u64 {
        self.snapshot().values().sum()
    }

    /// Empty the ledger (a sync resets it before it starts counting).
    pub fn clear(&self) {
        self.seen.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(extra_relations: &str) -> String {
        // The minimal valid document: every Producer present.
        let mut s = String::from(
            "version: 1\nprefixes: { vc: \"https://x/\" }\ndefault_colour: \"#AAAAAA\"\n\
             fanout_threshold: { env: F, default: 3, min: 1 }\nrelations:\n",
        );
        for (label, w) in [
            ("explicit_link", "1.0"),
            ("elevated_from", "1.0"),
            ("domain_member", "1.5"),
            ("inferred", "1.0"),
            ("co_citation", "0.5"),
            ("chain_payment", "1.0"),
        ] {
            s.push_str(&format!(
                "  - {{ label: {label}, kind: derived, legacy_weight: {w}, physics: true, drawn: true }}\n"
            ));
        }
        s.push_str(extra_relations);
        s
    }

    #[test]
    fn builtin_schema_is_valid() {
        let s = RelationSchema::from_yaml(RELATIONS_YAML)
            .expect("crates/visionclaw-domain/data/relations.yaml validates");
        assert!(s.relations().len() > 15);
    }

    #[test]
    fn minimal_document_loads() {
        RelationSchema::from_yaml(&doc("")).expect("valid");
    }

    #[test]
    fn duplicate_labels_are_rejected() {
        let extra = "  - { label: co_citation, kind: derived, legacy_weight: 0.5, physics: true, drawn: true }\n";
        assert_eq!(
            RelationSchema::from_yaml(&doc(extra)).unwrap_err(),
            SchemaError::DuplicateLabel("co_citation".into())
        );
    }

    #[test]
    fn a_predicate_maps_to_one_label() {
        let extra = "  - { label: a, kind: typed, predicates: [\"vc:p\"], legacy_weight: 1.0, physics: true, drawn: true }\n  - { label: b, kind: typed, predicates: [\"vc:p\"], legacy_weight: 1.0, physics: true, drawn: true }\n";
        match RelationSchema::from_yaml(&doc(extra)).unwrap_err() {
            SchemaError::DuplicatePredicate { predicate, .. } => {
                assert_eq!(predicate, "https://x/p");
            }
            other => panic!("wrong error {other:?}"),
        }
    }

    #[test]
    fn weights_must_be_finite_positive_and_at_most_two() {
        for w in ["0.0", "-1.0", "2.5", ".nan", ".inf"] {
            let extra = format!(
                "  - {{ label: a, kind: typed, predicates: [\"vc:p\"], legacy_weight: {w}, physics: true, drawn: true }}\n"
            );
            assert!(
                matches!(
                    RelationSchema::from_yaml(&doc(&extra)),
                    Err(SchemaError::WeightOutOfRange { .. })
                ),
                "{w} must be rejected"
            );
        }
        let ok = "  - { label: a, kind: typed, predicates: [\"vc:p\"], legacy_weight: 2.0, physics: true, drawn: true }\n";
        RelationSchema::from_yaml(&doc(ok)).expect("2.0 is the inclusive maximum");
    }

    #[test]
    fn a_producer_needs_a_weight() {
        let extra =
            "  - { label: a, kind: typed, predicates: [\"vc:p\"], physics: true, drawn: true }\n";
        assert_eq!(
            RelationSchema::from_yaml(&doc(extra)).unwrap_err(),
            SchemaError::MissingWeight("a".into())
        );
    }

    #[test]
    fn unknown_kinds_prefixes_and_fields_are_rejected() {
        let bad_kind = "  - { label: a, kind: sideways, physics: true, drawn: true }\n";
        assert!(matches!(
            RelationSchema::from_yaml(&doc(bad_kind)),
            Err(SchemaError::Parse(_))
        ));
        let bad_prefix = "  - { label: a, kind: typed, predicates: [\"nope:p\"], legacy_weight: 1.0, physics: true, drawn: true }\n";
        assert!(matches!(
            RelationSchema::from_yaml(&doc(bad_prefix)),
            Err(SchemaError::UnknownPrefix(_))
        ));
        let bad_field = "  - { label: a, kind: typed, weight: 1.0, physics: true, drawn: true }\n";
        assert!(matches!(
            RelationSchema::from_yaml(&doc(bad_field)),
            Err(SchemaError::Parse(_))
        ));
    }

    #[test]
    fn colours_must_be_hex_and_keys_unique() {
        let bad = "  - { label: a, kind: derived, colour: \"red\", physics: true, drawn: true }\n";
        assert!(matches!(
            RelationSchema::from_yaml(&doc(bad)),
            Err(SchemaError::BadColour(_))
        ));
        let dup = "  - { label: a, kind: derived, colour: \"#111111\", aliases: [b], physics: true, drawn: true }\n  - { label: b, kind: derived, colour: \"#222222\", physics: true, drawn: true }\n";
        assert_eq!(
            RelationSchema::from_yaml(&doc(dup)).unwrap_err(),
            SchemaError::DuplicateColourKey("b".into())
        );
    }

    #[test]
    fn inverse_pairs_must_belong_to_the_relation() {
        let extra = "  - { label: a, kind: typed, predicates: [\"vc:p\"], inverse_predicates: { \"vc:p\": \"vc:q\" }, legacy_weight: 1.0, physics: true, drawn: true }\n";
        assert!(matches!(
            RelationSchema::from_yaml(&doc(extra)),
            Err(SchemaError::ForeignInverse { .. })
        ));
    }

    #[test]
    fn a_missing_producer_is_rejected() {
        let text = doc("").replace("label: domain_member", "label: not_the_spoke");
        assert_eq!(
            RelationSchema::from_yaml(&text).unwrap_err(),
            SchemaError::MissingProducer("domain_member".into())
        );
    }

    #[test]
    fn every_physics_relation_has_an_affinity_inside_the_open_unit_interval() {
        for r in RelationSchema::builtin().relations() {
            if r.physics {
                let a = r
                    .affinity
                    .unwrap_or_else(|| panic!("{} has no affinity", r.label));
                assert!(a > 0.0 && a < 1.0, "{}: {a}", r.label);
            }
        }
        // Policy: the affinity is the pre-S3 registry strength, legacy_weight / 2.
        for r in RelationSchema::builtin().relations() {
            if let (Some(a), Some(w)) = (r.affinity, r.legacy_weight) {
                assert_eq!(a, w / 2.0, "{}", r.label);
            }
        }
    }

    #[test]
    fn affinity_outside_the_open_unit_interval_is_rejected() {
        for a in ["0.0", "1.0", "-0.2", "1.5", ".nan", ".inf"] {
            let extra = format!(
                "  - {{ label: a, kind: typed, predicates: [\"vc:p\"], legacy_weight: 1.0, affinity: {a}, physics: true, drawn: true }}\n"
            );
            assert!(
                matches!(
                    RelationSchema::from_yaml(&doc(&extra)),
                    Err(SchemaError::AffinityOutOfRange { .. })
                ),
                "{a} must be rejected"
            );
        }
    }

    /// ADR-2204 D3 / S2 leftover: one fan-out boundary. A target with exactly the threshold's
    /// referrers makes co-citation; one more makes none.
    #[test]
    fn the_fanout_boundary_is_inclusive_at_the_threshold() {
        let s = RelationSchema::builtin();
        let t = s.fanout_from(None);
        assert_eq!(t, 3);
        assert!(s.admits_cocitation_at(t - 1, t));
        assert!(s.admits_cocitation_at(t, t));
        assert!(!s.admits_cocitation_at(t + 1, t));
    }

    #[test]
    fn fanout_override_follows_the_old_rule() {
        let s = RelationSchema::builtin();
        assert_eq!(s.fanout_from(None), 3);
        assert_eq!(s.fanout_from(Some("7")), 7);
        assert_eq!(s.fanout_from(Some("0")), 3, "below min falls back");
        assert_eq!(s.fanout_from(Some("many")), 3, "unparsable falls back");
    }

    #[test]
    fn rdf_type_is_ignored_and_unknown_is_unknown() {
        let s = RelationSchema::builtin();
        assert!(matches!(
            s.classify("http://www.w3.org/1999/02/22-rdf-syntax-ns#type"),
            PredicateClass::Ignored
        ));
        assert!(matches!(
            s.classify("https://narrativegoldmine.com/ns/v1#produces"),
            PredicateClass::Unknown
        ));
    }

    #[test]
    fn quarantine_counts_and_reports_first_sighting() {
        let q = Quarantine::default();
        assert!(q.record("urn:x", 2));
        assert!(!q.record("urn:x", 1));
        assert_eq!(q.snapshot().get("urn:x"), Some(&3));
        assert_eq!(q.total(), 3);
        q.clear();
        assert_eq!(q.total(), 0);
    }
}
