//! Reusable IRI → node-id resolver (PRD-018 WS-2 §B, ADR-100).
//!
//! This is the lifted, public form of the `resolve_endpoint` closure that
//! previously lived inside `github_sync_service::run_post_sync_reasoning`. It is
//! promoted here, into the ontology crate, so the GPU / constraint mapper
//! (ADR-098) can resolve an axiom-endpoint IRI to its numeric node id WITHOUT
//! depending on the server binary or the GitHub sync service.
//!
//! Behaviour is identical to the original closure:
//!   1. Index every IRI form a node can be addressed by — `owl_class_iri`
//!      (typed field + `metadata["owl_class_iri"]`), `metadata["class_iri"]`,
//!      `metadata["page_iri"]`, the canonical slug (`metadata_id`), and the
//!      canonical `vc:{domain}/{slug}` IRI built from the node's `group` +
//!      `metadata_id`.
//!   2. On lookup, try the direct map; on miss, take the IRI local-name
//!      (after the last `#`, `/`, or `:`), derive its deterministic node id via
//!      the SAME hash that minted every node (`NodeIdHasher::derive_id(slugify)`),
//!      and accept that candidate ONLY if it names a real, indexed node.
//!   3. Unresolved lookups are counted and logged for the ≥95 % coverage gate.
//!
//! The map insertion order preserves the closure's `insert` vs `entry().or_insert`
//! semantics: direct-IRI forms overwrite (last write wins), while the slug /
//! canonical-IRI forms are inserted only if absent.

use std::collections::HashMap;

use visionclaw_domain::models::node::Node;

use crate::services::canonical_iri::{canonical_iri, slugify, NodeIdHasher};

/// Index of every IRI form → node id, with deterministic local-name fallback.
///
/// Build once from the current graph, then call [`IriNodeResolver::resolve`] per
/// endpoint IRI. Cheap to clone is *not* a goal — build it once and share by
/// reference.
#[derive(Debug, Default, Clone)]
pub struct IriNodeResolver {
    /// IRI (any addressable form) or canonical slug → node id.
    iri_to_node_id: HashMap<String, u32>,
    /// Reverse membership set so a hashed local-name candidate is only accepted
    /// when it names a real, indexed node (mirrors the closure's
    /// `iri_to_node_id.values().find(...)`).
    known_ids: std::collections::HashSet<u32>,
    /// Endpoints that resolved to nothing (coverage-gate metric).
    unresolved: u64,
}

impl IriNodeResolver {
    /// Empty resolver; populate with [`index_node`](Self::index_node) or build
    /// from a node slice with [`from_nodes`](Self::from_nodes).
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the resolver from the current graph's nodes — the exact indexing
    /// the original closure performed.
    pub fn from_nodes(nodes: &[Node]) -> Self {
        let mut r = Self::new();
        for node in nodes {
            r.index_node(node);
        }
        r
    }

    /// Index one node under every IRI form it can be addressed by. Idempotent.
    pub fn index_node(&mut self, node: &Node) {
        self.known_ids.insert(node.id);

        // Direct IRI forms — last write wins (closure used `insert`).
        if let Some(ref iri) = node.owl_class_iri {
            self.iri_to_node_id.insert(iri.clone(), node.id);
        }
        if let Some(iri) = node.metadata.get("owl_class_iri") {
            self.iri_to_node_id.insert(iri.clone(), node.id);
        }
        if let Some(iri) = node.metadata.get("class_iri") {
            self.iri_to_node_id.insert(iri.clone(), node.id);
        }
        if let Some(iri) = node.metadata.get("page_iri") {
            self.iri_to_node_id.insert(iri.clone(), node.id);
        }

        // Canonical slug + vc: IRI — insert only if absent (closure used
        // `entry().or_insert`).
        if !node.metadata_id.is_empty() {
            self.iri_to_node_id
                .entry(node.metadata_id.clone())
                .or_insert(node.id);
            if let Some(domain) = node.group.as_deref().filter(|g| !g.is_empty()) {
                let canon = canonical_iri(domain, &node.metadata_id);
                self.iri_to_node_id.entry(canon).or_insert(node.id);
            }
        }
    }

    /// Resolve an IRI to a node id. Tries the direct index, then the
    /// deterministic local-name hash (accepted only if it names an indexed
    /// node). Records a miss for the coverage gate. `&self` — does not mutate
    /// the index; miss accounting uses interior counting via [`resolve_counted`].
    pub fn resolve(&self, iri: &str) -> Option<u32> {
        if let Some(&id) = self.iri_to_node_id.get(iri) {
            return Some(id);
        }
        let local = iri
            .rsplit_once(['#', '/', ':'])
            .map(|(_, r)| r)
            .unwrap_or(iri);
        if local.is_empty() {
            return None;
        }
        // Same derivation that minted every node id.
        let candidate = NodeIdHasher::derive_id(&slugify(local));
        if self.known_ids.contains(&candidate) {
            Some(candidate)
        } else {
            None
        }
    }

    /// Like [`resolve`](Self::resolve) but increments the internal unresolved
    /// counter on a miss, so a batch caller can report coverage exactly as the
    /// original closure did.
    pub fn resolve_counted(&mut self, iri: &str) -> Option<u32> {
        match self.resolve(iri) {
            Some(id) => Some(id),
            None => {
                self.unresolved += 1;
                None
            }
        }
    }

    /// Count of unresolved endpoints seen via [`resolve_counted`].
    pub fn unresolved_count(&self) -> u64 {
        self.unresolved
    }

    /// Number of distinct IRI forms indexed.
    pub fn len(&self) -> usize {
        self.iri_to_node_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.iri_to_node_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use visionclaw_domain::models::node::Node;

    fn node_with(id: u32, metadata_id: &str, owl_iri: Option<&str>, group: Option<&str>) -> Node {
        Node {
            id,
            metadata_id: metadata_id.to_string(),
            owl_class_iri: owl_iri.map(|s| s.to_string()),
            group: group.map(|s| s.to_string()),
            metadata: HashMap::new(),
            ..Default::default()
        }
    }

    /// The lifted resolver returns IDENTICAL results to the old inline closure
    /// across the same fixture: direct IRI hit, metadata IRI hit, canonical vc:
    /// IRI, slug local-name fallback, and a true miss.
    #[test]
    fn resolver_matches_old_closure_behaviour() {
        // Fixture node: id minted from its slug so the local-name fallback works.
        let slug = "transformer-models";
        let id = NodeIdHasher::derive_id(slug);
        let mut node = node_with(
            id,
            slug,
            Some("urn:ngm:class:transformer-models"),
            Some("ml"),
        );
        node.metadata.insert(
            "class_iri".to_string(),
            "https://example.org/Transformer".to_string(),
        );

        let resolver = IriNodeResolver::from_nodes(&[node]);

        // Direct owl_class_iri hit.
        assert_eq!(
            resolver.resolve("urn:ngm:class:transformer-models"),
            Some(id)
        );
        // metadata class_iri hit.
        assert_eq!(
            resolver.resolve("https://example.org/Transformer"),
            Some(id)
        );
        // Canonical vc: IRI built from group + slug.
        assert_eq!(resolver.resolve(&canonical_iri("ml", slug)), Some(id));
        // Slug (metadata_id) direct hit.
        assert_eq!(resolver.resolve(slug), Some(id));
        // Local-name hash fallback: an unindexed IRI whose local name slugifies
        // to the same slug resolves through the minting hash.
        assert_eq!(
            resolver.resolve("http://other.example/Transformer_Models"),
            Some(id)
        );
        // True miss — local name names no node.
        assert_eq!(resolver.resolve("urn:ngm:class:nonexistent-thing"), None);
    }

    #[test]
    fn resolve_counted_tracks_misses() {
        let mut resolver = IriNodeResolver::new();
        assert_eq!(resolver.resolve_counted("urn:ngm:class:absent"), None);
        assert_eq!(resolver.resolve_counted("urn:ngm:class:also-absent"), None);
        assert_eq!(resolver.unresolved_count(), 2);
    }
}
