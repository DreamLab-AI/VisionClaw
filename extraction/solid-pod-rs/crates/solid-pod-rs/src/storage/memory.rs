//! In-memory storage backend.
//!
//! Designed for tests. The state lives in a single
//! `Arc<RwLock<HashMap<String, (Bytes, ResourceMeta)>>>`. Change
//! events are broadcast to all registered watchers; a watcher only
//! receives events for paths that are equal to, or descend from, the
//! path it was registered with.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::error::PodError;
use crate::storage::{ResourceMeta, Storage, StorageEvent};

/// In-memory `Storage` implementation.
#[derive(Clone)]
pub struct MemoryBackend {
    inner: Arc<Inner>,
}

struct Inner {
    data: RwLock<HashMap<String, Entry>>,
    events: broadcast::Sender<StorageEvent>,
}

#[derive(Clone)]
struct Entry {
    body: Bytes,
    meta: ResourceMeta,
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBackend {
    /// Create a new empty backend.
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Inner {
                data: RwLock::new(HashMap::new()),
                events,
            }),
        }
    }

    fn compute_etag(body: &[u8]) -> String {
        hex::encode(Sha256::digest(body))
    }

    fn normalize(path: &str) -> String {
        if path.is_empty() {
            "/".into()
        } else if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        }
    }

    fn is_under(child: &str, container: &str) -> bool {
        if container == "/" {
            return child != "/";
        }
        let c = container.trim_end_matches('/');
        child == c || child.starts_with(&format!("{c}/"))
    }
}

#[async_trait]
impl Storage for MemoryBackend {
    async fn get(&self, path: &str) -> Result<(Bytes, ResourceMeta), PodError> {
        let path = Self::normalize(path);
        let guard = self.inner.data.read().await;
        guard
            .get(&path)
            .map(|e| (e.body.clone(), e.meta.clone()))
            .ok_or(PodError::NotFound(path))
    }

    async fn put(
        &self,
        path: &str,
        body: Bytes,
        content_type: &str,
    ) -> Result<ResourceMeta, PodError> {
        let path = Self::normalize(path);
        let etag = Self::compute_etag(&body);
        let meta = ResourceMeta {
            etag,
            modified: chrono::Utc::now(),
            size: body.len() as u64,
            content_type: content_type.to_string(),
            links: Vec::new(),
        };
        let mut guard = self.inner.data.write().await;
        let existed = guard.contains_key(&path);
        guard.insert(
            path.clone(),
            Entry {
                body,
                meta: meta.clone(),
            },
        );
        drop(guard);
        let event = if existed {
            StorageEvent::Updated(path)
        } else {
            StorageEvent::Created(path)
        };
        let _ = self.inner.events.send(event);
        Ok(meta)
    }

    async fn delete(&self, path: &str) -> Result<(), PodError> {
        let path = Self::normalize(path);
        let mut guard = self.inner.data.write().await;
        match guard.remove(&path) {
            Some(_) => {
                drop(guard);
                let _ = self.inner.events.send(StorageEvent::Deleted(path));
                Ok(())
            }
            None => Err(PodError::NotFound(path)),
        }
    }

    async fn list(&self, container: &str) -> Result<Vec<String>, PodError> {
        let container = Self::normalize(container);
        let container = if container.ends_with('/') {
            container
        } else {
            format!("{container}/")
        };
        let guard = self.inner.data.read().await;
        let mut seen = std::collections::BTreeSet::new();
        for key in guard.keys() {
            if !key.starts_with(&container) {
                continue;
            }
            let remainder = &key[container.len()..];
            if remainder.is_empty() {
                continue;
            }
            let name = match remainder.find('/') {
                Some(pos) => &remainder[..=pos],
                None => remainder,
            };
            seen.insert(name.to_string());
        }
        Ok(seen.into_iter().collect())
    }

    async fn head(&self, path: &str) -> Result<ResourceMeta, PodError> {
        let path = Self::normalize(path);
        let guard = self.inner.data.read().await;
        guard
            .get(&path)
            .map(|e| e.meta.clone())
            .ok_or(PodError::NotFound(path))
    }

    async fn exists(&self, path: &str) -> Result<bool, PodError> {
        let path = Self::normalize(path);
        let guard = self.inner.data.read().await;
        Ok(guard.contains_key(&path))
    }

    async fn watch(&self, path: &str) -> Result<mpsc::Receiver<StorageEvent>, PodError> {
        let filter_path = Self::normalize(path);
        let mut rx = self.inner.events.subscribe();
        let (tx, out_rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                let target = match &event {
                    StorageEvent::Created(p)
                    | StorageEvent::Updated(p)
                    | StorageEvent::Deleted(p) => p.clone(),
                };
                if MemoryBackend::is_under(&target, &filter_path)
                    && tx.send(event).await.is_err()
                {
                    return;
                }
            }
        });
        Ok(out_rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_get_roundtrip() {
        let m = MemoryBackend::new();
        m.put("/foo", Bytes::from_static(b"hello"), "text/plain")
            .await
            .unwrap();
        let (body, meta) = m.get("/foo").await.unwrap();
        assert_eq!(&body[..], b"hello");
        assert_eq!(meta.size, 5);
        assert_eq!(meta.content_type, "text/plain");
    }

    #[tokio::test]
    async fn list_direct_children_only() {
        let m = MemoryBackend::new();
        m.put("/a/b", Bytes::from_static(b""), "text/plain")
            .await
            .unwrap();
        m.put("/a/c/d", Bytes::from_static(b""), "text/plain")
            .await
            .unwrap();
        let mut items = m.list("/a").await.unwrap();
        items.sort();
        assert_eq!(items, vec!["b".to_string(), "c/".to_string()]);
    }

    #[tokio::test]
    async fn watch_receives_created_event() {
        let m = MemoryBackend::new();
        let mut rx = m.watch("/").await.unwrap();
        m.put("/x", Bytes::from_static(b"hi"), "text/plain")
            .await
            .unwrap();
        let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event, StorageEvent::Created("/x".into()));
    }
}
