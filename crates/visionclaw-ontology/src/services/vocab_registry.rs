// src/services/vocab_registry.rs
//! Well-known-vocabulary registry + PURL resolution (ADR-100 D4).
//!
//! A curated table mapping `prefix → canonical namespace IRI → PURL`,
//! adapted from ontosphere's ~55-entry well-known-ontology set. It is the
//! single source of truth the ingest pipeline consults when it encounters
//! `owl:imports` so it can resolve a referenced vocabulary to a canonical
//! namespace (and, if desired, fetch the ontology document via its PURL).
//!
//! ## Reuse discipline
//!
//! The namespace IRIs are *imported* from
//! [`crate::services::jsonld_ingest::expander`] (the `*_NS` constants) rather
//! than re-declared, so the registry can never drift from the prefix map the
//! expander applies. Only the PURL column and the entries the expander does
//! not carry (XSD, BFO, OBO) are introduced here.
//!
//! ## What a PURL is here
//!
//! A *Persistent URL* — a stable, redirecting URL under `purl.org`,
//! `w3.org`, or `purl.obolibrary.org` that dereferences to the canonical
//! ontology document. ADR-100 D1/D4 mandate standards-aligned, resolvable
//! identifiers; the PURL is the document locator, the namespace IRI is the
//! term prefix. They are frequently — but not always — identical (FOAF's
//! namespace `http://xmlns.com/foaf/0.1/` resolves directly; SKOS's
//! namespace and its PURL differ).
//!
//! ## Resolution failure is NON-FATAL
//!
//! ADR-100: "PURL resolution adds a network dependency at ingest; mitigated
//! by caching resolved vocabularies and treating resolution failure as
//! non-fatal (vocab terms degrade to local mints with a logged warning)."
//! This module owns the registry table only; the fetch+cache layer that the
//! json-ld pipeline drove was deleted with that pipeline.

use std::collections::HashMap;

use once_cell::sync::Lazy;

use crate::services::jsonld_ingest::expander::{
    DCTERMS_NS, FOAF_NS, OWL_NS, PROV_NS, RDFS_NS, RDF_NS, SCHEMA_NS, SH_NS, SKOS_NS, XSD_NS,
};

/// XSD is referenced by the expander but the canonical namespace constant
/// lives there; BFO/OBO are not in the corpus prefix map yet, so their
/// namespace IRIs are introduced here (and only here) per ADR-100 D4.
pub const BFO_NS: &str = "http://purl.obolibrary.org/obo/bfo.owl#";
/// OBO Core / OBO Foundry term base (terms mint as `obo:BFO_0000001`, etc.).
pub const OBO_NS: &str = "http://purl.obolibrary.org/obo/";

/// A single well-known-vocabulary registration (ADR-100 D4 value object).
///
/// - `prefix` — the compact prefix the corpus uses (`skos`, `prov`, `bfo`…).
/// - `namespace_iri` — the canonical term namespace the expander prepends.
/// - `purl` — the persistent, dereferenceable document locator used by
///   `owl:imports` resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyRegistration {
    pub prefix: &'static str,
    pub namespace_iri: &'static str,
    pub purl: &'static str,
}

/// The curated registry table. Order is the lookup order for
/// namespace-prefix-matching (longest-namespace-first is unnecessary because
/// the namespaces are disjoint URI authorities). ADR-100 D4 minimum set:
/// RDF, RDFS, OWL, XSD, SKOS, PROV-O, FOAF, BFO 2020, OBO, DCTERMS — plus
/// `schema` and `sh` which the expander already carries.
static REGISTRY: Lazy<Vec<VocabularyRegistration>> = Lazy::new(|| {
    vec![
        VocabularyRegistration {
            prefix: "rdf",
            namespace_iri: RDF_NS,
            purl: "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
        },
        VocabularyRegistration {
            prefix: "rdfs",
            namespace_iri: RDFS_NS,
            purl: "http://www.w3.org/2000/01/rdf-schema#",
        },
        VocabularyRegistration {
            prefix: "owl",
            namespace_iri: OWL_NS,
            purl: "http://www.w3.org/2002/07/owl#",
        },
        VocabularyRegistration {
            prefix: "xsd",
            namespace_iri: XSD_NS,
            purl: "http://www.w3.org/2001/XMLSchema#",
        },
        VocabularyRegistration {
            prefix: "skos",
            namespace_iri: SKOS_NS,
            // SKOS namespace and PURL diverge: the document lives at the
            // bare core URL (no fragment).
            purl: "http://www.w3.org/2004/02/skos/core",
        },
        VocabularyRegistration {
            prefix: "prov",
            namespace_iri: PROV_NS,
            purl: "http://www.w3.org/ns/prov-o",
        },
        VocabularyRegistration {
            prefix: "foaf",
            namespace_iri: FOAF_NS,
            purl: "http://xmlns.com/foaf/0.1/",
        },
        VocabularyRegistration {
            prefix: "bfo",
            namespace_iri: BFO_NS,
            // BFO 2020 release IRI (OBO Foundry PURL).
            purl: "http://purl.obolibrary.org/obo/bfo/2020/bfo.owl",
        },
        VocabularyRegistration {
            prefix: "obo",
            namespace_iri: OBO_NS,
            purl: "http://purl.obolibrary.org/obo/obo-basic.owl",
        },
        VocabularyRegistration {
            prefix: "dcterms",
            namespace_iri: DCTERMS_NS,
            purl: "http://purl.org/dc/terms/",
        },
        VocabularyRegistration {
            prefix: "schema",
            namespace_iri: SCHEMA_NS,
            purl: "https://schema.org/version/latest/schemaorg-current-https.jsonld",
        },
        VocabularyRegistration {
            prefix: "sh",
            namespace_iri: SH_NS,
            purl: "http://www.w3.org/ns/shacl",
        },
    ]
});

/// Prefix → registration index, built once.
static BY_PREFIX: Lazy<HashMap<&'static str, usize>> = Lazy::new(|| {
    REGISTRY
        .iter()
        .enumerate()
        .map(|(i, r)| (r.prefix, i))
        .collect()
});

/// Namespace-IRI → registration index, built once.
static BY_NAMESPACE: Lazy<HashMap<&'static str, usize>> = Lazy::new(|| {
    REGISTRY
        .iter()
        .enumerate()
        .map(|(i, r)| (r.namespace_iri, i))
        .collect()
});

/// PURL → registration index, built once.
static BY_PURL: Lazy<HashMap<&'static str, usize>> = Lazy::new(|| {
    REGISTRY
        .iter()
        .enumerate()
        .map(|(i, r)| (r.purl, i))
        .collect()
});

/// Look up a registration by its compact prefix (`"skos"`, `"bfo"`, …).
pub fn by_prefix(prefix: &str) -> Option<&'static VocabularyRegistration> {
    BY_PREFIX.get(prefix).map(|&i| &REGISTRY[i])
}

/// Look up a registration by its exact canonical namespace IRI.
pub fn by_namespace_iri(iri: &str) -> Option<&'static VocabularyRegistration> {
    BY_NAMESPACE.get(iri).map(|&i| &REGISTRY[i])
}

/// Look up a registration by its exact PURL.
pub fn by_purl(purl: &str) -> Option<&'static VocabularyRegistration> {
    BY_PURL.get(purl).map(|&i| &REGISTRY[i])
}

/// Resolve an `owl:imports` *target IRI* to a registration. The target may be
/// the namespace IRI, the PURL, or either with a trailing `#`/`/` the source
/// document tacked on — we normalise and try the registry both ways. Returns
/// `None` for an unknown import (the caller then degrades to a local mint with
/// a logged warning, per ADR-100 D4).
pub fn resolve_import_target(target: &str) -> Option<&'static VocabularyRegistration> {
    if let Some(r) = by_purl(target).or_else(|| by_namespace_iri(target)) {
        return Some(r);
    }
    // Tolerate a trailing fragment/slash mismatch between the imports IRI and
    // the registered namespace (e.g. `…/prov-o#` vs `…/prov-o`).
    let trimmed = target.trim_end_matches(['#', '/']);
    by_purl(trimmed)
        .or_else(|| by_namespace_iri(trimmed))
        .or_else(|| {
            // Or the import names the namespace authority; match by namespace
            // prefix membership.
            REGISTRY.iter().find(|r| {
                let ns = r.namespace_iri.trim_end_matches(['#', '/']);
                ns == trimmed || target.starts_with(r.namespace_iri)
            })
        })
}

/// The full registry, for callers that want to enumerate (e.g. a `/vocab`
/// diagnostics endpoint or a serialiser that emits `owl:imports` for every
/// referenced vocabulary).
pub fn all() -> &'static [VocabularyRegistration] {
    &REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_adr100_minimum_set() {
        for p in [
            "rdf", "rdfs", "owl", "xsd", "skos", "prov", "foaf", "bfo", "obo", "dcterms",
        ] {
            assert!(by_prefix(p).is_some(), "missing required vocab prefix {p}");
        }
    }

    #[test]
    fn prefix_namespace_purl_roundtrip_is_deterministic() {
        let skos = by_prefix("skos").expect("skos present");
        assert_eq!(skos.namespace_iri, SKOS_NS);
        assert_eq!(skos.purl, "http://www.w3.org/2004/02/skos/core");
        // namespace → same registration → same PURL.
        let back = by_namespace_iri(skos.namespace_iri).expect("namespace lookup");
        assert_eq!(back, skos);
        // PURL → same registration.
        let by_p = by_purl(skos.purl).expect("purl lookup");
        assert_eq!(by_p, skos);
    }

    #[test]
    fn namespace_iris_reuse_expander_constants() {
        // Guards against the registry drifting from the expander prefix map.
        assert_eq!(by_prefix("prov").unwrap().namespace_iri, PROV_NS);
        assert_eq!(by_prefix("foaf").unwrap().namespace_iri, FOAF_NS);
        assert_eq!(by_prefix("owl").unwrap().namespace_iri, OWL_NS);
        assert_eq!(by_prefix("dcterms").unwrap().namespace_iri, DCTERMS_NS);
    }

    #[test]
    fn resolve_import_target_handles_purl_namespace_and_fragment() {
        // By PURL.
        let r = resolve_import_target("http://www.w3.org/ns/prov-o").unwrap();
        assert_eq!(r.prefix, "prov");
        // By namespace IRI.
        let r2 = resolve_import_target(PROV_NS).unwrap();
        assert_eq!(r2.prefix, "prov");
        // Trailing-fragment tolerance.
        let r3 = resolve_import_target("http://www.w3.org/ns/prov-o#").unwrap();
        assert_eq!(r3.prefix, "prov");
        // BFO by its OBO PURL.
        let bfo = resolve_import_target("http://purl.obolibrary.org/obo/bfo/2020/bfo.owl").unwrap();
        assert_eq!(bfo.prefix, "bfo");
        // Unknown import → None (caller degrades to local mint).
        assert!(resolve_import_target("http://example.com/unknown-vocab").is_none());
    }

    #[test]
    fn unknown_prefix_returns_none() {
        assert!(by_prefix("zzz").is_none());
        assert!(by_namespace_iri("http://example.com/nope#").is_none());
        assert!(by_purl("http://example.com/nope").is_none());
    }
}
