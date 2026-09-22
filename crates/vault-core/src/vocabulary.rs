//! `ontology/vocabulary.yaml` — the governed vocabulary of the corpus
//! (contract C1).
//!
//! WS-B derived this file from a census of every `json-ld` fence and every
//! `key:: value` line in the corpus, so the model here is shaped by what the
//! corpus actually carries rather than by what the spec once claimed. Three of
//! its properties are load-bearing and easy to lose:
//!
//! * **`emitted: false` is not "ignored".** A relation or scalar with
//!   `emitted: false` is valid frontmatter, is carried into the page API and
//!   the graph, and is deliberately absent from `ontology.ttl`. That is what
//!   keeps golden-build parity while the 34 provisional predicates and 3,580
//!   `same-as` values stop being silently discarded.
//! * **`sub_property_of` is an OWL IRI, not a relation key.** `uses` is a
//!   sub-property of `vc:utilises`, which no page ever authors; it is declared
//!   under `super_properties`.
//! * **`resource` is immutable.** The `identity` block says so and
//!   `validation.blockers` lists `RESOURCE_MUTATED`. 412 of 8,446 classes carry
//!   a slug a case/digit-boundary splitter produced, and those IRIs are the
//!   subject of 104,731 edges and of every published URL.
//!
//! # Example
//!
//! ```
//! use vault_core::vocabulary::Vocabulary;
//!
//! let vocab = Vocabulary::from_yaml_str(r#"
//! version: 1
//! namespace: "urn:ngm:class:"
//! relations:
//!   is-a:    { owl: "rdfs:subClassOf", characteristics: [transitive] }
//!   same-as: { owl: "skos:exactMatch", emitted: false, status: core }
//! scalars:
//!   domain: { type: text, required: true }
//! "#).unwrap();
//!
//! assert!(vocab.relations["is-a"].emitted);
//! assert!(!vocab.relations["same-as"].emitted);      // valid, but not in the TTL
//! assert_eq!(vocab.taxonomy_key(), Some("is-a"));
//! assert_eq!(vocab.json_key("same-as"), "sameAs");   // derived camelCase
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

use crate::error::{Result, VaultError};

/// Prefixes every vocabulary gets for free. A `prefixes:` block in the file
/// adds to (and may override) these.
const BUILTIN_PREFIXES: &[(&str, &str)] = &[
    ("owl", "http://www.w3.org/2002/07/owl#"),
    ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
    ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
    ("xsd", "http://www.w3.org/2001/XMLSchema#"),
    ("skos", "http://www.w3.org/2004/02/skos/core#"),
    ("dc", "http://purl.org/dc/terms/"),
    ("dcterms", "http://purl.org/dc/terms/"),
    ("prov", "http://www.w3.org/ns/prov#"),
    ("vc", "https://narrativegoldmine.com/ns/v1#"),
    ("ngm", "https://narrativegoldmine.com/class/"),
    ("ngmi", "https://narrativegoldmine.com/individual/"),
];

fn default_true() -> bool {
    true
}

fn default_namespace() -> String {
    "urn:ngm:class:".to_owned()
}

fn default_individual_namespace() -> String {
    "urn:ngm:individual:".to_owned()
}

/// An OKF `type` the `knowledge/` vault permits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDef {
    /// The OWL entity this type maps to, e.g. `owl:Class`.
    pub owl: String,
    /// Frontmatter keys a page of this type must carry.
    #[serde(default)]
    pub required: Vec<String>,
}

/// Property characteristics the reasoner is told about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Characteristic {
    /// `owl:TransitiveProperty`.
    Transitive,
    /// `owl:SymmetricProperty` — declared, never emitted (outside OWL 2 EL).
    Symmetric,
    /// `owl:ReflexiveProperty`.
    Reflexive,
}

/// Whether a relation is part of the stable OWL surface or a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationStatus {
    /// Part of the vocabulary's stable surface.
    #[default]
    Core,
    /// A real corpus predicate the current emitter discards. Valid in
    /// frontmatter, absent from `ontology.ttl` until a Schema-level proposal
    /// flips `emitted`.
    Provisional,
}

/// A relation key: a frontmatter key whose value is a list of wikilinks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationDef {
    /// The OWL property IRI, prefixed or absolute.
    pub owl: String,
    /// The inverse relation's frontmatter key. An **authoring and build** hint
    /// only: the Turtle emitter omits `owl:inverseOf`, which is outside OWL 2
    /// EL, but `vault build` materialises the reverse edge in the graph.
    #[serde(default)]
    pub inverse: Option<String>,
    /// Declared characteristics.
    #[serde(default)]
    pub characteristics: Vec<Characteristic>,
    /// An OWL super-property IRI (e.g. `vc:utilises`), emitted as
    /// `rdfs:subPropertyOf`. **Not** a relation key.
    #[serde(default)]
    pub sub_property_of: Option<String>,
    /// Emit `C rdfs:subClassOf [owl:onProperty P; owl:someValuesFrom D]` when
    /// both endpoints are declared pages. Whelk uses these for subsumption.
    #[serde(default)]
    pub restriction: bool,
    /// `false` => valid frontmatter, carried in the page API and the graph,
    /// **not** written to `ontology.ttl`.
    #[serde(default = "default_true")]
    pub emitted: bool,
    /// Core or provisional.
    #[serde(default)]
    pub status: RelationStatus,
    /// An explicit camelCase artefact key. Absent for every WS-B relation;
    /// [`Vocabulary::json_key`] derives one.
    #[serde(default)]
    pub json_key: Option<String>,
}

impl RelationDef {
    /// `true` when this relation is declared transitive.
    #[must_use]
    pub fn is_transitive(&self) -> bool {
        self.characteristics.contains(&Characteristic::Transitive)
    }
}

/// A super-property the emitter declares that no page authors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuperPropertyDef {
    /// `rdfs:label`.
    pub label: String,
    /// `rdfs:domain`, usually `owl:Thing`.
    #[serde(default)]
    pub domain: Option<String>,
    /// `rdfs:range`, usually `owl:Thing`.
    #[serde(default)]
    pub range: Option<String>,
    /// The relation keys that declare this as their `sub_property_of`.
    #[serde(default)]
    pub supers_of: Vec<String>,
}

/// The value type of a scalar frontmatter key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScalarType {
    /// Free text.
    Text,
    /// A number, optionally bounded by `min`/`max`.
    Number,
    /// `true` / `false`.
    Boolean,
    /// An ISO-8601 date or datetime.
    Date,
    /// A list of scalars (`aliases`, `tags`).
    List,
}

/// A non-relation frontmatter key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalarDef {
    /// The value type.
    #[serde(rename = "type")]
    pub value_type: ScalarType,
    /// Every `knowledge/` page must carry this key.
    #[serde(default)]
    pub required: bool,
    /// The OWL property the build emits for it, when it emits one.
    #[serde(default)]
    pub owl: Option<String>,
    /// `false` => carried but not written to `ontology.ttl`.
    #[serde(default = "default_true")]
    pub emitted: bool,
    /// The closed set of permitted values, when the key is an enumeration.
    #[serde(default)]
    pub r#enum: Vec<String>,
    /// Taxonomic roots, for a free-text key that has some — `domain`'s six
    /// domain roots. Not a closed set: a value outside it is legal.
    #[serde(default)]
    pub roots: Vec<String>,
    /// The census block, as written. **Deliberately untyped.**
    ///
    /// Its shape depends on the key: for a text key like `domain` it is
    /// `value: count`, and the keys are the values the corpus uses; for a number
    /// key like `quality` it is a statistics block (`min: 0.35`, `max`, `mean`),
    /// and the keys are statistic names, not values. Typing it as
    /// `value: count` broke loading on the first number key that had one, so it
    /// stays a `Value` and [`ScalarDef::known_values`] only reads it for text.
    #[serde(default)]
    pub observed: IndexMap<String, serde_yaml::Value>,
    /// Inclusive lower bound for a `number`.
    #[serde(default)]
    pub min: Option<f64>,
    /// Inclusive upper bound for a `number`.
    #[serde(default)]
    pub max: Option<f64>,
    /// `true` when the key holds a list rather than a single value.
    #[serde(default)]
    pub list: bool,
}

impl ScalarDef {
    /// `true` when the value is a list, by either `type: list` or `list: true`.
    #[must_use]
    pub fn is_list(&self) -> bool {
        self.list || self.value_type == ScalarType::List
    }
}

/// The `validation:` block: what `vault validate` enforces.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationSchema {
    /// `knowledge: fail`, `working: allow`.
    #[serde(default)]
    pub unknown_keys: BTreeMap<String, String>,
    /// Constructs that must not survive the migration, in prose.
    #[serde(default)]
    pub rejected_constructs: Vec<String>,
    /// Keys every `knowledge/` page must carry.
    #[serde(default)]
    pub required_knowledge_keys: Vec<String>,
    /// Codes that are never approvable (PRD Q6).
    #[serde(default)]
    pub blockers: Vec<String>,
}

impl ValidationSchema {
    /// `true` when unknown keys fail in the named vault, `None` when the
    /// vocabulary does not say.
    #[must_use]
    pub fn rejects_unknown_keys_in(&self, vault: &str) -> Option<bool> {
        self.unknown_keys.get(vault).map(|v| v == "fail")
    }
}

/// Where a migrated fence field or `key:: value` line ends up.
///
/// The vocabulary spells these as `drop`, `prose`, a plain frontmatter key, a
/// dotted field (`generated.by`), or a `sources[…]` form. They are not
/// interchangeable: `drop` deletes the value, `prose` keeps it as text in the
/// bullet it came from, and a key lifts it into frontmatter — where, for a
/// relation key, it is union-merged with the fence targets rather than
/// replacing them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// `drop` — discard it. An explicit owner decision, recorded in the file.
    Drop,
    /// `prose` — render the value into the surrounding bullet's prose.
    ///
    /// Block-level properties cannot be lifted to page-level frontmatter
    /// without collapsing many values into one, which is why this exists.
    Prose,
    /// `body` — the page body's leading paragraph.
    Body,
    /// `type` — the OKF type, read from the fence `@type`.
    Type,
    /// `per_key` — expand per entry of a `vc:legacyProperties` array.
    PerKey,
    /// `per_predicate` — expand per predicate of a nested `relations` object.
    PerPredicate,
    /// `per_shape` — match a `provenance` object against `provenance_shapes`.
    PerShape,
    /// A plain frontmatter key.
    Key(String),
    /// A field of a mapping-valued key, e.g. `generated.by`.
    Nested {
        /// The frontmatter key, e.g. `generated`.
        key: String,
        /// The field within it, e.g. `by`.
        field: String,
    },
    /// Append an entry to `sources` (`sources[]`).
    SourceAppend,
    /// A field of the `sources` entry with a given id, e.g.
    /// `sources[id=origin].resource`.
    SourceField {
        /// The entry's `id`.
        id: String,
        /// The field to set, e.g. `resource` or `date`.
        field: String,
    },
}

impl Destination {
    /// Parse a destination from the migration map. A YAML `null` is [`Self::Drop`].
    ///
    /// ```
    /// # use vault_core::vocabulary::Destination;
    /// assert_eq!(Destination::parse("drop"), Destination::Drop);
    /// assert_eq!(Destination::parse("prose"), Destination::Prose);
    /// assert_eq!(
    ///     Destination::parse("generated.by"),
    ///     Destination::Nested { key: "generated".into(), field: "by".into() }
    /// );
    /// assert_eq!(Destination::parse("sources[]"), Destination::SourceAppend);
    /// assert_eq!(Destination::parse("sources[].resource"), Destination::SourceAppend);
    /// assert_eq!(
    ///     Destination::parse("sources[id=origin].resource"),
    ///     Destination::SourceField { id: "origin".into(), field: "resource".into() }
    /// );
    /// assert_eq!(Destination::parse("is-a"), Destination::Key("is-a".into()));
    /// ```
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        // `!prose` and `prose` are the same instruction; the file has used both
        // spellings, and neither is worth failing a build over.
        let raw = raw.trim().trim_start_matches('!');
        match raw {
            "drop" | "null" | "~" => return Self::Drop,
            "prose" => return Self::Prose,
            "body" => return Self::Body,
            "type" => return Self::Type,
            "per_key" => return Self::PerKey,
            "per_predicate" => return Self::PerPredicate,
            "per_shape" => return Self::PerShape,
            "sources[]" => return Self::SourceAppend,
            _ => {}
        }
        // `sources[].resource` is the same instruction as `sources[]`: append
        // an entry whose `resource` is the value.
        if let Some(field) = raw.strip_prefix("sources[].") {
            if field == "resource" {
                return Self::SourceAppend;
            }
        }
        if let Some(rest) = raw.strip_prefix("sources[id=") {
            if let Some((id, tail)) = rest.split_once(']') {
                let field = tail.trim_start_matches('.').trim();
                if !id.is_empty() && !field.is_empty() {
                    return Self::SourceField {
                        id: id.to_owned(),
                        field: field.to_owned(),
                    };
                }
            }
        }
        if let Some((key, field)) = raw.split_once('.') {
            if !key.is_empty() && !field.is_empty() && !field.contains('.') {
                return Self::Nested {
                    key: key.to_owned(),
                    field: field.to_owned(),
                };
            }
        }
        Self::Key(raw.to_owned())
    }

    /// The frontmatter key this destination writes, when it writes one.
    ///
    /// `sources[…]` and `generated.…` report `sources` and `generated`, which
    /// is what a "does the vocabulary declare this key?" check needs.
    #[must_use]
    pub fn frontmatter_key(&self) -> Option<&str> {
        match self {
            Self::Key(k) | Self::Nested { key: k, .. } => Some(k),
            Self::SourceAppend | Self::SourceField { .. } => Some("sources"),
            Self::Drop
            | Self::Prose
            | Self::Body
            | Self::Type
            | Self::PerKey
            | Self::PerPredicate
            | Self::PerShape => None,
        }
    }

    /// `true` when the destination discards the value outright.
    #[must_use]
    pub fn is_drop(&self) -> bool {
        matches!(self, Self::Drop)
    }
}

impl<'de> Deserialize<'de> for Destination {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = Option::<String>::deserialize(d)?;
        Ok(raw.as_deref().map_or(Self::Drop, Self::parse))
    }
}

impl Serialize for Destination {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let text = match self {
            Self::Drop => "drop".to_owned(),
            Self::Prose => "prose".to_owned(),
            Self::Body => "body".to_owned(),
            Self::Type => "type".to_owned(),
            Self::PerKey => "per_key".to_owned(),
            Self::PerPredicate => "per_predicate".to_owned(),
            Self::PerShape => "per_shape".to_owned(),
            Self::Key(k) => k.clone(),
            Self::Nested { key, field } => format!("{key}.{field}"),
            Self::SourceAppend => "sources[]".to_owned(),
            Self::SourceField { id, field } => format!("sources[id={id}].{field}"),
        };
        s.serialize_str(&text)
    }
}

/// Where the fence `definition` lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionPlacement {
    /// A `definition:` frontmatter key.
    Frontmatter,
    /// The body's leading paragraph.
    LeadingParagraph,
}

/// The `migration.body:` rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BodyRules {
    /// Fence languages removed wholesale.
    #[serde(default)]
    pub fences_removed: Vec<String>,
    /// Where the fence `definition` goes: `frontmatter` or `leading-paragraph`.
    ///
    /// A genuine open decision, so it is data rather than code: the migration
    /// honours whichever the vocabulary declares and needs no change if the
    /// owner moves it.
    #[serde(default)]
    pub definition_placement: Option<String>,
    /// How a duplicated `### Definition` bullet is handled.
    #[serde(default)]
    pub definition_dedupe: Option<String>,
    /// `true` => the `### Relationships` section is dropped, because the build
    /// regenerates it from frontmatter.
    #[serde(default)]
    pub relationships_section_removed: bool,
    /// How `{{embed …}}` is handled, in prose.
    #[serde(default)]
    pub embeds: Option<String>,
    /// How Logseq task markers are rewritten, in prose.
    #[serde(default)]
    pub tasks: Option<String>,
    /// How `#[[multi word]]` tags are rewritten, in prose.
    #[serde(default)]
    pub tags: Option<String>,
    /// How asset links are rewritten, in prose.
    #[serde(default)]
    pub assets: Option<String>,
}

impl BodyRules {
    /// Where the fence `definition` lands, defaulting to `frontmatter`.
    #[must_use]
    pub fn definition_placement(&self) -> DefinitionPlacement {
        match self.definition_placement.as_deref() {
            Some("leading-paragraph") => DefinitionPlacement::LeadingParagraph,
            _ => DefinitionPlacement::Frontmatter,
        }
    }
}

/// The `migration.block_refs:` rules.
///
/// Logseq block references (`{{embed ((uuid))}}`, `((uuid))`) were assumed
/// dead; 33 of the 34 in `knowledge/` resolve to an `id::` line in `working/`.
/// Each resolved reference is inlined as a quoted block so the text survives in
/// the governed vault instead of being deleted with the reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRefRules {
    /// Directories searched for the `id::` target, in order.
    #[serde(default)]
    pub resolve_from: Vec<String>,
    /// Maximum lines inlined from one resolved block.
    #[serde(default = "default_block_ref_max_lines")]
    pub max_lines: usize,
    /// Uuids that are documentation *about* Logseq syntax rather than live
    /// references; escaped so they render verbatim.
    #[serde(default)]
    pub literal_placeholders: Vec<String>,
    /// Uuids known not to resolve, with the owner's disposition for each.
    #[serde(default)]
    pub unresolved: IndexMap<String, serde_yaml::Value>,
}

impl Default for BlockRefRules {
    fn default() -> Self {
        Self {
            resolve_from: Vec::new(),
            max_lines: default_block_ref_max_lines(),
            literal_placeholders: Vec::new(),
            unresolved: IndexMap::new(),
        }
    }
}

fn default_block_ref_max_lines() -> usize {
    40
}

impl BlockRefRules {
    /// `true` when this uuid is documentation, not a reference.
    #[must_use]
    pub fn is_literal_placeholder(&self, uuid: &str) -> bool {
        self.literal_placeholders.iter().any(|p| p == uuid)
    }
}

/// The `migration:` block — the one-shot fence-to-property map.
///
/// Consumed only by `vault migrate --fences-to-properties`. When the migration
/// has run, this block and the code that reads it are deleted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MigrationMap {
    /// This map describes a one-shot conversion.
    #[serde(default)]
    pub one_shot: bool,
    /// The subcommand is deleted after the run.
    #[serde(default)]
    pub deleted_after_run: bool,
    /// Where the migration writes its evidence report.
    #[serde(default)]
    pub report: Option<String>,
    /// Paths excluded from conversion entirely (deleted, not migrated).
    #[serde(default)]
    pub excluded_paths: Vec<String>,
    /// A flat fence-key map, the alternative spelling of the four blocks
    /// below; folded into [`MigrationMap::fences`] on load.
    #[serde(default)]
    pub fence_fields: IndexMap<String, Destination>,
    /// Fence keys that carry no page content, in the flat spelling.
    #[serde(default)]
    pub ignore: IndexSet<String>,
    /// `@type: Page` fence keys.
    #[serde(default)]
    pub page_fence: IndexMap<String, Destination>,
    /// Entries of the Page fence's `vc:legacyProperties` array.
    #[serde(default)]
    pub page_fence_legacy_properties: IndexMap<String, Destination>,
    /// `@type: Class` fence keys.
    #[serde(default)]
    pub class_fence: IndexMap<String, Destination>,
    /// The eight legacy `@type: OntologyClass` fences.
    #[serde(default)]
    pub ontology_class_fence: IndexMap<String, Destination>,
    /// The `vc:LinkResolutionsAnnotation` fence — wholly derived data.
    #[serde(default)]
    pub link_resolutions_fence: Option<Destination>,
    /// `provenance` object shapes, keyed by their field set.
    #[serde(default)]
    pub provenance_shapes: IndexMap<String, serde_yaml::Value>,
    /// Block-reference resolution rules.
    #[serde(default)]
    pub block_refs: BlockRefRules,
    /// Variant predicate spellings folded onto a canonical relation key.
    #[serde(default)]
    pub relation_aliases: IndexMap<String, String>,
    /// Variant `maturity` values folded onto a canonical one.
    #[serde(default)]
    pub maturity_aliases: IndexMap<String, String>,
    /// `key:: value` keys.
    #[serde(default)]
    pub logseq_keys: IndexMap<String, Destination>,
    /// Keys already present in a page's **own** frontmatter before migration.
    ///
    /// The third input category, beside the `json-ld` fences and the Logseq
    /// `key:: value` lines. A page that predates the governed schema carries
    /// keys of its own (`elevatedFrom`, `legacy_iri`, `schema_version`, the
    /// `working/` episodic extensions, Logseq editor state); without a map
    /// they pass through untouched and `vault validate` then reports each one
    /// as `UNKNOWN_KEY`. The grammar is the same [`Destination`] grammar the
    /// fence and `key::` maps use.
    ///
    /// A key absent from this map is **authored**: it survives unchanged and
    /// wins over anything the fences supply.
    #[serde(default)]
    pub frontmatter_keys: IndexMap<String, Destination>,
    /// Body rewriting rules.
    #[serde(default)]
    pub body: BodyRules,
    /// Deltas against the retired Python build, stated so the golden-parity
    /// test accounts for them rather than being surprised.
    #[serde(default)]
    pub intentional_deltas: Vec<String>,
    /// **Normalised** fence-key map: every fence block and the `ignore` list
    /// folded into one source-key to destination map, built on load. This is
    /// what the migration reads.
    #[serde(skip)]
    pub fences: IndexMap<String, Destination>,
}

impl MigrationMap {
    /// Fold every fence-key spelling into [`MigrationMap::fences`].
    ///
    /// The governed file has used two equivalent layouts — a flat
    /// `fence_fields` + `ignore` pair, and four per-fence-type blocks
    /// (`page_fence`, `class_fence`, …). They carry the same information, so
    /// rather than pick one and break whenever the other is written, the
    /// reader accepts both and the rest of the crate sees one map.
    ///
    /// A key mapped in one place and ignored in another is a contradiction the
    /// caller must resolve, so `ignore` is applied first and an explicit
    /// mapping wins.
    fn normalise(&mut self) {
        let mut fences: IndexMap<String, Destination> = IndexMap::new();
        for key in &self.ignore {
            fences.insert(key.clone(), Destination::Drop);
        }
        for block in [
            &self.fence_fields,
            &self.page_fence,
            &self.page_fence_legacy_properties,
            &self.class_fence,
            &self.ontology_class_fence,
        ] {
            for (key, destination) in block {
                fences.insert(key.clone(), destination.clone());
            }
        }
        self.fences = fences;
    }

    /// `true` when the block is populated at all.
    ///
    /// An empty migration map means the vocabulary was read mid-write or the
    /// block is missing; either way the migration must refuse rather than
    /// convert the corpus against nothing.
    #[must_use]
    pub fn is_populated(&self) -> bool {
        !self.fences.is_empty() && !self.logseq_keys.is_empty()
    }

    /// The destination for a fence key, or `None` when the key appears nowhere
    /// in the map — the condition that makes `vault migrate` refuse rather
    /// than silently discard a field.
    #[must_use]
    pub fn fence_destination(&self, key: &str) -> Option<&Destination> {
        self.fences.get(key)
    }

    /// The destination for a key found in a page's own frontmatter, or `None`
    /// when the vocabulary does not name it — in which case the key is
    /// authored and passes through unchanged.
    #[must_use]
    pub fn frontmatter_destination(&self, key: &str) -> Option<&Destination> {
        self.frontmatter_keys.get(key)
    }

    /// The canonical relation key for a predicate spelling, if it names one.
    #[must_use]
    pub fn canonical_relation(&self, spelling: &str) -> Option<&str> {
        self.relation_aliases.get(spelling).map(String::as_str)
    }

    /// The canonical `maturity` value for a spelling, unchanged when it is
    /// already canonical.
    #[must_use]
    pub fn canonical_maturity<'a>(&'a self, value: &'a str) -> &'a str {
        self.maturity_aliases
            .get(value)
            .map_or(value, String::as_str)
    }

    /// Every `(source key, destination)` pair across the normalised fence map
    /// and the `key::` map — what a coverage check iterates.
    pub fn all_destinations(&self) -> impl Iterator<Item = (&str, &Destination)> {
        self.fences
            .iter()
            .chain(&self.logseq_keys)
            .map(|(k, v)| (k.as_str(), v))
    }
}

impl ScalarDef {
    /// Every value this key is known to take: the closed `enum` when it has one,
    /// otherwise the census's `observed` keys unioned with its `roots`.
    ///
    /// The distinction is the point. `domain` is deliberately **free text** —
    /// normalising `ai` (513 pages) onto `artificial-intelligence` is a content
    /// decision for the governance loop, not something `migrate` should do
    /// silently — so validating it against a closed set is wrong. Validating it
    /// against *what the census saw* is right: it still catches a typo, and it
    /// stays silent about the seventeen values the corpus legitimately uses.
    #[must_use]
    pub fn known_values(&self) -> Vec<&str> {
        if !self.r#enum.is_empty() {
            return self.r#enum.iter().map(String::as_str).collect();
        }
        // `observed`'s keys are *values* only for a text key. On a number key
        // they are statistic names (`min`, `max`), which would be nonsense here.
        let mut out: Vec<&str> = self.roots.iter().map(String::as_str).collect();
        if self.value_type == ScalarType::Text {
            out.extend(self.observed.keys().map(String::as_str));
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// The parsed `ontology/vocabulary.yaml`.
///
/// Informational keys the census carries (`count_2026_09_22`, `note`,
/// `observed`, …) are ignored rather than rejected, so WS-B can annotate the
/// file without breaking the build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vocabulary {
    /// Schema version of this file.
    pub version: u32,
    /// A stable identifier for this vocabulary revision.
    #[serde(default)]
    pub vocabulary_id: Option<String>,
    /// IRI namespace for class `resource` values.
    #[serde(default = "default_namespace")]
    pub namespace: String,
    /// IRI namespace for individuals.
    #[serde(default = "default_individual_namespace")]
    pub individual_namespace: String,
    /// The JSON-LD context URL the corpus cites.
    #[serde(default)]
    pub context: String,
    /// Prefix to IRI expansions, merged over the built-in set.
    #[serde(default)]
    pub prefixes: BTreeMap<String, String>,
    /// OKF types permitted in `knowledge/`.
    #[serde(default)]
    pub types: IndexMap<String, TypeDef>,
    /// OKF types permitted in `working/`.
    #[serde(default)]
    pub working_types: IndexSet<String>,
    /// Documented episodic extensions of `working/`.
    #[serde(default)]
    pub working_extensions: IndexMap<String, ScalarDef>,
    /// Relation keys.
    #[serde(default)]
    pub relations: IndexMap<String, RelationDef>,
    /// Super-properties declared but never authored.
    #[serde(default)]
    pub super_properties: IndexMap<String, SuperPropertyDef>,
    /// Scalar keys.
    #[serde(default)]
    pub scalars: IndexMap<String, ScalarDef>,
    /// What `vault validate` enforces.
    #[serde(default)]
    pub validation: ValidationSchema,
    /// The migration map (one-shot).
    #[serde(default)]
    pub migration: MigrationMap,
    /// Long-tail concept label to IRI, for references whose IRI is neither a
    /// page's `resource` nor recoverable from the link. An override; normally
    /// unnecessary, because `vault migrate` writes a long-tail reference as
    /// `[[<iri-tail>|<label>]]`.
    #[serde(default)]
    pub tail_iris: IndexMap<String, String>,
    /// Keys that are always legal: Obsidian's own plus the OKF block.
    #[serde(default = "default_reserved")]
    pub reserved: IndexSet<String>,
}

fn default_reserved() -> IndexSet<String> {
    [
        "type",
        "title",
        "resource",
        "public",
        "aliases",
        "tags",
        "cssclasses",
        "status",
        "stale_after",
        "generated",
        "verified",
        "sources",
        "slug",
        "label",
        "links",
        "page_resource",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

impl Vocabulary {
    /// Load from a path.
    ///
    /// # Errors
    /// [`VaultError::Io`] if the file cannot be read, [`VaultError::Vocabulary`]
    /// if it does not parse or is internally inconsistent.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| VaultError::io(path, e))?;
        Self::from_yaml_str(&text).map_err(|message| VaultError::Vocabulary {
            path: path.to_path_buf(),
            message,
        })
    }

    /// Find and load `ontology/vocabulary.yaml` by walking up from `start`.
    ///
    /// # Errors
    /// [`VaultError::NotAVault`] when no vocabulary is found in any ancestor.
    pub fn discover(start: impl AsRef<Path>) -> Result<(PathBuf, Self)> {
        let start = start.as_ref();
        let mut dir = Some(start);
        while let Some(d) = dir {
            let candidate = d.join("ontology").join("vocabulary.yaml");
            if candidate.is_file() {
                let vocab = Self::load(&candidate)?;
                return Ok((candidate, vocab));
            }
            dir = d.parent();
        }
        Err(VaultError::NotAVault {
            root: start.to_path_buf(),
            message: "no ontology/vocabulary.yaml in this directory or any ancestor".into(),
        })
    }

    /// Parse from a YAML string and check internal consistency.
    ///
    /// # Errors
    /// A human-readable message when the YAML is invalid, a declared inverse
    /// or relation alias names an undeclared relation, or a key is both a
    /// relation and a scalar.
    pub fn from_yaml_str(yaml: &str) -> std::result::Result<Self, String> {
        let mut vocab: Self = serde_yaml::from_str(yaml).map_err(|e| e.to_string())?;
        vocab.migration.normalise();
        vocab.check()?;
        Ok(vocab)
    }

    fn check(&self) -> std::result::Result<(), String> {
        for (key, def) in &self.relations {
            if let Some(inv) = &def.inverse {
                if !self.relations.contains_key(inv) {
                    return Err(format!(
                        "relation {key:?} declares inverse {inv:?}, which is not itself a relation"
                    ));
                }
            }
            if self.scalars.contains_key(key) {
                return Err(format!("{key:?} is declared both a relation and a scalar"));
            }
        }
        for def in self.scalars.values() {
            if let (Some(min), Some(max)) = (def.min, def.max) {
                if min > max {
                    return Err(format!("scalar bound min {min} exceeds max {max}"));
                }
            }
        }
        // Every migration destination must name a key this vocabulary
        // declares, or the migration would write frontmatter that
        // `vault validate` immediately rejects.
        for (source, destination) in self.migration.all_destinations() {
            let Some(target) = destination.frontmatter_key() else {
                continue;
            };
            if !self.is_known_working_key(target) {
                return Err(format!(
                    "migration maps {source:?} onto {target:?}, which is not a \
                     declared relation, scalar or reserved key"
                ));
            }
        }
        Ok(())
    }

    /// Expand a `prefix:local` IRI. An absolute IRI or an unknown prefix is
    /// returned unchanged.
    #[must_use]
    pub fn expand(&self, curie: &str) -> String {
        if curie.starts_with("http://")
            || curie.starts_with("https://")
            || curie.starts_with("urn:")
        {
            return curie.to_owned();
        }
        let Some((prefix, local)) = curie.split_once(':') else {
            return curie.to_owned();
        };
        if let Some(base) = self.prefixes.get(prefix) {
            return format!("{base}{local}");
        }
        for (p, base) in BUILTIN_PREFIXES {
            if *p == prefix {
                return format!("{base}{local}");
            }
        }
        curie.to_owned()
    }

    /// `true` when `key` is a declared relation.
    #[must_use]
    pub fn is_relation(&self, key: &str) -> bool {
        self.relations.contains_key(key)
    }

    /// `true` when `key` is a relation whose axioms reach `ontology.ttl`.
    #[must_use]
    pub fn is_emitted_relation(&self, key: &str) -> bool {
        self.relations.get(key).is_some_and(|d| d.emitted)
    }

    /// The camelCase artefact key for a relation.
    ///
    /// An explicit `json_key` wins; otherwise the kebab key is camel-cased,
    /// which reproduces every one of the twelve v1 spellings (`has-part` to
    /// `hasPart`, `standardized-by` to `standardizedBy`) and gives the
    /// provisional predicates a consistent one.
    #[must_use]
    pub fn json_key(&self, key: &str) -> String {
        if let Some(explicit) = self.relations.get(key).and_then(|d| d.json_key.clone()) {
            return explicit;
        }
        camel_case(key)
    }

    /// `true` when `key` is legal in `knowledge/` frontmatter.
    #[must_use]
    pub fn is_known_key(&self, key: &str) -> bool {
        self.relations.contains_key(key)
            || self.scalars.contains_key(key)
            || self.reserved.contains(key)
    }

    /// `true` when `key` is legal in `working/` frontmatter.
    #[must_use]
    pub fn is_known_working_key(&self, key: &str) -> bool {
        self.is_known_key(key) || self.working_extensions.contains_key(key)
    }

    /// Every key that is legal in `knowledge/`, for a "did you mean" hint.
    #[must_use]
    pub fn known_keys(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = self
            .relations
            .keys()
            .chain(self.scalars.keys())
            .chain(self.reserved.iter())
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    /// The relation whose `owl` IRI is `rdfs:subClassOf` — the taxonomy key.
    ///
    /// Resolved by IRI, not by name, so a future rename in the vocabulary does
    /// not break reasoning.
    #[must_use]
    pub fn taxonomy_key(&self) -> Option<&str> {
        const SUBCLASS: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
        self.relations
            .iter()
            .find(|(_, def)| self.expand(&def.owl) == SUBCLASS)
            .map(|(k, _)| k.as_str())
    }

    /// The relation keys whose axioms reach `ontology.ttl`, in declaration
    /// order, excluding the taxonomy key.
    #[must_use]
    pub fn emitted_relations(&self) -> Vec<&str> {
        let taxonomy = self.taxonomy_key();
        self.relations
            .iter()
            .filter(|(k, d)| d.emitted && Some(k.as_str()) != taxonomy)
            .map(|(k, _)| k.as_str())
            .collect()
    }

    /// Relation keys that carry an existential restriction.
    #[must_use]
    pub fn restriction_relations(&self) -> Vec<&str> {
        self.relations
            .iter()
            .filter(|(_, d)| d.restriction && d.emitted)
            .map(|(k, _)| k.as_str())
            .collect()
    }
}

/// `has-part` to `hasPart`, `standardized-by` to `standardizedBy`.
fn camel_case(kebab: &str) -> String {
    let mut out = String::with_capacity(kebab.len());
    let mut upper_next = false;
    for c in kebab.chars() {
        if c == '-' || c == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORE: &str = r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status], count_2026_09_22: 8446 }
working_types: [Note]
working_extensions:
  topic: { type: text }
relations:
  is-a:     { owl: rdfs:subClassOf, characteristics: [transitive], edges_2026_09_22: 8446 }
  requires: { owl: vc:requires, sub_property_of: vc:dependsOn, restriction: true }
  has-part: { owl: vc:hasPart, inverse: part-of, restriction: true }
  part-of:  { owl: vc:isPartOf, inverse: has-part }
  same-as:  { owl: skos:exactMatch, emitted: false, status: core }
  uses:     { owl: vc:uses, sub_property_of: vc:utilises }
  produces: { owl: vc:produces, emitted: false, status: provisional }
super_properties:
  vc:utilises: { label: utilises, domain: owl:Thing, range: owl:Thing, supers_of: [uses] }
scalars:
  domain:   { type: text, required: true, owl: vc:sourceDomain, observed: { ai: 513 } }
  maturity: { type: text, required: true, enum: [emerging, established, mature] }
  quality:  { type: number, min: 0.0, max: 1.0 }
  aliases:  { type: list }
validation:
  unknown_keys: { knowledge: fail, working: allow }
  required_knowledge_keys: [type, title, resource, public, status, generated]
  blockers: [WHELK_INCONSISTENCY, RESOURCE_MUTATED]
"#;

    /// The flat spelling: `fence_fields` + `ignore`.
    fn flat() -> Vocabulary {
        Vocabulary::from_yaml_str(&format!(
            "{CORE}
migration:
  fence_fields:
    \"@id\": resource
    \"@type\": type
    definition: body
    relations: per_predicate
    relations.hasPart: has-part
  ignore: [\"@context\", vc:schemaVersion]
  logseq_keys:
    attributedTo: generated.by
    elevatedFrom: \"sources[id=origin].resource\"
    sources: \"sources[]\"
    collapsed: drop
    confidence: prose
    hasPart: has-part
  body:
    definition_placement: frontmatter
  intentional_deltas: [\"maturity `mature` stops collapsing to `draft`\"]
"
        ))
        .unwrap()
    }

    /// The structured spelling: one block per fence type.
    fn structured() -> Vocabulary {
        Vocabulary::from_yaml_str(&format!(
            "{CORE}
migration:
  page_fence:
    \"@context\": drop
    \"@type\": type
  class_fence:
    \"@id\": resource
    definition: body
    relations: per_predicate
    relations.hasPart: has-part
    vc:schemaVersion: drop
  relation_aliases: {{ hasPart: has-part, subClassOf: is-a }}
  maturity_aliases: {{ experimental: emerging }}
  block_refs:
    resolve_from: [working/pages]
    max_lines: 40
    literal_placeholders: [block-uuid]
  logseq_keys:
    attributedTo: generated.by
    elevatedFrom: \"sources[id=origin].resource\"
    sources: \"sources[]\"
    collapsed: drop
    confidence: prose
  body:
    definition_placement: leading-paragraph
  intentional_deltas: [\"maturity `mature` stops collapsing to `draft`\"]
"
        ))
        .unwrap()
    }

    #[test]
    fn informational_census_keys_are_ignored_not_rejected() {
        let v = flat();
        assert_eq!(v.relations["is-a"].owl, "rdfs:subClassOf");
        assert!(v.scalars["domain"].required);
    }

    #[test]
    fn emitted_defaults_to_true_and_false_is_honoured() {
        let v = flat();
        assert!(v.is_emitted_relation("is-a"));
        assert!(v.is_emitted_relation("has-part"), "default is true");
        assert!(!v.is_emitted_relation("same-as"));
        assert!(!v.is_emitted_relation("produces"));
        assert!(v.is_relation("same-as"), "still valid frontmatter");
    }

    #[test]
    fn sub_property_of_is_an_owl_iri_not_a_relation_key() {
        let v = flat();
        assert_eq!(
            v.relations["uses"].sub_property_of.as_deref(),
            Some("vc:utilises")
        );
        assert!(!v.relations.contains_key("vc:utilises"));
        assert_eq!(v.super_properties["vc:utilises"].label, "utilises");
    }

    #[test]
    fn json_keys_are_derived_by_camel_casing() {
        let v = flat();
        assert_eq!(v.json_key("has-part"), "hasPart");
        assert_eq!(v.json_key("part-of"), "partOf");
        assert_eq!(v.json_key("requires"), "requires");
        assert_eq!(v.json_key("same-as"), "sameAs");
    }

    #[test]
    fn restriction_relations_are_listed() {
        let v = flat();
        let mut r = v.restriction_relations();
        r.sort_unstable();
        assert_eq!(r, ["has-part", "requires"]);
    }

    #[test]
    fn emitted_relations_exclude_the_taxonomy_key_and_the_unemitted() {
        let v = flat();
        let emitted = v.emitted_relations();
        assert!(!emitted.contains(&"is-a"));
        assert!(!emitted.contains(&"same-as"));
        assert!(emitted.contains(&"requires"));
    }

    #[test]
    fn both_fence_spellings_normalise_to_the_same_map() {
        for v in [flat(), structured()] {
            let m = &v.migration;
            assert!(m.is_populated());
            assert_eq!(m.fences["@id"], Destination::Key("resource".into()));
            assert_eq!(m.fences["@type"], Destination::Type);
            assert_eq!(m.fences["definition"], Destination::Body);
            assert_eq!(m.fences["relations"], Destination::PerPredicate);
            assert_eq!(
                m.fences["relations.hasPart"],
                Destination::Key("has-part".into())
            );
            assert_eq!(m.fences["@context"], Destination::Drop);
            assert_eq!(m.fences["vc:schemaVersion"], Destination::Drop);
            assert!(m.fence_destination("vc:unseen").is_none());
        }
    }

    #[test]
    fn destinations_parse_every_shape_in_the_map() {
        let v = flat();
        let m = &v.migration;
        assert_eq!(m.logseq_keys["collapsed"], Destination::Drop);
        assert_eq!(m.logseq_keys["confidence"], Destination::Prose);
        assert_eq!(m.logseq_keys["sources"], Destination::SourceAppend);
        assert_eq!(
            m.logseq_keys["attributedTo"],
            Destination::Nested {
                key: "generated".into(),
                field: "by".into()
            }
        );
        assert_eq!(
            m.logseq_keys["elevatedFrom"],
            Destination::SourceField {
                id: "origin".into(),
                field: "resource".into()
            }
        );
    }

    #[test]
    fn a_yaml_null_and_a_bang_prose_are_both_understood() {
        // The file has written `null`/`drop` and `prose`/`!prose` at different
        // times; neither spelling is worth failing a build over.
        assert_eq!(Destination::parse("!prose"), Destination::Prose);
        assert_eq!(Destination::parse("null"), Destination::Drop);
        let v = Vocabulary::from_yaml_str(&format!(
            "{CORE}\nmigration:\n  fence_fields: {{ \"@id\": resource }}\n  \
             logseq_keys: {{ collapsed: null, tier: \"!prose\" }}\n"
        ))
        .unwrap();
        assert_eq!(v.migration.logseq_keys["collapsed"], Destination::Drop);
        assert_eq!(v.migration.logseq_keys["tier"], Destination::Prose);
    }

    #[test]
    fn frontmatter_key_reports_the_key_a_destination_writes() {
        assert_eq!(
            Destination::parse("sources[id=origin].resource").frontmatter_key(),
            Some("sources")
        );
        assert_eq!(
            Destination::parse("generated.by").frontmatter_key(),
            Some("generated")
        );
        assert_eq!(Destination::parse("is-a").frontmatter_key(), Some("is-a"));
        assert_eq!(Destination::parse("prose").frontmatter_key(), None);
        assert_eq!(Destination::parse("drop").frontmatter_key(), None);
    }

    #[test]
    fn a_destination_onto_an_undeclared_key_fails_the_load() {
        let err = Vocabulary::from_yaml_str(&format!(
            "{CORE}\nmigration:\n  fence_fields: {{ \"@id\": nowhere }}\n"
        ))
        .unwrap_err();
        assert!(err.contains("nowhere"), "{err}");
    }

    #[test]
    fn a_dangling_inverse_fails_the_load() {
        let err = Vocabulary::from_yaml_str(
            "version: 1\nrelations:\n  requires: { owl: vc:requires, inverse: nowhere }\n",
        )
        .unwrap_err();
        assert!(err.contains("nowhere"), "{err}");
    }

    #[test]
    fn alias_folding_is_available_to_the_migration() {
        let v = structured();
        assert_eq!(v.migration.canonical_relation("hasPart"), Some("has-part"));
        assert_eq!(v.migration.canonical_relation("nope"), None);
        assert_eq!(v.migration.canonical_maturity("experimental"), "emerging");
        assert_eq!(v.migration.canonical_maturity("established"), "established");
    }

    #[test]
    fn definition_placement_is_read_from_the_vocabulary() {
        assert_eq!(
            flat().migration.body.definition_placement(),
            DefinitionPlacement::Frontmatter
        );
        assert_eq!(
            structured().migration.body.definition_placement(),
            DefinitionPlacement::LeadingParagraph
        );
    }

    #[test]
    fn block_ref_rules_default_sensibly_and_read_placeholders() {
        let v = structured();
        let refs = &v.migration.block_refs;
        assert_eq!(refs.max_lines, 40);
        assert!(refs.is_literal_placeholder("block-uuid"));
        assert!(!refs.is_literal_placeholder("66f13d66"));
        assert_eq!(flat().migration.block_refs.max_lines, 40, "default");
    }

    #[test]
    fn validation_rules_are_readable() {
        let v = flat();
        assert_eq!(
            v.validation.rejects_unknown_keys_in("knowledge"),
            Some(true)
        );
        assert_eq!(v.validation.rejects_unknown_keys_in("working"), Some(false));
        assert!(v
            .validation
            .blockers
            .contains(&"RESOURCE_MUTATED".to_owned()));
    }

    #[test]
    fn working_extensions_are_known_only_in_working() {
        let v = flat();
        assert!(!v.is_known_key("topic"));
        assert!(v.is_known_working_key("topic"));
    }

    #[test]
    fn list_scalars_are_recognised_by_either_spelling() {
        let v = flat();
        assert!(v.scalars["aliases"].is_list());
        assert!(!v.scalars["domain"].is_list());
    }

    #[test]
    fn the_taxonomy_key_is_found_by_iri() {
        assert_eq!(flat().taxonomy_key(), Some("is-a"));
    }

    #[test]
    fn expansion_covers_the_declared_and_builtin_prefixes() {
        let v = flat();
        assert_eq!(
            v.expand("vc:requires"),
            "https://narrativegoldmine.com/ns/v1#requires"
        );
        assert_eq!(
            v.expand("skos:exactMatch"),
            "http://www.w3.org/2004/02/skos/core#exactMatch"
        );
        assert_eq!(v.expand("urn:ngm:class:x"), "urn:ngm:class:x");
        assert_eq!(v.expand("nope:thing"), "nope:thing");
    }
}
