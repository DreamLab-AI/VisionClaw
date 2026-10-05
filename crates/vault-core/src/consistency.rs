//! The Whelk consistency gate's shared vocabulary (ADR-2125).
//!
//! Two write paths decide whether a change leaves the corpus satisfiable: the
//! `vault propose` assessment and `VisionClaw`'s elevation actor. They used to
//! disagree about disjointness, and about the blocker they reported. This
//! module is the one place both read, so they cannot diverge again:
//!
//! * [`DISJOINT_WITH_KEY`] is the frontmatter key, emitted as
//!   `owl:disjointWith` and reasoned over as `DisjointClasses`.
//! * [`check_disjoint_pair`] is the **sibling-only** rule
//!   ([`DISJOINT_NOT_SIBLINGS`]): the two members must share a direct parent,
//!   and neither may be a domain root or taxonomy category. Disjoint domain
//!   roots once made 98.8% of the corpus unsatisfiable, because the corpus is
//!   a deliberate cross-domain lattice.
//! * [`whelk_inconsistent`] is the one [`WHELK_INCONSISTENT`] blocker shape,
//!   naming the unsatisfiable class.
//!
//! ```
//! use vault_core::consistency::{check_disjoint_pair, DISJOINT_NOT_SIBLINGS};
//!
//! let parent = ["urn:ngm:class:vehicle".to_owned()];
//! // Siblings under one parent: allowed.
//! assert!(check_disjoint_pair(
//!     "urn:ngm:class:car", &parent, "urn:ngm:class:boat", &parent,
//! ).is_ok());
//! // No shared parent: refused.
//! let other = ["urn:ngm:class:animal".to_owned()];
//! let err = check_disjoint_pair(
//!     "urn:ngm:class:car", &parent, "urn:ngm:class:cat", &other,
//! ).unwrap_err();
//! assert_eq!(err.code, DISJOINT_NOT_SIBLINGS);
//! ```

use crate::promotion::Blocker;

/// The frontmatter key that declares class disjointness.
pub const DISJOINT_WITH_KEY: &str = "disjoint-with";

/// The OWL IRI `disjoint-with` is emitted as.
pub const OWL_DISJOINT_WITH: &str = "http://www.w3.org/2002/07/owl#disjointWith";

/// Blocker code: a class Whelk subsumes under `owl:Nothing`.
pub const WHELK_INCONSISTENT: &str = "WHELK_INCONSISTENT";

/// Validation code: a `disjoint-with` pair that is not two siblings, or that
/// involves a domain root or taxonomy category.
pub const DISJOINT_NOT_SIBLINGS: &str = "DISJOINT_NOT_SIBLINGS";

/// The top-level domain roots; membership never implies disjointness.
pub const DOMAIN_ROOT_SLUGS: &[&str] = &[
    "artificial-intelligence",
    "spatial-computing",
    "blockchain",
    "infrastructure",
    "distributed-collaboration",
    "robotics",
    "space-science-and-systems",
    "earth-observation-and-geospatial-sensing",
];

/// The 34 intermediate taxonomy categories.
pub const CATEGORY_SLUGS: &[&str] = &[
    "ai-technique",
    "ai-model-architecture",
    "ai-application",
    "ai-governance-and-ethics",
    "cat-ai-infrastructure",
    "ai-research-area",
    "sc-display-and-rendering",
    "sc-interaction",
    "sc-content-and-assets",
    "sc-platform-and-environment",
    "sc-standards-and-interop",
    "sc-governance-and-safety",
    "bc-protocol-and-consensus",
    "bc-cryptographic-primitive",
    "bc-token-and-asset",
    "bc-defi-and-economics",
    "bc-network-component",
    "bc-governance-and-regulation",
    "infra-computing-and-cloud",
    "infra-network-and-comms",
    "infra-security-and-identity",
    "infra-data-management",
    "infra-legal-and-regulatory",
    "infra-software-engineering",
    "robo-perception",
    "robo-actuation-and-control",
    "robo-robot-type",
    "robo-navigation-and-planning",
    "robo-safety-and-standards",
    "robo-human-robot-interaction",
    "dc-communication",
    "dc-workspace-tools",
    "dc-telepresence",
    "dc-protocol-and-infra",
];

/// The comparable tail of a class reference: the segment after the last `/`,
/// `#` or `:`, lower-cased. `urn:ngm:class:Foo`,
/// `https://narrativegoldmine.com/class/foo` and a bare `foo` all reduce to
/// `foo`, which is how the corpus identifies a class across its IRI forms.
///
/// ```
/// use vault_core::consistency::class_key;
/// assert_eq!(class_key("urn:ngm:class:Ai-Technique"), "ai-technique");
/// assert_eq!(class_key("https://narrativegoldmine.com/class/robotics"), "robotics");
/// ```
#[must_use]
pub fn class_key(reference: &str) -> String {
    reference
        .trim()
        .rsplit(['/', '#', ':'])
        .next()
        .unwrap_or(reference)
        .to_lowercase()
}

/// `true` when the reference names a domain root or a taxonomy category.
///
/// ```
/// use vault_core::consistency::is_taxonomic;
/// assert!(is_taxonomic("urn:ngm:class:robotics"));
/// assert!(is_taxonomic("https://narrativegoldmine.com/class/ai-technique"));
/// assert!(!is_taxonomic("urn:ngm:class:knowledge-graph"));
/// ```
#[must_use]
pub fn is_taxonomic(reference: &str) -> bool {
    let key = class_key(reference);
    DOMAIN_ROOT_SLUGS
        .iter()
        .chain(CATEGORY_SLUGS)
        .any(|s| *s == key)
}

/// The sibling-only rule for one `member disjoint-with other` pair.
///
/// Parents are the **direct** (asserted) superclasses of each member, in any
/// IRI form [`class_key`] understands.
///
/// # Errors
/// A [`DISJOINT_NOT_SIBLINGS`] blocker when either member is a domain root or
/// taxonomy category, when a member is disjoint with itself, or when the two
/// share no direct parent.
pub fn check_disjoint_pair(
    member: &str,
    member_parents: &[String],
    other: &str,
    other_parents: &[String],
) -> Result<(), Blocker> {
    let refuse = |why: String| {
        Err(Blocker::new(
            DISJOINT_NOT_SIBLINGS,
            format!("{member} disjoint-with {other}: {why}"),
        ))
    };
    for end in [member, other] {
        if is_taxonomic(end) {
            return refuse(format!(
                "{end} is a domain root or taxonomy category; domain membership is not disjointness"
            ));
        }
    }
    if class_key(member) == class_key(other) {
        return refuse("a class cannot be disjoint with itself".to_owned());
    }
    let shared = member_parents
        .iter()
        .map(|p| class_key(p))
        .any(|p| other_parents.iter().any(|q| class_key(q) == p));
    if shared {
        Ok(())
    } else {
        refuse("the two classes share no direct parent; disjointness is for siblings".to_owned())
    }
}

/// The [`WHELK_INCONSISTENT`] blocker for one unsatisfiable class, in the one
/// shape both write paths report.
///
/// ```
/// use vault_core::consistency::{whelk_inconsistent, WHELK_INCONSISTENT};
/// let b = whelk_inconsistent("urn:ngm:class:p");
/// assert_eq!(b.code, WHELK_INCONSISTENT);
/// assert_eq!(b.to_string(), "[WHELK_INCONSISTENT] urn:ngm:class:p is subsumed by owl:Nothing");
/// ```
#[must_use]
pub fn whelk_inconsistent(class: &str) -> Blocker {
    Blocker::new(
        WHELK_INCONSISTENT,
        format!("{class} is subsumed by owl:Nothing"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parents(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn siblings_across_iri_forms_share_a_parent() {
        assert!(check_disjoint_pair(
            "urn:ngm:class:a",
            &parents(&["urn:ngm:class:parent"]),
            "https://narrativegoldmine.com/class/b",
            &parents(&["https://narrativegoldmine.com/class/parent"]),
        )
        .is_ok());
    }

    #[test]
    fn one_shared_parent_among_several_is_enough() {
        assert!(check_disjoint_pair(
            "urn:ngm:class:a",
            &parents(&["urn:ngm:class:x", "urn:ngm:class:parent"]),
            "urn:ngm:class:b",
            &parents(&["urn:ngm:class:parent", "urn:ngm:class:y"]),
        )
        .is_ok());
    }

    #[test]
    fn non_siblings_are_refused() {
        let err = check_disjoint_pair(
            "urn:ngm:class:a",
            &parents(&["urn:ngm:class:x"]),
            "urn:ngm:class:b",
            &parents(&["urn:ngm:class:y"]),
        )
        .unwrap_err();
        assert_eq!(err.code, DISJOINT_NOT_SIBLINGS);
        assert!(err.detail.contains("share no direct parent"), "{err}");
    }

    #[test]
    fn a_parentless_pair_is_refused() {
        assert!(check_disjoint_pair("urn:ngm:class:a", &[], "urn:ngm:class:b", &[]).is_err());
    }

    #[test]
    fn every_domain_root_and_category_is_refused_even_as_siblings() {
        let shared = parents(&["urn:ngm:class:root"]);
        for slug in DOMAIN_ROOT_SLUGS.iter().chain(CATEGORY_SLUGS) {
            let iri = format!("urn:ngm:class:{slug}");
            for (m, o) in [
                (iri.as_str(), "urn:ngm:class:b"),
                ("urn:ngm:class:b", iri.as_str()),
            ] {
                let err = check_disjoint_pair(m, &shared, o, &shared).unwrap_err();
                assert_eq!(err.code, DISJOINT_NOT_SIBLINGS, "{slug}");
                assert!(
                    err.detail.contains("domain root or taxonomy category"),
                    "{err}"
                );
            }
        }
        assert_eq!(DOMAIN_ROOT_SLUGS.len(), 8);
        assert_eq!(CATEGORY_SLUGS.len(), 34);
    }

    #[test]
    fn self_disjointness_is_refused() {
        let shared = parents(&["urn:ngm:class:parent"]);
        assert!(
            check_disjoint_pair("urn:ngm:class:a", &shared, "urn:ngm:class:A", &shared).is_err()
        );
    }

    #[test]
    fn the_domain_roots_are_the_published_registry() {
        let mut ours: Vec<&str> = DOMAIN_ROOT_SLUGS.to_vec();
        let mut registry: Vec<&str> = crate::domains::DOMAIN_SLUGS.to_vec();
        ours.sort_unstable();
        registry.sort_unstable();
        assert_eq!(ours, registry);
    }
}
