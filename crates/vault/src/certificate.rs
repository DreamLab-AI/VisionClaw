//! The derivation certificate of a built graph (ADR-2147), over the
//! `rdfs-certificate` crate shared with ontomark (ADR-OM-009).
//!
//! Whelk ([`crate::whelk`]) remains the reasoner that gates the corpus. The
//! certificate is the **warrant** for the closure it computes: an `oo-cert/1`
//! directory an independent Lean 4 checker re-derives with no engine and no
//! network. On the published corpus the RDFS table reproduces 100% of Whelk's
//! named-class closure, because the emitter puts restrictions only on the
//! superclass side of `rdfs:subClassOf`; `vault certify` enforces that parity.

pub use rdfs_certificate::{
    consequence_delta, Certificate, ConsequenceDelta, Summary, CONSERVATIVE, EXAMPLE_CAP,
    FORMAT, NOT_CONSERVATIVE, RULE_TABLE,
};
use rdfs_certificate::nt;

use crate::build::turtle::{Graph, Subject, Term};

/// Spell every triple of `graph` as N-Triples terms, in the graph's order.
#[must_use]
pub fn spell(graph: &Graph) -> Vec<(String, String, String)> {
    graph
        .iter()
        .map(|(s, p, o)| {
            let s = match s {
                Subject::Iri(i) => nt::iri(i),
                Subject::Blank(b) => nt::blank(b),
            };
            let o = match o {
                Term::Iri(i) => nt::iri(i),
                Term::Blank(b) => nt::blank(b),
                Term::Literal {
                    value,
                    lang,
                    datatype,
                } => nt::literal(value, lang.as_deref(), datatype.as_deref()),
            };
            (s, nt::iri(p), o)
        })
        .collect()
}

/// Reason `graph` to its RDFS fixpoint, recording every step.
#[must_use]
pub fn certify(graph: &Graph) -> Certificate {
    let spelled = spell(graph);
    rdfs_certificate::certify(
        spelled
            .iter()
            .map(|(s, p, o)| (s.as_str(), p.as_str(), o.as_str())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";

    fn ex(local: &str) -> String {
        format!("urn:ex:{local}")
    }

    fn t(local: &str) -> Term {
        Term::Iri(ex(local))
    }

    #[test]
    fn spelling_escapes_literals_and_keeps_blank_nodes() {
        let mut g = Graph::new();
        let b = g.fresh_blank();
        g.add_iri(&ex("a"), RDFS_SUBCLASS_OF, Term::Blank(b.clone()));
        g.add(
            Subject::Blank(b),
            "http://www.w3.org/2000/01/rdf-schema#label",
            Term::Literal {
                value: "tab\there".into(),
                lang: Some("en".into()),
                datatype: None,
            },
        );
        let s = spell(&g);
        assert_eq!(s.len(), 2);
        assert!(s[0].2.starts_with("_:"));
        assert_eq!(s[1].2, "\"tab\\there\"@en");
        assert_eq!(certify(&g).derivation_count(), 0);
    }

    #[test]
    fn whelk_and_the_certificate_agree_on_the_named_closure() {
        let owl = |l: &str| Term::Iri(format!("http://www.w3.org/2002/07/owl#{l}"));
        let mut g = Graph::new();
        for c in ["a", "b", "c", "d", "e", "x"] {
            g.add_iri(&ex(c), RDF_TYPE, owl("Class"));
        }
        g.add_iri(&ex("p"), RDF_TYPE, owl("ObjectProperty"));
        for (s, o) in [("a", "b"), ("b", "c"), ("c", "d"), ("x", "c"), ("e", "a")] {
            g.add_iri(&ex(s), RDFS_SUBCLASS_OF, t(o));
        }
        let r = g.fresh_blank();
        g.add_iri(&ex("b"), RDFS_SUBCLASS_OF, Term::Blank(r.clone()));
        g.add(
            Subject::Blank(r.clone()),
            "http://www.w3.org/2002/07/owl#onProperty",
            t("p"),
        );
        g.add(
            Subject::Blank(r),
            "http://www.w3.org/2002/07/owl#someValuesFrom",
            t("d"),
        );
        let whelk: BTreeSet<(String, String)> =
            crate::whelk::reason(&g).unwrap().inferred.into_iter().collect();
        assert!(!whelk.is_empty());
        assert_eq!(certify(&g).named_subclass_closure(), whelk);
    }
}
