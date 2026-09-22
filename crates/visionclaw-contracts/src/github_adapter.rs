//! GitHub adapter contract.
//!
//! Canonical specification: ADR-10 §D11 (transport, env vars, error envelope)
//! + DDD-08 §"To Section 10 (GitHub adapter)" (value-object fields).
//!
//! Section 10 is the transport; Section 8 owns the parse and the domain.
//! The on-disk corpus format both sides assume is the Obsidian vault
//! specified in `docs/VAULT-corpus-format.md` (ADR-2040, ADR-2112).
//! This module defines the wire shape that crosses the boundary — the
//! `ParsedMarkdown` value object that Section 10 produces and Section 8
//! consumes via the `IngestPage` / `IngestOntologyOnly` commands.
//!
//! ## Auth and sync gating
//!
//! - Transport: `octocrab` REST client.
//! - Auth: `GITHUB_TOKEN` environment variable.
//! - Sync gating: `sync_graphs()` compares each page's change marker against
//!   the cached one and skips unchanged pages (ADR-2114: the blob SHA for the
//!   GitHub source, `mtime:size` for the local vault).
//! - `FORCE_FULL_SYNC=1` bypasses gating and forces full reparse.
//!
//! ## Error reporting
//!
//! Parse errors do not fail the sync. The failing file is retained at its
//! previous good version in the triple store and a `ParseErrorReport`
//! envelope is surfaced via metrics
//! (`github_sync_parse_errors_total{error_kind}`).

use serde::{Deserialize, Serialize};

#[cfg(feature = "typescript-export")]
use ts_rs::TS;

// ---------------------------------------------------------------------------
// ParsedMarkdown value object
// ---------------------------------------------------------------------------

/// Output of the GitHub adapter for one source file.
///
/// The domain receives this via `IngestPage` / `IngestOntologyOnly`
/// commands and never sees raw HTTP responses, `octocrab` types, or
/// corpus-specific frontmatter / wikilink syntax. That insulation has been
/// exercised twice: the move from a Logseq graph to an Obsidian vault
/// (ADR-2040), and the move from a remote pull to the local vault source
/// (ADR-2114). Neither changed this value object.
///
/// The corpus format is specified by `docs/VAULT-corpus-format.md`: YAML
/// frontmatter is the one metadata carrier (ADR-2112). There is no
/// `key:: value` property syntax.
///
/// `frontmatter_json` and `jsonld_blocks` are deliberately `serde_json::Value`
/// because:
///
/// - Frontmatter is open-ended: §V2 gives meaning to a fixed set of keys and
///   preserves every other key verbatim.
/// - JSON-LD blocks must round-trip without loss for the ontology parser.
///
/// The richer DDD-08 fields (`prose_blocks`, `ontology_blocks`,
/// `outbound_wikilinks`) are domain projections built *from* this raw shape;
/// they belong in the Section 8 parser, not in this cross-boundary contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "typescript-export", derive(TS), ts(export))]
pub struct ParsedMarkdown {
    /// Repo-relative path, normalised to forward slashes.
    /// (Section 8 calls this `path`; we use `canonical_path` to make the
    /// normalisation guarantee explicit at the boundary.)
    pub canonical_path: String,
    /// Raw file body, UTF-8.
    pub raw: String,
    /// Parsed vault frontmatter as a JSON object
    /// (`docs/VAULT-corpus-format.md` §V2). Keys preserved verbatim. YAML
    /// frontmatter is the only carrier: a `key:: value` line in the body is
    /// body text and reaches the domain inside `raw`, not here.
    #[cfg_attr(feature = "typescript-export", ts(type = "Record<string, unknown>"))]
    pub frontmatter_json: serde_json::Value,
    /// JSON-LD block bodies, one per block, order preserved — plain `json-ld`
    /// code fences in the page body (§V3). Empty once the corpus is
    /// frontmatter-only (ADR-2112) and `vault_core::parse_page` supersedes the
    /// fence parser.
    #[cfg_attr(
        feature = "typescript-export",
        ts(type = "Array<Record<string, unknown>>")
    )]
    pub jsonld_blocks: Vec<serde_json::Value>,
    /// Git blob SHA1 (40 hex chars). Used by the SHA1-gated sync.
    pub commit_sha: String,
}

// ---------------------------------------------------------------------------
// Parse-error envelope
// ---------------------------------------------------------------------------

/// One parse failure surfaced by the GitHub adapter.
///
/// Errors are logged but do not fail the sync; the failed file is retained
/// at its previous good version. The shape matches ADR-10 §D11 verbatim.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "typescript-export", derive(TS), ts(export))]
pub struct ParseErrorReport {
    pub path: String,
    pub sha: String,
    pub error_kind: ParseErrorKind,
    pub message: String,
}

/// Discriminated parse-error category. Drives the metric label
/// `github_sync_parse_errors_total{error_kind}`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "typescript-export", derive(TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub enum ParseErrorKind {
    /// Frontmatter YAML failed to parse.
    Yaml,
    /// `[[Wikilink]]` syntax malformed (unbalanced brackets, empty label, …).
    Wikilink,
    /// `### OntologyBlock` body failed JSON-LD parse.
    OntologyBlock,
    /// I/O level failure — file unreadable, blob missing, etc.
    Io,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parsed_markdown_round_trips() {
        let v = ParsedMarkdown {
            canonical_path: "knowledge/pages/example.md".into(),
            raw: "public:: true\n\n# Example\n".into(),
            frontmatter_json: json!({ "public": true }),
            jsonld_blocks: vec![json!({"@id": "x", "@type": "Thing"})],
            commit_sha: "0".repeat(40),
        };
        let s = serde_json::to_string(&v).unwrap();
        let back: ParsedMarkdown = serde_json::from_str(&s).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn parse_error_kind_uses_kebab_case() {
        let r = ParseErrorReport {
            path: "x.md".into(),
            sha: "deadbeef".into(),
            error_kind: ParseErrorKind::OntologyBlock,
            message: "missing @type".into(),
        };
        let s = serde_json::to_value(&r).unwrap();
        assert_eq!(s["error_kind"], "ontology-block");
    }
}
