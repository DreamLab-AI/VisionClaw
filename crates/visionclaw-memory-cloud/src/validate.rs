//! Validation of `POST /api/memory-cloud/query` bodies.

use thiserror::Error;

use crate::config::NamespacePatterns;
use crate::wire::MemoryCloudQueryRequest;

/// Longest accepted query, in characters (after trimming).
pub const MAX_QUERY_CHARS: usize = 2000;
/// Result count when `k` is omitted.
pub const DEFAULT_K: usize = 10;
/// Largest accepted `k`.
pub const MAX_K: usize = 50;

/// A query that passed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedQuery {
    /// Trimmed query text.
    pub text: String,
    /// Result count.
    pub k: usize,
    /// Namespace restriction, if any.
    pub namespace: Option<String>,
}

/// Why a query was refused (each maps to HTTP 400).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QueryValidationError {
    /// Text was empty or whitespace.
    #[error("text must not be empty")]
    EmptyText,
    /// Text exceeded [`MAX_QUERY_CHARS`].
    #[error("text is {chars} characters; the limit is {max}")]
    TextTooLong {
        /// Characters supplied.
        chars: usize,
        /// The limit.
        max: usize,
    },
    /// `k` outside `1..=MAX_K`.
    #[error("k must be between 1 and {max}; got {k}")]
    KOutOfRange {
        /// Value supplied.
        k: i64,
        /// The limit.
        max: usize,
    },
    /// The namespace is withheld by server policy.
    #[error("namespace '{0}' is excluded by server policy")]
    NamespaceExcluded(String),
}

/// Validate a query body against the limits and the exclusion policy. A
/// blank `namespace` is treated as "all namespaces".
///
/// ```
/// use visionclaw_memory_cloud::config::NamespacePatterns;
/// use visionclaw_memory_cloud::validate::validate_query;
/// use visionclaw_memory_cloud::wire::MemoryCloudQueryRequest;
///
/// let excluded = NamespacePatterns::parse_list("personal-context");
/// let req = MemoryCloudQueryRequest { text: "  graph layout ".into(), k: None, namespace: None };
/// let q = validate_query(&req, &excluded).unwrap();
/// assert_eq!((q.text.as_str(), q.k), ("graph layout", 10));
/// ```
pub fn validate_query(
    req: &MemoryCloudQueryRequest,
    excluded: &NamespacePatterns,
) -> Result<ValidatedQuery, QueryValidationError> {
    let text = req.text.trim();
    if text.is_empty() {
        return Err(QueryValidationError::EmptyText);
    }
    let chars = text.chars().count();
    if chars > MAX_QUERY_CHARS {
        return Err(QueryValidationError::TextTooLong {
            chars,
            max: MAX_QUERY_CHARS,
        });
    }
    let k = match req.k {
        None => DEFAULT_K,
        Some(k) if (1..=MAX_K as i64).contains(&k) => k as usize,
        Some(k) => return Err(QueryValidationError::KOutOfRange { k, max: MAX_K }),
    };
    let namespace = req
        .namespace
        .as_deref()
        .map(str::trim)
        .filter(|ns| !ns.is_empty())
        .map(str::to_string);
    if let Some(ns) = &namespace {
        if excluded.matches(ns) {
            return Err(QueryValidationError::NamespaceExcluded(ns.clone()));
        }
    }
    Ok(ValidatedQuery {
        text: text.to_string(),
        k,
        namespace,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(text: &str, k: Option<i64>, ns: Option<&str>) -> MemoryCloudQueryRequest {
        MemoryCloudQueryRequest {
            text: text.into(),
            k,
            namespace: ns.map(Into::into),
        }
    }

    fn excluded() -> NamespacePatterns {
        NamespacePatterns::parse_list("personal-context,private/*")
    }

    #[test]
    fn rejects_empty_and_blank_text() {
        assert_eq!(
            validate_query(&req("", None, None), &excluded()),
            Err(QueryValidationError::EmptyText)
        );
        assert_eq!(
            validate_query(&req(" \n\t", None, None), &excluded()),
            Err(QueryValidationError::EmptyText)
        );
    }

    #[test]
    fn length_limit_counts_characters_not_bytes() {
        let ok = "é".repeat(2000);
        assert!(validate_query(&req(&ok, None, None), &excluded()).is_ok());
        let long = "é".repeat(2001);
        assert_eq!(
            validate_query(&req(&long, None, None), &excluded()),
            Err(QueryValidationError::TextTooLong {
                chars: 2001,
                max: 2000
            })
        );
    }

    #[test]
    fn k_bounds() {
        for bad in [0, -1, 51, i64::MAX] {
            assert!(matches!(
                validate_query(&req("q", Some(bad), None), &excluded()),
                Err(QueryValidationError::KOutOfRange { .. })
            ));
        }
        assert_eq!(
            validate_query(&req("q", Some(1), None), &excluded())
                .unwrap()
                .k,
            1
        );
        assert_eq!(
            validate_query(&req("q", Some(50), None), &excluded())
                .unwrap()
                .k,
            50
        );
    }

    #[test]
    fn excluded_namespaces_are_refused() {
        assert_eq!(
            validate_query(&req("q", None, Some("personal-context")), &excluded()),
            Err(QueryValidationError::NamespaceExcluded(
                "personal-context".into()
            ))
        );
        assert!(validate_query(&req("q", None, Some("private/x")), &excluded()).is_err());
        let ok = validate_query(&req("q", None, Some(" patterns ")), &excluded()).unwrap();
        assert_eq!(ok.namespace.as_deref(), Some("patterns"));
        let all = validate_query(&req("q", None, Some("  ")), &excluded()).unwrap();
        assert_eq!(all.namespace, None);
    }
}
