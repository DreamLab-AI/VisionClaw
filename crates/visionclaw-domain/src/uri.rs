//! Shared legacy `urn:ngm:*` identifier constructors.
//!
//! The converged `urn:visionclaw:*` minter lives in the server crate
//! (`src/uri/mod.rs`), which re-exports everything here under `uri::ngm` so
//! there is exactly one definition of each legacy scheme. It lives in the
//! domain crate because the *adapters* crate also mints class IRIs, and the
//! server crate is downstream of adapters — a typed mint that only the server
//! could reach would have left the adapter minting by hand, which is precisely
//! the drift ADR-2095 closes.
//!
//! **These constructors mint legacy identifiers on purpose. Nothing new should
//! use them.** New durable identifiers go through the converged
//! `urn:visionclaw:*` kinds.

/// Legacy `urn:ngm:*` identifier constructors and parsers.
pub mod ngm {
    /// `urn:ngm:class:<slug>` — the canonical legacy OWL class IRI prefix.
    ///
    /// The `<slug>` component is the lowercase, dash-separated local name
    /// produced by the callers' slugifiers (`slug()` in the Oxigraph ontology
    /// repository, `slugify()` in the elevation actor).
    pub const CLASS_PREFIX: &str = "urn:ngm:class:";

    /// Mint the canonical legacy class IRI from an already-slugified local
    /// name.
    ///
    /// Infallible by design, mirroring `ngm::node_iri`: the constructor never
    /// rewrites what it is handed, so every existing emitted IRI is reproduced
    /// byte-for-byte. Callers slugify first; [`parse_class_iri`] is the paired
    /// validator and rejects the degenerate empty slug.
    pub fn class_iri(slug: &str) -> String {
        format!("{CLASS_PREFIX}{slug}")
    }

    /// Parse a legacy class IRI back into its slug.
    ///
    /// Paired with [`class_iri`]: `parse_class_iri(&class_iri(s)) == Some(s)`
    /// for every non-empty `s`. An empty slug is rejected — `urn:ngm:class:`
    /// on its own names nothing.
    pub fn parse_class_iri(iri: &str) -> Option<&str> {
        match iri.strip_prefix(CLASS_PREFIX) {
            Some(slug) if !slug.is_empty() => Some(slug),
            _ => None,
        }
    }

    /// Is `s` already a legacy class IRI?
    ///
    /// Used by mint sites that prefer an explicit stored IRI over a
    /// label-derived one, so they can tell the two apart.
    pub fn is_class_iri(s: &str) -> bool {
        parse_class_iri(s).is_some()
    }

    /// `urn:ngm:domain:<slug>` — the legacy IRI of a domain-root node's
    /// `owl_class_iri` (ADR-2021: the corpus sync's domain roots).
    pub const DOMAIN_PREFIX: &str = "urn:ngm:domain:";

    /// Mint the legacy domain IRI from an already-slugified domain name.
    ///
    /// Infallible and non-rewriting, like [`class_iri`]: the persisted form
    /// is frozen, so the bytes are exactly `urn:ngm:domain:<slug>`.
    pub fn domain_iri(slug: &str) -> String {
        format!("{DOMAIN_PREFIX}{slug}")
    }

    /// Parse a legacy domain IRI back into its slug; the empty slug is
    /// rejected. Paired with [`domain_iri`].
    pub fn parse_domain_iri(iri: &str) -> Option<&str> {
        match iri.strip_prefix(DOMAIN_PREFIX) {
            Some(slug) if !slug.is_empty() => Some(slug),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ngm;

    /// The frozen wire form. If this test changes, every persisted class IRI
    /// in every Oxigraph store and every elevated corpus page has been
    /// invalidated — that is a migration, not an edit.
    #[test]
    fn class_iri_reproduces_the_pre_typed_literal() {
        assert_eq!(
            ngm::class_iri("finality-mechanism"),
            "urn:ngm:class:finality-mechanism"
        );
        // Byte-identical to the raw `format!("urn:ngm:class:{}", slug)` the
        // elevation actor and the ontology repository used before ADR-2095.
        for slug in ["camera", "built-environment", "transformer-models", "x"] {
            assert_eq!(ngm::class_iri(slug), format!("urn:ngm:class:{}", slug));
        }
    }

    #[test]
    fn class_iri_round_trips() {
        for slug in ["camera", "built-environment", "a-b-c-d", "0", "unnamed"] {
            let iri = ngm::class_iri(slug);
            assert!(iri.starts_with(ngm::CLASS_PREFIX));
            assert_eq!(ngm::parse_class_iri(&iri), Some(slug));
            assert!(ngm::is_class_iri(&iri));
        }
    }

    #[test]
    fn parse_class_iri_rejects_non_class_iris() {
        // Empty slug — `slugify()` can return "" for an all-punctuation label.
        assert_eq!(ngm::parse_class_iri(&ngm::class_iri("")), None);
        assert_eq!(ngm::parse_class_iri("urn:ngm:class:"), None);
        assert_eq!(ngm::parse_class_iri("urn:ngm:node:5"), None);
        assert_eq!(ngm::parse_class_iri("urn:ngm:property:has-part"), None);
        assert_eq!(
            ngm::parse_class_iri("urn:visionclaw:concept:xr:camera"),
            None
        );
        assert_eq!(ngm::parse_class_iri(""), None);
        assert!(!ngm::is_class_iri("urn:ngm:class:"));
    }

    /// A slug containing `:` still round-trips — the tail is opaque, so the
    /// parser must not split it.
    #[test]
    fn parse_class_iri_treats_the_tail_as_opaque() {
        assert_eq!(ngm::parse_class_iri("urn:ngm:class:a:b"), Some("a:b"));
    }

    /// Frozen wire form of a domain-root `owl_class_iri` (ADR-2021): the typed
    /// mint must reproduce the pre-typed `format!("urn:ngm:domain:{}", slug)`.
    #[test]
    fn domain_iri_reproduces_the_pre_typed_literal() {
        for slug in [
            "infrastructure",
            "knowledge",
            "built-environment",
            "a:b",
            "",
        ] {
            assert_eq!(ngm::domain_iri(slug), format!("urn:ngm:domain:{}", slug));
        }
        assert_eq!(
            ngm::domain_iri("infrastructure"),
            "urn:ngm:domain:infrastructure"
        );
    }

    #[test]
    fn domain_iri_round_trips_and_rejects_foreign_input() {
        for slug in ["infrastructure", "a-b", "0"] {
            assert_eq!(ngm::parse_domain_iri(&ngm::domain_iri(slug)), Some(slug));
        }
        assert_eq!(ngm::parse_domain_iri("urn:ngm:domain:"), None);
        assert_eq!(ngm::parse_domain_iri("urn:ngm:class:camera"), None);
        assert_eq!(ngm::parse_domain_iri(""), None);
    }
}
