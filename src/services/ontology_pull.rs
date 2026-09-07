//! Pull the published ontology into the in-process Solid pod at boot (ADR-2106).
//!
//! `ontology-publish.yml` builds the pod resources from the vault and attaches
//! them to the rolling GitHub release `ontology-latest` on this repository.
//! A GitHub-hosted runner cannot reach the pod (it is served in-process on the
//! LAN, ADR-2098), so delivery is inverted: the server fetches the release at
//! start-up and writes the resources into its own `FsBackend` under
//! `/public/ontology/`, exactly where the workflow's push path would have PUT
//! them. Nothing on the LAN accepts inbound traffic and no runner is
//! registered.
//!
//! Idempotent and fail-open: the release manifest's `visionflow:buildSha` is
//! compared with the one already in the pod and the download is skipped when
//! they match; any network or verification failure is logged and the pod
//! keeps whatever it held. Every content file is verified against the
//! release's `SHA256SUMS` before anything is written, and the manifest is
//! included in an immutable generation activated by a durable pointer switch.
//! Canonical single-resource reads are complete; multi-resource readers pin the
//! manifest generation. Old unpinned clients can straddle activation.
//!
//! The network and storage layers are injected ([`Fetch`], [`Storage`]) so the
//! whole sequence is unit-tested against an in-memory map and
//! `MemoryBackend`; [`spawn_boot_pull`] is the only place the real client and
//! the real backend meet.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::ontology_generation::OntologyPublication;
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;
use sha2::{Digest, Sha256};
use solid_pod_rs::error::PodError;
use solid_pod_rs::wac::{AclAuthorization, AclDocument, IdOrIds, IdRef};
use solid_pod_rs::Storage;
use tracing::{info, warn};

/// Rolling release that `ontology-publish.yml` keeps current on `main`.
pub const DEFAULT_RELEASE_URL: &str =
    "https://github.com/DreamLab-AI/VisionClaw/releases/download/ontology-latest";

/// Container the client reads (`client/.../jss/contextLoader.ts`) and the
/// workflow's push path PUTs to (`JSS_PUBLIC_PATH`).
pub const POD_CONTAINER: &str = "/public/ontology/";

const MANIFEST: &str = "index.jsonld";
const SUMS: &str = "SHA256SUMS";

/// Content files in the release, with the MIME type they are stored under.
/// Order is the write order; the manifest is written separately, last.
const CONTENT_FILES: [(&str, &str); 4] = [
    ("visionflow.ttl", "text/turtle"),
    ("context.jsonld", "application/ld+json"),
    ("ontology.jsonld", "application/ld+json"),
    ("visionflow.stats.json", "application/json"),
];

/// Boot-time configuration, read from the environment.
#[derive(Debug, Clone)]
pub struct OntologyPullConfig {
    /// Base URL under which `index.jsonld`, `SHA256SUMS` and the content files
    /// are fetched.
    pub base_url: String,
    /// `ONTOLOGY_PULL_ENABLED=false|0` turns the pull off entirely.
    pub enabled: bool,
    /// Per-request timeout; the JSON-LD is ~22 MB.
    pub timeout: Duration,
    /// Re-check cadence after the boot pull; `None` (from `0`) means boot only.
    /// Each check is one small GET of `index.jsonld` unless the build moved.
    pub interval: Option<Duration>,
}

impl OntologyPullConfig {
    /// `ONTOLOGY_PULL_URL` (default [`DEFAULT_RELEASE_URL`]),
    /// `ONTOLOGY_PULL_ENABLED` (default true),
    /// `ONTOLOGY_PULL_TIMEOUT_SECS` (default 120),
    /// `ONTOLOGY_PULL_INTERVAL_SECS` (default 3600; `0` disables re-checks).
    pub fn from_env() -> Self {
        let enabled = std::env::var("ONTOLOGY_PULL_ENABLED")
            .map(|v| !matches!(v.trim(), "0" | "false" | "no" | "off"))
            .unwrap_or(true);
        let base_url = std::env::var("ONTOLOGY_PULL_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_RELEASE_URL.to_string());
        let timeout = std::env::var("ONTOLOGY_PULL_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(120));
        let interval = std::env::var("ONTOLOGY_PULL_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map_or(Some(Duration::from_secs(3600)), |n| {
                (n > 0).then(|| Duration::from_secs(n))
            });
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            enabled,
            timeout,
            interval,
        }
    }
}

/// What a pull did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullOutcome {
    /// `ONTOLOGY_PULL_ENABLED` is off.
    Disabled,
    /// The pod already holds the release's `visionflow:buildSha`.
    UpToDate { build_sha: String },
    /// Resources were written.
    Updated {
        build_sha: String,
        classes: u64,
        triples: u64,
    },
}

/// Why a pull did nothing. The pod is untouched in every case.
#[derive(Debug, thiserror::Error)]
pub enum PullError {
    #[error("fetch {url}: {reason}")]
    Fetch { url: String, reason: String },
    #[error("release manifest is not usable: {0}")]
    Manifest(String),
    #[error("SHA256SUMS has no entry for {0}")]
    MissingSum(String),
    #[error("sha256 mismatch for {file}: release says {expected}, body is {actual}")]
    Digest {
        file: String,
        expected: String,
        actual: String,
    },
    #[error("pod storage: {0}")]
    Storage(#[from] PodError),
}

/// The one network operation the puller needs.
#[async_trait]
pub trait Fetch: Send + Sync {
    async fn get(&self, url: &str) -> Result<Bytes, PullError>;
}

/// `reqwest`-backed [`Fetch`]; follows GitHub's release redirects.
pub struct HttpFetch {
    client: reqwest::Client,
}

impl HttpFetch {
    pub fn new(timeout: Duration) -> Result<Self, PullError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "visionclaw-server/",
                env!("CARGO_PKG_VERSION"),
                " ontology-pull"
            ))
            .timeout(timeout)
            .build()
            .map_err(|e| PullError::Fetch {
                url: String::new(),
                reason: e.to_string(),
            })?;
        Ok(Self { client })
    }
}

#[async_trait]
impl Fetch for HttpFetch {
    async fn get(&self, url: &str) -> Result<Bytes, PullError> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| PullError::Fetch {
                url: url.to_string(),
                reason: e.to_string(),
            })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(PullError::Fetch {
                url: url.to_string(),
                reason: format!("HTTP {status}"),
            });
        }
        resp.bytes().await.map_err(|e| PullError::Fetch {
            url: url.to_string(),
            reason: e.to_string(),
        })
    }
}

/// Parsed view of the release's `index.jsonld`.
#[derive(Debug, Clone)]
struct Manifest {
    build_sha: String,
    classes: u64,
    triples: u64,
}

fn parse_manifest(body: &[u8]) -> Result<Manifest, PullError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| PullError::Manifest(e.to_string()))?;
    let build_sha = v
        .get("visionflow:buildSha")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| PullError::Manifest("missing visionflow:buildSha".into()))?
        .to_string();
    let contains = v
        .get("ldp:contains")
        .and_then(Value::as_array)
        .ok_or_else(|| PullError::Manifest("missing ldp:contains".into()))?;
    for (name, _) in CONTENT_FILES.iter().take(3) {
        let listed = contains
            .iter()
            .any(|e| e.get("@id").and_then(Value::as_str) == Some(name));
        if !listed {
            return Err(PullError::Manifest(format!("ldp:contains lacks {name}")));
        }
    }
    let num = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    Ok(Manifest {
        build_sha,
        classes: num("visionflow:classes"),
        triples: num("visionflow:triples"),
    })
}

/// `sha256sum` output: `<hex>  <name>` per line; names may carry a `*` or `./`.
fn parse_sums(body: &[u8]) -> HashMap<String, String> {
    let text = String::from_utf8_lossy(body);
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let hex = parts.next()?;
            let name = parts.next()?;
            if hex.len() != 64 {
                return None;
            }
            let name = name.trim_start_matches('*').trim_start_matches("./");
            Some((name.to_string(), hex.to_ascii_lowercase()))
        })
        .collect()
}

fn sha256_hex(body: &[u8]) -> String {
    hex::encode(Sha256::digest(body))
}

/// Public-read ACL for the container and everything under it. Absolute paths
/// rather than `./` so the grant is exactly this subtree whichever container
/// the evaluator was walking when it found the document.
fn public_read_acl() -> AclDocument {
    let target = || {
        Some(IdOrIds::Single(IdRef {
            id: POD_CONTAINER.into(),
        }))
    };
    AclDocument {
        context: None,
        graph: Some(vec![AclAuthorization {
            id: Some("#public".into()),
            r#type: Some("acl:Authorization".into()),
            agent: None,
            agent_class: Some(IdOrIds::Single(IdRef {
                id: "foaf:Agent".into(),
            })),
            agent_group: None,
            origin: None,
            access_to: target(),
            default: target(),
            mode: Some(IdOrIds::Single(IdRef {
                id: "acl:Read".into(),
            })),
            condition: None,
        }]),
    }
}

async fn pod_build_sha<S: Storage + ?Sized>(storage: &S) -> Option<String> {
    let (body, _) = storage
        .get(&format!("{POD_CONTAINER}{MANIFEST}"))
        .await
        .ok()?;
    parse_manifest(&body).ok().map(|m| m.build_sha)
}

/// Run one pull against `storage`. See the module docs for the contract.
pub async fn pull_once<F, S>(
    fetch: &F,
    storage: &S,
    cfg: &OntologyPullConfig,
) -> Result<PullOutcome, PullError>
where
    F: Fetch + ?Sized,
    S: OntologyPublication + ?Sized,
{
    if !cfg.enabled {
        return Ok(PullOutcome::Disabled);
    }
    let url = |name: &str| format!("{}/{}", cfg.base_url, name);

    let manifest_bytes = fetch.get(&url(MANIFEST)).await?;
    let manifest = parse_manifest(&manifest_bytes)?;

    if pod_build_sha(storage).await.as_deref() == Some(manifest.build_sha.as_str())
        && storage.has_atomic_generation().await?
    {
        return Ok(PullOutcome::UpToDate {
            build_sha: manifest.build_sha,
        });
    }

    let sums = parse_sums(&fetch.get(&url(SUMS)).await?);
    let mut verified: Vec<(&str, &str, Bytes)> = Vec::with_capacity(CONTENT_FILES.len());
    for (name, content_type) in CONTENT_FILES {
        let expected = sums
            .get(name)
            .ok_or_else(|| PullError::MissingSum(name.to_string()))?;
        let body = fetch.get(&url(name)).await?;
        let actual = sha256_hex(&body);
        if &actual != expected {
            return Err(PullError::Digest {
                file: name.to_string(),
                expected: expected.clone(),
                actual,
            });
        }
        verified.push((name, content_type, body));
    }
    // The manifest is covered by SHA256SUMS too; verify it the same way.
    if let Some(expected) = sums.get(MANIFEST) {
        let actual = sha256_hex(&manifest_bytes);
        if &actual != expected {
            return Err(PullError::Digest {
                file: MANIFEST.to_string(),
                expected: expected.clone(),
                actual,
            });
        }
    } else {
        return Err(PullError::MissingSum(MANIFEST.to_string()));
    }

    // Everything verified; now touch the pod. Containers first, ACL only if
    // absent so an operator's edit survives, content, manifest last.
    for container in ["/public/", POD_CONTAINER] {
        if !storage.exists(container).await? {
            match storage.create_container(container).await {
                Ok(_) | Err(PodError::AlreadyExists(_)) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    let acl_path = format!("{POD_CONTAINER}.acl");
    if !storage.exists(&acl_path).await? {
        let acl = serde_json::to_vec(&public_read_acl())
            .map_err(|e| PullError::Manifest(format!("serialise ACL: {e}")))?;
        storage
            .put(&acl_path, Bytes::from(acl), "application/ld+json")
            .await?;
    }
    let mut files = verified
        .into_iter()
        .map(|(n, c, b)| (n.to_string(), c.to_string(), b))
        .collect::<Vec<_>>();
    files.push((
        MANIFEST.into(),
        "application/ld+json".into(),
        manifest_bytes,
    ));
    storage.publish_ontology(files).await?;

    Ok(PullOutcome::Updated {
        build_sha: manifest.build_sha,
        classes: manifest.classes,
        triples: manifest.triples,
    })
}

/// Boot pull plus periodic re-check against the live pod backend. Never
/// blocks start-up and never fails the server: outcomes are logged.
pub fn spawn_boot_pull<S>(storage: Arc<S>)
where
    S: OntologyPublication + ?Sized + 'static,
{
    let cfg = OntologyPullConfig::from_env();
    if !cfg.enabled {
        info!("ontology pull disabled (ONTOLOGY_PULL_ENABLED)");
        return;
    }
    tokio::spawn(async move {
        let fetch = match HttpFetch::new(cfg.timeout) {
            Ok(f) => f,
            Err(e) => {
                warn!("ontology pull: cannot build HTTP client: {e}");
                return;
            }
        };
        loop {
            log_outcome(pull_once(&fetch, storage.as_ref(), &cfg).await, &cfg);
            match cfg.interval {
                Some(every) => tokio::time::sleep(every).await,
                None => break,
            }
        }
    });
}

fn log_outcome(result: Result<PullOutcome, PullError>, cfg: &OntologyPullConfig) {
    match result {
        Ok(PullOutcome::Updated {
            build_sha,
            classes,
            triples,
        }) => info!(
            "ontology pull: {POD_CONTAINER} updated to build {build_sha} ({classes} classes, {triples} triples) from {}",
            cfg.base_url
        ),
        Ok(PullOutcome::UpToDate { build_sha }) => {
            info!("ontology pull: {POD_CONTAINER} already at build {build_sha}")
        }
        Ok(PullOutcome::Disabled) => {}
        Err(e) => warn!(
            "ontology pull: {e}; {POD_CONTAINER} update failed; active generation remains complete; activation durability may require inspection (source {})",
            cfg.base_url
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solid_pod_rs::storage::memory::MemoryBackend;
    use std::sync::Mutex;

    /// Map-backed fetcher that records every URL it was asked for.
    struct MapFetch {
        files: HashMap<String, Bytes>,
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Fetch for MapFetch {
        async fn get(&self, url: &str) -> Result<Bytes, PullError> {
            self.calls.lock().unwrap().push(url.to_string());
            let name = url.rsplit('/').next().unwrap();
            self.files
                .get(name)
                .cloned()
                .ok_or_else(|| PullError::Fetch {
                    url: url.to_string(),
                    reason: "HTTP 404".into(),
                })
        }
    }

    // Sequence-only test backends retain the original fetch/check test fixtures;
    // durable atomic visibility is tested on PublishedStorage's real filesystem.
    #[async_trait]
    impl OntologyPublication for MemoryBackend {
        async fn publish_ontology(
            &self,
            files: Vec<super::super::ontology_generation::PublishedFile>,
        ) -> Result<(), PodError> {
            for (n, c, b) in files {
                self.put(&format!("{POD_CONTAINER}{n}"), b, &c).await?;
            }
            Ok(())
        }
    }
    #[async_trait]
    impl OntologyPublication for FaultStorage {
        async fn publish_ontology(
            &self,
            files: Vec<super::super::ontology_generation::PublishedFile>,
        ) -> Result<(), PodError> {
            self.inner.publish_ontology(files).await
        }
    }

    struct FaultStorage {
        inner: MemoryBackend,
        fail_exists: String,
    }

    #[async_trait]
    impl Storage for FaultStorage {
        async fn get(
            &self,
            p: &str,
        ) -> Result<(Bytes, solid_pod_rs::storage::ResourceMeta), PodError> {
            self.inner.get(p).await
        }
        async fn put(
            &self,
            p: &str,
            b: Bytes,
            c: &str,
        ) -> Result<solid_pod_rs::storage::ResourceMeta, PodError> {
            self.inner.put(p, b, c).await
        }
        async fn delete(&self, p: &str) -> Result<(), PodError> {
            self.inner.delete(p).await
        }
        async fn list(&self, p: &str) -> Result<Vec<String>, PodError> {
            self.inner.list(p).await
        }
        async fn head(&self, p: &str) -> Result<solid_pod_rs::storage::ResourceMeta, PodError> {
            self.inner.head(p).await
        }
        async fn exists(&self, p: &str) -> Result<bool, PodError> {
            if p == self.fail_exists {
                return Err(PodError::AlreadyExists("injected existence failure".into()));
            }
            self.inner.exists(p).await
        }
        async fn create_container(
            &self,
            p: &str,
        ) -> Result<solid_pod_rs::storage::ResourceMeta, PodError> {
            self.inner.create_container(p).await
        }
        async fn watch(
            &self,
            p: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<solid_pod_rs::storage::StorageEvent>, PodError>
        {
            self.inner.watch(p).await
        }
    }

    #[tokio::test]
    async fn acl_probe_failure_preserves_operator_acl_and_content() {
        let acl_path = format!("{POD_CONTAINER}.acl");
        let storage = FaultStorage {
            inner: MemoryBackend::new(),
            fail_exists: acl_path.clone(),
        };
        storage.inner.create_container("/public/").await.unwrap();
        storage.inner.create_container(POD_CONTAINER).await.unwrap();
        storage
            .inner
            .put(
                &acl_path,
                Bytes::from_static(b"operator-private-acl"),
                "application/ld+json",
            )
            .await
            .unwrap();
        let fetch = MapFetch {
            files: release("new"),
            calls: Mutex::new(Vec::new()),
        };
        assert!(pull_once(&fetch, &storage, &cfg()).await.is_err());
        assert_eq!(
            storage.inner.get(&acl_path).await.unwrap().0,
            Bytes::from_static(b"operator-private-acl")
        );
        assert!(!storage
            .inner
            .exists(&format!("{POD_CONTAINER}visionflow.ttl"))
            .await
            .unwrap());
    }

    #[actix_web::test]
    async fn pinned_generation_obeys_canonical_resource_acl() {
        use crate::handlers::solid_proxy_handler::{handle_solid_proxy, SolidPodState};
        use crate::services::nostr_service::NostrService;
        use crate::services::ontology_generation::PublishedStorage;
        use actix_web::{test::TestRequest, web};
        let dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(PublishedStorage::new(dir.path()).await.unwrap());
        pull_once(&fetcher(release("one")), storage.as_ref(), &cfg())
            .await
            .unwrap();
        let manifest: Value = serde_json::from_slice(
            &storage
                .get(&format!("{POD_CONTAINER}{MANIFEST}"))
                .await
                .unwrap()
                .0,
        )
        .unwrap();
        let gen = manifest["visionflow:generation"].as_str().unwrap();
        let state = web::Data::new(SolidPodState {
            storage: storage.clone(),
            data_root: dir.path().into(),
            server_keys: None,
            allow_anonymous: true,
        });
        let path = format!("public/ontology/@{gen}/ontology.jsonld");
        let request = || {
            TestRequest::get()
                .uri(&format!("/solid/{path}"))
                .to_http_request()
        };
        let first = handle_solid_proxy(
            request(),
            Bytes::new(),
            web::Path::from(path.clone()),
            state.clone(),
            web::Data::new(NostrService::new()),
        )
        .await;
        assert_eq!(first.status(), actix_web::http::StatusCode::OK);
        let mut private = public_read_acl();
        private.graph = Some(vec![]);
        storage
            .put(
                &format!("{POD_CONTAINER}ontology.jsonld.acl"),
                Bytes::from(serde_json::to_vec(&private).unwrap()),
                "application/ld+json",
            )
            .await
            .unwrap();
        let denied = handle_solid_proxy(
            request(),
            Bytes::new(),
            web::Path::from(path),
            state,
            web::Data::new(NostrService::new()),
        )
        .await;
        assert_eq!(denied.status(), actix_web::http::StatusCode::UNAUTHORIZED);
    }

    fn cfg() -> OntologyPullConfig {
        OntologyPullConfig {
            base_url: "https://example.test/rel".into(),
            enabled: true,
            timeout: Duration::from_secs(1),
            interval: None,
        }
    }

    fn manifest_json(build_sha: &str) -> String {
        format!(
            r#"{{"@id":"/public/ontology/","ldp:contains":[{{"@id":"ontology.jsonld"}},{{"@id":"context.jsonld"}},{{"@id":"visionflow.ttl"}}],"visionflow:buildSha":"{build_sha}","visionflow:classes":8434,"visionflow:triples":265455}}"#
        )
    }

    /// A complete, self-consistent release: content files, manifest, and a
    /// SHA256SUMS that covers all five.
    fn release(build_sha: &str) -> HashMap<String, Bytes> {
        let mut files: HashMap<String, Bytes> = HashMap::new();
        files.insert(
            "visionflow.ttl".into(),
            Bytes::from_static(b"@prefix owl: <o> .\n"),
        );
        files.insert(
            "context.jsonld".into(),
            Bytes::from_static(b"{\"@context\":{}}"),
        );
        files.insert(
            "ontology.jsonld".into(),
            Bytes::from_static(b"{\"@graph\":[]}"),
        );
        files.insert(
            "visionflow.stats.json".into(),
            Bytes::from_static(b"{\"triples\":1}"),
        );
        files.insert(MANIFEST.into(), Bytes::from(manifest_json(build_sha)));
        let sums: String = files
            .iter()
            .map(|(n, b)| format!("{}  {}\n", sha256_hex(b), n))
            .collect();
        files.insert(SUMS.into(), Bytes::from(sums));
        files
    }

    fn fetcher(files: HashMap<String, Bytes>) -> MapFetch {
        MapFetch {
            files,
            calls: Mutex::new(Vec::new()),
        }
    }

    #[tokio::test]
    async fn first_pull_writes_everything_with_acl_and_manifest_last() {
        let storage = MemoryBackend::new();
        let f = fetcher(release("abc123"));
        let out = pull_once(&f, &storage, &cfg()).await.unwrap();
        assert_eq!(
            out,
            PullOutcome::Updated {
                build_sha: "abc123".into(),
                classes: 8434,
                triples: 265455
            }
        );
        for (name, ct) in CONTENT_FILES {
            let (_, meta) = storage
                .get(&format!("{POD_CONTAINER}{name}"))
                .await
                .unwrap();
            assert_eq!(meta.content_type, ct, "{name}");
        }
        let (acl, _) = storage.get(&format!("{POD_CONTAINER}.acl")).await.unwrap();
        let doc: AclDocument = serde_json::from_slice(&acl).unwrap();
        assert_eq!(doc.graph.unwrap().len(), 1);
        assert_eq!(pod_build_sha(&storage).await.as_deref(), Some("abc123"));
        // manifest fetched first, then sums, then content; the order the
        // writes happen in is what makes a partial pull safe.
        let calls = f.calls.lock().unwrap();
        assert!(calls[0].ends_with(MANIFEST));
        assert!(calls[1].ends_with(SUMS));
    }

    #[tokio::test]
    async fn same_build_sha_is_a_no_op_without_downloading_content() {
        let storage = MemoryBackend::new();
        let f = fetcher(release("abc123"));
        pull_once(&f, &storage, &cfg()).await.unwrap();
        let before = f.calls.lock().unwrap().len();
        let out = pull_once(&f, &storage, &cfg()).await.unwrap();
        assert_eq!(
            out,
            PullOutcome::UpToDate {
                build_sha: "abc123".into()
            }
        );
        assert_eq!(
            f.calls.lock().unwrap().len(),
            before + 1,
            "only the manifest"
        );
    }

    #[tokio::test]
    async fn new_build_sha_replaces_content_but_keeps_operator_acl() {
        let storage = MemoryBackend::new();
        pull_once(&fetcher(release("one")), &storage, &cfg())
            .await
            .unwrap();
        let acl_path = format!("{POD_CONTAINER}.acl");
        storage
            .put(
                &acl_path,
                Bytes::from_static(b"{\"graph\":[]}"),
                "application/ld+json",
            )
            .await
            .unwrap();
        let mut files = release("two");
        files.insert(
            "visionflow.ttl".into(),
            Bytes::from_static(b"@prefix owl: <two> .\n"),
        );
        // re-sign after the edit
        let sums: String = files
            .iter()
            .filter(|(n, _)| n.as_str() != SUMS)
            .map(|(n, b)| format!("{}  {}\n", sha256_hex(b), n))
            .collect();
        files.insert(SUMS.into(), Bytes::from(sums));
        let out = pull_once(&fetcher(files), &storage, &cfg()).await.unwrap();
        assert!(matches!(out, PullOutcome::Updated { ref build_sha, .. } if build_sha == "two"));
        let (ttl, _) = storage
            .get(&format!("{POD_CONTAINER}visionflow.ttl"))
            .await
            .unwrap();
        assert_eq!(&ttl[..], b"@prefix owl: <two> .\n");
        let (acl, _) = storage.get(&acl_path).await.unwrap();
        assert_eq!(&acl[..], b"{\"graph\":[]}", "operator ACL not clobbered");
    }

    #[tokio::test]
    async fn digest_mismatch_writes_nothing() {
        let storage = MemoryBackend::new();
        let mut files = release("abc123");
        files.insert(
            "ontology.jsonld".into(),
            Bytes::from_static(b"{\"@graph\":[\"tampered\"]}"),
        );
        let err = pull_once(&fetcher(files), &storage, &cfg())
            .await
            .unwrap_err();
        assert!(matches!(err, PullError::Digest { ref file, .. } if file == "ontology.jsonld"));
        assert!(!storage.exists(POD_CONTAINER).await.unwrap_or(false));
        assert!(pod_build_sha(&storage).await.is_none());
    }

    #[tokio::test]
    async fn missing_sum_entry_writes_nothing() {
        let storage = MemoryBackend::new();
        let mut files = release("abc123");
        files.insert(SUMS.into(), Bytes::from_static(b""));
        let err = pull_once(&fetcher(files), &storage, &cfg())
            .await
            .unwrap_err();
        assert!(matches!(err, PullError::MissingSum(ref f) if f == "visionflow.ttl"));
        assert!(pod_build_sha(&storage).await.is_none());
    }

    #[tokio::test]
    async fn unreachable_release_leaves_pod_untouched() {
        let storage = MemoryBackend::new();
        pull_once(&fetcher(release("one")), &storage, &cfg())
            .await
            .unwrap();
        let err = pull_once(&fetcher(HashMap::new()), &storage, &cfg())
            .await
            .unwrap_err();
        assert!(matches!(err, PullError::Fetch { .. }));
        assert_eq!(pod_build_sha(&storage).await.as_deref(), Some("one"));
    }

    #[tokio::test]
    async fn disabled_config_short_circuits() {
        let storage = MemoryBackend::new();
        let f = fetcher(release("x"));
        let c = OntologyPullConfig {
            enabled: false,
            ..cfg()
        };
        assert_eq!(
            pull_once(&f, &storage, &c).await.unwrap(),
            PullOutcome::Disabled
        );
        assert!(f.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn sums_parser_tolerates_binary_marker_and_dot_slash() {
        let sums = parse_sums(b"AA00  *visionflow.ttl\nbb11  ./index.jsonld\nshort  x\n");
        assert_eq!(sums.len(), 0, "64-hex only");
        let h = "0".repeat(64);
        let sums = parse_sums(format!("{h}  *visionflow.ttl\n{h}  ./index.jsonld\n").as_bytes());
        assert_eq!(sums.get("visionflow.ttl"), Some(&h));
        assert_eq!(sums.get("index.jsonld"), Some(&h));
    }

    #[test]
    fn manifest_requires_build_sha_and_the_three_resources() {
        assert!(parse_manifest(br#"{"ldp:contains":[]}"#).is_err());
        assert!(parse_manifest(manifest_json("s").as_bytes()).is_ok());
        let missing = r#"{"ldp:contains":[{"@id":"ontology.jsonld"}],"visionflow:buildSha":"s"}"#;
        assert!(parse_manifest(missing.as_bytes()).is_err());
    }

    #[test]
    fn env_config_defaults_and_overrides() {
        std::env::remove_var("ONTOLOGY_PULL_URL");
        std::env::remove_var("ONTOLOGY_PULL_ENABLED");
        std::env::remove_var("ONTOLOGY_PULL_TIMEOUT_SECS");
        let c = OntologyPullConfig::from_env();
        assert_eq!(c.base_url, DEFAULT_RELEASE_URL);
        assert!(c.enabled);
        assert_eq!(c.timeout, Duration::from_secs(120));
        assert_eq!(c.interval, Some(Duration::from_secs(3600)));
        std::env::set_var("ONTOLOGY_PULL_INTERVAL_SECS", "0");
        assert_eq!(OntologyPullConfig::from_env().interval, None);
        std::env::set_var("ONTOLOGY_PULL_URL", "http://loom:8080/ontology/");
        std::env::set_var("ONTOLOGY_PULL_ENABLED", "off");
        std::env::set_var("ONTOLOGY_PULL_TIMEOUT_SECS", "7");
        let c = OntologyPullConfig::from_env();
        assert_eq!(c.base_url, "http://loom:8080/ontology");
        assert!(!c.enabled);
        assert_eq!(c.timeout, Duration::from_secs(7));
        std::env::remove_var("ONTOLOGY_PULL_URL");
        std::env::remove_var("ONTOLOGY_PULL_ENABLED");
        std::env::remove_var("ONTOLOGY_PULL_TIMEOUT_SECS");
        std::env::remove_var("ONTOLOGY_PULL_INTERVAL_SECS");
    }
}
