use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tracing::info;

use crate::events::types::{
    DomainEvent, EventError, EventMetadata, EventResult, EventSnapshot, StoredEvent,
};
use crate::utils::time;

#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn append(&self, event: StoredEvent) -> EventResult<()>;

    async fn get_events(&self, aggregate_id: &str) -> EventResult<Vec<StoredEvent>>;

    async fn get_events_after(&self, sequence: i64) -> EventResult<Vec<StoredEvent>>;

    async fn get_events_by_type(&self, event_type: &str) -> EventResult<Vec<StoredEvent>>;

    async fn save_snapshot(&self, snapshot: EventSnapshot) -> EventResult<()>;

    async fn get_snapshot(&self, aggregate_id: &str) -> EventResult<Option<EventSnapshot>>;

    async fn get_event_count(&self, aggregate_id: &str) -> EventResult<usize>;
}

pub struct InMemoryEventRepository {
    events: Arc<RwLock<Vec<StoredEvent>>>,
    snapshots: Arc<RwLock<HashMap<String, EventSnapshot>>>,
}

impl InMemoryEventRepository {
    pub fn new() -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
            snapshots: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn clear(&self) {
        self.events.write().await.clear();
        self.snapshots.write().await.clear();
    }

    pub async fn event_count(&self) -> usize {
        self.events.read().await.len()
    }
}

impl Default for InMemoryEventRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventRepository for InMemoryEventRepository {
    async fn append(&self, mut event: StoredEvent) -> EventResult<()> {
        let mut events = self.events.write().await;
        // Assign sequence number based on current count
        event.sequence = events.len() as i64 + 1;
        events.push(event);
        Ok(())
    }

    async fn get_events(&self, aggregate_id: &str) -> EventResult<Vec<StoredEvent>> {
        let events = self.events.read().await;
        Ok(events
            .iter()
            .filter(|e| e.metadata.aggregate_id == aggregate_id)
            .cloned()
            .collect())
    }

    async fn get_events_after(&self, sequence: i64) -> EventResult<Vec<StoredEvent>> {
        let events = self.events.read().await;
        Ok(events
            .iter()
            .filter(|e| e.sequence > sequence)
            .cloned()
            .collect())
    }

    async fn get_events_by_type(&self, event_type: &str) -> EventResult<Vec<StoredEvent>> {
        let events = self.events.read().await;
        Ok(events
            .iter()
            .filter(|e| e.metadata.event_type == event_type)
            .cloned()
            .collect())
    }

    async fn save_snapshot(&self, snapshot: EventSnapshot) -> EventResult<()> {
        let mut snapshots = self.snapshots.write().await;
        snapshots.insert(snapshot.aggregate_id.clone(), snapshot);
        Ok(())
    }

    async fn get_snapshot(&self, aggregate_id: &str) -> EventResult<Option<EventSnapshot>> {
        let snapshots = self.snapshots.read().await;
        Ok(snapshots.get(aggregate_id).cloned())
    }

    async fn get_event_count(&self, aggregate_id: &str) -> EventResult<usize> {
        let events = self.get_events(aggregate_id).await?;
        Ok(events.len())
    }
}

pub struct FileEventRepository {
    base_dir: PathBuf,
}

impl FileEventRepository {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn events_path(&self, aggregate_type: &str, aggregate_id: &str) -> PathBuf {
        self.base_dir
            .join(aggregate_type)
            .join(format!("{}.jsonl", aggregate_id))
    }

    fn snapshot_path(&self, aggregate_id: &str) -> PathBuf {
        self.base_dir
            .join("snapshots")
            .join(format!("{}.json", aggregate_id))
    }
}

#[async_trait]
impl EventRepository for FileEventRepository {
    async fn append(&self, event: StoredEvent) -> EventResult<()> {
        let path = self.events_path(&event.metadata.aggregate_type, &event.metadata.aggregate_id);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| EventError::Storage(format!("Failed to create directory: {}", e)))?;
        }
        let mut line =
            serde_json::to_string(&event).map_err(|e| EventError::Serialization(e.to_string()))?;
        line.push('\n');
        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .map_err(|e| EventError::Storage(format!("Failed to open file: {}", e)))?
            .write_all(line.as_bytes())
            .await
            .map_err(|e| EventError::Storage(format!("Failed to write event: {}", e)))?;
        Ok(())
    }

    async fn get_events(&self, aggregate_id: &str) -> EventResult<Vec<StoredEvent>> {
        let mut results = Vec::new();
        let read_dir = tokio::fs::read_dir(&self.base_dir).await;
        let mut dirs_to_scan = Vec::new();
        if let Ok(mut entries) = read_dir {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let ft = entry.file_type().await;
                if ft.map(|t| t.is_dir()).unwrap_or(false) {
                    dirs_to_scan.push(entry.path());
                }
            }
        }
        for dir in dirs_to_scan {
            let path = dir.join(format!("{}.jsonl", aggregate_id));
            if let Ok(contents) = tokio::fs::read_to_string(&path).await {
                for line in contents.lines() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let event: StoredEvent = serde_json::from_str(line).map_err(|e| {
                        EventError::Serialization(format!("Failed to parse event: {}", e))
                    })?;
                    results.push(event);
                }
            }
        }
        results.sort_by_key(|e| e.sequence);
        Ok(results)
    }

    async fn get_events_after(&self, sequence: i64) -> EventResult<Vec<StoredEvent>> {
        let mut results = Vec::new();
        let read_dir = tokio::fs::read_dir(&self.base_dir).await;
        let mut dirs_to_scan = Vec::new();
        if let Ok(mut entries) = read_dir {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let ft = entry.file_type().await;
                if ft.map(|t| t.is_dir()).unwrap_or(false) && entry.file_name() != "snapshots" {
                    dirs_to_scan.push(entry.path());
                }
            }
        }
        for dir in dirs_to_scan {
            let mut files = match tokio::fs::read_dir(&dir).await {
                Ok(f) => f,
                Err(_) => continue,
            };
            while let Ok(Some(file_entry)) = files.next_entry().await {
                let path = file_entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                if let Ok(contents) = tokio::fs::read_to_string(&path).await {
                    for line in contents.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        let event: StoredEvent = serde_json::from_str(line).map_err(|e| {
                            EventError::Serialization(format!("Failed to parse event: {}", e))
                        })?;
                        if event.sequence > sequence {
                            results.push(event);
                        }
                    }
                }
            }
        }
        results.sort_by_key(|e| e.sequence);
        Ok(results)
    }

    async fn get_events_by_type(&self, event_type: &str) -> EventResult<Vec<StoredEvent>> {
        let all_events = self.get_events_after(0).await?;
        Ok(all_events
            .into_iter()
            .filter(|e| e.metadata.event_type == event_type)
            .collect())
    }

    async fn save_snapshot(&self, snapshot: EventSnapshot) -> EventResult<()> {
        let path = self.snapshot_path(&snapshot.aggregate_id);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                EventError::Storage(format!("Failed to create snapshots dir: {}", e))
            })?;
        }
        let data = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| EventError::Serialization(e.to_string()))?;
        tokio::fs::write(&path, data)
            .await
            .map_err(|e| EventError::Storage(format!("Failed to write snapshot: {}", e)))?;
        Ok(())
    }

    async fn get_snapshot(&self, aggregate_id: &str) -> EventResult<Option<EventSnapshot>> {
        let path = self.snapshot_path(aggregate_id);
        match tokio::fs::read_to_string(&path).await {
            Ok(contents) => {
                let snapshot: EventSnapshot = serde_json::from_str(&contents).map_err(|e| {
                    EventError::Serialization(format!("Failed to parse snapshot: {}", e))
                })?;
                Ok(Some(snapshot))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(EventError::Storage(format!(
                "Failed to read snapshot: {}",
                e
            ))),
        }
    }

    async fn get_event_count(&self, aggregate_id: &str) -> EventResult<usize> {
        let events = self.get_events(aggregate_id).await?;
        Ok(events.len())
    }
}

pub struct EventStore {
    repo: Arc<dyn EventRepository>,
    snapshot_threshold: usize,
}

impl EventStore {
    pub fn new(repo: Arc<dyn EventRepository>) -> Self {
        Self {
            repo,
            snapshot_threshold: 100,
        }
    }

    pub fn with_file_backend(base_dir: PathBuf) -> Self {
        Self {
            repo: Arc::new(FileEventRepository::new(base_dir)),
            snapshot_threshold: 100,
        }
    }

    pub fn with_snapshot_threshold(mut self, threshold: usize) -> Self {
        self.snapshot_threshold = threshold;
        self
    }

    pub async fn append(&self, event: &dyn DomainEvent) -> EventResult<()> {
        let metadata = EventMetadata::new(
            event.aggregate_id().to_string(),
            event.aggregate_type().to_string(),
            event.event_type().to_string(),
        );

        let data = event
            .to_json_string()
            .map_err(|e| EventError::Serialization(e.to_string()))?;

        let stored_event = StoredEvent {
            metadata,
            data,
            sequence: 0,
        };

        self.repo.append(stored_event).await?;

        let count = self.repo.get_event_count(event.aggregate_id()).await?;
        if count % self.snapshot_threshold == 0 {
            let events = self.repo.get_events(event.aggregate_id()).await?;
            let last_sequence = events.last().map(|e| e.sequence).unwrap_or(0);
            let state_data: Vec<String> = events.iter().map(|e| e.data.clone()).collect();
            let snapshot = EventSnapshot {
                aggregate_id: event.aggregate_id().to_string(),
                aggregate_type: event.aggregate_type().to_string(),
                sequence: last_sequence,
                timestamp: time::now(),
                state: serde_json::to_string(&state_data).unwrap_or_else(|_| "[]".to_string()),
            };
            self.repo.save_snapshot(snapshot).await?;
            info!(
                aggregate_id = event.aggregate_id(),
                version = last_sequence,
                "Created event snapshot for aggregate {} at version {}",
                event.aggregate_id(),
                last_sequence
            );
        }

        Ok(())
    }

    pub async fn get_events(&self, aggregate_id: &str) -> EventResult<Vec<StoredEvent>> {
        self.repo.get_events(aggregate_id).await
    }

    pub async fn get_events_after(&self, sequence: i64) -> EventResult<Vec<StoredEvent>> {
        self.repo.get_events_after(sequence).await
    }

    pub async fn get_events_by_type(&self, event_type: &str) -> EventResult<Vec<StoredEvent>> {
        self.repo.get_events_by_type(event_type).await
    }

    pub async fn replay_events(&self, aggregate_id: &str) -> EventResult<Vec<StoredEvent>> {
        if let Some(snapshot) = self.repo.get_snapshot(aggregate_id).await? {
            let events = self.get_events(aggregate_id).await?;
            Ok(events
                .into_iter()
                .filter(|e| e.sequence > snapshot.sequence)
                .collect())
        } else {
            self.get_events(aggregate_id).await
        }
    }

    pub async fn get_event_count(&self, aggregate_id: &str) -> EventResult<usize> {
        self.repo.get_event_count(aggregate_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::domain_events::NodeAddedEvent;
    use crate::utils::time;

    use std::collections::HashMap;

    #[tokio::test]
    async fn test_in_memory_repository() {
        let repo = InMemoryEventRepository::new();

        let event = StoredEvent {
            metadata: EventMetadata::new(
                "node-1".to_string(),
                "Node".to_string(),
                "NodeAdded".to_string(),
            ),
            data: "{}".to_string(),
            sequence: 1,
        };

        repo.append(event.clone()).await.unwrap();

        let events = repo.get_events("node-1").await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].metadata.aggregate_id, "node-1");
    }

    #[tokio::test]
    async fn test_event_store() {
        let repo = Arc::new(InMemoryEventRepository::new());
        let store = EventStore::new(repo);

        let event = NodeAddedEvent {
            node_id: "node-1".to_string(),
            label: "Test".to_string(),
            node_type: "Person".to_string(),
            properties: HashMap::new(),
            timestamp: time::now(),
        };

        store.append(&event).await.unwrap();

        let events = store.get_events("node-1").await.unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn test_get_events_after() {
        let repo = Arc::new(InMemoryEventRepository::new());
        let store = EventStore::new(repo.clone());

        for i in 0..5 {
            let event = NodeAddedEvent {
                node_id: format!("node-{}", i),
                label: "Test".to_string(),
                node_type: "Person".to_string(),
                properties: HashMap::new(),
                timestamp: time::now(),
            };
            store.append(&event).await.unwrap();
        }

        let events = store.get_events_after(2).await.unwrap();
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn test_get_events_by_type() {
        let repo = Arc::new(InMemoryEventRepository::new());
        let store = EventStore::new(repo);

        let event = NodeAddedEvent {
            node_id: "node-1".to_string(),
            label: "Test".to_string(),
            node_type: "Person".to_string(),
            properties: HashMap::new(),
            timestamp: time::now(),
        };

        store.append(&event).await.unwrap();

        let events = store.get_events_by_type("NodeAdded").await.unwrap();
        assert_eq!(events.len(), 1);
    }
}
