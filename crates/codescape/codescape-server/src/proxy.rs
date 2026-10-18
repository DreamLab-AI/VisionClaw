//! Reverse-proxy trust (`--trust-proxy`, `--trust-proxy-from`, `--public-path-prefix`).
//!
//! Codescape is reachable directly on its own port and, optionally, through a
//! reverse proxy that strips a path prefix (the VisionClaw dev nginx serves it
//! at `/codescape/`). Behind such a proxy the TCP peer is the proxy, `Host` has
//! lost the dialled port, and the origin the browser sees is the proxy's. The
//! proxy restores that information in `X-Forwarded-Host`, `X-Forwarded-Proto`
//! and `X-Forwarded-For`.
//!
//! Those headers are client-controlled whenever the server is reached without
//! the proxy, so they are honoured only when trust is switched on. With trust
//! off, every forwarded header is ignored and the behaviour is the direct one:
//! `Host` for the same-origin check, the TCP peer for rate limits, and the
//! bind-derived or configured public origin for NIP-98 URLs.
//!
//! Trust can be narrowed to the proxy's address with `--trust-proxy-from`
//! (addresses, CIDRs or hostnames; hostnames are resolved at boot and every
//! [`PEER_REFRESH`], and match nothing while they do not resolve): a request from any other peer, or from an unknown
//! peer, is then treated exactly as with trust off. With trust applying:
//!
//! * the same-origin check compares `Origin` with `X-Forwarded-Host` (falling
//!   back to `Host`);
//! * the public origin is `X-Forwarded-Proto://X-Forwarded-Host` plus
//!   `X-Forwarded-Prefix` (or `--public-path-prefix` when the proxy sends none
//!   or a malformed one), unless `--public-origin` is set (an explicit origin
//!   always wins);
//! * per-address limits key on the **last** `X-Forwarded-For` hop. nginx's
//!   `$proxy_add_x_forwarded_for` *appends* the peer it saw, so the leftmost
//!   entries are whatever the client sent and the last one is the only hop the
//!   trusted proxy vouches for.
//!
//! # Example
//!
//! ```
//! use actix_web::http::header::{HeaderMap, HeaderName, HeaderValue};
//! use codescape_server::proxy::{ProxyTrust, PublicOrigin};
//!
//! let mut h = HeaderMap::new();
//! h.insert(HeaderName::from_static("host"), HeaderValue::from_static("box"));
//! h.insert(HeaderName::from_static("x-forwarded-host"), HeaderValue::from_static("box:3001"));
//! h.insert(HeaderName::from_static("x-forwarded-for"), HeaderValue::from_static("1.2.3.4, 10.0.0.7"));
//!
//! let nginx = Some("172.18.0.2".parse().unwrap());
//! let off = ProxyTrust::off();
//! assert_eq!(off.host(&h, nginx), Some("box"));
//! assert_eq!(off.client_ip(&h, nginx), nginx);
//!
//! let on = ProxyTrust::new(true, "/codescape/").unwrap();
//! assert_eq!(on.host(&h, nginx), Some("box:3001"));
//! assert_eq!(on.client_ip(&h, nginx), Some("10.0.0.7".parse().unwrap()));
//! let public = PublicOrigin::new(None, "http://localhost:4100".into(), on.clone());
//! assert_eq!(public.for_request(&h, nginx), "http://box:3001/codescape");
//!
//! // Narrowed to the nginx peer: another peer's headers are ignored.
//! let narrow = on.from_peers(vec![codescape_server::proxy::parse_peer("172.18.0.2").unwrap()]);
//! assert_eq!(narrow.host(&h, nginx), Some("box:3001"));
//! assert_eq!(narrow.host(&h, Some("172.18.0.9".parse().unwrap())), Some("box"));
//! ```

use std::net::IpAddr;

use actix_web::http::header::HeaderMap;
use ipnet::IpNet;
use reqwest::Url;

/// `X-Forwarded-Host`.
pub const X_FORWARDED_HOST: &str = "x-forwarded-host";
/// `X-Forwarded-Proto`.
pub const X_FORWARDED_PROTO: &str = "x-forwarded-proto";
/// `X-Forwarded-For`.
pub const X_FORWARDED_FOR: &str = "x-forwarded-for";
/// `X-Forwarded-Prefix`.
pub const X_FORWARDED_PREFIX: &str = "x-forwarded-prefix";

/// Normalise `--public-path-prefix`: `""`, `"/"` → `""`; `"codescape/"` →
/// `"/codescape"`. Only path-safe characters (`A-Z a-z 0-9 - _ . ~ /`) are
/// accepted, and no `.`/`..` segment, so the prefix can be pasted into a URL verbatim.
pub fn normalise_prefix(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let ok_chars = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '/'));
    let ok_segments = trimmed
        .split('/')
        .all(|s| !s.is_empty() && s != "." && s != "..");
    if !ok_chars || !ok_segments || trimmed.len() > 128 {
        return Err(format!(
            "public path prefix `{raw}` must be a plain path such as /codescape"
        ));
    }
    Ok(format!("/{trimmed}"))
}

/// First comma-separated value of a header, trimmed, if it is non-empty ASCII.
fn first_value<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let v = h.get(name)?.to_str().ok()?;
    let first = v.split(',').next()?.trim();
    (!first.is_empty()).then_some(first)
}

/// `true` when `authority` is a bare `host[:port]` (no user info, path, query or fragment).
fn is_authority(authority: &str) -> bool {
    if authority.contains(['/', '?', '#', '@', ' ', '\\']) {
        return false;
    }
    Url::parse(&format!("http://{authority}"))
        .ok()
        .is_some_and(|u| u.host_str().is_some() && u.path() == "/")
}

/// How often hostname entries of `--trust-proxy-from` are re-resolved.
pub const PEER_REFRESH: std::time::Duration = std::time::Duration::from_secs(60);

/// One `--trust-proxy-from` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerSpec {
    /// An address (as a /32 or /128) or a CIDR.
    Net(IpNet),
    /// A hostname (`visionclaw_container`), resolved at boot and every [`PEER_REFRESH`].
    Host(String),
}

impl std::fmt::Display for PeerSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Net(n) => write!(f, "{n}"),
            Self::Host(h) => write!(f, "{h}"),
        }
    }
}

/// `true` for a syntactically valid DNS or Docker service name.
fn is_hostname(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

/// Parse one `--trust-proxy-from` entry: a CIDR (`172.18.0.0/16`), a bare
/// address (one host) or a hostname. Nothing is resolved here.
pub fn parse_peer(raw: &str) -> Result<PeerSpec, String> {
    let raw = raw.trim();
    if let Ok(n) = raw.parse::<IpNet>() {
        return Ok(PeerSpec::Net(n));
    }
    if let Ok(a) = raw.parse::<IpAddr>() {
        return Ok(PeerSpec::Net(IpNet::from(a)));
    }
    // Anything with a `/` or `:` was meant as an address or CIDR and is malformed.
    if !raw.contains(['/', ':']) && is_hostname(raw) {
        return Ok(PeerSpec::Host(raw.to_ascii_lowercase()));
    }
    Err(format!(
        "trusted proxy `{raw}` must be an address, a CIDR or a hostname"
    ))
}

/// The addresses `host` resolves to through the system resolver
/// (`ToSocketAddrs`, so `/etc/hosts` and Docker DNS); an error when it does not resolve.
pub fn resolve_host(host: &str) -> std::io::Result<Vec<IpAddr>> {
    use std::net::ToSocketAddrs;
    let mut out: Vec<IpAddr> = (host, 0u16).to_socket_addrs()?.map(|a| a.ip()).collect();
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// Whether forwarded headers are honoured, from which peers, and the prefix the proxy strips.
/// Cheap to clone; clones share the resolved hostname addresses.
#[derive(Debug, Clone, Default)]
pub struct ProxyTrust {
    enabled: bool,
    prefix: String,
    /// `--trust-proxy-from`; empty = any peer (when enabled).
    specs: Vec<PeerSpec>,
    /// Current addresses of the hostname entries.
    resolved: std::sync::Arc<parking_lot::RwLock<Vec<IpAddr>>>,
}

impl ProxyTrust {
    /// Trust off: every forwarded header is ignored.
    pub fn off() -> Self {
        Self::default()
    }

    /// Trust from any peer when `enabled`; the prefix is normalised with [`normalise_prefix`].
    pub fn new(enabled: bool, prefix: &str) -> Result<Self, String> {
        Ok(Self {
            enabled,
            prefix: normalise_prefix(prefix)?,
            ..Self::default()
        })
    }

    /// Narrow trust to these peers (`--trust-proxy-from`); an empty list keeps
    /// "any peer". Hostnames match nothing until [`Self::refresh_hosts`] resolves them.
    pub fn from_peers(mut self, specs: Vec<PeerSpec>) -> Self {
        self.specs = specs;
        self
    }

    /// Whether forwarded headers are honoured at all.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The configured path prefix (`""` or `/segment[/segment…]`); a trusted
    /// `X-Forwarded-Prefix` overrides it per request.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The configured peer entries (empty = any).
    pub fn peers(&self) -> &[PeerSpec] {
        &self.specs
    }

    /// The addresses the hostname entries currently resolve to.
    pub fn resolved(&self) -> Vec<IpAddr> {
        self.resolved.read().clone()
    }

    /// Whether any entry is a hostname (and so needs refreshing).
    pub fn has_hosts(&self) -> bool {
        self.specs.iter().any(|s| matches!(s, PeerSpec::Host(_)))
    }

    /// Re-resolve every hostname entry (blocking: run it off the async
    /// workers) and swap the result in atomically. Returns the names that did
    /// not resolve; their addresses are dropped, so a name that stops resolving
    /// stops being trusted.
    pub fn refresh_hosts(&self) -> Vec<(String, std::io::Error)> {
        let mut addrs = Vec::new();
        let mut failed = Vec::new();
        for spec in &self.specs {
            if let PeerSpec::Host(h) = spec {
                match resolve_host(h) {
                    Ok(a) if !a.is_empty() => addrs.extend(a),
                    Ok(_) => failed.push((
                        h.clone(),
                        std::io::Error::new(std::io::ErrorKind::NotFound, "no addresses"),
                    )),
                    Err(e) => failed.push((h.clone(), e)),
                }
            }
        }
        addrs.sort_unstable();
        addrs.dedup();
        *self.resolved.write() = addrs;
        failed
    }

    /// Whether this request's forwarded headers are honoured: trust is on and,
    /// when peers are listed, the TCP peer matches a listed address/CIDR or a
    /// currently resolved hostname. An unknown peer is never a listed one.
    pub fn applies(&self, peer: Option<IpAddr>) -> bool {
        if !self.enabled {
            return false;
        }
        if self.specs.is_empty() {
            return true;
        }
        let Some(p) = peer else { return false };
        self.specs
            .iter()
            .any(|s| matches!(s, PeerSpec::Net(n) if n.contains(&p)))
            || self.resolved.read().contains(&p)
    }

    /// The authority the client dialled: `X-Forwarded-Host` when trusted and
    /// well-formed, otherwise `Host`.
    pub fn host<'a>(&self, h: &'a HeaderMap, peer: Option<IpAddr>) -> Option<&'a str> {
        if self.applies(peer) {
            if let Some(x) = first_value(h, X_FORWARDED_HOST).filter(|x| is_authority(x)) {
                return Some(x);
            }
        }
        h.get(actix_web::http::header::HOST)
            .and_then(|v| v.to_str().ok())
    }

    /// The address per-client limits key on: the last `X-Forwarded-For` hop
    /// when trusted and parseable, otherwise the TCP peer.
    pub fn client_ip(&self, h: &HeaderMap, peer: Option<IpAddr>) -> Option<IpAddr> {
        if self.applies(peer) {
            let last = h
                .get_all(X_FORWARDED_FOR)
                .filter_map(|v| v.to_str().ok())
                .flat_map(|v| v.split(','))
                .map(str::trim)
                .rfind(|s| !s.is_empty());
            if let Some(ip) = last.and_then(|s| s.parse::<IpAddr>().ok()) {
                return Some(ip);
            }
        }
        peer
    }

    /// `proto://host<prefix>` from the forwarded headers, or `None` when trust
    /// does not apply or the proxy sent no usable `X-Forwarded-Host` (a direct
    /// request). The prefix is a well-formed `X-Forwarded-Prefix`, else the configured one.
    pub fn forwarded_origin(&self, h: &HeaderMap, peer: Option<IpAddr>) -> Option<String> {
        if !self.applies(peer) {
            return None;
        }
        let host = first_value(h, X_FORWARDED_HOST).filter(|x| is_authority(x))?;
        let proto = match first_value(h, X_FORWARDED_PROTO) {
            Some(p) if p.eq_ignore_ascii_case("https") => "https",
            _ => "http",
        };
        let prefix = first_value(h, X_FORWARDED_PREFIX)
            .and_then(|p| normalise_prefix(p).ok())
            .unwrap_or_else(|| self.prefix.clone());
        Some(format!("{proto}://{}{prefix}", host.to_ascii_lowercase()))
    }
}

/// How the absolute URL a NIP-98 signature covers is rebuilt per request.
#[derive(Debug, Clone)]
pub struct PublicOrigin {
    explicit: Option<String>,
    fallback: String,
    proxy: ProxyTrust,
}

impl PublicOrigin {
    /// `explicit` is `--public-origin` (wins verbatim); `fallback` is the
    /// bind-derived origin used for direct requests.
    pub fn new(explicit: Option<String>, fallback: String, proxy: ProxyTrust) -> Self {
        Self {
            explicit: explicit.map(|o| o.trim_end_matches('/').to_string()),
            fallback: fallback.trim_end_matches('/').to_string(),
            proxy,
        }
    }

    /// From the configuration (call [`crate::config::Config::validate`] first).
    pub fn from_config(cfg: &crate::config::Config) -> Result<Self, String> {
        let peers = cfg
            .trust_proxy_from
            .iter()
            .map(|p| parse_peer(p))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::new(
            cfg.public_origin.clone(),
            cfg.public_origin(),
            ProxyTrust::new(cfg.trust_proxy, &cfg.public_path_prefix)?.from_peers(peers),
        ))
    }

    /// The proxy policy.
    pub fn proxy(&self) -> &ProxyTrust {
        &self.proxy
    }

    /// The origin (with any proxy prefix, no trailing `/`) for this request.
    pub fn for_request(&self, h: &HeaderMap, peer: Option<IpAddr>) -> String {
        if let Some(o) = &self.explicit {
            return o.clone();
        }
        self.proxy
            .forwarded_origin(h, peer)
            .unwrap_or_else(|| self.fallback.clone())
    }

    /// The absolute URL of this request as the client addressed it.
    pub fn url_for(
        &self,
        h: &HeaderMap,
        peer: Option<IpAddr>,
        uri: &actix_web::http::Uri,
    ) -> String {
        crate::http::public_url(&self.for_request(h, peer), uri)
    }
}

#[cfg(test)]
mod tests {
    use actix_web::http::header::{HeaderName, HeaderValue};

    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(HeaderName::from_static(k), HeaderValue::from_static(v));
        }
        h
    }

    const PEER: &str = "172.18.0.9";

    fn peer() -> Option<IpAddr> {
        Some(PEER.parse().unwrap())
    }

    /// The headers the dev nginx sends for `ws://box:3001/codescape/wss`.
    fn via_nginx() -> HeaderMap {
        headers(&[
            ("host", "box"),
            (X_FORWARDED_HOST, "box:3001"),
            (X_FORWARDED_PROTO, "http"),
            (X_FORWARDED_FOR, "192.168.2.50"),
        ])
    }

    #[test]
    fn prefixes_normalise() {
        assert_eq!(normalise_prefix("").unwrap(), "");
        assert_eq!(normalise_prefix("/").unwrap(), "");
        assert_eq!(normalise_prefix("/codescape/").unwrap(), "/codescape");
        assert_eq!(normalise_prefix("codescape").unwrap(), "/codescape");
        assert_eq!(normalise_prefix("/a/b-c_d.e").unwrap(), "/a/b-c_d.e");
        for bad in ["/a b", "/a?x", "/a#x", "/../x", "/a//b", "/x/./y", "/%2e"] {
            assert!(normalise_prefix(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn trust_off_ignores_every_forwarded_header() {
        let off = ProxyTrust::off();
        let spoofed = headers(&[
            ("host", "codescape.lan:4100"),
            (X_FORWARDED_HOST, "evil:9"),
            (X_FORWARDED_PROTO, "https"),
            (X_FORWARDED_FOR, "6.6.6.6"),
        ]);
        assert_eq!(off.host(&spoofed, peer()), Some("codescape.lan:4100"));
        assert_eq!(off.client_ip(&spoofed, peer()), peer());
        assert_eq!(off.forwarded_origin(&spoofed, peer()), None);
        let public = PublicOrigin::new(None, "http://localhost:4100".into(), off);
        assert_eq!(
            public.for_request(&spoofed, peer()),
            "http://localhost:4100"
        );
    }

    #[test]
    fn trust_on_uses_the_forwarded_host_proto_and_prefix() {
        let on = ProxyTrust::new(true, "/codescape").unwrap();
        let h = via_nginx();
        assert_eq!(on.host(&h, peer()), Some("box:3001"));
        assert_eq!(
            on.client_ip(&h, peer()),
            Some("192.168.2.50".parse().unwrap())
        );
        let public = PublicOrigin::new(None, "http://localhost:4100".into(), on.clone());
        let uri: actix_web::http::Uri = "/api/graph/search?q=x".parse().unwrap();
        assert_eq!(
            public.url_for(&h, peer(), &uri),
            "http://box:3001/codescape/api/graph/search?q=x"
        );
        let tls = headers(&[
            (X_FORWARDED_HOST, "cs.example"),
            (X_FORWARDED_PROTO, "HTTPS"),
        ]);
        assert_eq!(
            public.for_request(&tls, peer()),
            "https://cs.example/codescape"
        );
    }

    #[test]
    fn trust_on_falls_back_for_direct_requests() {
        let on = ProxyTrust::new(true, "/codescape").unwrap();
        let direct = headers(&[("host", "127.0.0.1:4100")]);
        assert_eq!(on.host(&direct, peer()), Some("127.0.0.1:4100"));
        assert_eq!(on.client_ip(&direct, peer()), peer());
        let public = PublicOrigin::new(None, "http://localhost:4100".into(), on);
        assert_eq!(
            public.for_request(&direct, peer()),
            "http://localhost:4100",
            "no X-Forwarded-Host: not proxied, no prefix"
        );
    }

    #[test]
    fn trust_on_rejects_malformed_forwarded_values() {
        let on = ProxyTrust::new(true, "").unwrap();
        for bad in ["evil/path", "user@evil", "evil?x", "", "a b"] {
            let mut h = headers(&[("host", "box"), (X_FORWARDED_FOR, "not-an-ip")]);
            if !bad.is_empty() {
                h.insert(
                    HeaderName::from_static(X_FORWARDED_HOST),
                    HeaderValue::from_str(bad).unwrap(),
                );
            }
            assert_eq!(on.host(&h, peer()), Some("box"), "{bad:?}");
            assert_eq!(on.forwarded_origin(&h, peer()), None, "{bad:?}");
            assert_eq!(on.client_ip(&h, peer()), peer(), "unparseable XFF");
        }
    }

    #[test]
    fn last_forwarded_for_hop_wins_over_a_client_supplied_chain() {
        let on = ProxyTrust::new(true, "").unwrap();
        // The client sent `X-Forwarded-For: 1.1.1.1`; nginx appended the real peer.
        let h = headers(&[(X_FORWARDED_FOR, "1.1.1.1, 192.168.2.50")]);
        assert_eq!(
            on.client_ip(&h, peer()),
            Some("192.168.2.50".parse().unwrap())
        );
        // Repeated header lines are one list, in order.
        let h = headers(&[
            (X_FORWARDED_FOR, "1.1.1.1"),
            (X_FORWARDED_FOR, "2001:db8::1"),
        ]);
        assert_eq!(
            on.client_ip(&h, peer()),
            Some("2001:db8::1".parse().unwrap())
        );
    }

    #[test]
    fn explicit_public_origin_always_wins() {
        let on = ProxyTrust::new(true, "/codescape").unwrap();
        let public = PublicOrigin::new(
            Some("https://codescape.example/".into()),
            "http://localhost:4100".into(),
            on,
        );
        assert_eq!(
            public.for_request(&via_nginx(), peer()),
            "https://codescape.example"
        );
    }

    #[test]
    fn forwarded_prefix_wins_over_the_flag_only_when_trusted() {
        let mut h = via_nginx();
        h.insert(
            HeaderName::from_static(X_FORWARDED_PREFIX),
            HeaderValue::from_static("/codescape"),
        );
        let on = ProxyTrust::new(true, "/configured").unwrap();
        assert_eq!(
            on.forwarded_origin(&h, peer()).unwrap(),
            "http://box:3001/codescape",
            "the proxy's prefix wins"
        );
        h.insert(
            HeaderName::from_static(X_FORWARDED_PREFIX),
            HeaderValue::from_static("/../evil"),
        );
        assert_eq!(
            on.forwarded_origin(&h, peer()).unwrap(),
            "http://box:3001/configured",
            "a malformed prefix falls back to the flag"
        );
        let off = PublicOrigin::new(None, "http://localhost:4100".into(), ProxyTrust::off());
        assert_eq!(off.for_request(&h, peer()), "http://localhost:4100");
    }

    #[test]
    fn trust_from_narrows_to_listed_peers() {
        let nginx: IpAddr = "172.18.0.5".parse().unwrap();
        let sibling: IpAddr = "172.18.0.9".parse().unwrap();
        let narrow = ProxyTrust::new(true, "")
            .unwrap()
            .from_peers(vec![parse_peer("172.18.0.5").unwrap()]);
        let h = via_nginx();
        assert!(narrow.applies(Some(nginx)));
        assert_eq!(narrow.host(&h, Some(nginx)), Some("box:3001"));
        assert_eq!(
            narrow.client_ip(&h, Some(nginx)),
            Some("192.168.2.50".parse().unwrap())
        );
        // A sibling container spoofing the same headers is treated as trust off.
        assert!(!narrow.applies(Some(sibling)));
        assert_eq!(narrow.host(&h, Some(sibling)), Some("box"));
        assert_eq!(narrow.client_ip(&h, Some(sibling)), Some(sibling));
        assert_eq!(narrow.forwarded_origin(&h, Some(sibling)), None);
        assert!(!narrow.applies(None), "unknown peer: fail closed");
        // CIDR entries and IPv6.
        let cidr = ProxyTrust::new(true, "").unwrap().from_peers(vec![
            parse_peer("172.18.0.0/16").unwrap(),
            parse_peer("::1").unwrap(),
        ]);
        assert!(cidr.applies(Some(sibling)));
        assert!(cidr.applies(Some("::1".parse().unwrap())));
        assert!(!cidr.applies(Some("10.0.0.1".parse().unwrap())));
        // Listing peers never switches trust on by itself.
        let off = ProxyTrust::off().from_peers(vec![parse_peer("172.18.0.5").unwrap()]);
        assert!(!off.applies(Some(nginx)));
    }

    #[test]
    fn trusted_peer_entries_parse() {
        assert!(parse_peer("10.0.0.1").is_ok());
        assert!(parse_peer(" 10.0.0.0/8 ").is_ok());
        assert!(parse_peer("fd00::/8").is_ok());
        assert_eq!(
            parse_peer("visionclaw_container").unwrap(),
            PeerSpec::Host("visionclaw_container".into())
        );
        assert_eq!(
            parse_peer("Nginx.Local").unwrap(),
            PeerSpec::Host("nginx.local".into())
        );
        for bad in [
            "",
            "10.0.0.0/33",
            "10.0.0.1/x",
            "fd00::zz",
            "a b",
            "-x",
            "a..b",
            "h@st",
        ] {
            assert!(parse_peer(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn localhost_resolves_to_loopback_and_is_trusted() {
        let t = ProxyTrust::new(true, "")
            .unwrap()
            .from_peers(vec![parse_peer("localhost").unwrap()]);
        assert!(
            !t.applies(Some("127.0.0.1".parse().unwrap())),
            "nothing before resolution"
        );
        assert!(t.refresh_hosts().is_empty());
        let r = t.resolved();
        assert!(!r.is_empty() && r.iter().all(IpAddr::is_loopback), "{r:?}");
        let clone = t.clone();
        assert!(clone.applies(Some(r[0])), "clones share the resolved set");
        assert!(!t.applies(Some("10.0.0.1".parse().unwrap())));
    }

    #[test]
    fn unresolvable_hostname_trusts_no_peer() {
        // RFC 6761: `.invalid` never resolves.
        let t = ProxyTrust::new(true, "")
            .unwrap()
            .from_peers(vec![parse_peer("no-such-proxy.invalid").unwrap()]);
        let failed = t.refresh_hosts();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, "no-such-proxy.invalid");
        assert!(t.resolved().is_empty());
        assert!(
            !t.applies(Some("127.0.0.1".parse().unwrap())),
            "listed but empty: no peer"
        );
        assert!(!t.applies(None));
        // A literal entry alongside still works.
        let mixed = ProxyTrust::new(true, "").unwrap().from_peers(vec![
            parse_peer("no-such-proxy.invalid").unwrap(),
            parse_peer("172.18.0.5").unwrap(),
        ]);
        mixed.refresh_hosts();
        assert!(mixed.applies(Some("172.18.0.5".parse().unwrap())));
    }
}
