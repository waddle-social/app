use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Utc};
use tracing::{debug, error, warn};

use waddle_core::event::{Event, EventPayload, PresenceShow};

#[cfg(feature = "native")]
use std::sync::Arc;

#[cfg(feature = "native")]
use waddle_core::event::{Channel, EventBus, EventSource};

#[derive(Debug, thiserror::Error)]
pub enum PresenceError {
    #[error("failed to send presence: {0}")]
    SendFailed(String),

    #[error("invalid priority value: {0} (must be -128..127)")]
    InvalidPriority(i16),

    #[error("event bus error: {0}")]
    EventBus(String),
}

#[derive(Debug, Clone)]
pub struct PresenceInfo {
    pub jid: String,
    pub show: PresenceShow,
    pub status: Option<String>,
    pub priority: i8,
    pub last_updated: DateTime<Utc>,
}

impl PresenceInfo {
    fn unavailable(jid: &str) -> Self {
        Self {
            jid: jid.to_string(),
            show: PresenceShow::Unavailable,
            status: None,
            priority: 0,
            last_updated: Utc::now(),
        }
    }
}

pub struct PresenceManager {
    own_presence: RwLock<PresenceInfo>,
    contacts: RwLock<HashMap<String, PresenceInfo>>,
    #[cfg(feature = "native")]
    event_bus: Arc<dyn EventBus>,
}

impl PresenceManager {
    #[cfg(feature = "native")]
    pub fn new(event_bus: Arc<dyn EventBus>) -> Self {
        Self {
            own_presence: RwLock::new(PresenceInfo {
                jid: String::new(),
                show: PresenceShow::Unavailable,
                status: None,
                priority: 0,
                last_updated: Utc::now(),
            }),
            contacts: RwLock::new(HashMap::new()),
            event_bus,
        }
    }

    pub fn own_presence(&self) -> PresenceInfo {
        self.own_presence.read().unwrap().clone()
    }

    pub fn get_presence(&self, jid: &str) -> PresenceInfo {
        let bare = bare_jid(jid);
        self.contacts
            .read()
            .unwrap()
            .get(&bare)
            .cloned()
            .unwrap_or_else(|| PresenceInfo::unavailable(&bare))
    }

    #[cfg(feature = "native")]
    pub fn set_own_presence(
        &self,
        show: PresenceShow,
        status: Option<&str>,
        priority: Option<i8>,
    ) -> Result<(), PresenceError> {
        {
            let mut own = self.own_presence.write().unwrap();
            own.show = show.clone();
            own.status = status.map(String::from);
            if let Some(p) = priority {
                own.priority = p;
            }
            own.last_updated = Utc::now();
        }

        let _ = self.event_bus.publish(Event::new(
            Channel::new("ui.presence.set").unwrap(),
            EventSource::System("presence".into()),
            EventPayload::PresenceSetRequested {
                show,
                status: status.map(String::from),
            },
        ));

        Ok(())
    }

    #[cfg(feature = "native")]
    pub async fn handle_event(&self, event: &Event) {
        match &event.payload {
            EventPayload::ConnectionEstablished { jid } => {
                debug!(jid = %jid, "connection established, sending initial presence");
                {
                    let mut own = self.own_presence.write().unwrap();
                    own.jid = jid.clone();
                    own.show = PresenceShow::Available;
                    own.status = None;
                    own.priority = 0;
                    own.last_updated = Utc::now();
                }
                self.contacts.write().unwrap().clear();
                self.send_initial_presence();
            }
            EventPayload::ConnectionLost { .. } => {
                debug!("connection lost, clearing presence map");
                self.contacts.write().unwrap().clear();
                {
                    let mut own = self.own_presence.write().unwrap();
                    own.show = PresenceShow::Unavailable;
                    own.status = None;
                    own.last_updated = Utc::now();
                }
            }
            EventPayload::PresenceChanged { jid, show, status } => {
                debug!(jid = %jid, ?show, "contact presence changed");
                let bare = bare_jid(jid);
                let info = PresenceInfo {
                    jid: bare.clone(),
                    show: show.clone(),
                    status: status.clone(),
                    priority: 0,
                    last_updated: Utc::now(),
                };
                self.contacts.write().unwrap().insert(bare, info);
            }
            EventPayload::OwnPresenceChanged { show, status } => {
                debug!(?show, "own presence changed");
                let mut own = self.own_presence.write().unwrap();
                own.show = show.clone();
                own.status = status.clone();
                own.last_updated = Utc::now();
            }
            _ => {}
        }
    }

    #[cfg(feature = "native")]
    fn send_initial_presence(&self) {
        let _ = self.event_bus.publish(Event::new(
            Channel::new("ui.presence.set").unwrap(),
            EventSource::System("presence".into()),
            EventPayload::PresenceSetRequested {
                show: PresenceShow::Available,
                status: None,
            },
        ));
    }

    #[cfg(feature = "native")]
    pub async fn run(self: Arc<Self>) -> Result<(), PresenceError> {
        let mut sub = self
            .event_bus
            .subscribe("{system,xmpp}.**")
            .map_err(|e| PresenceError::EventBus(e.to_string()))?;

        loop {
            match sub.recv().await {
                Ok(event) => {
                    self.handle_event(&event).await;
                }
                Err(waddle_core::error::EventBusError::ChannelClosed) => {
                    debug!("event bus closed, presence manager stopping");
                    return Ok(());
                }
                Err(waddle_core::error::EventBusError::Lagged(count)) => {
                    warn!(count, "presence manager lagged, some events dropped");
                }
                Err(e) => {
                    error!(error = %e, "presence manager subscription error");
                    return Err(PresenceError::EventBus(e.to_string()));
                }
            }
        }
    }
}

fn bare_jid(jid: &str) -> String {
    match jid.find('/') {
        Some(pos) => jid[..pos].to_string(),
        None => jid.to_string(),
    }
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use waddle_core::event::{BroadcastEventBus, Channel, Event, EventBus, EventSource};

    fn make_manager() -> (Arc<PresenceManager>, Arc<dyn EventBus>) {
        let event_bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());
        let manager = Arc::new(PresenceManager::new(event_bus.clone()));
        (manager, event_bus)
    }

    fn make_event(channel: &str, payload: EventPayload) -> Event {
        Event::new(
            Channel::new(channel).unwrap(),
            EventSource::System("test".into()),
            payload,
        )
    }

    #[tokio::test]
    async fn initial_own_presence_is_unavailable() {
        let (manager, _) = make_manager();
        let own = manager.own_presence();
        assert!(matches!(own.show, PresenceShow::Unavailable));
    }

    #[tokio::test]
    async fn unknown_contact_returns_unavailable() {
        let (manager, _) = make_manager();
        let info = manager.get_presence("unknown@example.com");
        assert!(matches!(info.show, PresenceShow::Unavailable));
        assert_eq!(info.jid, "unknown@example.com");
    }

    #[tokio::test]
    async fn connection_established_sends_initial_presence() {
        let (manager, event_bus) = make_manager();
        let mut sub = event_bus.subscribe("ui.**").unwrap();

        let event = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "user@example.com".to_string(),
            },
        );
        manager.handle_event(&event).await;

        let own = manager.own_presence();
        assert!(matches!(own.show, PresenceShow::Available));
        assert_eq!(own.jid, "user@example.com");

        let received = tokio::time::timeout(Duration::from_millis(100), sub.recv())
            .await
            .expect("timed out")
            .expect("should receive event");

        assert!(matches!(
            received.payload,
            EventPayload::PresenceSetRequested {
                show: PresenceShow::Available,
                status: None,
            }
        ));
    }

    #[tokio::test]
    async fn connection_lost_clears_contacts_and_sets_unavailable() {
        let (manager, _) = make_manager();

        // Establish connection
        let event = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "user@example.com".to_string(),
            },
        );
        manager.handle_event(&event).await;

        // Receive some presence
        let event = make_event(
            "xmpp.presence.changed",
            EventPayload::PresenceChanged {
                jid: "alice@example.com".to_string(),
                show: PresenceShow::Available,
                status: None,
            },
        );
        manager.handle_event(&event).await;
        assert!(matches!(
            manager.get_presence("alice@example.com").show,
            PresenceShow::Available
        ));

        // Disconnect
        let event = make_event(
            "system.connection.lost",
            EventPayload::ConnectionLost {
                reason: "network error".to_string(),
                will_retry: true,
            },
        );
        manager.handle_event(&event).await;

        assert!(matches!(
            manager.own_presence().show,
            PresenceShow::Unavailable
        ));
        assert!(matches!(
            manager.get_presence("alice@example.com").show,
            PresenceShow::Unavailable
        ));
    }

    #[tokio::test]
    async fn presence_changed_updates_contact_map() {
        let (manager, _) = make_manager();

        let event = make_event(
            "xmpp.presence.changed",
            EventPayload::PresenceChanged {
                jid: "alice@example.com/desktop".to_string(),
                show: PresenceShow::Away,
                status: Some("brb".to_string()),
            },
        );
        manager.handle_event(&event).await;

        let info = manager.get_presence("alice@example.com");
        assert!(matches!(info.show, PresenceShow::Away));
        assert_eq!(info.status, Some("brb".to_string()));
        assert_eq!(info.jid, "alice@example.com");
    }

    #[tokio::test]
    async fn presence_changed_strips_resource() {
        let (manager, _) = make_manager();

        let event = make_event(
            "xmpp.presence.changed",
            EventPayload::PresenceChanged {
                jid: "bob@example.com/mobile".to_string(),
                show: PresenceShow::Dnd,
                status: Some("busy".to_string()),
            },
        );
        manager.handle_event(&event).await;

        // Query with bare JID
        let info = manager.get_presence("bob@example.com");
        assert!(matches!(info.show, PresenceShow::Dnd));

        // Query with full JID also resolves to bare
        let info2 = manager.get_presence("bob@example.com/mobile");
        assert!(matches!(info2.show, PresenceShow::Dnd));
    }

    #[tokio::test]
    async fn own_presence_changed_updates_own_state() {
        let (manager, _) = make_manager();

        let event = Event::new(
            Channel::new("xmpp.presence.own_changed").unwrap(),
            EventSource::Xmpp,
            EventPayload::OwnPresenceChanged {
                show: PresenceShow::Dnd,
                status: Some("do not disturb".to_string()),
            },
        );
        manager.handle_event(&event).await;

        let own = manager.own_presence();
        assert!(matches!(own.show, PresenceShow::Dnd));
        assert_eq!(own.status, Some("do not disturb".to_string()));
    }

    #[tokio::test]
    async fn set_own_presence_emits_event() {
        let (manager, event_bus) = make_manager();
        let mut sub = event_bus.subscribe("ui.**").unwrap();

        manager
            .set_own_presence(PresenceShow::Away, Some("lunch"), None)
            .unwrap();

        let received = tokio::time::timeout(Duration::from_millis(100), sub.recv())
            .await
            .expect("timed out")
            .expect("should receive event");

        assert!(matches!(
            received.payload,
            EventPayload::PresenceSetRequested {
                show: PresenceShow::Away,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn set_own_presence_updates_local_state() {
        let (manager, _) = make_manager();

        manager
            .set_own_presence(PresenceShow::Xa, Some("vacation"), Some(5))
            .unwrap();

        let own = manager.own_presence();
        assert!(matches!(own.show, PresenceShow::Xa));
        assert_eq!(own.status, Some("vacation".to_string()));
        assert_eq!(own.priority, 5);
    }

    #[tokio::test]
    async fn multiple_contacts_tracked_independently() {
        let (manager, _) = make_manager();

        let contacts = vec![
            ("alice@example.com", PresenceShow::Available, None),
            ("bob@example.com", PresenceShow::Away, Some("brb")),
            ("carol@example.com", PresenceShow::Dnd, Some("busy")),
        ];

        for (jid, show, status) in &contacts {
            let event = make_event(
                "xmpp.presence.changed",
                EventPayload::PresenceChanged {
                    jid: jid.to_string(),
                    show: show.clone(),
                    status: status.map(String::from),
                },
            );
            manager.handle_event(&event).await;
        }

        let alice = manager.get_presence("alice@example.com");
        assert!(matches!(alice.show, PresenceShow::Available));
        assert_eq!(alice.status, None);

        let bob = manager.get_presence("bob@example.com");
        assert!(matches!(bob.show, PresenceShow::Away));
        assert_eq!(bob.status, Some("brb".to_string()));

        let carol = manager.get_presence("carol@example.com");
        assert!(matches!(carol.show, PresenceShow::Dnd));
        assert_eq!(carol.status, Some("busy".to_string()));
    }

    #[tokio::test]
    async fn presence_updates_overwrite_previous() {
        let (manager, _) = make_manager();

        let event = make_event(
            "xmpp.presence.changed",
            EventPayload::PresenceChanged {
                jid: "alice@example.com".to_string(),
                show: PresenceShow::Available,
                status: None,
            },
        );
        manager.handle_event(&event).await;

        let event = make_event(
            "xmpp.presence.changed",
            EventPayload::PresenceChanged {
                jid: "alice@example.com".to_string(),
                show: PresenceShow::Away,
                status: Some("stepped out".to_string()),
            },
        );
        manager.handle_event(&event).await;

        let info = manager.get_presence("alice@example.com");
        assert!(matches!(info.show, PresenceShow::Away));
        assert_eq!(info.status, Some("stepped out".to_string()));
    }

    #[tokio::test]
    async fn unavailable_presence_keeps_entry() {
        let (manager, _) = make_manager();

        let event = make_event(
            "xmpp.presence.changed",
            EventPayload::PresenceChanged {
                jid: "alice@example.com".to_string(),
                show: PresenceShow::Available,
                status: None,
            },
        );
        manager.handle_event(&event).await;

        let event = make_event(
            "xmpp.presence.changed",
            EventPayload::PresenceChanged {
                jid: "alice@example.com".to_string(),
                show: PresenceShow::Unavailable,
                status: None,
            },
        );
        manager.handle_event(&event).await;

        let info = manager.get_presence("alice@example.com");
        assert!(matches!(info.show, PresenceShow::Unavailable));
    }

    #[tokio::test]
    async fn run_loop_processes_events() {
        let (manager, event_bus) = make_manager();

        let manager_clone = manager.clone();
        let handle = tokio::spawn(async move { manager_clone.run().await });

        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(10)).await;

        event_bus
            .publish(Event::new(
                Channel::new("xmpp.presence.changed").unwrap(),
                EventSource::Xmpp,
                EventPayload::PresenceChanged {
                    jid: "test@example.com".to_string(),
                    show: PresenceShow::Chat,
                    status: Some("free to chat".to_string()),
                },
            ))
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;

        let info = manager.get_presence("test@example.com");
        assert!(matches!(info.show, PresenceShow::Chat));
        assert_eq!(info.status, Some("free to chat".to_string()));

        handle.abort();
    }

    #[test]
    fn bare_jid_strips_resource() {
        assert_eq!(bare_jid("user@example.com/resource"), "user@example.com");
        assert_eq!(bare_jid("user@example.com"), "user@example.com");
        assert_eq!(bare_jid("user@example.com/res/extra"), "user@example.com");
    }
}
