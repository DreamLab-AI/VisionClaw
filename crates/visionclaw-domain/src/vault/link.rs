//! Wikilink resolution — Obsidian's own rule, as amended into
//! `docs/VAULT-corpus-format.md` §V1.
//!
//! Page identity is the vault-relative path (`Ns/Title`), but the corpus links
//! to pages **bare** (`[[Title]]`). Obsidian resolves a bare link by basename
//! across the whole vault; without that rule every bare link to a page that
//! lives in a subfolder mints a phantom `linked_page` stub beside the real
//! node, and the two never join.
//!
//! Resolution order:
//! 1. Normalise the target ([`normalise_link_target`]).
//! 2. A target containing `/` resolves by **exact path**.
//! 3. Otherwise it resolves by **basename**: unique wins; several prefer the
//!    one in the linking page's own folder, else the first in sorted path
//!    order, and the ambiguity is reported to the caller for the sync log.
//! 4. No match leaves the link unresolved and the caller mints a stub.
//!
//! Matching is case-insensitive because node ids are derived through
//! `slugify`, which lowercases — so a case-only difference could never have
//! produced two distinct nodes anyway.

use std::collections::BTreeMap;

/// Where a wikilink pointed, once resolved against the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkResolution {
    /// A page with this identity exists; link to it.
    Resolved(String),
    /// Several pages share the basename. `chosen` is the one to link
    /// (same-folder preferred, else first in sorted path order); `alternatives`
    /// lists every candidate so the sync can report the ambiguity.
    Ambiguous {
        chosen: String,
        alternatives: Vec<String>,
    },
    /// No page matches. The normalised target is returned so the caller mints
    /// its stub against a decoded name rather than the raw link text.
    Unresolved(String),
}

impl LinkResolution {
    /// The page name to link to, resolved or not.
    pub fn target(&self) -> &str {
        match self {
            LinkResolution::Resolved(id) => id,
            LinkResolution::Ambiguous { chosen, .. } => chosen,
            LinkResolution::Unresolved(name) => name,
        }
    }

    /// Did this link land on a real authored page?
    pub fn is_resolved(&self) -> bool {
        !matches!(self, LinkResolution::Unresolved(_))
    }
}

/// Normalise a raw wikilink target to a comparable page reference.
///
/// Strips the `[[…]]` wrapper if present, then the `|alias` display text, then
/// a `#heading` or `^block` anchor, and finally decodes the legacy namespace
/// encodings `___` and `%2F` to `/` — so `[[Ns___Title|shown]]` and
/// `[[Ns/Title#Section]]` both reduce to `Ns/Title`.
pub fn normalise_link_target(raw: &str) -> String {
    let mut target = raw.trim();
    if let Some(inner) = target.strip_prefix("[[") {
        target = inner.split("]]").next().unwrap_or(inner);
    }
    // Alias first: `[[Page#Heading|Shown]]` puts the alias last.
    let target = target.split('|').next().unwrap_or(target);
    let target = target.split('#').next().unwrap_or(target);
    let target = target.split('^').next().unwrap_or(target);

    target
        .trim()
        .replace("___", "/")
        .replace("%2F", "/")
        .replace("%2f", "/")
        .trim()
        .trim_matches('/')
        .to_string()
}

/// The folder part of a vault identity — everything before the last `/`, or
/// `""` for a page at the vault root.
fn parent_folder(identity: &str) -> &str {
    match identity.rfind('/') {
        Some(idx) => &identity[..idx],
        None => "",
    }
}

/// The basename part of a vault identity — everything after the last `/`.
pub fn identity_basename(identity: &str) -> &str {
    match identity.rfind('/') {
        Some(idx) => &identity[idx + 1..],
        None => identity,
    }
}

/// An index of every page identity in the vault, supporting Obsidian's
/// bare-link resolution.
///
/// Built once per sync from the full file listing — never from the changed
/// subset, or an incremental sync would fail to resolve links into unchanged
/// pages and mint stubs for them.
#[derive(Debug, Clone, Default)]
pub struct VaultIndex {
    /// lowercased identity → canonical identity
    by_identity: BTreeMap<String, String>,
    /// lowercased basename → candidate identities, sorted
    by_basename: BTreeMap<String, Vec<String>>,
}

impl VaultIndex {
    /// Build from every page identity in the vault (paths without `.md`).
    pub fn from_identities<I, S>(identities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut index = VaultIndex::default();
        for identity in identities {
            let identity = identity.into();
            if identity.is_empty() {
                continue;
            }
            index
                .by_identity
                .entry(identity.to_lowercase())
                .or_insert_with(|| identity.clone());
            index
                .by_basename
                .entry(identity_basename(&identity).to_lowercase())
                .or_default()
                .push(identity);
        }
        for candidates in index.by_basename.values_mut() {
            candidates.sort();
            candidates.dedup();
        }
        index
    }

    /// Number of distinct page identities indexed.
    pub fn len(&self) -> usize {
        self.by_identity.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_identity.is_empty()
    }

    /// Resolve a raw wikilink target as it appears on the page `from`.
    pub fn resolve(&self, raw_target: &str, from: &str) -> LinkResolution {
        let target = normalise_link_target(raw_target);
        if target.is_empty() {
            return LinkResolution::Unresolved(target);
        }

        // A path-form target addresses a page exactly.
        if target.contains('/') {
            return match self.by_identity.get(&target.to_lowercase()) {
                Some(identity) => LinkResolution::Resolved(identity.clone()),
                None => LinkResolution::Unresolved(target),
            };
        }

        // A bare target resolves by basename, Obsidian-style.
        let Some(candidates) = self.by_basename.get(&target.to_lowercase()) else {
            return LinkResolution::Unresolved(target);
        };
        match candidates.as_slice() {
            [] => LinkResolution::Unresolved(target),
            [only] => LinkResolution::Resolved(only.clone()),
            many => {
                let folder = parent_folder(from);
                let chosen = many
                    .iter()
                    .find(|candidate| parent_folder(candidate) == folder)
                    .unwrap_or(&many[0])
                    .clone();
                LinkResolution::Ambiguous {
                    chosen,
                    alternatives: many.to_vec(),
                }
            }
        }
    }
}

/// The vault as the sync sees it: the identity index plus the configured
/// source prefixes needed to derive an identity from a repository path.
///
/// Bundled because the two are always used together — deriving a page's
/// identity and then resolving that page's links against the index — and
/// passing them separately pushed the sync's per-file functions past a
/// readable arity.
#[derive(Debug, Clone, Copy)]
pub struct VaultContext<'a> {
    index: &'a VaultIndex,
    base_paths: &'a [String],
}

impl<'a> VaultContext<'a> {
    pub fn new(index: &'a VaultIndex, base_paths: &'a [String]) -> Self {
        Self { index, base_paths }
    }

    /// The vault identity (§V1) of a repository path, or `None` when the path
    /// is not a page: outside every configured base path, or under vault
    /// furniture ([`crate::vault::page_name_from_repo_path`]).
    pub fn identity_of(&self, repo_path: &str) -> Option<String> {
        crate::vault::page_name_from_repo_path(repo_path, self.base_paths)
    }

    /// The configured base paths this context resolves identities against.
    pub fn base_paths(&self) -> &'a [String] {
        self.base_paths
    }

    /// Resolve a wikilink target as it appears on the page `from`.
    pub fn resolve(&self, target: &str, from: &str) -> LinkResolution {
        self.index.resolve(target, from)
    }

    pub fn index(&self) -> &'a VaultIndex {
        self.index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> VaultIndex {
        VaultIndex::from_identities([
            "Agentic AI",
            "Security",
            "ETSI_Domain_Infrastructure/Security",
            "ETSI_Domain_Governance/Economy",
            "ETSI_Domain_Governance/Society",
            "podcast-evidence/black-friday-gpt",
        ])
    }

    // -- normalisation -------------------------------------------------------

    #[test]
    fn normalise_strips_brackets_alias_and_anchors() {
        assert_eq!(normalise_link_target("[[Page]]"), "Page");
        assert_eq!(normalise_link_target("[[Page|Shown]]"), "Page");
        assert_eq!(normalise_link_target("[[Page#Section]]"), "Page");
        assert_eq!(normalise_link_target("[[Page#Section|Shown]]"), "Page");
        assert_eq!(normalise_link_target("[[Page#^block-id]]"), "Page");
        assert_eq!(normalise_link_target("  Page  "), "Page");
    }

    #[test]
    fn normalise_decodes_legacy_namespace_encodings() {
        assert_eq!(normalise_link_target("[[Ns___Title]]"), "Ns/Title");
        assert_eq!(normalise_link_target("Ns%2FTitle"), "Ns/Title");
        assert_eq!(normalise_link_target("[[Ns___Title|shown]]"), "Ns/Title");
    }

    // -- the bug the shadow sync found ---------------------------------------

    #[test]
    fn a_bare_link_resolves_to_a_subfolder_page() {
        // The defect: the corpus links BARE, the page lives in a folder. Without
        // basename resolution this minted a stub beside the real node.
        assert_eq!(
            vault().resolve("[[black-friday-gpt]]", "AI Daily Brief"),
            LinkResolution::Resolved("podcast-evidence/black-friday-gpt".to_string())
        );
    }

    #[test]
    fn a_legacy_underscore_link_resolves_to_the_converted_folder_page() {
        // `[[ETSI_Domain_Governance___Economy]]` still appears in the corpus
        // while the page is now `ETSI_Domain_Governance/Economy.md`.
        assert_eq!(
            vault().resolve("[[ETSI_Domain_Governance___Economy]]", "Some Page"),
            LinkResolution::Resolved("ETSI_Domain_Governance/Economy".to_string())
        );
    }

    #[test]
    fn a_path_form_link_resolves_exactly() {
        assert_eq!(
            vault().resolve("[[ETSI_Domain_Governance/Society]]", "Some Page"),
            LinkResolution::Resolved("ETSI_Domain_Governance/Society".to_string())
        );
    }

    // -- ambiguity -----------------------------------------------------------

    #[test]
    fn an_ambiguous_basename_prefers_the_linking_pages_own_folder() {
        let resolved = vault().resolve("[[Security]]", "ETSI_Domain_Infrastructure/Interop");
        assert_eq!(
            resolved,
            LinkResolution::Ambiguous {
                chosen: "ETSI_Domain_Infrastructure/Security".to_string(),
                alternatives: vec![
                    "ETSI_Domain_Infrastructure/Security".to_string(),
                    "Security".to_string(),
                ],
            }
        );
        assert_eq!(resolved.target(), "ETSI_Domain_Infrastructure/Security");
        assert!(resolved.is_resolved());
    }

    #[test]
    fn an_ambiguous_basename_falls_back_to_sorted_path_order() {
        // Linking page is at the vault root and shares no folder with either
        // candidate's folder... except the root-level `Security`, which wins.
        let resolved = vault().resolve("[[Security]]", "Agentic AI");
        assert_eq!(
            resolved.target(),
            "Security",
            "same folder (root) preferred"
        );

        // From a third folder, neither matches, so sorted order decides —
        // deterministically, so the same sync twice gives the same graph.
        let resolved = vault().resolve("[[Security]]", "podcast-evidence/x");
        assert_eq!(resolved.target(), "ETSI_Domain_Infrastructure/Security");
        match resolved {
            LinkResolution::Ambiguous { alternatives, .. } => assert_eq!(alternatives.len(), 2),
            other => panic!("expected an ambiguity, got {other:?}"),
        }
    }

    // -- unresolved ----------------------------------------------------------

    #[test]
    fn an_unknown_target_stays_unresolved_for_the_caller_to_stub() {
        assert_eq!(
            vault().resolve("[[No Such Page]]", "Agentic AI"),
            LinkResolution::Unresolved("No Such Page".to_string())
        );
        // The stub is minted against the DECODED name, so a later sync that
        // adds the page joins it instead of orphaning the stub.
        assert_eq!(
            vault().resolve("[[Missing___Page]]", "Agentic AI"),
            LinkResolution::Unresolved("Missing/Page".to_string())
        );
    }

    #[test]
    fn an_unknown_path_form_target_does_not_fall_back_to_basename() {
        // `[[Wrong_Folder/Economy]]` names a page that does not exist; silently
        // rebinding it to another folder's Economy would invent an edge.
        assert_eq!(
            vault().resolve("[[Wrong_Folder/Economy]]", "Agentic AI"),
            LinkResolution::Unresolved("Wrong_Folder/Economy".to_string())
        );
    }

    #[test]
    fn resolution_is_case_insensitive_because_ids_are_slugified() {
        assert_eq!(
            vault().resolve("[[agentic ai]]", "Other"),
            LinkResolution::Resolved("Agentic AI".to_string())
        );
    }

    #[test]
    fn an_empty_or_anchor_only_target_is_unresolved() {
        assert_eq!(
            vault().resolve("[[#Section]]", "Agentic AI"),
            LinkResolution::Unresolved(String::new())
        );
    }

    // -- index ---------------------------------------------------------------

    #[test]
    fn the_index_deduplicates_and_reports_size() {
        let index = VaultIndex::from_identities(["A", "A", "Ns/B"]);
        assert_eq!(index.len(), 2);
        assert!(!index.is_empty());
        assert_eq!(identity_basename("Ns/B"), "B");
        assert_eq!(identity_basename("A"), "A");
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;

    #[test]
    fn context_derives_identity_and_resolves_against_the_index() {
        let index = VaultIndex::from_identities(["podcast-evidence/black-friday-gpt"]);
        let bases = vec!["working".to_string()];
        let ctx = VaultContext::new(&index, &bases);

        assert_eq!(
            ctx.identity_of("working/podcast-evidence/black-friday-gpt.md")
                .as_deref(),
            Some("podcast-evidence/black-friday-gpt")
        );
        assert_eq!(ctx.identity_of("working/templates/Episode.md"), None);
        assert_eq!(
            ctx.resolve("[[black-friday-gpt]]", "AI Daily Brief"),
            LinkResolution::Resolved("podcast-evidence/black-friday-gpt".to_string())
        );
        assert_eq!(ctx.index().len(), 1);
    }
}
