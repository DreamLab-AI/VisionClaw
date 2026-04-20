//! Storage trait conformance suite.
//!
//! Every backend shipped with the crate must pass this suite.

use std::sync::Arc;

use bytes::Bytes;
use solid_pod_rs::storage::{fs::FsBackend, memory::MemoryBackend, Storage, StorageEvent};
use solid_pod_rs::PodError;
use tempfile::TempDir;

async fn make_memory() -> Arc<dyn Storage> {
    Arc::new(MemoryBackend::new())
}

async fn make_fs() -> (Arc<dyn Storage>, TempDir) {
    let dir = TempDir::new().unwrap();
    let fsb = FsBackend::new(dir.path()).await.unwrap();
    (Arc::new(fsb), dir)
}

async fn run_put_get_roundtrip(s: Arc<dyn Storage>) {
    let meta = s
        .put("/a.txt", Bytes::from_static(b"hello"), "text/plain")
        .await
        .unwrap();
    assert_eq!(meta.size, 5);
    assert_eq!(meta.content_type, "text/plain");
    let (body, meta2) = s.get("/a.txt").await.unwrap();
    assert_eq!(&body[..], b"hello");
    assert_eq!(meta2.etag, meta.etag);
}

async fn run_head_without_body(s: Arc<dyn Storage>) {
    s.put("/h.txt", Bytes::from_static(b"12345"), "text/plain")
        .await
        .unwrap();
    let m = s.head("/h.txt").await.unwrap();
    assert_eq!(m.size, 5);
    assert!(!m.etag.is_empty());
}

async fn run_delete_then_get_404(s: Arc<dyn Storage>) {
    s.put("/d.txt", Bytes::from_static(b"x"), "text/plain")
        .await
        .unwrap();
    s.delete("/d.txt").await.unwrap();
    let err = s.get("/d.txt").await.err().unwrap();
    assert!(matches!(err, PodError::NotFound(_)));
}

async fn run_list_direct_children(s: Arc<dyn Storage>) {
    s.put("/box/a", Bytes::from_static(b""), "text/plain")
        .await
        .unwrap();
    s.put("/box/b", Bytes::from_static(b""), "text/plain")
        .await
        .unwrap();
    s.put("/box/sub/c", Bytes::from_static(b""), "text/plain")
        .await
        .unwrap();
    let mut list = s.list("/box").await.unwrap();
    list.sort();
    assert!(list.contains(&"a".to_string()));
    assert!(list.contains(&"b".to_string()));
    assert!(list.contains(&"sub/".to_string()));
}

async fn run_exists(s: Arc<dyn Storage>) {
    assert!(!s.exists("/e").await.unwrap());
    s.put("/e", Bytes::from_static(b""), "text/plain")
        .await
        .unwrap();
    assert!(s.exists("/e").await.unwrap());
}

async fn run_etag_changes_on_update(s: Arc<dyn Storage>) {
    let m1 = s
        .put("/u", Bytes::from_static(b"one"), "text/plain")
        .await
        .unwrap();
    let m2 = s
        .put("/u", Bytes::from_static(b"two"), "text/plain")
        .await
        .unwrap();
    assert_ne!(m1.etag, m2.etag);
}

async fn run_concurrent_writes(s: Arc<dyn Storage>) {
    let mut handles = Vec::with_capacity(100);
    for i in 0..100u32 {
        let s2 = s.clone();
        handles.push(tokio::spawn(async move {
            let path = format!("/c/{i}");
            let body = Bytes::from(format!("value-{i}"));
            s2.put(&path, body, "text/plain").await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    let list = s.list("/c").await.unwrap();
    assert_eq!(list.len(), 100);
    for i in [0u32, 7, 42, 99] {
        let (body, _) = s.get(&format!("/c/{i}")).await.unwrap();
        assert_eq!(&body[..], format!("value-{i}").as_bytes());
    }
}

async fn run_watch_receives_event(s: Arc<dyn Storage>) {
    let mut rx = s.watch("/").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    s.put("/w.txt", Bytes::from_static(b"hi"), "text/plain")
        .await
        .unwrap();
    let received = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("watcher timed out")
        .expect("watcher channel closed");
    match received {
        StorageEvent::Created(p) | StorageEvent::Updated(p) => {
            assert!(p.ends_with("w.txt"), "unexpected path: {p}");
        }
        StorageEvent::Deleted(_) => panic!("unexpected delete event"),
    }
}

// --- Memory backend ---------------------------------------------------------

#[tokio::test]
async fn memory_put_get_roundtrip() {
    run_put_get_roundtrip(make_memory().await).await;
}

#[tokio::test]
async fn memory_head_without_body() {
    run_head_without_body(make_memory().await).await;
}

#[tokio::test]
async fn memory_delete_then_get_404() {
    run_delete_then_get_404(make_memory().await).await;
}

#[tokio::test]
async fn memory_list_direct_children() {
    run_list_direct_children(make_memory().await).await;
}

#[tokio::test]
async fn memory_exists() {
    run_exists(make_memory().await).await;
}

#[tokio::test]
async fn memory_etag_changes_on_update() {
    run_etag_changes_on_update(make_memory().await).await;
}

#[tokio::test]
async fn memory_concurrent_writes() {
    run_concurrent_writes(make_memory().await).await;
}

#[tokio::test]
async fn memory_watch_receives_event() {
    run_watch_receives_event(make_memory().await).await;
}

// --- FS backend -------------------------------------------------------------

#[tokio::test]
async fn fs_put_get_roundtrip() {
    let (s, _dir) = make_fs().await;
    run_put_get_roundtrip(s).await;
}

#[tokio::test]
async fn fs_head_without_body() {
    let (s, _dir) = make_fs().await;
    run_head_without_body(s).await;
}

#[tokio::test]
async fn fs_delete_then_get_404() {
    let (s, _dir) = make_fs().await;
    run_delete_then_get_404(s).await;
}

#[tokio::test]
async fn fs_list_direct_children() {
    let (s, _dir) = make_fs().await;
    run_list_direct_children(s).await;
}

#[tokio::test]
async fn fs_exists() {
    let (s, _dir) = make_fs().await;
    run_exists(s).await;
}

#[tokio::test]
async fn fs_etag_changes_on_update() {
    let (s, _dir) = make_fs().await;
    run_etag_changes_on_update(s).await;
}

#[tokio::test]
async fn fs_concurrent_writes() {
    let (s, _dir) = make_fs().await;
    run_concurrent_writes(s).await;
}
