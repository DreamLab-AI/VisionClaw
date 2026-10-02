//! Stable corpus domain identities shared by publication and live graph consumers.

/// Domain slugs and labels in append-only publication order. Existing IDs 0–5 stay fixed.
pub const DOMAIN_ROOTS: &[(&str, &str)] = &[
    ("artificial-intelligence", "Artificial Intelligence"),
    ("blockchain", "Blockchain"),
    ("spatial-computing", "Spatial Computing"),
    ("robotics", "Robotics"),
    ("distributed-collaboration", "Distributed Collaboration"),
    ("infrastructure", "Infrastructure"),
    ("space-science-and-systems", "Space Science and Systems"),
    (
        "earth-observation-and-geospatial-sensing",
        "Earth Observation and Geospatial Sensing",
    ),
];

/// Ordered slugs derived from the single domain registry.
pub const DOMAIN_SLUGS: &[&str] = &[
    DOMAIN_ROOTS[0].0,
    DOMAIN_ROOTS[1].0,
    DOMAIN_ROOTS[2].0,
    DOMAIN_ROOTS[3].0,
    DOMAIN_ROOTS[4].0,
    DOMAIN_ROOTS[5].0,
    DOMAIN_ROOTS[6].0,
    DOMAIN_ROOTS[7].0,
];

/// GPU clustering class: zero for unknown, publication domain ID plus one otherwise.
/// Historical short names retain their existing class identities.
#[must_use]
pub fn domain_class_id(domain: &str) -> i32 {
    let canonical = match domain {
        "ai" => "artificial-intelligence",
        "bc" => "blockchain",
        "mv" => "spatial-computing",
        "rb" => "robotics",
        "ngm" => "distributed-collaboration",
        "tc" => "infrastructure",
        other => other,
    };
    DOMAIN_SLUGS
        .iter()
        .position(|s| *s == canonical)
        .and_then(|id| i32::try_from(id + 1).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_and_legacy_names_share_stable_ids_and_space_ids_are_distinct() {
        for (id, alias) in ["ai", "bc", "mv", "rb", "ngm", "tc"].iter().enumerate() {
            assert_eq!(domain_class_id(alias), i32::try_from(id + 1).unwrap());
            assert_eq!(domain_class_id(DOMAIN_SLUGS[id]), domain_class_id(alias));
        }
        assert_eq!(domain_class_id("space-science-and-systems"), 7);
        assert_eq!(
            domain_class_id("earth-observation-and-geospatial-sensing"),
            8
        );
        assert_eq!(domain_class_id(""), 0);
        assert_eq!(domain_class_id("space-science-typo"), 0);
    }
}
