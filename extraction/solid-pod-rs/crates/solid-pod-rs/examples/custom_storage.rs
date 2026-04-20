//! Implement `Storage` for a custom backend (a `BTreeMap`).
//!
//! The crate ships `MemoryBackend` (`HashMap`-backed) and `FsBackend`;
//! but any `Send + Sync + 'static` type can back a pod if it
//! implements the trait. This example shows the minimal set of
//! methods you need.
//!
//! Run with:
//! ```bash
//! cargo run --example custom_storage -p solid-pod-rs
//! ```
//!
//! Expected output:
//! ```text
//! PUT  /notes/a.txt  -> etag=...
//! PUT  /notes/b.txt  -> etag=...
//! LIST /notes        -> ["a.txt", "b.txt"]
//! GET  /notes/a.txt  -> "alpha"
//! DEL  /notes/a.txt
//! LIST /notes        -> ["b.txt"]
//! watcher received: Created("/notes/c.txt")
//! ```

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use solid_pod_rs::{
    storage::{ResourceMeta, Storage, StorageEvent},
    PodError,
};
use tokio::sync::{broadcast, mpsc, RwLock};

/// A trivial BTreeMap-backed backend. Ordered iteration makes
/// `list()` deterministic without an explicit sort.
#[derive(Clone)]
struct BTreeBackend {
    inner: Arc<BTreeInner>,
}

struct BTreeInner {
    data: RwLock<BTreeMap<String, (Bytes, ResourceMeta)>>,
    events: broadcast::Sender<StorageEvent>,
}

impl BTreeBackend {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            inner: Arc::new(BTreeInner {
                data: RwLock::new(BTreeMap::new()),
                events: tx,
            }),
        }
    }

    fn normalize(path: &str) -> String {
        if path.is_empty() {
            "/".into()
        } else if path.starts_with('/') {
            path.into()
        } else {
            format!("/{path}")
        }
    }

    fn etag(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }
}

#[async_trait]
impl Storage for BTreeBackend {
    async fn get(&self, path: &str) -> Result<(Bytes, ResourceMeta), PodError> {
        let path = Self::normalize(path);
        let guard = self.inner.data.read().await;
        guard
            .get(&path)
            .map(|(b, m)| (b.clone(), m.clone()))
            .ok_or(PodError::NotFound(path))
    }

    async fn put(
        &self,
        path: &str,
        body: Bytes,
        content_type: &str,
    ) -> Result<ResourceMeta, PodError> {
        let path = Self::normalize(path);
        let meta = ResourceMeta {
            etag: Self::etag(&body),
            modified: chrono::Utc::now(),
            size: body.len() as u64,
            content_type: content_type.to_string(),
            links: Vec::new(),
        };
        let mut guard = self.inner.data.write().await;
        let existed = guard.contains_key(&path);
        guard.insert(path.clone(), (body, meta.clone()));
        drop(guard);
        let _ = self.inner.events.send(if existed {
            StorageEvent::Updated(path)
        } else {
            StorageEvent::Created(path)
        });
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
        let prefix = if container.ends_with('/') {
            container
        } else {
            format!("{container}/")
        };
        let guard = self.inner.data.read().await;
        // BTreeMap gives us sorted iteration for free.
        let mut seen = std::collections::BTreeSet::new();
        for key in guard.keys() {
            if !key.starts_with(&prefix) {
                continue;
            }
            let remainder = &key[prefix.len()..];
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
            .map(|(_, m)| m.clone())
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
        let (tx, out_rx) = mpsc::channel(32);
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                let target = match &event {
                    StorageEvent::Created(p)
                    | StorageEvent::Updated(p)
                    | StorageEvent::Deleted(p) => p.clone(),
                };
                let under = filter_path == "/"
                    || target == filter_path
                    || target.starts_with(&format!(
                        "{}/",
                        filter_path.trim_end_matches('/')
                    ));
                if under && tx.send(event).await.is_err() {
                    return;
                }
            }
        });
        Ok(out_rx)
    }
}

#[tokio::main]
async fn main() -> Result<(), PodError> {
    let backend: Arc<dyn Storage> = Arc::new(BTreeBackend::new());

    // Start a watcher before any writes so we can observe `Created`.
    let mut watcher = backend.watch("/notes").await?;

    let m1 = backend
        .put("/notes/a.txt", Bytes::from_static(b"alpha"), "text/plain")
        .await?;
    println!("PUT  /notes/a.txt  -> etag={}", m1.etag);

    let m2 = backend
        .put("/notes/b.txt", Bytes::from_static(b"bravo"), "text/plain")
        .await?;
    println!("PUT  /notes/b.txt  -> etag={}", m2.etag);

    let listing = backend.list("/notes").await?;
    println!("LIST /notes        -> {listing:?}");

    let (body, _) = backend.get("/notes/a.txt").await?;
    println!("GET  /notes/a.txt  -> {:?}", std::str::from_utf8(&body).unwrap_or(""));

    backend.delete("/notes/a.txt").await?;
    println!("DEL  /notes/a.txt");

    let listing = backend.list("/notes").await?;
    println!("LIST /notes        -> {listing:?}");

    // Drain any events produced so far, then emit a fresh Create for
    // the output line.
    while let Ok(Some(_)) =
        tokio::time::timeout(std::time::Duration::from_millis(10), watcher.recv()).await
    {}
    backend
        .put("/notes/c.txt", Bytes::from_static(b"charlie"), "text/plain")
        .await?;
    if let Some(ev) =
        tokio::time::timeout(std::time::Duration::from_secs(1), watcher.recv())
            .await
            .ok()
            .flatten()
    {
        println!("watcher received: {ev:?}");
    }

    Ok(())
}
