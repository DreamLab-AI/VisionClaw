//! Command line and environment configuration (`CODESCAPE_*`).
//!
//! Every option is settable as a flag or as an environment variable with the
//! `CODESCAPE_` prefix. The data directory layout per repository is
//! `<data>/<repo>/{ids.bin, actors.bin, positions.bin, graphicle/, timeline.jsonl}`.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::builder::BoolishValueParser;
use clap::{ArgAction, Parser};

/// Name of the variable whose presence refuses boot: the Codescape process must
/// never be wired to the knowledge-work memory store (ADR-2143 §6).
pub const FORBIDDEN_ENV: &str = "RUVECTOR_PG_CONNINFO";

/// One `--repo label=path` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSpec {
    /// Short label used in URLs and as the data subdirectory name.
    pub label: String,
    /// Repository root on disk (may be a read-only mount).
    pub root: PathBuf,
}

impl FromStr for RepoSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (label, path) = s
            .split_once('=')
            .ok_or_else(|| format!("repo `{s}` must be label=path"))?;
        let label = label.trim();
        validate_label(label)?;
        let path = path.trim();
        if path.is_empty() {
            return Err(format!("repo `{label}` has an empty path"));
        }
        Ok(Self {
            label: label.to_string(),
            root: PathBuf::from(path),
        })
    }
}

/// A repo label becomes a directory name under the data dir, so it is restricted
/// to `[A-Za-z0-9_.-]`, must not start with `.`, and is at most 64 bytes.
pub fn validate_label(label: &str) -> Result<(), String> {
    if label.is_empty() || label.len() > 64 {
        return Err(format!("repo label `{label}` must be 1-64 characters"));
    }
    if label.starts_with('.') {
        return Err(format!("repo label `{label}` must not start with `.`"));
    }
    if !label
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(format!(
            "repo label `{label}` may only contain A-Z a-z 0-9 _ - ."
        ));
    }
    Ok(())
}

/// Codescape server configuration.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "codescape-server",
    version,
    about = "Codescape: a live 3D view of a code graph"
)]
pub struct Config {
    /// Main HTTP/WS port.
    #[arg(long, env = "CODESCAPE_PORT", default_value_t = 4100)]
    pub port: u16,

    /// Presence-ingest port (never published outside the compose network).
    #[arg(long, env = "CODESCAPE_INGEST_PORT", default_value_t = 4101)]
    pub ingest_port: u16,

    /// Bind address for both listeners.
    #[arg(long, env = "CODESCAPE_BIND", default_value = "0.0.0.0")]
    pub bind: String,

    /// Repository to serve, as `label=path`. Repeatable; in the environment, comma-separated.
    #[arg(
        long = "repo",
        env = "CODESCAPE_REPO",
        value_delimiter = ',',
        required = true
    )]
    pub repos: Vec<RepoSpec>,

    /// Writable data directory (interner, graphicle cache, positions, timeline).
    #[arg(long, env = "CODESCAPE_DATA_DIR", default_value = "/data/codescape")]
    pub data_dir: PathBuf,

    /// Built desktop client to serve at `/` (SPA fallback to `index.html`).
    #[arg(long, env = "CODESCAPE_STATIC_DIR")]
    pub static_dir: Option<PathBuf>,

    /// File holding the bearer token for `:4101` presence ingest. Without it, the
    /// ingest listener is not started.
    #[arg(long, env = "CODESCAPE_INGEST_TOKEN_FILE")]
    pub ingest_token_file: Option<PathBuf>,

    /// Rewrite rules for hook paths, `FROM=TO[:FROM=TO…]`, longest prefix wins.
    #[arg(long, env = "CODESCAPE_PATH_MAP")]
    pub path_map: Option<String>,

    /// Prefer the CUDA layout engine when it is compiled in and usable.
    #[arg(long, env = "CODESCAPE_PREFER_CUDA", default_value_t = true, action = ArgAction::Set, value_parser = BoolishValueParser::new())]
    pub prefer_cuda: bool,

    /// Position frame rate per client (1-30 Hz).
    #[arg(long, env = "CODESCAPE_FRAME_HZ", default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..=30))]
    pub frame_hz: u32,

    /// OpenAI-compatible Loom base URL (`…/v1`) for the command fallback. Unset = no fallback.
    #[arg(long, env = "CODESCAPE_LOOM_URL")]
    pub loom_url: Option<String>,

    /// Model id for Loom; `auto` asks `GET /models` once.
    #[arg(long, env = "CODESCAPE_LOOM_MODEL", default_value = "auto")]
    pub loom_model: String,

    /// nemotron streaming ASR WebSocket.
    #[arg(
        long,
        env = "CODESCAPE_ASR_URL",
        default_value = "ws://nemotron-asr:8000/api/asr-streaming"
    )]
    pub asr_url: String,

    /// pocket-tts base URL.
    #[arg(
        long,
        env = "CODESCAPE_TTS_URL",
        default_value = "http://pocket-tts:8000"
    )]
    pub tts_url: String,

    /// pocket-tts voice.
    #[arg(long, env = "CODESCAPE_TTS_VOICE", default_value = "alba")]
    pub tts_voice: String,

    /// management-api voice-intent endpoint (P6 dispatch); only added to the egress allowlist.
    #[arg(long, env = "CODESCAPE_VOICE_INTENT_URL")]
    pub voice_intent_url: Option<String>,

    /// File of allowed NIP-98 pubkeys (P6).
    #[arg(long, env = "CODESCAPE_PUBKEY_ALLOWLIST")]
    pub pubkey_allowlist: Option<PathBuf>,

    /// Require NIP-98 on every route on the main port.
    #[arg(long, env = "CODESCAPE_REQUIRE_AUTH", default_value_t = false, action = ArgAction::Set, value_parser = BoolishValueParser::new())]
    pub require_auth: bool,

    /// Discard cached ids and graphicle output and extract from scratch (bumps the id epoch).
    #[arg(long, env = "CODESCAPE_REEXTRACT", default_value_t = false, action = ArgAction::Set, value_parser = BoolishValueParser::new())]
    pub reextract: bool,

    /// Include Markdown and other documents in the graph.
    #[arg(long, env = "CODESCAPE_INCLUDE_DOCS", default_value_t = false, action = ArgAction::Set, value_parser = BoolishValueParser::new())]
    pub include_docs: bool,

    /// Extra glob excludes on top of `.gitignore`. Repeatable; in the environment, comma-separated.
    #[arg(long = "exclude", env = "CODESCAPE_EXCLUDES", value_delimiter = ',')]
    pub excludes: Vec<String>,

    /// Browser origins allowed to open WebSockets, comma-separated
    /// (`scheme://host[:port]` or `scheme://host:*`). Same-origin is always allowed;
    /// unset = same-origin plus loopback.
    #[arg(
        long = "allowed-origins",
        env = "CODESCAPE_ALLOWED_ORIGINS",
        value_delimiter = ','
    )]
    pub allowed_origins: Vec<String>,

    /// The origin clients reach this server at (`https://codescape.lan`), used to rebuild
    /// absolute URLs for NIP-98 instead of trusting `Host`. Default: `http://<bind>:<port>`
    /// (`localhost` for a wildcard bind), or, with `--trust-proxy`, the forwarded origin
    /// plus `--public-path-prefix`. When set, it is used verbatim and wins over both.
    #[arg(long, env = "CODESCAPE_PUBLIC_ORIGIN")]
    pub public_origin: Option<String>,

    /// Honour `X-Forwarded-Host`, `X-Forwarded-Proto` and `X-Forwarded-For` from a
    /// reverse proxy (same-origin check, public origin, per-address limits). Only
    /// enable it when every client reaches this port through that proxy.
    #[arg(long, env = "CODESCAPE_TRUST_PROXY", default_value_t = false, action = ArgAction::Set, value_parser = BoolishValueParser::new())]
    pub trust_proxy: bool,

    /// Path prefix the reverse proxy strips (`/codescape`), prepended to the
    /// forwarded origin under `--trust-proxy`.
    #[arg(long, env = "CODESCAPE_PUBLIC_PATH_PREFIX", default_value = "")]
    pub public_path_prefix: String,

    /// Narrow `--trust-proxy` to these peers: addresses, CIDRs or hostnames (e.g.
    /// the nginx container's service name, re-resolved every 60 s; an unresolvable
    /// name is a warning and trusts no peer until it resolves). Repeatable; in the
    /// environment, comma-separated. Unset = any peer.
    #[arg(
        long = "trust-proxy-from",
        env = "CODESCAPE_TRUST_PROXY_FROM",
        value_delimiter = ','
    )]
    pub trust_proxy_from: Vec<String>,

    /// Share agents' prompt and answer text with every viewer (default: tool names only, until NIP-98).
    #[arg(long, env = "CODESCAPE_SHARE_AGENT_TEXT", default_value_t = false, action = ArgAction::Set, value_parser = BoolishValueParser::new())]
    pub share_agent_text: bool,

    /// Log filter (`info`, `debug`, `codescape_server=trace`…).
    #[arg(long, env = "CODESCAPE_LOG_LEVEL", default_value = "info")]
    pub log_level: String,
}

impl Config {
    /// Cross-field checks clap cannot express.
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for r in &self.repos {
            if !seen.insert(r.label.as_str()) {
                return Err(format!("repo label `{}` given twice", r.label));
            }
        }
        // 0 asks the OS for an ephemeral port, so two zeros never collide.
        if self.port == self.ingest_port && self.port != 0 {
            return Err("port and ingest port must differ".into());
        }
        crate::proxy::normalise_prefix(&self.public_path_prefix)?;
        for p in &self.trust_proxy_from {
            crate::proxy::parse_peer(p)?;
        }
        if !self.trust_proxy_from.is_empty() && !self.trust_proxy {
            return Err(
                "--trust-proxy-from needs --trust-proxy (it narrows trust, it does not enable it)"
                    .into(),
            );
        }
        Ok(())
    }

    /// [`Config::public_origin`] or the default derived from bind and port, without a trailing `/`.
    pub fn public_origin(&self) -> String {
        match &self.public_origin {
            Some(o) => o.trim_end_matches('/').to_string(),
            None => {
                let host = match self.bind.as_str() {
                    "0.0.0.0" | "::" | "[::]" | "" => "localhost".to_string(),
                    h if h.contains(':') && !h.starts_with('[') => format!("[{h}]"),
                    h => h.to_string(),
                };
                format!("http://{host}:{}", self.port)
            }
        }
    }

    /// Outbound URLs the egress allowlist is built from.
    ///
    /// Optional URLs set to an empty or whitespace-only string (compose passes
    /// `${VAR:-}` as `""`) count as unset rather than as malformed URLs.
    pub fn egress_urls(&self) -> Vec<String> {
        let mut v = vec![self.asr_url.clone(), self.tts_url.clone()];
        v.extend(
            self.loom_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from),
        );
        v.extend(
            self.voice_intent_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from),
        );
        v
    }

    /// The Loom URL when configured and non-empty.
    pub fn loom_url(&self) -> Option<&str> {
        self.loom_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// The voice-intent URL when configured and non-empty.
    pub fn voice_intent_url(&self) -> Option<&str> {
        self.voice_intent_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Data layout for one repository.
    pub fn layout_for(&self, label: &str) -> DataLayout {
        DataLayout::new(&self.data_dir, label)
    }
}

/// Refuse to run with the knowledge-work memory store configured. `lookup` is the
/// environment accessor (`std::env::var_os` in production).
pub fn check_environment(
    lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Result<(), String> {
    if lookup(FORBIDDEN_ENV).is_some() {
        return Err(format!(
            "{FORBIDDEN_ENV} is set: Codescape must not be wired to the RuVector memory store (ADR-2143 §6); unset it and restart"
        ));
    }
    Ok(())
}

/// Paths under `<data>/<repo>/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataLayout {
    /// `<data>/<repo>`.
    pub dir: PathBuf,
}

impl DataLayout {
    /// Layout for `label` under `data_dir`.
    pub fn new(data_dir: &Path, label: &str) -> Self {
        Self {
            dir: data_dir.join(label),
        }
    }
    /// Persisted id interner.
    pub fn ids(&self) -> PathBuf {
        self.dir.join("ids.bin")
    }
    /// Persisted actor id table.
    pub fn actors(&self) -> PathBuf {
        self.dir.join("actors.bin")
    }
    /// Warm layout positions.
    pub fn positions(&self) -> PathBuf {
        self.dir.join("positions.bin")
    }
    /// Graphicle output directory.
    pub fn graphicle(&self) -> PathBuf {
        self.dir.join("graphicle")
    }
    /// Append-only presence and revision log.
    pub fn timeline(&self) -> PathBuf {
        self.dir.join("timeline.jsonl")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Config, clap::Error> {
        let mut v = vec!["codescape-server"];
        v.extend_from_slice(args);
        Config::try_parse_from(v)
    }

    #[test]
    fn defaults_and_repeatable_repos() {
        let c = parse(&[
            "--repo",
            "vc=/repos/visionclaw",
            "--repo",
            "ab=/repos/agentbox",
        ])
        .unwrap();
        assert_eq!(c.port, 4100);
        assert_eq!(c.ingest_port, 4101);
        assert_eq!(c.frame_hz, 10);
        assert!(c.prefer_cuda);
        assert!(!c.require_auth);
        assert!(!c.share_agent_text);
        assert!(c.allowed_origins.is_empty());
        assert!(!c.trust_proxy, "proxy headers are ignored by default");
        assert_eq!(c.public_path_prefix, "");
        assert!(c.trust_proxy_from.is_empty());
        assert_eq!(c.repos.len(), 2);
        assert_eq!(
            c.repos[1],
            RepoSpec {
                label: "ab".into(),
                root: "/repos/agentbox".into()
            }
        );
        assert_eq!(c.asr_url, "ws://nemotron-asr:8000/api/asr-streaming");
        assert_eq!(
            c.layout_for("vc").ids(),
            PathBuf::from("/data/codescape/vc/ids.bin")
        );
        assert_eq!(
            c.layout_for("vc").graphicle(),
            PathBuf::from("/data/codescape/vc/graphicle")
        );
        assert_eq!(
            c.layout_for("vc").timeline(),
            PathBuf::from("/data/codescape/vc/timeline.jsonl")
        );
        c.validate().unwrap();
    }

    #[test]
    fn bad_values_are_rejected() {
        assert!(parse(&[]).is_err(), "a repo is required");
        assert!(parse(&["--repo", "novalue"]).is_err());
        assert!(parse(&["--repo", "../x=/r"]).is_err());
        assert!(parse(&["--repo", "a/b=/r"]).is_err());
        assert!(parse(&["--repo", "a=/r", "--frame-hz", "31"]).is_err());
        assert!(parse(&["--repo", "a=/r", "--frame-hz", "0"]).is_err());
        let dup = parse(&["--repo", "a=/r", "--repo", "a=/s"]).unwrap();
        assert!(dup.validate().is_err());
    }

    #[test]
    fn public_origin_defaults_from_bind_and_port() {
        assert_eq!(
            parse(&["--repo", "a=/r"]).unwrap().public_origin(),
            "http://localhost:4100"
        );
        assert_eq!(
            parse(&["--repo", "a=/r", "--bind", "127.0.0.1", "--port", "14100"])
                .unwrap()
                .public_origin(),
            "http://127.0.0.1:14100"
        );
        assert_eq!(
            parse(&["--repo", "a=/r", "--bind", "::1"])
                .unwrap()
                .public_origin(),
            "http://[::1]:4100"
        );
        assert_eq!(
            parse(&["--repo", "a=/r", "--public-origin", "https://cs.lan/"])
                .unwrap()
                .public_origin(),
            "https://cs.lan"
        );
    }

    #[test]
    fn proxy_flags_parse_and_validate() {
        let c = parse(&[
            "--repo",
            "a=/r",
            "--trust-proxy",
            "1",
            "--public-path-prefix",
            "/codescape/",
        ])
        .unwrap();
        assert!(c.trust_proxy);
        c.validate().unwrap();
        let bad = parse(&["--repo", "a=/r", "--public-path-prefix", "/a?b"]).unwrap();
        assert!(bad.validate().is_err());
        let narrow = parse(&[
            "--repo",
            "a=/r",
            "--trust-proxy",
            "1",
            "--trust-proxy-from",
            "172.18.0.5,10.0.0.0/8",
            "--trust-proxy-from",
            "::1",
        ])
        .unwrap();
        assert_eq!(narrow.trust_proxy_from.len(), 3);
        narrow.validate().unwrap();
        let typo = parse(&[
            "--repo",
            "a=/r",
            "--trust-proxy",
            "1",
            "--trust-proxy-from",
            "10.0.0.1/x",
        ])
        .unwrap();
        assert!(typo.validate().is_err(), "malformed CIDR");
        let host = parse(&[
            "--repo",
            "a=/r",
            "--trust-proxy",
            "1",
            "--trust-proxy-from",
            "visionclaw_container",
        ])
        .unwrap();
        host.validate()
            .expect("a hostname is valid config, resolved at boot");
        let orphan = parse(&["--repo", "a=/r", "--trust-proxy-from", "172.18.0.5"]).unwrap();
        assert!(
            orphan.validate().is_err(),
            "narrowing without trust is a config error"
        );
    }

    #[test]
    fn ephemeral_ports_parse() {
        let c = parse(&["--repo", "a=/r", "--port", "0", "--ingest-port", "0"]).unwrap();
        assert_eq!((c.port, c.ingest_port), (0, 0));
        c.validate().unwrap();
        let clash = parse(&["--repo", "a=/r", "--port", "5000", "--ingest-port", "5000"]).unwrap();
        assert!(clash.validate().is_err());
    }

    #[test]
    fn empty_optional_urls_mean_unset() {
        // Compose passes `${CODESCAPE_LOOM_URL:-}` as an empty string; that must
        // not reach the egress allowlist as a malformed URL (first compose boot
        // crashed on exactly this, 2026-10-18).
        let c = parse(&[
            "--repo",
            "a=/r",
            "--loom-url",
            "",
            "--voice-intent-url",
            "  ",
        ])
        .unwrap();
        assert_eq!(c.loom_url(), None);
        assert_eq!(c.voice_intent_url(), None);
        assert_eq!(c.egress_urls().len(), 2, "only asr and tts remain");
        let c = parse(&["--repo", "a=/r", "--loom-url", " http://loom:8084/v1 "]).unwrap();
        assert_eq!(c.loom_url(), Some("http://loom:8084/v1"));
        assert_eq!(c.egress_urls().len(), 3);
    }

    #[test]
    fn boolish_flags_accept_digits() {
        let c = parse(&[
            "--repo",
            "a=/r",
            "--prefer-cuda",
            "0",
            "--require-auth",
            "1",
        ])
        .unwrap();
        assert!(!c.prefer_cuda);
        assert!(c.require_auth);
    }

    #[test]
    fn egress_urls_include_optional_endpoints() {
        let c = parse(&["--repo", "a=/r", "--loom-url", "http://loom:8080/v1"]).unwrap();
        let urls = c.egress_urls();
        assert_eq!(urls.len(), 3);
        assert!(urls.contains(&"http://loom:8080/v1".to_string()));
    }

    #[test]
    fn ruvector_conninfo_refuses_boot() {
        assert!(check_environment(|_| None).is_ok());
        let err = check_environment(|k| (k == FORBIDDEN_ENV).then(|| "host=x".into())).unwrap_err();
        assert!(err.contains(FORBIDDEN_ENV));
    }
}
