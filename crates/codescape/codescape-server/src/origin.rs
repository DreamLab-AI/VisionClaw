//! WebSocket `Origin` allowlist against cross-site WebSocket hijacking.
//!
//! A browser attaches `Origin` to every WebSocket handshake and lets any page
//! open a socket to any host, so without this check a page on another site
//! could drive `/ws/events`, `/ws/voice` or `/wss` from a visitor's browser.
//! Same-origin is always allowed. Without `--allowed-origins`, so are
//! `http(s)://localhost:*`, `http(s)://127.0.0.1:*` and `http(s)://[::1]:*`; a
//! configured list replaces those loopback defaults.
//!
//! A handshake *without* `Origin` is allowed. Browsers always send it, so its
//! absence means a native client (the Godot XR client, CLI tools), which no
//! hostile page can impersonate. `Origin: null` (sandboxed frames, `file:`) is refused.
//!
//! # Example
//!
//! ```
//! use codescape_server::origin::OriginPolicy;
//!
//! let p = OriginPolicy::new(&[]).unwrap();
//! assert!(p.allows(Some("http://codescape.lan:4100"), Some("codescape.lan:4100")));
//! assert!(p.allows(Some("http://localhost:5173"), Some("codescape.lan:4100")));
//! assert!(!p.allows(Some("https://evil.example"), Some("codescape.lan:4100")));
//! assert!(p.allows(None, Some("codescape.lan:4100")));
//! ```

use reqwest::Url;

/// One allowlist entry: scheme, host and a port or any port.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pattern {
    scheme: String,
    host: String,
    /// `None` = any port.
    port: Option<u16>,
}

/// Errors in `--allowed-origins`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid allowed origin `{0}`: use scheme://host[:port] or scheme://host:*")]
pub struct OriginError(pub String);

fn default_port(scheme: &str) -> u16 {
    if scheme == "https" {
        443
    } else {
        80
    }
}

fn parse_pattern(raw: &str) -> Result<Pattern, OriginError> {
    let bad = || OriginError(raw.to_string());
    let (scheme, rest) = raw.split_once("://").ok_or_else(bad)?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(bad());
    }
    let rest = rest.trim_end_matches('/');
    if rest.contains('/') || rest.is_empty() {
        return Err(bad());
    }
    if let Some(host) = rest.strip_suffix(":*") {
        if host.is_empty() {
            return Err(bad());
        }
        return Ok(Pattern {
            scheme,
            host: host.to_ascii_lowercase(),
            port: None,
        });
    }
    let url = Url::parse(&format!("{scheme}://{rest}")).map_err(|_| bad())?;
    let host = url.host_str().ok_or_else(bad)?.to_ascii_lowercase();
    Ok(Pattern {
        port: Some(url.port().unwrap_or(default_port(&scheme))),
        scheme,
        host,
    })
}

/// `(scheme, host, port)` of an `Origin` header value.
fn parse_origin(raw: &str) -> Option<(String, String, u16)> {
    let url = Url::parse(raw).ok()?;
    let scheme = url.scheme().to_string();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let port = url.port().unwrap_or(default_port(&scheme));
    Some((scheme, host, port))
}

/// The handshake-origin policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginPolicy {
    patterns: Vec<Pattern>,
}

impl OriginPolicy {
    /// Loopback origins allowed when no list is configured.
    pub const LOOPBACK_DEFAULTS: &'static [&'static str] = &[
        "http://localhost:*",
        "https://localhost:*",
        "http://127.0.0.1:*",
        "https://127.0.0.1:*",
        "http://[::1]:*",
        "https://[::1]:*",
    ];

    /// Build from `--allowed-origins`; an empty list means the loopback defaults.
    pub fn new(configured: &[String]) -> Result<Self, OriginError> {
        let patterns = if configured.is_empty() {
            Self::LOOPBACK_DEFAULTS
                .iter()
                .map(|p| parse_pattern(p))
                .collect::<Result<_, _>>()?
        } else {
            configured
                .iter()
                .map(|p| parse_pattern(p.trim()))
                .collect::<Result<_, _>>()?
        };
        Ok(Self { patterns })
    }

    /// Whether a handshake with this `Origin` and `Host` may upgrade.
    pub fn allows(&self, origin: Option<&str>, host: Option<&str>) -> bool {
        let Some(origin) = origin else {
            return true;
        };
        let Some((scheme, ohost, oport)) = parse_origin(origin.trim()) else {
            return false;
        };
        if let Some(h) = host.and_then(|h| Url::parse(&format!("{scheme}://{}", h.trim())).ok()) {
            let same_host = h.host_str().is_some_and(|x| x.eq_ignore_ascii_case(&ohost));
            if same_host && h.port().unwrap_or(default_port(&scheme)) == oport {
                return true;
            }
        }
        self.patterns.iter().any(|p| {
            p.scheme == scheme && p.host == ohost && p.port.is_none_or(|port| port == oport)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: Option<&str> = Some("visionclaw_container:4100");

    #[test]
    fn same_origin_and_loopback_are_allowed_by_default() {
        let p = OriginPolicy::new(&[]).unwrap();
        assert!(p.allows(Some("http://visionclaw_container:4100"), HOST));
        assert!(
            p.allows(Some("https://VisionClaw_Container:4100"), HOST),
            "host compare is case-insensitive"
        );
        assert!(p.allows(Some("http://localhost:5173"), HOST));
        assert!(p.allows(Some("http://localhost"), HOST));
        assert!(p.allows(Some("https://127.0.0.1:4100"), HOST));
        assert!(p.allows(Some("http://[::1]:8080"), HOST));
        assert!(
            p.allows(Some("https://codescape.lan"), Some("codescape.lan")),
            "default ports compare equal"
        );
    }

    #[test]
    fn cross_site_origins_are_refused() {
        let p = OriginPolicy::new(&[]).unwrap();
        assert!(!p.allows(Some("https://evil.example"), HOST));
        assert!(
            !p.allows(Some("http://visionclaw_container:4101"), HOST),
            "another port is another origin"
        );
        assert!(!p.allows(Some("http://localhost.evil.example"), HOST));
        assert!(!p.allows(Some("http://127.0.0.1.evil.example:4100"), HOST));
        assert!(!p.allows(Some("null"), HOST));
        assert!(!p.allows(Some("file://"), HOST));
        assert!(!p.allows(Some("chrome-extension://abc"), HOST));
        assert!(
            !p.allows(Some("https://evil.example"), None),
            "no Host header: only the patterns apply"
        );
    }

    #[test]
    fn missing_origin_is_a_native_client() {
        assert!(OriginPolicy::new(&[]).unwrap().allows(None, HOST));
    }

    #[test]
    fn configured_list_replaces_loopback_defaults() {
        let p = OriginPolicy::new(&[
            "https://codescape.example".into(),
            "http://dev.lan:*".into(),
        ])
        .unwrap();
        assert!(p.allows(Some("https://codescape.example"), HOST));
        assert!(!p.allows(Some("https://codescape.example:8443"), HOST));
        assert!(p.allows(Some("http://dev.lan:3000"), HOST));
        assert!(
            !p.allows(Some("http://localhost:5173"), HOST),
            "loopback no longer implied"
        );
        assert!(
            p.allows(Some("http://visionclaw_container:4100"), HOST),
            "same-origin still allowed"
        );
    }

    #[test]
    fn malformed_patterns_are_rejected() {
        for bad in [
            "codescape.example",
            "ftp://x",
            "https://",
            "https://x/path",
            "https://:*",
        ] {
            assert!(OriginPolicy::new(&[bad.to_string()]).is_err(), "{bad}");
        }
    }
}
