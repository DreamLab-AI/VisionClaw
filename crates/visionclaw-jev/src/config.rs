//! Flags and endpoint resolution for the Jev option.
//!
//! Everything reads through a lookup closure so tests never touch the process
//! environment. Resolution order (first match wins):
//!
//! 1. `VISIONCLAW_JEV_ENDPOINT` (+ `VISIONCLAW_JEV_MODEL`, and the key read
//!    from the env var *named* by `VISIONCLAW_JEV_KEY_ENV`);
//! 2. `AGENTBOX_SYSTEM_ONE_ENDPOINT` + `AGENTBOX_SYSTEM_ONE_MODEL` when
//!    `AGENTBOX_SYSTEM_ONE_PROXY=1` (the agentbox loopback proxy holds the
//!    vendor key, so **no** Authorization header is sent);
//! 3. `https://api.typesafe.ai/v1/systemone`, model `jev-latest`, bearer
//!    `$TYPESAFE_API_KEY`, only if that key is set;
//! 4. otherwise Jev is unavailable ([`crate::Fallback::NoEndpoint`]).
//!
//! A **local-only** item (its payload is private memory, graph or corpus
//! content) refuses steps 2 and 3 unless the endpoint is declared local with
//! `AGENTBOX_SYSTEM_ONE_BACKEND_LOCAL=1` or `VISIONCLAW_JEV_LOCAL=1`. An
//! endpoint named explicitly in step 1 is the operator's own declaration and
//! is accepted.

use std::time::Duration;

use crate::Fallback;

/// The public TypeSafe endpoint used by step 3.
pub const DEFAULT_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
/// The default model alias.
pub const DEFAULT_MODEL: &str = "jev-latest";
/// Default hard deadline for one call.
pub const DEFAULT_TIMEOUT_MS: u64 = 3_000;
/// Upper bound on the deadline: interactive and hook paths use ≤ 4 s.
pub const MAX_TIMEOUT_MS: u64 = 4_000;
/// Lower bound on the deadline.
pub const MIN_TIMEOUT_MS: u64 = 100;

/// One Jev-wired opportunity. Each has its own off-by-default flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    /// VC-38-S1-01: rerank memory-cloud hits before the answer route.
    MemoryRerank,
    /// VC-20-S1-01: rank ontology patch proposals by evidence sufficiency.
    PatchRank,
    /// VC-21-S1-01: metadata and entity-type suggestions during ingest.
    IngestSuggest,
    /// VC-24-S1-01: enrichment proposal queue triage.
    ProposalTriage,
    /// VC-08-S1-01: degraded-state and canary triage.
    HealthTriage,
}

impl Item {
    /// Short stable name for logs.
    pub fn name(self) -> &'static str {
        match self {
            Item::MemoryRerank => "memory-rerank",
            Item::PatchRank => "patch-rank",
            Item::IngestSuggest => "ingest-suggest",
            Item::ProposalTriage => "proposal-triage",
            Item::HealthTriage => "health-triage",
        }
    }

    /// The opportunity ID.
    pub fn opportunity(self) -> &'static str {
        match self {
            Item::MemoryRerank => "VC-38-S1-01",
            Item::PatchRank => "VC-20-S1-01",
            Item::IngestSuggest => "VC-21-S1-01",
            Item::ProposalTriage => "VC-24-S1-01",
            Item::HealthTriage => "VC-08-S1-01",
        }
    }

    /// The environment flag that enables the item (`1` or `true`).
    pub fn flag_var(self) -> &'static str {
        match self {
            Item::MemoryRerank => "VISIONCLAW_JEV_MEMORY_RERANK",
            Item::PatchRank => "VISIONCLAW_JEV_PATCH_RANK",
            Item::IngestSuggest => "VISIONCLAW_JEV_INGEST_SUGGEST",
            Item::ProposalTriage => "VISIONCLAW_JEV_PROPOSAL_TRIAGE",
            Item::HealthTriage => "VISIONCLAW_JEV_HEALTH_TRIAGE",
        }
    }

    /// Whether the payload is private content, so only a declared-local
    /// endpoint may receive it (steps 2 and 3 are refused otherwise).
    ///
    /// Memory, ontology-patch, ingest-page and proposal content all come from
    /// the private graph/corpus. Health triage sends sanitised canary results.
    pub fn local_only(self) -> bool {
        !matches!(self, Item::HealthTriage)
    }
}

/// Where a resolved endpoint came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `VISIONCLAW_JEV_ENDPOINT`.
    Config,
    /// The agentbox loopback proxy.
    AgentboxProxy,
    /// The public TypeSafe API with `TYPESAFE_API_KEY`.
    TypeSafe,
}

/// A usable endpoint.
#[derive(Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Full `…/v1/systemone` URL.
    pub url: String,
    /// Model name or alias.
    pub model: String,
    /// Bearer token; `None` for the proxy and for unauthenticated local ones.
    pub bearer: Option<String>,
    /// Which resolution step matched.
    pub source: Source,
    /// Hard deadline for one call.
    pub timeout: Duration,
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the bearer.
        f.debug_struct("Resolved")
            .field("url", &self.url)
            .field("model", &self.model)
            .field("bearer", &self.bearer.as_ref().map(|_| "<set>"))
            .field("source", &self.source)
            .field("timeout", &self.timeout)
            .finish()
    }
}

fn truthy(v: Option<String>) -> bool {
    matches!(
        v.as_deref().map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("1") | Some("true")
    )
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Whether the item's flag is on.
pub fn enabled(item: Item, lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    truthy(lookup(item.flag_var()))
}

/// Normalise a base URL or full endpoint to `…/v1/systemone`.
pub fn systemone_url(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    if b.ends_with("/v1/systemone") {
        b.to_string()
    } else {
        format!("{b}/v1/systemone")
    }
}

/// The per-call deadline: `VISIONCLAW_JEV_TIMEOUT_MS`, clamped to
/// [`MIN_TIMEOUT_MS`]..=[`MAX_TIMEOUT_MS`]; unparsable falls back to the default.
pub fn timeout(lookup: &dyn Fn(&str) -> Option<String>) -> Duration {
    let ms = non_empty(lookup("VISIONCLAW_JEV_TIMEOUT_MS"))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS);
    Duration::from_millis(ms)
}

/// Read a threshold in `0.0..=1.0` from `var`, or `default`.
pub fn threshold(lookup: &dyn Fn(&str) -> Option<String>, var: &str, default: f64) -> f64 {
    non_empty(lookup(var))
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(0.0, 1.0))
        .unwrap_or(default)
}

/// Read an unsigned count from `var`, clamped to `1..=max`, or `default`.
pub fn count(lookup: &dyn Fn(&str) -> Option<String>, var: &str, default: usize, max: usize) -> usize {
    non_empty(lookup(var))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(1, max)
}

/// Resolve the endpoint for `item`, or the [`Fallback`] reason.
///
/// # Errors
///
/// [`Fallback::FlagOff`] when the item is not enabled; [`Fallback::NoEndpoint`]
/// when nothing resolves (including a local-only item whose only candidate
/// endpoint is not declared local).
pub fn resolve(item: Item, lookup: &dyn Fn(&str) -> Option<String>) -> Result<Resolved, Fallback> {
    if !enabled(item, lookup) {
        return Err(Fallback::FlagOff);
    }
    let timeout = timeout(lookup);
    // `VISIONCLAW_JEV_ALLOW_CLOUD=1` is the operator's explicit acceptance that
    // a local-only item may use a non-local endpoint (allow_cloud).
    let declared_local = truthy(lookup("AGENTBOX_SYSTEM_ONE_BACKEND_LOCAL"))
        || truthy(lookup("VISIONCLAW_JEV_LOCAL"))
        || truthy(lookup("VISIONCLAW_JEV_ALLOW_CLOUD"));

    // 1. explicit config
    if let Some(endpoint) = non_empty(lookup("VISIONCLAW_JEV_ENDPOINT")) {
        let bearer = non_empty(lookup("VISIONCLAW_JEV_KEY_ENV"))
            .and_then(|name| non_empty(lookup(&name)));
        return Ok(Resolved {
            url: systemone_url(&endpoint),
            model: non_empty(lookup("VISIONCLAW_JEV_MODEL")).unwrap_or_else(|| DEFAULT_MODEL.into()),
            bearer,
            source: Source::Config,
            timeout,
        });
    }
    if item.local_only() && !declared_local {
        return Err(Fallback::NoEndpoint);
    }
    // 2. agentbox loopback proxy: no Authorization header
    if truthy(lookup("AGENTBOX_SYSTEM_ONE_PROXY")) {
        if let (Some(endpoint), Some(model)) = (
            non_empty(lookup("AGENTBOX_SYSTEM_ONE_ENDPOINT")),
            non_empty(lookup("AGENTBOX_SYSTEM_ONE_MODEL")),
        ) {
            return Ok(Resolved {
                url: systemone_url(&endpoint),
                model,
                bearer: None,
                source: Source::AgentboxProxy,
                timeout,
            });
        }
    }
    // 3. the public API, only with a key
    if let Some(key) = non_empty(lookup("TYPESAFE_API_KEY")) {
        return Ok(Resolved {
            url: DEFAULT_ENDPOINT.into(),
            model: DEFAULT_MODEL.into(),
            bearer: Some(key),
            source: Source::TypeSafe,
            timeout,
        });
    }
    Err(Fallback::NoEndpoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn flag_off_is_the_default_even_with_every_endpoint_set() {
        let e = env(&[
            ("VISIONCLAW_JEV_ENDPOINT", "http://x"),
            ("TYPESAFE_API_KEY", "k"),
            ("AGENTBOX_SYSTEM_ONE_PROXY", "1"),
        ]);
        for item in [Item::MemoryRerank, Item::PatchRank, Item::IngestSuggest, Item::ProposalTriage, Item::HealthTriage] {
            assert_eq!(resolve(item, &e), Err(Fallback::FlagOff));
        }
    }

    #[test]
    fn no_endpoint_when_enabled_but_nothing_configured() {
        let e = env(&[("VISIONCLAW_JEV_HEALTH_TRIAGE", "1")]);
        assert_eq!(resolve(Item::HealthTriage, &e), Err(Fallback::NoEndpoint));
    }

    #[test]
    fn explicit_config_wins_and_reads_the_named_key() {
        let e = env(&[
            ("VISIONCLAW_JEV_MEMORY_RERANK", "true"),
            ("VISIONCLAW_JEV_ENDPOINT", "http://loom:8097/"),
            ("VISIONCLAW_JEV_MODEL", "jev-1.14.0"),
            ("VISIONCLAW_JEV_KEY_ENV", "MY_KEY"),
            ("MY_KEY", "secret"),
            ("TYPESAFE_API_KEY", "other"),
            ("AGENTBOX_SYSTEM_ONE_PROXY", "1"),
            ("AGENTBOX_SYSTEM_ONE_ENDPOINT", "http://p"),
            ("AGENTBOX_SYSTEM_ONE_MODEL", "m"),
        ]);
        let r = resolve(Item::MemoryRerank, &e).unwrap();
        assert_eq!(r.url, "http://loom:8097/v1/systemone");
        assert_eq!(r.model, "jev-1.14.0");
        assert_eq!(r.bearer.as_deref(), Some("secret"));
        assert_eq!(r.source, Source::Config);
        assert!(!format!("{r:?}").contains("secret"));
    }

    #[test]
    fn proxy_sends_no_authorization() {
        let e = env(&[
            ("VISIONCLAW_JEV_PROPOSAL_TRIAGE", "1"),
            ("AGENTBOX_SYSTEM_ONE_PROXY", "1"),
            ("AGENTBOX_SYSTEM_ONE_ENDPOINT", "http://127.0.0.1:8097/v1/systemone"),
            ("AGENTBOX_SYSTEM_ONE_MODEL", "jev-latest"),
            ("AGENTBOX_SYSTEM_ONE_BACKEND_LOCAL", "1"),
            ("TYPESAFE_API_KEY", "k"),
        ]);
        let r = resolve(Item::ProposalTriage, &e).unwrap();
        assert_eq!(r.source, Source::AgentboxProxy);
        assert_eq!(r.bearer, None);
    }

    #[test]
    fn proxy_requires_the_proxy_flag_endpoint_and_model() {
        let e = env(&[
            ("VISIONCLAW_JEV_HEALTH_TRIAGE", "1"),
            ("AGENTBOX_SYSTEM_ONE_ENDPOINT", "http://p"),
            ("AGENTBOX_SYSTEM_ONE_MODEL", "m"),
        ]);
        assert_eq!(resolve(Item::HealthTriage, &e), Err(Fallback::NoEndpoint));
        let e = env(&[
            ("VISIONCLAW_JEV_HEALTH_TRIAGE", "1"),
            ("AGENTBOX_SYSTEM_ONE_PROXY", "1"),
            ("AGENTBOX_SYSTEM_ONE_ENDPOINT", "http://p"),
        ]);
        assert_eq!(resolve(Item::HealthTriage, &e), Err(Fallback::NoEndpoint));
    }

    #[test]
    fn typesafe_needs_a_key_and_uses_bearer() {
        let e = env(&[("VISIONCLAW_JEV_HEALTH_TRIAGE", "1"), ("TYPESAFE_API_KEY", "tk")]);
        let r = resolve(Item::HealthTriage, &e).unwrap();
        assert_eq!(r.url, DEFAULT_ENDPOINT);
        assert_eq!(r.model, DEFAULT_MODEL);
        assert_eq!(r.bearer.as_deref(), Some("tk"));
    }

    #[test]
    fn local_only_items_refuse_proxy_and_typesafe_unless_declared_local() {
        for item in [Item::MemoryRerank, Item::PatchRank, Item::IngestSuggest, Item::ProposalTriage] {
            let flag = item.flag_var();
            let e = env(&[
                (flag, "1"),
                ("TYPESAFE_API_KEY", "tk"),
                ("AGENTBOX_SYSTEM_ONE_PROXY", "1"),
                ("AGENTBOX_SYSTEM_ONE_ENDPOINT", "http://p"),
                ("AGENTBOX_SYSTEM_ONE_MODEL", "m"),
            ]);
            assert_eq!(resolve(item, &e), Err(Fallback::NoEndpoint), "{item:?}");
            let e = env(&[
                (flag, "1"),
                ("VISIONCLAW_JEV_LOCAL", "1"),
                ("AGENTBOX_SYSTEM_ONE_PROXY", "1"),
                ("AGENTBOX_SYSTEM_ONE_ENDPOINT", "http://p"),
                ("AGENTBOX_SYSTEM_ONE_MODEL", "m"),
            ]);
            assert_eq!(resolve(item, &e).unwrap().source, Source::AgentboxProxy);
        }
    }

    #[test]
    fn allow_cloud_is_the_explicit_override_for_local_only_items() {
        let e = env(&[("VISIONCLAW_JEV_PATCH_RANK", "1"), ("TYPESAFE_API_KEY", "tk")]);
        assert_eq!(resolve(Item::PatchRank, &e), Err(Fallback::NoEndpoint));
        let e = env(&[("VISIONCLAW_JEV_PATCH_RANK", "1"), ("TYPESAFE_API_KEY", "tk"), ("VISIONCLAW_JEV_ALLOW_CLOUD", "1")]);
        assert_eq!(resolve(Item::PatchRank, &e).unwrap().source, Source::TypeSafe);
    }

    #[test]
    fn timeout_is_clamped_to_the_interactive_budget() {
        assert_eq!(timeout(&env(&[])), Duration::from_millis(3_000));
        assert_eq!(timeout(&env(&[("VISIONCLAW_JEV_TIMEOUT_MS", "60000")])), Duration::from_millis(4_000));
        assert_eq!(timeout(&env(&[("VISIONCLAW_JEV_TIMEOUT_MS", "1")])), Duration::from_millis(100));
        assert_eq!(timeout(&env(&[("VISIONCLAW_JEV_TIMEOUT_MS", "abc")])), Duration::from_millis(3_000));
    }

    #[test]
    fn thresholds_and_counts_parse_and_clamp() {
        let e = env(&[("T", "0.4"), ("BAD", "x"), ("BIG", "9"), ("N", "500")]);
        assert_eq!(threshold(&e, "T", 0.1), 0.4);
        assert_eq!(threshold(&e, "BAD", 0.1), 0.1);
        assert_eq!(threshold(&e, "BIG", 0.1), 1.0);
        assert_eq!(count(&e, "N", 5, 50), 50);
        assert_eq!(count(&e, "MISSING", 5, 50), 5);
    }
}
