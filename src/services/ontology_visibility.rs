//! The ontology classes one caller may not see, and the scrubbers that make each of them read
//! exactly as a class that was never in the store (ADR-060).
//!
//! The Oxigraph store keeps a formal class whose page is private (`public: false`), so every
//! ontology read surface takes the caller's [`HiddenClasses`] and passes its data through the
//! matching method here: a hidden class is dropped, and so is every edge, axiom, binding and
//! triple that names it. Hierarchy gaps are accepted; nothing is re-parented across a hidden
//! class. An empty set (filter off, or full access) passes everything through untouched.

use std::collections::{HashMap, HashSet};

use crate::services::owl_validator::ValidationReport;
use visionclaw_adapters::provenance_emitter::ProvenanceChain;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::ports::ontology_repository::{
    InferenceResults, OwlAxiom, OwlClass, OwlProperty,
};

/// The class IRIs hidden from one caller
/// ([`crate::handlers::graph_access::hidden_class_iris`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HiddenClasses(HashSet<String>);

impl From<HashSet<String>> for HiddenClasses {
    fn from(iris: HashSet<String>) -> Self {
        Self(iris)
    }
}

impl HiddenClasses {
    /// Nothing hidden: the full-access view.
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether nothing is hidden, so every scrubber is the identity.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `iri` is hidden.
    pub fn contains(&self, iri: &str) -> bool {
        self.0.contains(iri)
    }

    /// The hidden IRIs.
    pub fn as_set(&self) -> &HashSet<String> {
        &self.0
    }

    /// Whether `axiom` names a hidden class as its subject or object.
    pub fn mentions(&self, axiom: &OwlAxiom) -> bool {
        self.contains(&axiom.subject) || self.contains(&axiom.object)
    }

    /// `iris` without the hidden ones.
    fn keep_visible(&self, iris: &mut Vec<String>) {
        iris.retain(|iri| !self.contains(iri));
    }

    /// A single-class read: `None` for a hidden class, as for one never stored.
    pub fn class(&self, class: Option<OwlClass>) -> Option<OwlClass> {
        if self.is_empty() {
            return class;
        }
        class
            .filter(|c| !self.contains(&c.iri))
            .map(|c| self.scrub_class(c))
    }

    /// A class list without the hidden classes, each survivor scrubbed of hidden IRIs.
    pub fn classes(&self, classes: Vec<OwlClass>) -> Vec<OwlClass> {
        if self.is_empty() {
            return classes;
        }
        classes
            .into_iter()
            .filter(|c| !self.contains(&c.iri))
            .map(|c| self.scrub_class(c))
            .collect()
    }

    /// `class` with every edge to a hidden class removed: parents, the ADR-014 relationship
    /// lists, the other relationships (a key left empty goes too) and IRI-valued properties.
    fn scrub_class(&self, mut class: OwlClass) -> OwlClass {
        for iris in [
            &mut class.parent_classes,
            &mut class.has_part,
            &mut class.is_part_of,
            &mut class.requires,
            &mut class.depends_on,
            &mut class.enables,
            &mut class.relates_to,
            &mut class.bridges_to,
            &mut class.bridges_from,
        ] {
            self.keep_visible(iris);
        }
        class.other_relationships.retain(|_, targets| {
            targets.retain(|t| !self.contains(t));
            !targets.is_empty()
        });
        class.properties.retain(|_, value| !self.contains(value));
        for domain in [&mut class.belongs_to_domain, &mut class.bridges_to_domain] {
            if domain.as_deref().is_some_and(|d| self.contains(d)) {
                *domain = None;
            }
        }
        class
    }

    /// A property with hidden classes removed from its domain and range.
    pub fn property(&self, mut property: OwlProperty) -> OwlProperty {
        self.keep_visible(&mut property.domain);
        self.keep_visible(&mut property.range);
        property
    }

    /// [`Self::property`] over a list.
    pub fn properties(&self, properties: Vec<OwlProperty>) -> Vec<OwlProperty> {
        if self.is_empty() {
            return properties;
        }
        properties.into_iter().map(|p| self.property(p)).collect()
    }

    /// Axioms without those that name a hidden class.
    pub fn axioms(&self, mut axioms: Vec<OwlAxiom>) -> Vec<OwlAxiom> {
        axioms.retain(|a| !self.mentions(a));
        axioms
    }

    /// Inference results without the inferred axioms that name a hidden class.
    pub fn inference_results(&self, results: Option<InferenceResults>) -> Option<InferenceResults> {
        results.map(|mut r| {
            r.inferred_axioms = self.axioms(std::mem::take(&mut r.inferred_axioms));
            r
        })
    }

    /// `(sub, sup)` pairs (the Whelk closure) without those that name a hidden class.
    pub fn pairs(&self, mut pairs: Vec<(String, String)>) -> Vec<(String, String)> {
        pairs.retain(|(sub, sup)| !self.contains(sub) && !self.contains(sup));
        pairs
    }

    /// The ontology graph without hidden class nodes or the record nodes of axioms naming one
    /// (`axiom_record_iri` maps an axiom to the IRI its record node is stored under), and
    /// without any edge touching a removed node. Node ids are renumbered densely in order so
    /// no gap marks where a node was.
    pub fn graph(
        &self,
        graph: &GraphData,
        axioms: &[OwlAxiom],
        axiom_record_iri: impl Fn(&OwlAxiom) -> String,
    ) -> GraphData {
        if self.is_empty() {
            return graph.clone();
        }
        let mut removed: HashSet<String> = self.0.clone();
        removed.extend(
            axioms
                .iter()
                .filter(|a| self.mentions(a))
                .map(axiom_record_iri),
        );
        let is_removed = |n: &visionclaw_domain::models::node::Node| {
            removed.contains(&n.metadata_id)
                || n.owl_class_iri
                    .as_deref()
                    .is_some_and(|i| removed.contains(i))
        };
        let mut renumbered: HashMap<u32, u32> = HashMap::new();
        let mut nodes = Vec::with_capacity(graph.nodes.len());
        for node in graph.nodes.iter().filter(|n| !is_removed(n)) {
            let mut node = node.clone();
            let id = u32::try_from(nodes.len() + 1).expect("node count fits the wire id");
            renumbered.insert(node.id, id);
            node.id = id;
            nodes.push(node);
        }
        let edges = graph
            .edges
            .iter()
            .filter_map(|e| {
                let (source, target) = (*renumbered.get(&e.source)?, *renumbered.get(&e.target)?);
                let mut e = e.clone();
                e.source = source;
                e.target = target;
                Some(e)
            })
            .collect();
        let mut metadata = graph.metadata.clone();
        metadata.retain(|key, _| !removed.contains(key));
        let mut id_to_metadata = graph.id_to_metadata.clone();
        id_to_metadata.retain(|_, key| !removed.contains(key));
        GraphData {
            nodes,
            edges,
            metadata,
            id_to_metadata,
        }
    }

    /// SPARQL 1.1 JSON results without any row in which a binding's value is a hidden IRI.
    pub fn sparql_json(&self, mut results: serde_json::Value) -> serde_json::Value {
        if self.is_empty() {
            return results;
        }
        if let Some(rows) = results
            .pointer_mut("/results/bindings")
            .and_then(serde_json::Value::as_array_mut)
        {
            rows.retain(|row| {
                !row.as_object().is_some_and(|bindings| {
                    bindings.values().any(|b| {
                        b.get("value")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|v| self.contains(v))
                    })
                })
            });
        }
        results
    }

    /// `/ontology/query` rows without any row in which a value is a hidden IRI.
    pub fn query_rows(
        &self,
        mut rows: Vec<HashMap<String, String>>,
    ) -> Vec<HashMap<String, String>> {
        rows.retain(|row| !row.values().any(|v| self.contains(v)));
        rows
    }

    /// A `{ triples: [{s, p, o}] }` graph read without any triple naming a hidden IRI, and
    /// without every triple of a `vc:Axiom` record node that names one.
    pub fn triples_json(&self, mut graph: serde_json::Value) -> serde_json::Value {
        const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
        const VC_AXIOM: &str = "https://narrativegoldmine.com/ns/v1#Axiom";
        if self.is_empty() {
            return graph;
        }
        let Some(triples) = graph
            .get_mut("triples")
            .and_then(serde_json::Value::as_array_mut)
        else {
            return graph;
        };
        let term = |t: &serde_json::Value, k: &str| -> String {
            t.get(k)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let records: HashSet<String> = triples
            .iter()
            .filter(|t| term(t, "p") == RDF_TYPE && term(t, "o") == VC_AXIOM)
            .map(|t| term(t, "s"))
            .collect();
        let mut removed: HashSet<String> = self.0.clone();
        removed.extend(
            triples
                .iter()
                .filter(|t| records.contains(&term(t, "s")) && self.contains(&term(t, "o")))
                .map(|t| term(t, "s")),
        );
        triples.retain(|t| {
            ["s", "p", "o"]
                .iter()
                .all(|k| !removed.contains(&term(t, k)))
        });
        graph
    }

    /// A validation report without the violations and inferred triples that name a hidden
    /// class. Its whole-graph totals (`total_triples`, `statistics`) are the validator's own and
    /// are left as computed.
    pub fn validation_report(&self, mut report: ValidationReport) -> ValidationReport {
        if self.is_empty() {
            return report;
        }
        let names = |v: &Option<String>| v.as_deref().is_some_and(|i| self.contains(i));
        report
            .violations
            .retain(|v| !names(&v.subject) && !names(&v.predicate) && !names(&v.object));
        report.inferred_triples.retain(|t| {
            !self.contains(&t.subject) && !self.contains(&t.predicate) && !self.contains(&t.object)
        });
        report
    }

    /// A provenance chain as for an entity never stored when its root is hidden (no nodes),
    /// otherwise without the nodes for or about a hidden class and without derivation links to
    /// one.
    pub fn provenance_chain(&self, mut chain: ProvenanceChain) -> ProvenanceChain {
        if self.is_empty() {
            return chain;
        }
        if self.contains(&chain.root) {
            chain.nodes.clear();
            return chain;
        }
        chain.nodes.retain(|n| {
            !self.contains(&n.entity) && !n.subject.as_deref().is_some_and(|s| self.contains(s))
        });
        for node in &mut chain.nodes {
            self.keep_visible(&mut node.derived_from);
        }
        chain
    }
}

/// Helpers the route tests share: read a response and normalise it for comparison.
#[cfg(test)]
pub(crate) mod testing {
    use actix_web::HttpResponse;
    use serde_json::Value;

    /// The status and JSON body of `response`, with every `timestamp` removed (the response
    /// envelope stamps the time of the reply) and every string mention of each `(iri, label)`
    /// replaced by `label`, so a probe that echoes its IRI compares equal to another probe.
    pub(crate) async fn reply(
        response: Result<HttpResponse, actix_web::Error>,
        echoes: &[(&str, &str)],
    ) -> (u16, Value) {
        let response = response.expect("handler returns a response");
        let status = response.status().as_u16();
        let bytes = actix_web::body::to_bytes(response.into_body())
            .await
            .expect("body reads");
        let mut body: Value = serde_json::from_slice(&bytes).expect("JSON body");
        normalise(&mut body, echoes);
        (status, body)
    }

    /// [`reply`]'s normalisation on an already-parsed value.
    pub(crate) fn normalise(value: &mut Value, echoes: &[(&str, &str)]) {
        match value {
            Value::Object(map) => {
                map.remove("timestamp");
                for v in map.values_mut() {
                    normalise(v, echoes);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| normalise(v, echoes)),
            Value::String(s) => {
                for (iri, label) in echoes {
                    if s.contains(iri) {
                        *s = s.replace(iri, label);
                    }
                }
            }
            _ => {}
        }
    }

    /// Whether `iri` appears anywhere in `value`.
    pub(crate) fn mentions(value: &Value, iri: &str) -> bool {
        value.to_string().contains(iri)
    }

    /// `value` with every array sorted by its serialised elements, so two answers that list
    /// the same things in a different (store- or hash-dependent) order compare equal.
    pub(crate) fn sorted(mut value: Value) -> Value {
        fn sort(v: &mut Value) {
            match v {
                Value::Object(map) => map.values_mut().for_each(sort),
                Value::Array(items) => {
                    items.iter_mut().for_each(sort);
                    items.sort_by_key(|a| a.to_string());
                }
                _ => {}
            }
        }
        sort(&mut value);
        value
    }
}
