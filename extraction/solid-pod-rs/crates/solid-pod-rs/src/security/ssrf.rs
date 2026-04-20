//! SSRF guard (F1).
//!
//! Validates the resolved IP of a target URL against an
//! operator-configured policy before the server issues an outbound
//! request. Defaults are fail-safe: RFC 1918, RFC 4193, loopback,
//! link-local, multicast, and cloud-metadata ranges are denied.
//!
//! Upstream parity: `JavaScriptSolidServer/src/utils/ssrf.js:15-157`.
//! Design context: `docs/design/jss-parity/01-security-primitives-context.md`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use thiserror::Error;
use url::Url;

use crate::metrics::SecurityMetrics;

/// Environment variable: comma-separated hostnames (or `host:port`) whose
/// resolved IP is permitted regardless of classification. Operator
/// escape hatch for known-good internal hosts.
pub const ENV_SSRF_ALLOWLIST: &str = "SSRF_ALLOWLIST";

/// Environment variable: comma-separated hostnames whose resolved IP is
/// always denied, even when otherwise permitted by policy.
pub const ENV_SSRF_DENYLIST: &str = "SSRF_DENYLIST";

/// Environment variable: when set to `1`/`true`, permits RFC 1918 and
/// RFC 4193 private address space.
pub const ENV_SSRF_ALLOW_PRIVATE: &str = "SSRF_ALLOW_PRIVATE";

/// Environment variable: when set to `1`/`true`, permits loopback
/// (`127.0.0.0/8`, `::1`).
pub const ENV_SSRF_ALLOW_LOOPBACK: &str = "SSRF_ALLOW_LOOPBACK";

/// Environment variable: when set to `1`/`true`, permits link-local
/// (`169.254.0.0/16`, `fe80::/10`). Note: cloud-metadata endpoints on
/// link-local (169.254.169.254) are classified `Reserved` and cannot be
/// unlocked by this toggle.
pub const ENV_SSRF_ALLOW_LINK_LOCAL: &str = "SSRF_ALLOW_LINK_LOCAL";

/// Classification of an IP address against the SSRF-relevant address
/// space.
///
/// Total coverage: `IpClass::from(IpAddr)` (via [`SsrfPolicy::classify`])
/// is total — every `IpAddr` maps to exactly one variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IpClass {
    /// Publicly routable unicast (the only default-permitted class).
    Public,
    /// RFC 1918 (10/8, 172.16/12, 192.168/16) + RFC 4193 (fc00::/7).
    Private,
    /// 127.0.0.0/8 + ::1.
    Loopback,
    /// 169.254.0.0/16 + fe80::/10 (excluding well-known metadata IPs,
    /// which are classified `Reserved`).
    LinkLocal,
    /// IPv4 224.0.0.0/4 + IPv6 ff00::/8.
    Multicast,
    /// Reserved / unspecified / cloud-metadata (169.254.169.254,
    /// fd00:ec2::254) / documentation ranges / benchmarking / IETF
    /// protocol assignments.
    Reserved,
}

/// Errors emitted while evaluating an SSRF policy.
#[derive(Debug, Error)]
pub enum SsrfError {
    /// The target URL had no host component (e.g. `file:///…`).
    #[error("URL has no host component: {0}")]
    MissingHost(String),

    /// DNS resolution of the URL's host failed (propagates the OS
    /// error verbatim for operator triage).
    #[error("DNS resolution failed for host '{host}': {source}")]
    DnsFailure {
        host: String,
        #[source]
        source: std::io::Error,
    },

    /// DNS resolution returned zero addresses.
    #[error("DNS resolution returned no addresses for host '{host}'")]
    NoAddresses { host: String },

    /// The resolved IP is explicitly denylisted.
    #[error("host '{host}' (resolved to {ip}) is denylisted")]
    Denylisted { host: String, ip: IpAddr },

    /// The resolved IP falls into a blocked address class per policy.
    #[error("host '{host}' (resolved to {ip}) blocked: {class:?}")]
    BlockedClass {
        host: String,
        ip: IpAddr,
        class: IpClass,
    },
}

/// SSRF policy (aggregate root).
///
/// Immutable after construction. To change the effective policy, build
/// a new one and swap it atomically in the enclosing service state.
#[derive(Debug, Clone)]
pub struct SsrfPolicy {
    allow_private: bool,
    allow_loopback: bool,
    allow_link_local: bool,
    allowlist: Vec<String>,
    denylist: Vec<String>,
    metrics: Option<SecurityMetrics>,
}

impl SsrfPolicy {
    /// Construct a maximally restrictive default policy: all
    /// non-public classes blocked, no allowlist, no denylist, no
    /// metrics sink. Prefer [`SsrfPolicy::from_env`] for production;
    /// use [`SsrfPolicy::new`] only for tests and examples where the
    /// caller fully controls the policy shape.
    pub fn new() -> Self {
        Self {
            allow_private: false,
            allow_loopback: false,
            allow_link_local: false,
            allowlist: Vec::new(),
            denylist: Vec::new(),
            metrics: None,
        }
    }

    /// Load policy from the process environment. All toggles default
    /// to `false`; lists default to empty.
    ///
    /// - `SSRF_ALLOW_PRIVATE=1`       — permit RFC 1918 / RFC 4193
    /// - `SSRF_ALLOW_LOOPBACK=1`      — permit 127/8, ::1
    /// - `SSRF_ALLOW_LINK_LOCAL=1`    — permit 169.254/16, fe80::/10
    /// - `SSRF_ALLOWLIST=host1,host2` — hostname-keyed allowlist
    /// - `SSRF_DENYLIST=host3,host4`  — hostname-keyed denylist
    pub fn from_env() -> Self {
        Self {
            allow_private: env_bool(ENV_SSRF_ALLOW_PRIVATE),
            allow_loopback: env_bool(ENV_SSRF_ALLOW_LOOPBACK),
            allow_link_local: env_bool(ENV_SSRF_ALLOW_LINK_LOCAL),
            allowlist: env_csv(ENV_SSRF_ALLOWLIST),
            denylist: env_csv(ENV_SSRF_DENYLIST),
            metrics: None,
        }
    }

    /// Attach a metrics sink; counters are incremented on every
    /// block/deny event, labelled by [`IpClass`].
    pub fn with_metrics(mut self, metrics: SecurityMetrics) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Replace the allowlist. Hostnames are stored verbatim and
    /// compared case-insensitively at check time.
    pub fn with_allowlist(mut self, hosts: Vec<String>) -> Self {
        self.allowlist = hosts;
        self
    }

    /// Replace the denylist.
    pub fn with_denylist(mut self, hosts: Vec<String>) -> Self {
        self.denylist = hosts;
        self
    }

    /// Override the private-space toggle.
    pub fn with_allow_private(mut self, allow: bool) -> Self {
        self.allow_private = allow;
        self
    }

    /// Override the loopback toggle.
    pub fn with_allow_loopback(mut self, allow: bool) -> Self {
        self.allow_loopback = allow;
        self
    }

    /// Override the link-local toggle.
    pub fn with_allow_link_local(mut self, allow: bool) -> Self {
        self.allow_link_local = allow;
        self
    }

    /// Classify an IP. Pure, total function over `IpAddr`.
    pub fn classify(ip: IpAddr) -> IpClass {
        match ip {
            IpAddr::V4(v4) => classify_v4(v4),
            IpAddr::V6(v6) => classify_v6(v6),
        }
    }

    /// Resolve `url`'s host to an IP and enforce the policy.
    ///
    /// Returns the resolved `IpAddr` so callers can bind the
    /// subsequent socket connect to the same address, defeating DNS
    /// rebinding. On policy violation returns [`SsrfError::BlockedClass`]
    /// or [`SsrfError::Denylisted`] and increments the metrics counter
    /// labelled by the violating class.
    ///
    /// The allowlist short-circuits classification; a host on the
    /// allowlist is permitted regardless of IP class. The denylist
    /// overrides all permissive checks (including the allowlist) — a
    /// host on both lists is denied.
    pub async fn resolve_and_check(&self, url: &Url) -> Result<IpAddr, SsrfError> {
        let host = url
            .host_str()
            .ok_or_else(|| SsrfError::MissingHost(url.to_string()))?;
        let host_lower = host.to_ascii_lowercase();

        // Resolve via tokio. Use a synthetic port so `lookup_host`
        // accepts the input; we only care about the IP.
        let port = url.port_or_known_default().unwrap_or(80);
        let lookup_target = format!("{host}:{port}");
        let mut addrs = tokio::net::lookup_host(&lookup_target)
            .await
            .map_err(|e| SsrfError::DnsFailure {
                host: host.to_string(),
                source: e,
            })?;
        let sock_addr = addrs.next().ok_or_else(|| SsrfError::NoAddresses {
            host: host.to_string(),
        })?;
        let ip = sock_addr.ip();

        // Denylist first: absolute override.
        if list_contains_host(&self.denylist, &host_lower) {
            self.record_block(IpClass::Reserved);
            return Err(SsrfError::Denylisted {
                host: host.to_string(),
                ip,
            });
        }

        // Allowlist short-circuit (by hostname).
        if list_contains_host(&self.allowlist, &host_lower) {
            return Ok(ip);
        }

        let class = Self::classify(ip);
        let permitted = match class {
            IpClass::Public => true,
            IpClass::Private => self.allow_private,
            IpClass::Loopback => self.allow_loopback,
            IpClass::LinkLocal => self.allow_link_local,
            // Multicast and Reserved (incl. cloud metadata) are
            // absolute — no toggle unlocks them; operators must
            // allowlist explicitly by hostname.
            IpClass::Multicast | IpClass::Reserved => false,
        };

        if !permitted {
            self.record_block(class);
            return Err(SsrfError::BlockedClass {
                host: host.to_string(),
                ip,
                class,
            });
        }

        Ok(ip)
    }

    fn record_block(&self, class: IpClass) {
        if let Some(m) = &self.metrics {
            m.record_ssrf_block(class);
        }
    }
}

impl Default for SsrfPolicy {
    fn default() -> Self {
        Self::new()
    }
}

// --- classification ------------------------------------------------------

fn classify_v4(v4: Ipv4Addr) -> IpClass {
    let o = v4.octets();

    // Cloud metadata — AWS / GCP / Azure all use 169.254.169.254.
    // Classified `Reserved` so no toggle unlocks it; operators who
    // legitimately need it must allowlist by hostname.
    if o == [169, 254, 169, 254] {
        return IpClass::Reserved;
    }

    if v4.is_unspecified() || v4.is_broadcast() || v4.is_documentation() {
        return IpClass::Reserved;
    }
    if v4.is_loopback() {
        return IpClass::Loopback;
    }
    if v4.is_link_local() {
        return IpClass::LinkLocal;
    }
    if v4.is_multicast() {
        return IpClass::Multicast;
    }
    if v4.is_private() {
        return IpClass::Private;
    }

    // Additional IETF-reserved ranges not covered by std predicates:
    //   0.0.0.0/8          — current network
    //   100.64.0.0/10      — CGNAT (RFC 6598)
    //   192.0.0.0/24       — IETF protocol assignments (RFC 6890)
    //   192.0.2.0/24       — TEST-NET-1 (covered by is_documentation)
    //   192.88.99.0/24     — deprecated 6to4 anycast
    //   198.18.0.0/15      — benchmarking (RFC 2544)
    //   198.51.100.0/24    — TEST-NET-2 (covered by is_documentation)
    //   203.0.113.0/24     — TEST-NET-3 (covered by is_documentation)
    //   240.0.0.0/4        — reserved for future use (except broadcast)
    match o[0] {
        0 => return IpClass::Reserved,
        100 if (o[1] & 0xC0) == 0x40 => return IpClass::Reserved, // 100.64/10
        192 if o[1] == 0 && o[2] == 0 => return IpClass::Reserved,
        192 if o[1] == 88 && o[2] == 99 => return IpClass::Reserved,
        198 if o[1] == 18 || o[1] == 19 => return IpClass::Reserved,
        240..=255 => return IpClass::Reserved,
        _ => {}
    }

    IpClass::Public
}

fn classify_v6(v6: Ipv6Addr) -> IpClass {
    // AWS EC2 IMDS IPv6 endpoint: fd00:ec2::254.
    let segs = v6.segments();
    if segs == [0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254] {
        return IpClass::Reserved;
    }

    if v6.is_unspecified() {
        return IpClass::Reserved;
    }
    if v6.is_loopback() {
        return IpClass::Loopback;
    }
    if v6.is_multicast() {
        return IpClass::Multicast;
    }

    // IPv4-mapped (::ffff:0:0/96) and IPv4-compatible (::/96 low): route
    // through IPv4 classification.
    if let Some(v4) = v6.to_ipv4_mapped() {
        return classify_v4(v4);
    }

    let first = segs[0];

    // Link-local: fe80::/10
    if (first & 0xFFC0) == 0xFE80 {
        return IpClass::LinkLocal;
    }

    // Unique local: fc00::/7 (includes fd00::/8). RFC 4193.
    if (first & 0xFE00) == 0xFC00 {
        return IpClass::Private;
    }

    // Site-local (deprecated, fec0::/10) — treat as Private for safety.
    if (first & 0xFFC0) == 0xFEC0 {
        return IpClass::Private;
    }

    // Discard / documentation / reserved prefixes.
    //   100::/64               — discard-only
    //   2001:db8::/32          — documentation
    //   2001::/32 (Teredo)     — treat as Reserved (not public routable
    //                            for SSRF purposes; operators may allowlist)
    //   ::/128, ::1/128        — handled above
    if first == 0x0100 && segs[1] == 0 && segs[2] == 0 && segs[3] == 0 {
        return IpClass::Reserved;
    }
    if first == 0x2001 && segs[1] == 0x0db8 {
        return IpClass::Reserved;
    }

    IpClass::Public
}

// --- helpers -------------------------------------------------------------

fn list_contains_host(list: &[String], host_lower: &str) -> bool {
    list.iter().any(|entry| {
        let e = entry.trim().to_ascii_lowercase();
        // Allow entries of the form `host:port` — match on the host part.
        let e_host = e.split(':').next().unwrap_or(&e);
        !e_host.is_empty() && e_host == host_lower
    })
}

fn env_bool(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

fn env_csv(key: &str) -> Vec<String> {
    std::env::var(key)
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

// --- unit tests ----------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn classify_rfc1918_private() {
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))),
            IpClass::Private
        );
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))),
            IpClass::Private
        );
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))),
            IpClass::Private
        );
    }

    #[test]
    fn classify_loopback() {
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
            IpClass::Loopback
        );
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            IpClass::Loopback
        );
    }

    #[test]
    fn classify_public() {
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))),
            IpClass::Public
        );
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))),
            IpClass::Public
        );
    }

    #[test]
    fn classify_cloud_metadata() {
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))),
            IpClass::Reserved
        );
    }

    #[test]
    fn classify_ipv6_link_local() {
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V6("fe80::1".parse().unwrap())),
            IpClass::LinkLocal
        );
    }

    #[test]
    fn classify_ipv6_ula() {
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V6("fc00::1".parse().unwrap())),
            IpClass::Private
        );
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V6("fd12:3456:789a::1".parse().unwrap())),
            IpClass::Private
        );
    }

    #[test]
    fn classify_ipv6_public() {
        assert_eq!(
            SsrfPolicy::classify(IpAddr::V6("2001:4860:4860::8888".parse().unwrap())),
            IpClass::Public
        );
    }

    #[test]
    fn default_policy_blocks_private() {
        let p = SsrfPolicy::new();
        assert!(!p.allow_private);
        assert!(!p.allow_loopback);
        assert!(!p.allow_link_local);
    }
}
