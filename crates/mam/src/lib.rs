use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use waddle_core::event::{ChatMessage, Event, EventPayload, ScrollDirection};
use waddle_storage::{Database, FromRow, Row, SqlValue, StorageError};

#[cfg(feature = "native")]
use waddle_core::event::{Channel, EventBus, EventSource};

const MAM_PAGE_SIZE: u32 = 50;

#[derive(Debug, thiserror::Error)]
pub enum MamError {
    #[error("MAM not supported by server")]
    NotSupported,

    #[error("MAM query failed: {0}")]
    QueryFailed(String),

    #[error("MAM query timed out after {0}s")]
    Timeout(u64),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("event bus error: {0}")]
    EventBus(String),
}

#[derive(Debug, Clone)]
pub struct MamSyncResult {
    pub messages_synced: u64,
    pub complete: bool,
}

struct SyncState {
    last_stanza_id: String,
    #[allow(dead_code)]
    last_sync_at: String,
}

impl FromRow for SyncState {
    fn from_row(row: &Row) -> Result<Self, StorageError> {
        let last_stanza_id = match row.get(0) {
            Some(SqlValue::Text(s)) => s.clone(),
            _ => {
                return Err(StorageError::QueryFailed(
                    "missing last_stanza_id column".to_string(),
                ));
            }
        };
        let last_sync_at = match row.get(1) {
            Some(SqlValue::Text(s)) => s.clone(),
            _ => {
                return Err(StorageError::QueryFailed(
                    "missing last_sync_at column".to_string(),
                ));
            }
        };
        Ok(SyncState {
            last_stanza_id,
            last_sync_at,
        })
    }
}

fn message_type_to_str(mt: &waddle_core::event::MessageType) -> &'static str {
    match mt {
        waddle_core::event::MessageType::Chat => "chat",
        waddle_core::event::MessageType::Groupchat => "groupchat",
        waddle_core::event::MessageType::Normal => "normal",
        waddle_core::event::MessageType::Headline => "headline",
        waddle_core::event::MessageType::Error => "error",
    }
}

pub struct MamManager<D: Database> {
    db: Arc<D>,
    #[cfg(feature = "native")]
    event_bus: Arc<dyn EventBus>,
}

impl<D: Database> MamManager<D> {
    #[cfg(feature = "native")]
    pub fn new(db: Arc<D>, event_bus: Arc<dyn EventBus>) -> Self {
        Self { db, event_bus }
    }

    pub async fn sync_since(&self, _timestamp: DateTime<Utc>) -> Result<MamSyncResult, MamError> {
        let last_stanza_id = self.get_last_stanza_id("").await?;

        let query_id = Uuid::new_v4().to_string();
        let correlation_id = Uuid::new_v4();

        #[cfg(feature = "native")]
        {
            let _ = self.event_bus.publish(Event::with_correlation(
                Channel::new("system.sync.started").unwrap(),
                EventSource::System("mam".into()),
                EventPayload::SyncStarted,
                correlation_id,
            ));
        }

        let mut total_synced: u64 = 0;
        let mut complete = false;
        let mut after = last_stanza_id;

        while !complete {
            self.send_mam_query(&query_id, after.as_deref(), None)
                .await?;

            let (messages, fin_complete, last_id) = self.collect_query_results(&query_id).await?;

            let page_count = messages.len() as u64;

            for msg in &messages {
                self.persist_message(msg).await?;
            }

            total_synced += page_count;

            if let Some(ref id) = last_id {
                self.update_sync_state("", id).await?;
                after = Some(id.clone());
            }

            complete = fin_complete || page_count == 0;
        }

        #[cfg(feature = "native")]
        {
            let _ = self.event_bus.publish(Event::with_correlation(
                Channel::new("system.sync.completed").unwrap(),
                EventSource::System("mam".into()),
                EventPayload::SyncCompleted {
                    messages_synced: total_synced,
                },
                correlation_id,
            ));
        }

        Ok(MamSyncResult {
            messages_synced: total_synced,
            complete: true,
        })
    }

    pub async fn fetch_history(
        &self,
        _jid: &str,
        before: Option<&str>,
        _limit: u32,
    ) -> Result<Vec<ChatMessage>, MamError> {
        let query_id = Uuid::new_v4().to_string();

        self.send_mam_query(&query_id, None, before).await?;

        let (messages, _complete, _last_id) = self.collect_query_results(&query_id).await?;

        for msg in &messages {
            self.persist_message(msg).await?;
        }

        Ok(messages)
    }

    pub async fn is_supported(&self) -> bool {
        // TODO: implement disco#info check for urn:xmpp:mam:2
        true
    }

    async fn get_last_stanza_id(&self, jid: &str) -> Result<Option<String>, MamError> {
        let jid_s = if jid.is_empty() {
            "__global__".to_string()
        } else {
            jid.to_string()
        };

        let rows: Vec<SyncState> = self
            .db
            .query(
                "SELECT last_stanza_id, last_sync_at FROM mam_sync_state WHERE jid = ?1",
                &[&jid_s],
            )
            .await?;

        Ok(rows.into_iter().next().map(|s| s.last_stanza_id))
    }

    async fn update_sync_state(&self, jid: &str, stanza_id: &str) -> Result<(), MamError> {
        let jid_s = if jid.is_empty() {
            "__global__".to_string()
        } else {
            jid.to_string()
        };
        let stanza_id_s = stanza_id.to_string();
        let now = Utc::now().to_rfc3339();

        self.db
            .execute(
                "INSERT OR REPLACE INTO mam_sync_state (jid, last_stanza_id, last_sync_at) \
                 VALUES (?1, ?2, ?3)",
                &[&jid_s, &stanza_id_s, &now],
            )
            .await?;

        Ok(())
    }

    async fn persist_message(&self, message: &ChatMessage) -> Result<(), MamError> {
        let id = message.id.clone();
        let from = message.from.clone();
        let to = message.to.clone();
        let body = message.body.clone();
        let ts = message.timestamp.to_rfc3339();
        let mt = message_type_to_str(&message.message_type).to_string();
        let thread = message.thread.clone();
        let read = 0_i64;

        self.db
            .execute(
                "INSERT OR IGNORE INTO messages (id, from_jid, to_jid, body, timestamp, message_type, thread, read) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                &[&id, &from, &to, &body, &ts, &mt, &thread, &read],
            )
            .await?;

        Ok(())
    }

    #[cfg(feature = "native")]
    async fn send_mam_query(
        &self,
        query_id: &str,
        after: Option<&str>,
        before: Option<&str>,
    ) -> Result<(), MamError> {
        let _ = self.event_bus.publish(Event::new(
            Channel::new("ui.mam.query").unwrap(),
            EventSource::System("mam".into()),
            EventPayload::MamQueryRequested {
                query_id: query_id.to_string(),
                after: after.map(String::from),
                before: before.map(String::from),
                max: MAM_PAGE_SIZE,
            },
        ));

        Ok(())
    }

    #[cfg(feature = "native")]
    async fn collect_query_results(
        &self,
        _query_id: &str,
    ) -> Result<(Vec<ChatMessage>, bool, Option<String>), MamError> {
        let mut sub = self
            .event_bus
            .subscribe("xmpp.mam.**")
            .map_err(|e| MamError::EventBus(e.to_string()))?;

        let mut messages = Vec::new();
        let mut last_id = None;

        let timeout_duration = tokio::time::Duration::from_secs(30);

        loop {
            match tokio::time::timeout(timeout_duration, sub.recv()).await {
                Ok(Ok(event)) => match &event.payload {
                    EventPayload::MamResultReceived {
                        messages: page_msgs,
                        ..
                    } => {
                        for msg in page_msgs {
                            last_id = Some(msg.id.clone());
                            messages.push(msg.clone());
                        }
                    }
                    EventPayload::MamFinReceived {
                        complete,
                        last_id: fin_last,
                        ..
                    } => {
                        if let Some(id) = fin_last {
                            last_id = Some(id.clone());
                        }
                        return Ok((messages, *complete, last_id));
                    }
                    _ => {}
                },
                Ok(Err(waddle_core::error::EventBusError::Lagged(count))) => {
                    warn!(count, "MAM result collector lagged");
                }
                Ok(Err(e)) => {
                    return Err(MamError::QueryFailed(format!("event bus error: {e}")));
                }
                Err(_) => {
                    return Err(MamError::Timeout(30));
                }
            }
        }
    }

    #[cfg(feature = "native")]
    pub async fn handle_event(&self, event: &Event) {
        match &event.payload {
            EventPayload::ConnectionEstablished { jid } => {
                info!(jid = %jid, "connection established, starting MAM catch-up sync");
                match self.sync_since(Utc::now()).await {
                    Ok(result) => {
                        info!(
                            messages_synced = result.messages_synced,
                            "MAM catch-up sync complete"
                        );
                    }
                    Err(MamError::Timeout(_)) => {
                        warn!("MAM catch-up sync timed out");
                    }
                    Err(e) => {
                        error!(error = %e, "MAM catch-up sync failed");
                    }
                }
            }
            EventPayload::ScrollRequested {
                jid,
                direction: ScrollDirection::Up,
            } => {
                debug!(jid = %jid, "scroll up requested, fetching MAM history");
                match self.fetch_history(jid, None, MAM_PAGE_SIZE).await {
                    Ok(messages) => {
                        debug!(count = messages.len(), jid = %jid, "fetched MAM history");
                    }
                    Err(e) => {
                        error!(error = %e, jid = %jid, "MAM history fetch failed");
                    }
                }
            }
            _ => {}
        }
    }

    #[cfg(feature = "native")]
    pub async fn run(self: Arc<Self>) -> Result<(), MamError> {
        let mut sub = self
            .event_bus
            .subscribe("{system,ui}.**")
            .map_err(|e| MamError::EventBus(e.to_string()))?;

        loop {
            match sub.recv().await {
                Ok(event) => {
                    self.handle_event(&event).await;
                }
                Err(waddle_core::error::EventBusError::ChannelClosed) => {
                    debug!("event bus closed, MAM manager stopping");
                    return Ok(());
                }
                Err(waddle_core::error::EventBusError::Lagged(count)) => {
                    warn!(count, "MAM manager lagged, some events dropped");
                }
                Err(e) => {
                    error!(error = %e, "MAM manager subscription error");
                    return Err(MamError::EventBus(e.to_string()));
                }
            }
        }
    }
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;
    use waddle_core::event::{BroadcastEventBus, EventBus, MessageType};

    async fn setup() -> (Arc<MamManager<impl Database>>, Arc<dyn EventBus>, TempDir) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let db = waddle_storage::open_database(&db_path)
            .await
            .expect("failed to open database");
        let db = Arc::new(db);
        let event_bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());
        let manager = Arc::new(MamManager::new(db, event_bus.clone()));
        (manager, event_bus, dir)
    }

    fn make_chat_message(id: &str, from: &str, to: &str, body: &str) -> ChatMessage {
        ChatMessage {
            id: id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            body: body.to_string(),
            timestamp: Utc::now(),
            message_type: MessageType::Chat,
            thread: None,
        }
    }

    #[tokio::test]
    async fn persist_message_deduplicates() {
        let (manager, _, _dir) = setup().await;

        let msg = make_chat_message("mam-1", "alice@example.com", "bob@example.com", "Hello");

        manager.persist_message(&msg).await.unwrap();
        manager.persist_message(&msg).await.unwrap();

        let rows: Vec<Row> = manager
            .db
            .query(
                "SELECT id FROM messages WHERE id = ?1",
                &[&"mam-1".to_string()],
            )
            .await
            .unwrap();

        assert_eq!(rows.len(), 1);
    }

    #[tokio::test]
    async fn sync_state_round_trip() {
        let (manager, _, _dir) = setup().await;

        assert!(manager.get_last_stanza_id("").await.unwrap().is_none());

        manager
            .update_sync_state("", "archive-id-42")
            .await
            .unwrap();

        let last = manager.get_last_stanza_id("").await.unwrap();
        assert_eq!(last, Some("archive-id-42".to_string()));
    }

    #[tokio::test]
    async fn sync_state_update_replaces() {
        let (manager, _, _dir) = setup().await;

        manager.update_sync_state("", "archive-id-1").await.unwrap();
        manager.update_sync_state("", "archive-id-2").await.unwrap();

        let last = manager.get_last_stanza_id("").await.unwrap();
        assert_eq!(last, Some("archive-id-2".to_string()));
    }

    #[tokio::test]
    async fn sync_state_per_jid() {
        let (manager, _, _dir) = setup().await;

        manager
            .update_sync_state("alice@example.com", "a-1")
            .await
            .unwrap();
        manager
            .update_sync_state("bob@example.com", "b-1")
            .await
            .unwrap();

        let alice = manager
            .get_last_stanza_id("alice@example.com")
            .await
            .unwrap();
        let bob = manager.get_last_stanza_id("bob@example.com").await.unwrap();

        assert_eq!(alice, Some("a-1".to_string()));
        assert_eq!(bob, Some("b-1".to_string()));
    }

    #[tokio::test]
    async fn sync_since_emits_events_and_persists() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (manager, event_bus, _dir) = setup().await;

                let mut sys_sub = event_bus.subscribe("system.**").unwrap();
                let mut ui_sub = event_bus.subscribe("ui.**").unwrap();

                let manager_clone = manager.clone();
                let sync_handle =
                    tokio::task::spawn_local(
                        async move { manager_clone.sync_since(Utc::now()).await },
                    );

                tokio::task::yield_now().await;
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;

                // Wait for MAM query request
                let query_event =
                    tokio::time::timeout(std::time::Duration::from_millis(500), ui_sub.recv())
                        .await
                        .expect("timed out waiting for MAM query")
                        .expect("should receive query event");

                let query_id = match &query_event.payload {
                    EventPayload::MamQueryRequested { query_id, .. } => query_id.clone(),
                    _ => panic!(
                        "expected MamQueryRequested event, got {:?}",
                        query_event.payload
                    ),
                };

                // Simulate MAM result messages from MamProcessor
                let msg1 =
                    make_chat_message("arch-1", "alice@example.com", "bob@example.com", "Hi");
                event_bus
                    .publish(Event::new(
                        Channel::new("xmpp.mam.result.received").unwrap(),
                        EventSource::Xmpp,
                        EventPayload::MamResultReceived {
                            query_id: query_id.clone(),
                            messages: vec![msg1],
                            complete: false,
                        },
                    ))
                    .unwrap();

                let msg2 =
                    make_chat_message("arch-2", "bob@example.com", "alice@example.com", "Hey");
                event_bus
                    .publish(Event::new(
                        Channel::new("xmpp.mam.result.received").unwrap(),
                        EventSource::Xmpp,
                        EventPayload::MamResultReceived {
                            query_id: query_id.clone(),
                            messages: vec![msg2],
                            complete: false,
                        },
                    ))
                    .unwrap();

                // Simulate MAM fin
                event_bus
                    .publish(Event::new(
                        Channel::new("xmpp.mam.fin.received").unwrap(),
                        EventSource::Xmpp,
                        EventPayload::MamFinReceived {
                            iq_id: "iq-1".to_string(),
                            complete: true,
                            last_id: Some("arch-2".to_string()),
                        },
                    ))
                    .unwrap();

                let result = tokio::time::timeout(std::time::Duration::from_secs(5), sync_handle)
                    .await
                    .expect("sync timed out")
                    .expect("sync task should not panic")
                    .expect("sync should succeed");

                assert_eq!(result.messages_synced, 2);
                assert!(result.complete);

                // Verify messages persisted
                let rows: Vec<Row> = manager
                    .db
                    .query("SELECT COUNT(*) FROM messages", &[])
                    .await
                    .unwrap();
                assert_eq!(rows[0].get(0), Some(&SqlValue::Integer(2)));

                // Verify sync state updated
                let last = manager.get_last_stanza_id("").await.unwrap();
                assert_eq!(last, Some("arch-2".to_string()));

                // Verify SyncStarted event
                let started =
                    tokio::time::timeout(std::time::Duration::from_millis(100), sys_sub.recv())
                        .await
                        .expect("timed out waiting for SyncStarted")
                        .expect("should receive SyncStarted");
                assert!(matches!(started.payload, EventPayload::SyncStarted));
                let corr_id = started.correlation_id.expect("should have correlation ID");

                // Verify SyncCompleted event with matching correlation ID
                let completed =
                    tokio::time::timeout(std::time::Duration::from_millis(100), sys_sub.recv())
                        .await
                        .expect("timed out waiting for SyncCompleted")
                        .expect("should receive SyncCompleted");
                assert!(matches!(
                    completed.payload,
                    EventPayload::SyncCompleted { messages_synced: 2 }
                ));
                assert_eq!(completed.correlation_id, Some(corr_id));
            })
            .await;
    }

    #[tokio::test]
    async fn handle_connection_established_triggers_sync() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (manager, event_bus, _dir) = setup().await;

                let mut ui_sub = event_bus.subscribe("ui.**").unwrap();

                let manager_clone = manager.clone();
                let handle = tokio::task::spawn_local(async move {
                    let event = Event::new(
                        Channel::new("system.connection.established").unwrap(),
                        EventSource::Xmpp,
                        EventPayload::ConnectionEstablished {
                            jid: "alice@example.com".to_string(),
                        },
                    );
                    manager_clone.handle_event(&event).await;
                });

                tokio::task::yield_now().await;
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;

                // The handle_event should trigger sync_since which sends a MAM query
                let query_event =
                    tokio::time::timeout(std::time::Duration::from_millis(500), ui_sub.recv())
                        .await
                        .expect("timed out waiting for MAM query")
                        .expect("should receive query event");

                assert!(matches!(
                    query_event.payload,
                    EventPayload::MamQueryRequested { .. }
                ));

                // Send immediate fin to complete the sync
                event_bus
                    .publish(Event::new(
                        Channel::new("xmpp.mam.fin.received").unwrap(),
                        EventSource::Xmpp,
                        EventPayload::MamFinReceived {
                            iq_id: "iq-1".to_string(),
                            complete: true,
                            last_id: None,
                        },
                    ))
                    .unwrap();

                tokio::time::timeout(std::time::Duration::from_secs(5), handle)
                    .await
                    .expect("handle_event timed out")
                    .expect("handle_event should not panic");
            })
            .await;
    }

    #[tokio::test]
    async fn sync_with_existing_state_uses_after() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (manager, event_bus, _dir) = setup().await;

                // Pre-populate sync state
                manager
                    .update_sync_state("", "existing-id-99")
                    .await
                    .unwrap();

                let mut ui_sub = event_bus.subscribe("ui.**").unwrap();

                let manager_clone = manager.clone();
                let sync_handle =
                    tokio::task::spawn_local(
                        async move { manager_clone.sync_since(Utc::now()).await },
                    );

                tokio::task::yield_now().await;
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;

                let query_event =
                    tokio::time::timeout(std::time::Duration::from_millis(500), ui_sub.recv())
                        .await
                        .expect("timed out")
                        .expect("should receive query event");

                match &query_event.payload {
                    EventPayload::MamQueryRequested { after, .. } => {
                        assert_eq!(after.as_deref(), Some("existing-id-99"));
                    }
                    _ => panic!("expected MamQueryRequested"),
                }

                // Complete the sync
                event_bus
                    .publish(Event::new(
                        Channel::new("xmpp.mam.fin.received").unwrap(),
                        EventSource::Xmpp,
                        EventPayload::MamFinReceived {
                            iq_id: "iq-1".to_string(),
                            complete: true,
                            last_id: None,
                        },
                    ))
                    .unwrap();

                tokio::time::timeout(std::time::Duration::from_secs(5), sync_handle)
                    .await
                    .expect("timed out")
                    .expect("should not panic")
                    .expect("sync should succeed");
            })
            .await;
    }
}
