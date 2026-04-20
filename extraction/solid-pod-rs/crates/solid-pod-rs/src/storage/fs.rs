//! Filesystem storage backend.
//!
//! Persists pod resources under a root directory. Each resource body
//! is stored as a file. A sidecar file with the `.meta.json`
//! extension carries the content-type and Link header values.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::sync::mpsc;

use crate::error::PodError;
use crate::storage::{ResourceMeta, Storage, StorageEvent};

const META_SUFFIX: &str = ".meta.json";

/// Filesystem-rooted `Storage` implementation.
#[derive(Clone)]
pub struct FsBackend {
    root: Arc<PathBuf>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MetaSidecar {
    content_type: String,
    #[serde(default)]
    links: Vec<String>,
}

impl FsBackend {
    /// Create a new backend rooted at `root`. The directory must
    /// exist or be creatable; this call ensures it exists.
    pub async fn new(root: impl Into<PathBuf>) -> Result<Self, PodError> {
        let root: PathBuf = root.into();
        fs::create_dir_all(&root).await?;
        Ok(Self {
            root: Arc::new(root),
        })
    }

    /// Return the root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn normalize(path: &str) -> Result<String, PodError> {
        let p = if path.is_empty() {
            "/".to_string()
        } else if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        if p.contains("..") || p.contains('\0') {
            return Err(PodError::InvalidPath(p));
        }
        Ok(p)
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, PodError> {
        let norm = Self::normalize(path)?;
        let rel = norm.trim_start_matches('/');
        let full = self.root.join(rel);
        if !full.starts_with(self.root.as_path()) {
            return Err(PodError::InvalidPath(norm));
        }
        Ok(full)
    }

    fn meta_path(data_path: &Path) -> PathBuf {
        let mut p = data_path.as_os_str().to_owned();
        p.push(META_SUFFIX);
        PathBuf::from(p)
    }

    fn compute_etag(body: &[u8]) -> String {
        hex::encode(Sha256::digest(body))
    }

    async fn read_meta(
        &self,
        path: &str,
        body_len: u64,
        etag: String,
        modified: chrono::DateTime<chrono::Utc>,
    ) -> Result<ResourceMeta, PodError> {
        let data_path = self.resolve(path)?;
        let meta_path = Self::meta_path(&data_path);
        let (content_type, links) = match fs::read(&meta_path).await {
            Ok(bytes) => {
                let sidecar: MetaSidecar =
                    serde_json::from_slice(&bytes).unwrap_or_else(|_| MetaSidecar {
                        content_type: "application/octet-stream".into(),
                        links: Vec::new(),
                    });
                (sidecar.content_type, sidecar.links)
            }
            Err(_) => ("application/octet-stream".to_string(), Vec::new()),
        };
        Ok(ResourceMeta {
            etag,
            modified,
            size: body_len,
            content_type,
            links,
        })
    }
}

#[async_trait]
impl Storage for FsBackend {
    async fn get(&self, path: &str) -> Result<(Bytes, ResourceMeta), PodError> {
        let data_path = self.resolve(path)?;
        let body = match fs::read(&data_path).await {
            Ok(b) => Bytes::from(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(PodError::NotFound(path.into()));
            }
            Err(e) => return Err(e.into()),
        };
        let metadata = fs::metadata(&data_path).await?;
        let modified: chrono::DateTime<chrono::Utc> = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| {
                chrono::DateTime::from_timestamp(d.as_secs() as i64, d.subsec_nanos())
                    .unwrap_or_else(chrono::Utc::now)
            })
            .unwrap_or_else(chrono::Utc::now);
        let etag = Self::compute_etag(&body);
        let meta = self
            .read_meta(path, body.len() as u64, etag, modified)
            .await?;
        Ok((body, meta))
    }

    async fn put(
        &self,
        path: &str,
        body: Bytes,
        content_type: &str,
    ) -> Result<ResourceMeta, PodError> {
        let data_path = self.resolve(path)?;
        if let Some(parent) = data_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&data_path, &body).await?;
        let sidecar = MetaSidecar {
            content_type: content_type.to_string(),
            links: Vec::new(),
        };
        let sidecar_bytes = serde_json::to_vec(&sidecar)?;
        fs::write(Self::meta_path(&data_path), &sidecar_bytes).await?;
        let etag = Self::compute_etag(&body);
        Ok(ResourceMeta {
            etag,
            modified: chrono::Utc::now(),
            size: body.len() as u64,
            content_type: content_type.to_string(),
            links: Vec::new(),
        })
    }

    async fn delete(&self, path: &str) -> Result<(), PodError> {
        let data_path = self.resolve(path)?;
        match fs::remove_file(&data_path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(PodError::NotFound(path.into()));
            }
            Err(e) => return Err(e.into()),
        }
        let _ = fs::remove_file(Self::meta_path(&data_path)).await;
        Ok(())
    }

    async fn list(&self, container: &str) -> Result<Vec<String>, PodError> {
        let container_path = self.resolve(container)?;
        let mut out = Vec::new();
        let mut dir = match fs::read_dir(&container_path).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(out);
            }
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(META_SUFFIX) {
                continue;
            }
            let ft = entry.file_type().await?;
            if ft.is_dir() {
                out.push(format!("{name}/"));
            } else {
                out.push(name);
            }
        }
        out.sort();
        Ok(out)
    }

    async fn head(&self, path: &str) -> Result<ResourceMeta, PodError> {
        let data_path = self.resolve(path)?;
        let metadata = match fs::metadata(&data_path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(PodError::NotFound(path.into()));
            }
            Err(e) => return Err(e.into()),
        };
        let body = fs::read(&data_path).await?;
        let etag = Self::compute_etag(&body);
        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| {
                chrono::DateTime::from_timestamp(d.as_secs() as i64, d.subsec_nanos())
                    .unwrap_or_else(chrono::Utc::now)
            })
            .unwrap_or_else(chrono::Utc::now);
        self.read_meta(path, body.len() as u64, etag, modified).await
    }

    async fn exists(&self, path: &str) -> Result<bool, PodError> {
        let data_path = self.resolve(path)?;
        Ok(fs::try_exists(&data_path).await.unwrap_or(false))
    }

    async fn watch(&self, path: &str) -> Result<mpsc::Receiver<StorageEvent>, PodError> {
        use notify::{RecursiveMode, Watcher};

        let data_path = self.resolve(path)?;
        let filter_root = data_path.clone();
        let root = self.root.clone();
        let (tx, rx) = mpsc::channel::<StorageEvent>(64);

        let (raw_tx, raw_rx) =
            std::sync::mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = raw_tx.send(res);
        })?;
        let mode = if data_path.is_dir() {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        let watch_target = if data_path.exists() {
            data_path.clone()
        } else {
            root.to_path_buf()
        };
        watcher.watch(&watch_target, mode)?;

        tokio::task::spawn_blocking(move || {
            let _keep = watcher;
            while let Ok(Ok(event)) = raw_rx.recv() {
                for path in &event.paths {
                    let s = path.to_string_lossy();
                    if s.ends_with(META_SUFFIX) {
                        continue;
                    }
                    let virt = match path.strip_prefix(root.as_path()) {
                        Ok(p) => format!("/{}", p.to_string_lossy()),
                        Err(_) => continue,
                    };
                    if !path.starts_with(&filter_root) && path != &filter_root {
                        continue;
                    }
                    use notify::EventKind;
                    let storage_event = match event.kind {
                        EventKind::Create(_) => StorageEvent::Created(virt),
                        EventKind::Modify(_) => StorageEvent::Updated(virt),
                        EventKind::Remove(_) => StorageEvent::Deleted(virt),
                        _ => continue,
                    };
                    if tx.blocking_send(storage_event).is_err() {
                        return;
                    }
                }
            }
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn put_get_roundtrip() {
        let dir = TempDir::new().unwrap();
        let fsb = FsBackend::new(dir.path()).await.unwrap();
        fsb.put("/a/b.txt", Bytes::from_static(b"hello"), "text/plain")
            .await
            .unwrap();
        let (body, meta) = fsb.get("/a/b.txt").await.unwrap();
        assert_eq!(&body[..], b"hello");
        assert_eq!(meta.content_type, "text/plain");
        assert_eq!(meta.size, 5);
    }

    #[tokio::test]
    async fn list_skips_meta_sidecar() {
        let dir = TempDir::new().unwrap();
        let fsb = FsBackend::new(dir.path()).await.unwrap();
        fsb.put("/c/x.txt", Bytes::from_static(b"x"), "text/plain")
            .await
            .unwrap();
        let items = fsb.list("/c").await.unwrap();
        assert_eq!(items, vec!["x.txt".to_string()]);
    }

    #[tokio::test]
    async fn delete_removes_resource_and_sidecar() {
        let dir = TempDir::new().unwrap();
        let fsb = FsBackend::new(dir.path()).await.unwrap();
        fsb.put("/f.txt", Bytes::from_static(b"y"), "text/plain")
            .await
            .unwrap();
        fsb.delete("/f.txt").await.unwrap();
        assert!(!fsb.exists("/f.txt").await.unwrap());
        let sidecar = dir.path().join("f.txt.meta.json");
        assert!(!sidecar.exists());
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let fsb = FsBackend::new(dir.path()).await.unwrap();
        let err = fsb
            .put("/../escape.txt", Bytes::from_static(b""), "text/plain")
            .await
            .err()
            .unwrap();
        assert!(matches!(err, PodError::InvalidPath(_)));
    }
}
