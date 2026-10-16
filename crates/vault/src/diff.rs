//! Shared patch rendering for reviewable proposals.
//!
//! [`unified_diff`] renders the diff a `PatchProposal` carries. Its exact
//! inverse — the strict parser and applier that was `vault apply`'s staleness
//! guard — moved to the `ontomark` CLI with the verb (ADR-OM-005; ontomark
//! `docs/DEPRECATION.md` §2.1, row VC-29).

/// Render a unified diff with repository-relative file labels.
pub fn unified_diff(name: &str, before: &str, after: &str) -> String {
    similar::TextDiff::from_lines(before, after)
        .unified_diff()
        .header(&format!("a/{name}"), &format!("b/{name}"))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_labels_are_repository_relative() {
        let diff = unified_diff("Page", "a\n", "b\n");
        assert!(diff.starts_with("--- a/Page\n+++ b/Page\n"), "{diff}");
        assert!(diff.contains("-a\n+b\n"), "{diff}");
    }

    #[test]
    fn identical_texts_render_an_empty_diff() {
        assert_eq!(unified_diff("Page", "a\n", "a\n"), "");
    }
}
