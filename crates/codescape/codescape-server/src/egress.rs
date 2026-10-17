//! Runtime egress allowlist (ADR-2143 §6.2).
//!
//! Every outbound connection the server makes (ASR, TTS, Loom, voice intent)
//! goes through [`EgressAllowlist::check`]. The list is built once from the
//! configured URLs, so nothing reachable on the shared compose network
//! (ruvector-postgres, the knowledge graph, …) can be contacted by accident or
//! by a redirect. HTTP clients come from [`EgressAllowlist::http_client`], whose
//! redirect policy re-checks every hop.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Url;

/// Why a URL was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EgressError {
    /// The URL could not be parsed or has no host.
    #[error("malformed egress URL `{0}`")]
    Malformed(String),
    /// The scheme is not http(s) or ws(s).
    #[error("egress scheme `{0}` is not allowed")]
    Scheme(String),
    /// Host and port are not on the allowlist.
    #[error("egress to {0} is not on the allowlist")]
    Denied(String),
}

/// A `(secure, host, port)` origin. `ws`≡`http` and `wss`≡`https` because a
/// WebSocket is an upgraded HTTP connection to the same origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Origin {
    secure: bool,
    host: String,
    port: u16,
}

fn origin_of(raw: &str) -> Result<Origin, EgressError> {
    let url = Url::parse(raw).map_err(|_| EgressError::Malformed(raw.to_string()))?;
    let secure = match url.scheme() {
        "http" | "ws" => false,
        "https" | "wss" => true,
        other => return Err(EgressError::Scheme(other.to_string())),
    };
    let host = url
        .host_str()
        .ok_or_else(|| EgressError::Malformed(raw.to_string()))?
        .to_ascii_lowercase();
    let port = url.port().unwrap_or(if secure { 443 } else { 80 });
    Ok(Origin { secure, host, port })
}

/// The set of origins the server may contact.
#[derive(Debug, Clone, Default)]
pub struct EgressAllowlist {
    origins: Arc<HashSet<Origin>>,
}

impl EgressAllowlist {
    /// Build from configured endpoint URLs. A malformed URL is a configuration error.
    pub fn from_urls<I, S>(urls: I) -> Result<Self, EgressError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut set = HashSet::new();
        for u in urls {
            set.insert(origin_of(u.as_ref())?);
        }
        Ok(Self {
            origins: Arc::new(set),
        })
    }

    /// `Ok(())` when `url`'s origin is on the list.
    pub fn check(&self, url: &str) -> Result<(), EgressError> {
        let o = origin_of(url)?;
        if self.origins.contains(&o) {
            Ok(())
        } else {
            Err(EgressError::Denied(format!("{}:{}", o.host, o.port)))
        }
    }

    /// Number of allowed origins.
    pub fn len(&self) -> usize {
        self.origins.len()
    }

    /// `true` when nothing may be contacted.
    pub fn is_empty(&self) -> bool {
        self.origins.is_empty()
    }

    /// An HTTP client whose redirects are followed only to allowed origins (at most 3 hops).
    pub fn http_client(&self, timeout: Duration) -> reqwest::Client {
        let allow = self.clone();
        let policy = reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 3 {
                return attempt.error("too many redirects");
            }
            match allow.check(attempt.url().as_str()) {
                Ok(()) => attempt.follow(),
                Err(e) => attempt.error(e),
            }
        });
        reqwest::Client::builder()
            .redirect(policy)
            .connect_timeout(Duration::from_secs(5))
            .timeout(timeout)
            .build()
            .expect("reqwest client with rustls builds")
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    fn list() -> EgressAllowlist {
        EgressAllowlist::from_urls([
            "ws://nemotron-asr:8000/api/asr-streaming",
            "http://pocket-tts:8000",
            "http://192.168.2.132:8084/v1",
        ])
        .unwrap()
    }

    #[test]
    fn listed_origins_pass_and_others_are_refused() {
        let a = list();
        assert_eq!(a.len(), 3);
        a.check("http://pocket-tts:8000/v1/audio/speech").unwrap();
        a.check("http://nemotron-asr:8000/other").unwrap(); // ws ≡ http origin
        a.check("http://192.168.2.132:8084/v1/chat/completions")
            .unwrap();
        assert!(matches!(
            a.check("http://ruvector-postgres:5432/"),
            Err(EgressError::Denied(_))
        ));
        assert!(matches!(
            a.check("http://pocket-tts:8001/"),
            Err(EgressError::Denied(_))
        ));
        assert!(matches!(
            a.check("https://pocket-tts:8000/"),
            Err(EgressError::Denied(_))
        ));
        assert!(matches!(
            a.check("http://visionclaw_container:4000/api"),
            Err(EgressError::Denied(_))
        ));
        assert!(matches!(
            a.check("file:///etc/passwd"),
            Err(EgressError::Scheme(_))
        ));
        assert!(matches!(
            a.check("not a url"),
            Err(EgressError::Malformed(_))
        ));
    }

    #[test]
    fn malformed_config_is_an_error() {
        assert!(EgressAllowlist::from_urls(["::"]).is_err());
    }

    /// A 302 from an allowed origin to an unlisted one must fail, not be followed.
    #[tokio::test]
    async fn redirects_to_unlisted_hosts_are_refused() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf).await;
                let _ = s
                    .write_all(b"HTTP/1.1 302 Found\r\nLocation: http://ruvector-postgres:5432/\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });
        let base = format!("http://127.0.0.1:{port}");
        let allow = EgressAllowlist::from_urls([base.as_str()]).unwrap();
        let client = allow.http_client(Duration::from_secs(5));
        let err = client.get(format!("{base}/x")).send().await.unwrap_err();
        assert!(err.is_redirect(), "redirect must be refused: {err}");
    }
}
