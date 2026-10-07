//! Environment projection for the memory cloud and namespace pattern matching.
//!
//! Every knob is read through a lookup closure so tests never touch the
//! process environment. Values that fail to parse fall back to the default;
//! numeric values outside their permitted range are clamped.

/// Default total sample size (`MEMORY_CLOUD_SAMPLE`).
pub const DEFAULT_SAMPLE_TOTAL: usize = 6000;
/// Smallest permitted sample size.
pub const MIN_SAMPLE_TOTAL: usize = 500;
/// Largest permitted sample size.
pub const MAX_SAMPLE_TOTAL: usize = 20_000;
/// Default snapshot refresh interval in seconds (`MEMORY_CLOUD_REFRESH_SECS`).
pub const DEFAULT_REFRESH_SECS: u64 = 900;
/// Shortest permitted refresh interval in seconds.
pub const MIN_REFRESH_SECS: u64 = 60;
/// Default excluded namespaces (`MEMORY_CLOUD_EXCLUDE_NAMESPACES`).
pub const DEFAULT_EXCLUDED: &str = "personal-context";
/// Default OpenAI-compatible embeddings base URL (`MEMORY_CLOUD_EMBED_URL`).
pub const DEFAULT_EMBED_URL: &str = "http://xinference:9997/v1";
/// Default embedding model (`MEMORY_CLOUD_EMBED_MODEL`).
pub const DEFAULT_EMBED_MODEL: &str = "bge-small-en-v1.5";
/// Default `POST /api/memory-cloud/query` budget per pubkey per minute.
pub const DEFAULT_QUERY_PER_MINUTE: usize = 30;
/// Lowest accepted per-minute query budget.
pub const MIN_QUERY_PER_MINUTE: usize = 1;
/// Highest accepted per-minute query budget.
pub const MAX_QUERY_PER_MINUTE: usize = 600;

/// One namespace pattern: an exact name, or a prefix when written with a
/// trailing `*` (`hooks:*` matches `hooks:post-edit`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamespacePattern {
    /// Matches exactly this namespace.
    Exact(String),
    /// Matches every namespace starting with this prefix.
    Prefix(String),
}

impl NamespacePattern {
    /// Parse one pattern; a trailing `*` makes it a prefix pattern. Returns
    /// `None` for an empty pattern or a bare `*` (which would match
    /// everything and is never what an operator means by one list entry).
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        match raw.strip_suffix('*') {
            Some("") => None,
            Some(prefix) => Some(Self::Prefix(prefix.to_string())),
            None if raw.is_empty() => None,
            None => Some(Self::Exact(raw.to_string())),
        }
    }

    /// Whether `namespace` matches this pattern.
    pub fn matches(&self, namespace: &str) -> bool {
        match self {
            Self::Exact(name) => namespace == name,
            Self::Prefix(prefix) => namespace.starts_with(prefix.as_str()),
        }
    }

    /// The pattern as written (`name` or `prefix*`).
    pub fn as_written(&self) -> String {
        match self {
            Self::Exact(name) => name.clone(),
            Self::Prefix(prefix) => format!("{prefix}*"),
        }
    }
}

/// An ordered set of [`NamespacePattern`]s.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NamespacePatterns(Vec<NamespacePattern>);

impl NamespacePatterns {
    /// Parse a comma-separated list, dropping empty entries and duplicates.
    ///
    /// ```
    /// use visionclaw_memory_cloud::config::NamespacePatterns;
    /// let p = NamespacePatterns::parse_list("personal-context, hooks:*");
    /// assert!(p.matches("hooks:pre-edit"));
    /// assert!(p.matches("personal-context"));
    /// assert!(!p.matches("patterns"));
    /// ```
    pub fn parse_list(list: &str) -> Self {
        let mut out: Vec<NamespacePattern> = Vec::new();
        for pattern in list.split(',').filter_map(NamespacePattern::parse) {
            if !out.contains(&pattern) {
                out.push(pattern);
            }
        }
        Self(out)
    }

    /// Build from already-written patterns (`name` or `prefix*`).
    pub fn from_patterns<'a>(patterns: impl IntoIterator<Item = &'a str>) -> Self {
        Self::parse_list(&patterns.into_iter().collect::<Vec<_>>().join(","))
    }

    /// Whether any pattern matches `namespace`.
    pub fn matches(&self, namespace: &str) -> bool {
        self.0.iter().any(|p| p.matches(namespace))
    }

    /// The patterns as written, in configuration order.
    pub fn as_written(&self) -> Vec<String> {
        self.0.iter().map(NamespacePattern::as_written).collect()
    }

    /// Exact namespace names, for a SQL `namespace = ANY($n)` predicate.
    pub fn sql_exact(&self) -> Vec<String> {
        self.0
            .iter()
            .filter_map(|p| match p {
                NamespacePattern::Exact(name) => Some(name.clone()),
                NamespacePattern::Prefix(_) => None,
            })
            .collect()
    }

    /// Prefix patterns as SQL `LIKE` patterns (with `\` escaping of `%`, `_`
    /// and `\`), for a `namespace LIKE ANY($n)` predicate.
    ///
    /// ```
    /// use visionclaw_memory_cloud::config::NamespacePatterns;
    /// let p = NamespacePatterns::parse_list("legacy_ns/*");
    /// assert_eq!(p.sql_like_prefixes(), vec!["legacy\\_ns/%".to_string()]);
    /// ```
    pub fn sql_like_prefixes(&self) -> Vec<String> {
        self.0
            .iter()
            .filter_map(|p| match p {
                NamespacePattern::Prefix(prefix) => {
                    let mut escaped = String::with_capacity(prefix.len() + 1);
                    for ch in prefix.chars() {
                        if matches!(ch, '%' | '_' | '\\') {
                            escaped.push('\\');
                        }
                        escaped.push(ch);
                    }
                    escaped.push('%');
                    Some(escaped)
                }
                NamespacePattern::Exact(_) => None,
            })
            .collect()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Resolved memory-cloud configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryCloudConfig {
    /// Total sample size across all namespaces.
    pub sample_total: usize,
    /// Background refresh interval in seconds.
    pub refresh_secs: u64,
    /// Namespaces never sampled nor searched.
    pub excluded: NamespacePatterns,
    /// Embeddings base URL without a trailing slash.
    pub embed_url: String,
    /// Embedding model name.
    pub embed_model: String,
    /// Queries each pubkey may make per minute
    /// (`MEMORY_CLOUD_QUERY_PER_MINUTE`, default 30, clamped 1..=600).
    pub query_per_minute: usize,
}

impl MemoryCloudConfig {
    /// Resolve from a variable lookup (normally `std::env::var(..).ok()`).
    ///
    /// An empty or whitespace-only value counts as unset, matching the
    /// `${VAR:-default}` substitution in the compose file. In particular an
    /// empty `MEMORY_CLOUD_EXCLUDE_NAMESPACES` keeps the privacy default
    /// rather than silently exposing `personal-context`.
    ///
    /// ```
    /// use visionclaw_memory_cloud::config::MemoryCloudConfig;
    /// let cfg = MemoryCloudConfig::from_lookup(|k| match k {
    ///     "MEMORY_CLOUD_SAMPLE" => Some("100".into()),
    ///     _ => None,
    /// });
    /// assert_eq!(cfg.sample_total, 500); // clamped up
    /// assert!(cfg.excluded.matches("personal-context"));
    /// ```
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let get = |key: &str| lookup(key).filter(|v| !v.trim().is_empty());

        let sample_total = get("MEMORY_CLOUD_SAMPLE")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(DEFAULT_SAMPLE_TOTAL)
            .clamp(MIN_SAMPLE_TOTAL, MAX_SAMPLE_TOTAL);
        let refresh_secs = get("MEMORY_CLOUD_REFRESH_SECS")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_REFRESH_SECS)
            .max(MIN_REFRESH_SECS);
        let excluded = NamespacePatterns::parse_list(
            &get("MEMORY_CLOUD_EXCLUDE_NAMESPACES").unwrap_or_else(|| DEFAULT_EXCLUDED.into()),
        );
        let embed_url = get("MEMORY_CLOUD_EMBED_URL")
            .unwrap_or_else(|| DEFAULT_EMBED_URL.into())
            .trim()
            .trim_end_matches('/')
            .to_string();
        let embed_model = get("MEMORY_CLOUD_EMBED_MODEL")
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|| DEFAULT_EMBED_MODEL.into());
        let query_per_minute = get("MEMORY_CLOUD_QUERY_PER_MINUTE")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(DEFAULT_QUERY_PER_MINUTE)
            .clamp(MIN_QUERY_PER_MINUTE, MAX_QUERY_PER_MINUTE);

        Self {
            sample_total,
            refresh_secs,
            excluded,
            embed_url,
            embed_model,
            query_per_minute,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> MemoryCloudConfig {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        MemoryCloudConfig::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn defaults_apply_when_unset() {
        let c = cfg(&[]);
        assert_eq!(c.sample_total, 6000);
        assert_eq!(c.refresh_secs, 900);
        assert_eq!(c.excluded.as_written(), vec!["personal-context"]);
        assert_eq!(c.embed_url, "http://xinference:9997/v1");
        assert_eq!(c.embed_model, "bge-small-en-v1.5");
    }

    #[test]
    fn numeric_values_are_clamped_and_garbage_defaults() {
        assert_eq!(
            cfg(&[("MEMORY_CLOUD_SAMPLE", "999999")]).sample_total,
            20_000
        );
        assert_eq!(cfg(&[("MEMORY_CLOUD_SAMPLE", "12")]).sample_total, 500);
        assert_eq!(cfg(&[("MEMORY_CLOUD_SAMPLE", "lots")]).sample_total, 6000);
        assert_eq!(cfg(&[("MEMORY_CLOUD_SAMPLE", " 7000 ")]).sample_total, 7000);
        assert_eq!(cfg(&[("MEMORY_CLOUD_REFRESH_SECS", "5")]).refresh_secs, 60);
        assert_eq!(
            cfg(&[("MEMORY_CLOUD_REFRESH_SECS", "-5")]).refresh_secs,
            900
        );
    }

    #[test]
    fn empty_exclusion_list_keeps_privacy_default() {
        let c = cfg(&[("MEMORY_CLOUD_EXCLUDE_NAMESPACES", "  ")]);
        assert!(c.excluded.matches("personal-context"));
        let c = cfg(&[("MEMORY_CLOUD_EXCLUDE_NAMESPACES", "a, b*,,a")]);
        assert_eq!(c.excluded.as_written(), vec!["a", "b*"]);
        assert!(!c.excluded.matches("personal-context"));
    }

    #[test]
    fn embed_url_loses_trailing_slash() {
        let c = cfg(&[("MEMORY_CLOUD_EMBED_URL", "http://x:1/v1/")]);
        assert_eq!(c.embed_url, "http://x:1/v1");
    }

    #[test]
    fn patterns_match_exact_and_prefix_only() {
        let p = NamespacePatterns::parse_list("hooks:*,file-history,*, ");
        assert!(p.matches("hooks:post-edit"));
        assert!(p.matches("file-history"));
        assert!(!p.matches("file-history-2"));
        assert!(!p.matches("patterns"), "a bare * must not match everything");
        assert_eq!(p.sql_exact(), vec!["file-history"]);
        assert_eq!(p.sql_like_prefixes(), vec!["hooks:%"]);
    }

    #[test]
    fn query_budget_defaults_and_clamps() {
        assert_eq!(cfg(&[]).query_per_minute, DEFAULT_QUERY_PER_MINUTE);
        assert_eq!(
            cfg(&[("MEMORY_CLOUD_QUERY_PER_MINUTE", "0")]).query_per_minute,
            1
        );
        assert_eq!(
            cfg(&[("MEMORY_CLOUD_QUERY_PER_MINUTE", "5000")]).query_per_minute,
            600
        );
        assert_eq!(
            cfg(&[("MEMORY_CLOUD_QUERY_PER_MINUTE", " 12 ")]).query_per_minute,
            12
        );
        assert_eq!(
            cfg(&[("MEMORY_CLOUD_QUERY_PER_MINUTE", "lots")]).query_per_minute,
            30
        );
    }
}
