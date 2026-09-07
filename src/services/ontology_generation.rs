//! Immutable ontology generations over the actual embedded filesystem backend.
//! Canonical content reads resolve one atomic pointer; ACLs remain operator-owned.
use async_trait::async_trait;
use bytes::Bytes;
use solid_pod_rs::{
    error::PodError,
    storage::{fs::FsBackend, ResourceMeta, StorageEvent},
    Storage,
};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const PREFIX: &str = "/public/ontology/";
const PRIVATE: &str = "/.ontology-generations/";
const POINTER: &str = ".ontology-current";
const NAMES: [&str; 5] = [
    "visionflow.ttl",
    "context.jsonld",
    "ontology.jsonld",
    "visionflow.stats.json",
    "index.jsonld",
];
pub type PublishedFile = (String, String, Bytes);

#[async_trait]
pub trait OntologyPublication: Storage {
    async fn publish_ontology(&self, files: Vec<PublishedFile>) -> Result<(), PodError>;
    async fn has_atomic_generation(&self) -> Result<bool, PodError> {
        Ok(true)
    }
}

pub struct PublishedStorage {
    inner: FsBackend,
}
impl PublishedStorage {
    pub async fn new(root: impl Into<PathBuf>) -> Result<Self, PodError> {
        Ok(Self {
            inner: FsBackend::new(root).await?,
        })
    }
    /// Versioned reads use the SAME resource-specific and inherited WAC ACLs
    /// as the canonical resource. A pinned URL must not evade a private ACL.
    pub fn authorization_path(path: &str) -> String {
        if let Some((id, name)) = path
            .strip_prefix(PREFIX)
            .and_then(|s| s.strip_prefix('@'))
            .and_then(|s| s.split_once('/'))
        {
            if valid_id(id) && NAMES.contains(&name) {
                return format!("{PREFIX}{name}");
            }
        }
        path.into()
    }
    pub fn root(&self) -> &Path {
        self.inner.root()
    }
    async fn generation(&self) -> Result<Option<String>, PodError> {
        if let Ok(meta) = tokio::fs::symlink_metadata(self.root().join(POINTER)).await {
            if meta.file_type().is_symlink() {
                return Err(PodError::Forbidden);
            }
        }
        match tokio::fs::read_to_string(self.root().join(POINTER)).await {
            Ok(id) if valid_id(&id) => Ok(Some(id)),
            Ok(_) => Err(PodError::Backend(
                "invalid ontology generation pointer".into(),
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    async fn generation_path(&self, id: &str, name: &str) -> Result<String, PodError> {
        let rel = format!("{PRIVATE}{id}/{name}");
        let parent = self.root().join(PRIVATE.trim_matches('/'));
        for item in [
            parent.clone(),
            parent.join(id),
            parent.join(id).join(name),
            parent.join(id).join(format!("{name}.meta.json")),
        ] {
            if let Ok(meta) = tokio::fs::symlink_metadata(item).await {
                if meta.file_type().is_symlink() {
                    return Err(PodError::Forbidden);
                }
            }
        }
        Ok(rel)
    }
    async fn resolve(&self, path: &str) -> Result<String, PodError> {
        if path
            .split('/')
            .any(|s| s.starts_with(".ontology-") || s == "." || s == "..")
        {
            return Err(PodError::Forbidden);
        }
        if let Some(rest) = path.strip_prefix(PREFIX) {
            // An explicit generation URL is immutable and remains valid after
            // publication. Only the five declared content resources are exposed.
            if let Some((id, name)) = rest.strip_prefix('@').and_then(|s| s.split_once('/')) {
                if valid_id(id) && NAMES.contains(&name) {
                    return self.generation_path(id, name).await;
                }
                return Err(PodError::InvalidPath(path.into()));
            }
            if NAMES.contains(&rest) {
                if let Some(id) = self.generation().await? {
                    return self.generation_path(&id, rest).await;
                }
            }
        }
        Ok(path.into())
    }
    fn writable(path: &str) -> Result<(), PodError> {
        if path
            .split('/')
            .any(|s| s.starts_with(".ontology-") || s == "." || s == "..")
            || path
                .strip_prefix(PREFIX)
                .is_some_and(|p| NAMES.contains(&p) || p.starts_with('@'))
        {
            return Err(PodError::Forbidden);
        }
        Ok(())
    }
    async fn publish_with_failure(
        &self,
        mut files: Vec<PublishedFile>,
        fail_before: Option<usize>,
    ) -> Result<(), PodError> {
        if files.len() != NAMES.len()
            || NAMES
                .iter()
                .any(|n| files.iter().filter(|f| f.0 == *n).count() != 1)
        {
            return Err(PodError::Backend(
                "ontology publication requires exactly the five declared resources".into(),
            ));
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        // This pointer is a local publication identity, not the upstream release
        // hash. The manifest retains buildSha and gains an explicit pinned URL.
        let manifest = files.iter_mut().find(|f| f.0 == "index.jsonld").unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&manifest.2)?;
        if !value.is_object() {
            return Err(PodError::Backend(
                "ontology manifest must be an object".into(),
            ));
        }
        value["visionflow:generation"] = id.clone().into();
        manifest.2 = Bytes::from(serde_json::to_vec(&value)?);
        if let Ok(meta) =
            tokio::fs::symlink_metadata(self.root().join(PRIVATE.trim_matches('/'))).await
        {
            if meta.file_type().is_symlink() {
                return Err(PodError::Forbidden);
            }
        }
        let stage = format!("{PRIVATE}{id}/");
        self.inner.create_container(&stage).await?;
        for (i, (name, mime, bytes)) in files.into_iter().enumerate() {
            if fail_before == Some(i) {
                return Err(PodError::Backend(
                    "injected generation write failure".into(),
                ));
            }
            self.inner
                .put(&format!("{stage}{name}"), bytes, &mime)
                .await?;
        }
        // Sync content AND metadata sidecars before the pointer can name them.
        let dir = self.root().join(stage.trim_start_matches('/'));
        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            tokio::fs::File::open(entry.path())
                .await?
                .sync_all()
                .await?;
        }
        tokio::fs::File::open(&dir).await?.sync_all().await?;
        tokio::fs::File::open(dir.parent().unwrap())
            .await?
            .sync_all()
            .await?;
        let pending = self.root().join(format!("{POINTER}.{id}.tmp"));
        let mut pointer = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)
            .await?;
        if fail_before == Some(7) {
            return Err(PodError::Backend("injected pointer write failure".into()));
        }
        pointer.write_all(id.as_bytes()).await?;
        if fail_before == Some(6) {
            return Err(PodError::Backend("injected pointer sync failure".into()));
        }
        pointer.sync_all().await?;
        if fail_before == Some(NAMES.len()) {
            return Err(PodError::Backend("injected activation failure".into()));
        }
        tokio::fs::rename(&pending, self.root().join(POINTER)).await?;
        tokio::fs::File::open(self.root()).await?.sync_all().await?;
        Ok(())
    }
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
#[async_trait]
impl OntologyPublication for PublishedStorage {
    async fn has_atomic_generation(&self) -> Result<bool, PodError> {
        Ok(self.generation().await?.is_some())
    }
    async fn publish_ontology(&self, files: Vec<PublishedFile>) -> Result<(), PodError> {
        self.publish_with_failure(files, None).await
    }
}
#[async_trait]
impl Storage for PublishedStorage {
    async fn get(&self, p: &str) -> Result<(Bytes, ResourceMeta), PodError> {
        self.inner.get(&self.resolve(p).await?).await
    }
    async fn head(&self, p: &str) -> Result<ResourceMeta, PodError> {
        self.inner.head(&self.resolve(p).await?).await
    }
    async fn exists(&self, p: &str) -> Result<bool, PodError> {
        self.inner.exists(&self.resolve(p).await?).await
    }
    async fn put(&self, p: &str, b: Bytes, c: &str) -> Result<ResourceMeta, PodError> {
        Self::writable(p)?;
        self.inner.put(p, b, c).await
    }
    async fn delete(&self, p: &str) -> Result<(), PodError> {
        Self::writable(p)?;
        self.inner.delete(p).await
    }
    async fn list(&self, p: &str) -> Result<Vec<String>, PodError> {
        if p.split('/')
            .any(|s| s.starts_with(".ontology-") || s == "." || s == "..")
        {
            return Err(PodError::Forbidden);
        }
        let mut list = self.inner.list(p).await?;
        list.retain(|s| !s.starts_with(".ontology-"));
        if p.trim_end_matches('/') == PREFIX.trim_end_matches('/')
            && self.generation().await?.is_some()
        {
            for name in NAMES {
                if !list.iter().any(|s| s == name) {
                    list.push(name.into());
                }
            }
        }
        Ok(list)
    }
    async fn create_container(&self, p: &str) -> Result<ResourceMeta, PodError> {
        Self::writable(p)?;
        self.inner.create_container(p).await
    }
    async fn watch(&self, p: &str) -> Result<tokio::sync::mpsc::Receiver<StorageEvent>, PodError> {
        self.inner.watch(&self.resolve(p).await?).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn files(value: &str) -> Vec<PublishedFile> {
        NAMES
            .iter()
            .map(|n| {
                (
                    n.to_string(),
                    "application/json".into(),
                    Bytes::from(if *n == "index.jsonld" {
                        format!("{{\"visionflow:buildSha\":\"{value}\"}}")
                    } else {
                        value.into()
                    }),
                )
            })
            .collect()
    }
    #[tokio::test]
    async fn every_pre_activation_failure_preserves_prior_generation_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let store = PublishedStorage::new(dir.path()).await.unwrap();
        store.publish_ontology(files("old")).await.unwrap();
        let old = store.generation().await.unwrap().unwrap();
        for fail in 0..=7 {
            assert!(store
                .publish_with_failure(files("new"), Some(fail))
                .await
                .is_err());
            let restarted = PublishedStorage::new(dir.path()).await.unwrap();
            assert_eq!(
                restarted
                    .get(&format!("{PREFIX}ontology.jsonld"))
                    .await
                    .unwrap()
                    .0,
                Bytes::from("old")
            );
            assert_eq!(
                restarted.generation().await.unwrap().as_deref(),
                Some(old.as_str())
            );
        }
        store.publish_ontology(files("new")).await.unwrap();
        assert_eq!(
            store
                .get(&format!("{PREFIX}ontology.jsonld"))
                .await
                .unwrap()
                .0,
            Bytes::from("new")
        );
        assert_eq!(
            store
                .get(&format!("{PREFIX}@{old}/ontology.jsonld"))
                .await
                .unwrap()
                .0,
            Bytes::from("old")
        );
        assert!(store
            .put(
                &format!("{PREFIX}ontology.jsonld"),
                Bytes::new(),
                "text/plain"
            )
            .await
            .is_err());
        assert!(store
            .get(&format!("{PRIVATE}{old}/ontology.jsonld"))
            .await
            .is_err());
    }
    #[tokio::test]
    async fn pinned_bundle_stays_coherent_during_concurrent_activation() {
        let dir = tempfile::tempdir().unwrap();
        let store = std::sync::Arc::new(PublishedStorage::new(dir.path()).await.unwrap());
        store.publish_ontology(files("initial")).await.unwrap();
        let writer = store.clone();
        let work = tokio::spawn(async move {
            for i in 0..8 {
                writer
                    .publish_ontology(files(&format!("generation-{i}")))
                    .await
                    .unwrap();
            }
        });
        for _ in 0..16 {
            let manifest: serde_json::Value = serde_json::from_slice(
                &store.get(&format!("{PREFIX}index.jsonld")).await.unwrap().0,
            )
            .unwrap();
            let id = manifest["visionflow:generation"].as_str().unwrap();
            for name in ["ontology.jsonld", "visionflow.ttl"] {
                assert_eq!(
                    store.get(&format!("{PREFIX}@{id}/{name}")).await.unwrap().0,
                    Bytes::from(
                        manifest["visionflow:buildSha"]
                            .as_str()
                            .unwrap()
                            .to_string()
                    )
                );
            }
        }
        work.await.unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn private_aliases_and_symlink_generation_roots_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let store = PublishedStorage::new(dir.path()).await.unwrap();
        for path in [
            "/.ontology-current",
            "//.ontology-current",
            "/./.ontology-current",
            "/public/ontology/@../ontology.jsonld",
        ] {
            assert!(store.get(path).await.is_err());
        }
        std::os::unix::fs::symlink(outside.path(), dir.path().join(".ontology-generations"))
            .unwrap();
        assert!(store.publish_ontology(files("bad")).await.is_err());
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn corrupt_pointer_fails_closed_and_acl_stays_separate() {
        let dir = tempfile::tempdir().unwrap();
        let store = PublishedStorage::new(dir.path()).await.unwrap();
        let acl = format!("{PREFIX}.acl");
        store
            .put(&acl, Bytes::from("private"), "text/plain")
            .await
            .unwrap();
        store.publish_ontology(files("new")).await.unwrap();
        assert_eq!(store.get(&acl).await.unwrap().0, Bytes::from("private"));
        tokio::fs::write(dir.path().join(POINTER), "../escape")
            .await
            .unwrap();
        assert!(store
            .get(&format!("{PREFIX}ontology.jsonld"))
            .await
            .is_err());
    }
}
