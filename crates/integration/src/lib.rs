#[cfg(all(test, feature = "native"))]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::Utc;
    use tempfile::TempDir;
    use tokio::time::timeout;

    use waddle_core::event::{
        BroadcastEventBus, Channel, ChatMessage, ChatState, Event, EventBus, EventPayload,
        EventSource, MessageType, MucAffiliation, MucOccupant, MucRole, PresenceShow, RosterItem,
        Subscription,
    };
    use waddle_mam::MamManager;
    use waddle_messaging::{MessageManager, MucManager};
    use waddle_presence::PresenceManager;
    use waddle_roster::RosterManager;
    use waddle_storage::Database;

    const TIMEOUT: Duration = Duration::from_millis(500);

    async fn setup_db(dir: &TempDir) -> Arc<impl Database + use<>> {
        let db_path = dir.path().join("test.db");
        let db = waddle_storage::open_database(&db_path)
            .await
            .expect("failed to open database");
        Arc::new(db)
    }

    fn make_event(channel: &str, payload: EventPayload) -> Event {
        Event::new(
            Channel::new(channel).unwrap(),
            EventSource::System("test".into()),
            payload,
        )
    }

    fn make_xmpp_event(channel: &str, payload: EventPayload) -> Event {
        Event::new(Channel::new(channel).unwrap(), EventSource::Xmpp, payload)
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

    // ── 1. Connection/Auth ───────────────────────────────────────────
    // Verify that ConnectionEstablished propagates to all managers and
    // triggers the correct downstream behaviours.

    #[tokio::test]
    async fn connection_established_triggers_roster_fetch_and_presence_wait() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let roster = Arc::new(RosterManager::new(db.clone(), bus.clone()));
        let presence = Arc::new(PresenceManager::new(bus.clone()));

        let mut ui_sub = bus.subscribe("ui.**").unwrap();

        let connected = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "alice@example.com".to_string(),
            },
        );
        roster.handle_event(&connected).await;
        presence.handle_event(&connected).await;

        // Roster manager should emit RosterFetchRequested
        let event = timeout(TIMEOUT, ui_sub.recv())
            .await
            .expect("timed out")
            .unwrap();
        assert!(matches!(event.payload, EventPayload::RosterFetchRequested));

        // Presence should still be Unavailable (waiting for roster)
        let own = presence.own_presence();
        assert!(matches!(own.show, PresenceShow::Unavailable));
        assert_eq!(own.jid, "alice@example.com");
    }

    #[tokio::test]
    async fn connection_lost_propagates_to_all_managers() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let presence = Arc::new(PresenceManager::new(bus.clone()));
        let messaging = Arc::new(MessageManager::new(db.clone(), bus.clone()));

        // Establish connection first
        let connected = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "alice@example.com".to_string(),
            },
        );
        presence.handle_event(&connected).await;
        messaging.handle_event(&connected).await;

        // Now lose it
        let lost = make_event(
            "system.connection.lost",
            EventPayload::ConnectionLost {
                reason: "network error".to_string(),
                will_retry: true,
            },
        );
        presence.handle_event(&lost).await;
        messaging.handle_event(&lost).await;

        assert!(matches!(
            presence.own_presence().show,
            PresenceShow::Unavailable
        ));

        // Messaging should be offline - sends should enqueue
        let msg = messaging
            .send_message("bob@example.com", "offline msg")
            .await
            .unwrap();
        assert!(!msg.id.is_empty());

        // Verify it was enqueued (no ui event emitted)
        let mut sub = bus.subscribe("ui.message.send").unwrap();
        let result = timeout(Duration::from_millis(50), sub.recv()).await;
        assert!(result.is_err(), "offline send should not emit ui event");
    }

    // ── 2. Roster Sync ──────────────────────────────────────────────
    // Connection → RosterManager fetch → RosterReceived → persists
    // → PresenceManager gets roster → sends initial presence

    #[tokio::test]
    async fn roster_sync_flow_connection_to_initial_presence() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let roster = Arc::new(RosterManager::new(db.clone(), bus.clone()));
        let presence = Arc::new(PresenceManager::new(bus.clone()));

        let mut ui_sub = bus.subscribe("ui.**").unwrap();

        // Step 1: Connection established
        let connected = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "alice@example.com".to_string(),
            },
        );
        roster.handle_event(&connected).await;
        presence.handle_event(&connected).await;

        // Drain the RosterFetchRequested event
        let _ = timeout(TIMEOUT, ui_sub.recv()).await;

        // Step 2: Server sends full roster
        let items = vec![
            RosterItem {
                jid: "bob@example.com".to_string(),
                name: Some("Bob".to_string()),
                subscription: Subscription::Both,
                groups: vec!["Friends".to_string()],
            },
            RosterItem {
                jid: "carol@example.com".to_string(),
                name: None,
                subscription: Subscription::To,
                groups: vec![],
            },
        ];
        let roster_received = make_xmpp_event(
            "xmpp.roster.received",
            EventPayload::RosterReceived {
                items: items.clone(),
            },
        );
        roster.handle_event(&roster_received).await;
        presence.handle_event(&roster_received).await;

        // Step 3: Verify roster persisted
        let stored = roster.get_roster().await.unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].jid, "bob@example.com");
        assert_eq!(stored[0].name, Some("Bob".to_string()));
        assert!(matches!(stored[0].subscription, Subscription::Both));
        assert_eq!(stored[0].groups, vec!["Friends"]);
        assert_eq!(stored[1].jid, "carol@example.com");

        // Step 4: Presence manager should have sent initial presence
        let own = presence.own_presence();
        assert!(matches!(own.show, PresenceShow::Available));

        let event = timeout(TIMEOUT, ui_sub.recv())
            .await
            .expect("timed out waiting for initial presence")
            .unwrap();
        assert!(matches!(
            event.payload,
            EventPayload::PresenceSetRequested {
                show: PresenceShow::Available,
                status: None,
            }
        ));
    }

    #[tokio::test]
    async fn roster_push_updates_persist_incrementally() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let roster = Arc::new(RosterManager::new(db.clone(), bus.clone()));

        // Seed with initial roster
        let initial = make_xmpp_event(
            "xmpp.roster.received",
            EventPayload::RosterReceived {
                items: vec![RosterItem {
                    jid: "bob@example.com".to_string(),
                    name: Some("Bob".to_string()),
                    subscription: Subscription::Both,
                    groups: vec![],
                }],
            },
        );
        roster.handle_event(&initial).await;

        // Roster push: add new contact
        let push = make_xmpp_event(
            "xmpp.roster.updated",
            EventPayload::RosterUpdated {
                item: RosterItem {
                    jid: "dave@example.com".to_string(),
                    name: Some("Dave".to_string()),
                    subscription: Subscription::None,
                    groups: vec!["Work".to_string()],
                },
            },
        );
        roster.handle_event(&push).await;

        let stored = roster.get_roster().await.unwrap();
        assert_eq!(stored.len(), 2);

        // Roster push: remove contact
        let remove = make_xmpp_event(
            "xmpp.roster.removed",
            EventPayload::RosterRemoved {
                jid: "bob@example.com".to_string(),
            },
        );
        roster.handle_event(&remove).await;

        let stored = roster.get_roster().await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].jid, "dave@example.com");
    }

    // ── 3. 1:1 Messaging ────────────────────────────────────────────
    // MessageManager send/receive with persistence and events

    #[tokio::test]
    async fn one_to_one_messaging_send_receive_persist() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let messaging = Arc::new(MessageManager::new(db.clone(), bus.clone()));

        // Bring online
        let connected = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "alice@example.com".to_string(),
            },
        );
        messaging.handle_event(&connected).await;

        let mut ui_sub = bus.subscribe("ui.**").unwrap();

        // Send a message
        let sent = messaging
            .send_message("bob@example.com", "Hello Bob!")
            .await
            .unwrap();
        assert!(!sent.id.is_empty());
        assert_eq!(sent.to, "bob@example.com");

        // Verify send event emitted
        let event = timeout(TIMEOUT, ui_sub.recv())
            .await
            .expect("timed out")
            .unwrap();
        assert!(matches!(
            event.payload,
            EventPayload::MessageSendRequested { ref to, ref body, .. }
            if to == "bob@example.com" && body == "Hello Bob!"
        ));

        // Simulate receiving a reply
        let reply = make_chat_message("reply-1", "bob@example.com", "alice@example.com", "Hey!");
        let received_event = make_xmpp_event(
            "xmpp.message.received",
            EventPayload::MessageReceived {
                message: reply.clone(),
            },
        );
        messaging.handle_event(&received_event).await;

        // Verify both messages persisted
        let messages = messaging
            .get_messages("bob@example.com", 50, None)
            .await
            .unwrap();
        assert_eq!(messages.len(), 2);

        let bodies: Vec<&str> = messages.iter().map(|m| m.body.as_str()).collect();
        assert!(bodies.contains(&"Hello Bob!"));
        assert!(bodies.contains(&"Hey!"));
    }

    #[tokio::test]
    async fn message_delivery_receipt_flow() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let messaging = Arc::new(MessageManager::new(db.clone(), bus.clone()));

        // Send while offline (enqueues)
        let sent = messaging
            .send_message("bob@example.com", "queued message")
            .await
            .unwrap();

        // Come online - drains queue
        let connected = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "alice@example.com".to_string(),
            },
        );
        messaging.handle_event(&connected).await;

        // Simulate server echo (MessageSent)
        let echo = make_xmpp_event(
            "xmpp.message.sent",
            EventPayload::MessageSent {
                message: make_chat_message(
                    &sent.id,
                    "alice@example.com",
                    "bob@example.com",
                    "queued message",
                ),
            },
        );
        messaging.handle_event(&echo).await;

        // Simulate delivery receipt
        let receipt = make_xmpp_event(
            "xmpp.message.delivered",
            EventPayload::MessageDelivered {
                id: sent.id.clone(),
                to: "bob@example.com".to_string(),
            },
        );
        messaging.handle_event(&receipt).await;

        // Verify queue item is confirmed
        let rows: Vec<waddle_storage::Row> = db
            .query(
                "SELECT status FROM offline_queue ORDER BY id ASC LIMIT 1",
                &[],
            )
            .await
            .unwrap();
        assert_eq!(
            rows[0].get(0),
            Some(&waddle_storage::SqlValue::Text("confirmed".to_string()))
        );
    }

    #[tokio::test]
    async fn chat_state_notifications_across_messaging() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let messaging = Arc::new(MessageManager::new(db.clone(), bus.clone()));

        // Bring online
        let connected = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "alice@example.com".to_string(),
            },
        );
        messaging.handle_event(&connected).await;

        let mut ui_sub = bus.subscribe("ui.**").unwrap();

        // Send composing state
        messaging
            .send_chat_state("bob@example.com", ChatState::Composing)
            .await
            .unwrap();

        let event = timeout(TIMEOUT, ui_sub.recv())
            .await
            .expect("timed out")
            .unwrap();
        assert!(matches!(
            event.payload,
            EventPayload::ChatStateSendRequested {
                ref to,
                state: ChatState::Composing,
            } if to == "bob@example.com"
        ));

        // Receive chat state from peer
        let peer_state = make_xmpp_event(
            "xmpp.chatstate.received",
            EventPayload::ChatStateReceived {
                from: "bob@example.com".to_string(),
                state: ChatState::Active,
            },
        );
        messaging.handle_event(&peer_state).await;
        // No panic = success; chat state handling is log-only
    }

    // ── 4. MUC Messaging ────────────────────────────────────────────
    // MucManager join/leave/send with occupant tracking and message persistence

    #[tokio::test]
    async fn muc_join_message_occupant_leave_flow() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let muc = Arc::new(MucManager::new(db.clone(), bus.clone()));

        // Join a room
        muc.join_room("room@conference.example.com", "Alice")
            .await
            .unwrap();

        // Server confirms join
        let joined = make_xmpp_event(
            "xmpp.muc.joined",
            EventPayload::MucJoined {
                room: "room@conference.example.com".to_string(),
                nick: "Alice".to_string(),
            },
        );
        muc.handle_event(&joined).await;

        let rooms = muc.get_joined_rooms().await.unwrap();
        assert_eq!(rooms.len(), 1);
        assert!(rooms[0].joined);

        // Occupants join
        let bob_join = make_xmpp_event(
            "xmpp.muc.occupant.changed",
            EventPayload::MucOccupantChanged {
                room: "room@conference.example.com".to_string(),
                occupant: MucOccupant {
                    nick: "Bob".to_string(),
                    jid: Some("bob@example.com".to_string()),
                    affiliation: MucAffiliation::Member,
                    role: MucRole::Participant,
                },
            },
        );
        muc.handle_event(&bob_join).await;

        let occupants = muc.get_occupants("room@conference.example.com");
        assert_eq!(occupants.len(), 1);
        assert_eq!(occupants[0].nick, "Bob");

        // Receive a room message
        let msg = ChatMessage {
            id: "muc-msg-1".to_string(),
            from: "room@conference.example.com/Bob".to_string(),
            to: "room@conference.example.com".to_string(),
            body: "Hello room!".to_string(),
            timestamp: Utc::now(),
            message_type: MessageType::Groupchat,
            thread: None,
        };
        let msg_event = make_xmpp_event(
            "xmpp.muc.message.received",
            EventPayload::MucMessageReceived {
                room: "room@conference.example.com".to_string(),
                message: msg,
            },
        );
        muc.handle_event(&msg_event).await;

        let messages = muc
            .get_room_messages("room@conference.example.com", 50, None)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "Hello room!");

        // Subject change
        let subject = make_xmpp_event(
            "xmpp.muc.subject.changed",
            EventPayload::MucSubjectChanged {
                room: "room@conference.example.com".to_string(),
                subject: "Sprint Planning".to_string(),
            },
        );
        muc.handle_event(&subject).await;

        let rooms = muc.get_joined_rooms().await.unwrap();
        assert_eq!(rooms[0].subject, Some("Sprint Planning".to_string()));

        // Bob leaves (role=None)
        let bob_leave = make_xmpp_event(
            "xmpp.muc.occupant.changed",
            EventPayload::MucOccupantChanged {
                room: "room@conference.example.com".to_string(),
                occupant: MucOccupant {
                    nick: "Bob".to_string(),
                    jid: Some("bob@example.com".to_string()),
                    affiliation: MucAffiliation::Member,
                    role: MucRole::None,
                },
            },
        );
        muc.handle_event(&bob_leave).await;
        assert!(muc.get_occupants("room@conference.example.com").is_empty());

        // We leave
        let left = make_xmpp_event(
            "xmpp.muc.left",
            EventPayload::MucLeft {
                room: "room@conference.example.com".to_string(),
            },
        );
        muc.handle_event(&left).await;

        let joined = muc.get_joined_rooms().await.unwrap();
        assert!(joined.is_empty());
    }

    #[tokio::test]
    async fn muc_send_emits_event() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let muc = Arc::new(MucManager::new(db.clone(), bus.clone()));
        let mut ui_sub = bus.subscribe("ui.**").unwrap();

        muc.send_message("room@conference.example.com", "Hey everyone!")
            .await
            .unwrap();

        let event = timeout(TIMEOUT, ui_sub.recv())
            .await
            .expect("timed out")
            .unwrap();
        assert!(matches!(
            event.payload,
            EventPayload::MucSendRequested { ref room, ref body }
            if room == "room@conference.example.com" && body == "Hey everyone!"
        ));
    }

    // ── 5. MAM Sync ─────────────────────────────────────────────────
    // MamManager sync_since with event-based paginated query/response
    // and connection → presence → MAM trigger flow

    #[tokio::test]
    async fn mam_sync_triggered_by_own_presence_after_connection() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let dir = TempDir::new().unwrap();
                let db = setup_db(&dir).await;
                let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

                let mam = Arc::new(MamManager::new(db.clone(), bus.clone()));
                let presence = Arc::new(PresenceManager::new(bus.clone()));

                let mut ui_sub = bus.subscribe("ui.**").unwrap();

                // Step 1: Connection
                let connected = make_event(
                    "system.connection.established",
                    EventPayload::ConnectionEstablished {
                        jid: "alice@example.com".to_string(),
                    },
                );
                mam.handle_event(&connected).await;
                presence.handle_event(&connected).await;

                // No MAM query yet
                let no_query = timeout(Duration::from_millis(50), ui_sub.recv()).await;
                assert!(no_query.is_err(), "MAM should wait for own presence");

                // Step 2: Roster received → presence sends initial Available
                let roster_event = make_xmpp_event(
                    "xmpp.roster.received",
                    EventPayload::RosterReceived { items: vec![] },
                );
                presence.handle_event(&roster_event).await;

                // Drain the PresenceSetRequested from presence manager
                let _pres_event = timeout(TIMEOUT, ui_sub.recv()).await;

                // Step 3: OwnPresenceChanged triggers MAM sync
                let own_presence = make_xmpp_event(
                    "xmpp.presence.own_changed",
                    EventPayload::OwnPresenceChanged {
                        show: PresenceShow::Available,
                        status: None,
                    },
                );

                // Spawn handle_event so we can respond to the MAM query
                let mam_clone = mam.clone();
                let handle = tokio::task::spawn_local(async move {
                    mam_clone.handle_event(&own_presence).await;
                });

                // Wait for the MAM query request
                let query_event = timeout(TIMEOUT, ui_sub.recv())
                    .await
                    .expect("timed out waiting for MAM query")
                    .unwrap();

                let query_id = match &query_event.payload {
                    EventPayload::MamQueryRequested { query_id, .. } => query_id.clone(),
                    other => panic!("expected MamQueryRequested, got {other:?}"),
                };

                // Simulate MAM result
                let msg = make_chat_message(
                    "arch-1",
                    "bob@example.com",
                    "alice@example.com",
                    "Missed message",
                );
                bus.publish(Event::new(
                    Channel::new("xmpp.mam.result.received").unwrap(),
                    EventSource::Xmpp,
                    EventPayload::MamResultReceived {
                        query_id: query_id.clone(),
                        messages: vec![msg],
                        complete: false,
                    },
                ))
                .unwrap();

                // Simulate MAM fin
                bus.publish(Event::new(
                    Channel::new("xmpp.mam.fin.received").unwrap(),
                    EventSource::Xmpp,
                    EventPayload::MamFinReceived {
                        iq_id: query_id,
                        complete: true,
                        last_id: Some("arch-1".to_string()),
                    },
                ))
                .unwrap();

                timeout(Duration::from_secs(5), handle)
                    .await
                    .expect("MAM sync timed out")
                    .expect("MAM sync should not panic");

                // Verify message persisted
                let rows: Vec<waddle_storage::Row> = db
                    .query("SELECT COUNT(*) FROM messages", &[])
                    .await
                    .unwrap();
                assert_eq!(rows[0].get(0), Some(&waddle_storage::SqlValue::Integer(1)));

                // Verify sync state updated
                let state: Vec<waddle_storage::Row> = db
                    .query(
                        "SELECT last_stanza_id FROM mam_sync_state WHERE jid = '__global__'",
                        &[],
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    state[0].get(0),
                    Some(&waddle_storage::SqlValue::Text("arch-1".to_string()))
                );
            })
            .await;
    }

    #[tokio::test]
    async fn mam_unavailable_presence_does_not_trigger_sync() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let mam = Arc::new(MamManager::new(db.clone(), bus.clone()));

        let mut ui_sub = bus.subscribe("ui.**").unwrap();

        // Connection
        let connected = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "alice@example.com".to_string(),
            },
        );
        mam.handle_event(&connected).await;

        // OwnPresenceChanged with Unavailable should NOT trigger sync
        let unavailable = make_xmpp_event(
            "xmpp.presence.own_changed",
            EventPayload::OwnPresenceChanged {
                show: PresenceShow::Unavailable,
                status: None,
            },
        );
        mam.handle_event(&unavailable).await;

        let no_query = timeout(Duration::from_millis(50), ui_sub.recv()).await;
        assert!(
            no_query.is_err(),
            "unavailable presence should not trigger MAM sync"
        );
    }

    // ── 6. Offline Queue Drain ──────────────────────────────────────
    // MessageManager offline enqueue → reconnect → drain FIFO → status lifecycle

    #[tokio::test]
    async fn offline_queue_enqueue_drain_and_reconcile() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let messaging = Arc::new(MessageManager::new(db.clone(), bus.clone()));

        // Send messages while offline (both enqueued)
        let msg1 = messaging
            .send_message("bob@example.com", "first")
            .await
            .unwrap();
        let msg2 = messaging
            .send_message("carol@example.com", "second")
            .await
            .unwrap();

        // Verify messages persisted even while offline
        let stored = messaging
            .get_messages("bob@example.com", 50, None)
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].body, "first");

        // Verify queue has 2 pending items
        let rows: Vec<waddle_storage::Row> = db
            .query(
                "SELECT payload FROM offline_queue WHERE status = 'pending' ORDER BY id ASC",
                &[],
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);

        // Subscribe to drained events
        let mut ui_sub = bus.subscribe("ui.message.send").unwrap();

        // Come online - triggers drain
        let connected = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "alice@example.com".to_string(),
            },
        );
        messaging.handle_event(&connected).await;

        // Verify FIFO order of drained events
        let first_event = timeout(TIMEOUT, ui_sub.recv())
            .await
            .expect("timed out")
            .unwrap();
        assert!(matches!(
            first_event.payload,
            EventPayload::MessageSendRequested { ref body, .. } if body == "first"
        ));

        let second_event = timeout(TIMEOUT, ui_sub.recv())
            .await
            .expect("timed out")
            .unwrap();
        assert!(matches!(
            second_event.payload,
            EventPayload::MessageSendRequested { ref body, .. } if body == "second"
        ));

        // Simulate server echoing the messages back (MessageSent)
        messaging
            .handle_event(&make_xmpp_event(
                "xmpp.message.sent",
                EventPayload::MessageSent {
                    message: make_chat_message(
                        &msg1.id,
                        "alice@example.com",
                        "bob@example.com",
                        "first",
                    ),
                },
            ))
            .await;

        // Check first message moved to "sent"
        let rows: Vec<waddle_storage::Row> = db
            .query("SELECT status FROM offline_queue ORDER BY id ASC", &[])
            .await
            .unwrap();
        assert_eq!(
            rows[0].get(0),
            Some(&waddle_storage::SqlValue::Text("sent".to_string()))
        );

        // Simulate delivery receipt for first message
        messaging
            .handle_event(&make_xmpp_event(
                "xmpp.message.delivered",
                EventPayload::MessageDelivered {
                    id: msg1.id.clone(),
                    to: "bob@example.com".to_string(),
                },
            ))
            .await;

        let rows: Vec<waddle_storage::Row> = db
            .query("SELECT status FROM offline_queue ORDER BY id ASC", &[])
            .await
            .unwrap();
        assert_eq!(
            rows[0].get(0),
            Some(&waddle_storage::SqlValue::Text("confirmed".to_string()))
        );

        // MAM reconciliation for second message
        let mam_msg = ChatMessage {
            id: "archive-id-99".to_string(),
            from: "alice@example.com".to_string(),
            to: "carol@example.com".to_string(),
            body: "second".to_string(),
            timestamp: Utc::now(),
            message_type: MessageType::Chat,
            thread: None,
        };

        // First mark second as sent
        messaging
            .handle_event(&make_xmpp_event(
                "xmpp.message.sent",
                EventPayload::MessageSent {
                    message: make_chat_message(
                        &msg2.id,
                        "alice@example.com",
                        "carol@example.com",
                        "second",
                    ),
                },
            ))
            .await;

        // Then reconcile via MAM result
        messaging
            .handle_event(&make_xmpp_event(
                "xmpp.mam.result.received",
                EventPayload::MamResultReceived {
                    query_id: "q1".to_string(),
                    messages: vec![mam_msg],
                    complete: true,
                },
            ))
            .await;

        let rows: Vec<waddle_storage::Row> = db
            .query("SELECT status FROM offline_queue ORDER BY id ASC", &[])
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].get(0),
            Some(&waddle_storage::SqlValue::Text("confirmed".to_string()))
        );
        assert_eq!(
            rows[1].get(0),
            Some(&waddle_storage::SqlValue::Text("confirmed".to_string()))
        );
    }

    #[tokio::test]
    async fn offline_queue_non_message_commands_auto_confirmed() {
        let dir = TempDir::new().unwrap();
        let db = setup_db(&dir).await;
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let messaging = Arc::new(MessageManager::new(db.clone(), bus.clone()));
        let roster = Arc::new(RosterManager::new(db.clone(), bus.clone()));

        // Add a contact while offline - roster emits the event, messaging
        // intercepts the ui.roster.add event and enqueues it
        roster
            .add_contact("dave@example.com", Some("Dave"), &[])
            .await
            .unwrap();

        // The roster_add event was published on the bus but messaging is offline
        // so let's manually handle it as an offline command event
        let add_event = Event::new(
            Channel::new("ui.roster.add").unwrap(),
            EventSource::System("roster".into()),
            EventPayload::RosterAddRequested {
                jid: "dave@example.com".to_string(),
                name: Some("Dave".to_string()),
                groups: vec![],
            },
        );
        messaging.handle_event(&add_event).await;

        // Verify it was enqueued
        let rows: Vec<waddle_storage::Row> = db
            .query("SELECT stanza_type, status FROM offline_queue", &[])
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].get(0),
            Some(&waddle_storage::SqlValue::Text("iq".to_string()))
        );
        assert_eq!(
            rows[0].get(1),
            Some(&waddle_storage::SqlValue::Text("pending".to_string()))
        );

        // Come online - drain; non-message commands go straight to confirmed
        let connected = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "alice@example.com".to_string(),
            },
        );
        messaging.handle_event(&connected).await;

        let rows: Vec<waddle_storage::Row> = db
            .query("SELECT status FROM offline_queue", &[])
            .await
            .unwrap();
        assert_eq!(
            rows[0].get(0),
            Some(&waddle_storage::SqlValue::Text("confirmed".to_string()))
        );
    }

    // ── Cross-manager: presence tracks contacts from roster events ──

    #[tokio::test]
    async fn presence_tracks_contacts_after_roster_and_connection() {
        let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

        let presence = Arc::new(PresenceManager::new(bus.clone()));

        // Connection
        let connected = make_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: "alice@example.com".to_string(),
            },
        );
        presence.handle_event(&connected).await;

        // Roster received
        let roster = make_xmpp_event(
            "xmpp.roster.received",
            EventPayload::RosterReceived { items: vec![] },
        );
        presence.handle_event(&roster).await;

        // Contact comes online
        let bob_available = make_xmpp_event(
            "xmpp.presence.changed",
            EventPayload::PresenceChanged {
                jid: "bob@example.com/desktop".to_string(),
                show: PresenceShow::Available,
                status: Some("online".to_string()),
                priority: 5,
            },
        );
        presence.handle_event(&bob_available).await;

        let info = presence.get_presence("bob@example.com");
        assert!(matches!(info.show, PresenceShow::Available));
        assert_eq!(info.status, Some("online".to_string()));

        // Bob on mobile with higher priority
        let bob_mobile = make_xmpp_event(
            "xmpp.presence.changed",
            EventPayload::PresenceChanged {
                jid: "bob@example.com/mobile".to_string(),
                show: PresenceShow::Away,
                status: Some("on phone".to_string()),
                priority: 10,
            },
        );
        presence.handle_event(&bob_mobile).await;

        let info = presence.get_presence("bob@example.com");
        assert!(matches!(info.show, PresenceShow::Away));
        assert_eq!(info.priority, 10);

        // Connection lost clears all
        let lost = make_event(
            "system.connection.lost",
            EventPayload::ConnectionLost {
                reason: "timeout".to_string(),
                will_retry: true,
            },
        );
        presence.handle_event(&lost).await;

        let info = presence.get_presence("bob@example.com");
        assert!(matches!(info.show, PresenceShow::Unavailable));
    }

    // ── Full lifecycle: connection → roster → presence → MAM ────────

    #[tokio::test]
    async fn full_startup_lifecycle_connection_to_mam_sync() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let dir = TempDir::new().unwrap();
                let db = setup_db(&dir).await;
                let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::default());

                let roster = Arc::new(RosterManager::new(db.clone(), bus.clone()));
                let presence = Arc::new(PresenceManager::new(bus.clone()));
                let messaging = Arc::new(MessageManager::new(db.clone(), bus.clone()));
                let mam = Arc::new(MamManager::new(db.clone(), bus.clone()));

                let mut ui_sub = bus.subscribe("ui.**").unwrap();
                let mut sys_sub = bus.subscribe("system.**").unwrap();

                // 1. ConnectionEstablished
                let connected = make_event(
                    "system.connection.established",
                    EventPayload::ConnectionEstablished {
                        jid: "alice@example.com".to_string(),
                    },
                );
                roster.handle_event(&connected).await;
                presence.handle_event(&connected).await;
                messaging.handle_event(&connected).await;
                mam.handle_event(&connected).await;

                // Drain RosterFetchRequested
                let fetch = timeout(TIMEOUT, ui_sub.recv())
                    .await
                    .expect("timed out")
                    .unwrap();
                assert!(matches!(fetch.payload, EventPayload::RosterFetchRequested));

                // Drain ComingOnline from messaging
                let coming = timeout(TIMEOUT, sys_sub.recv())
                    .await
                    .expect("timed out")
                    .unwrap();
                assert!(matches!(coming.payload, EventPayload::ComingOnline));

                // 2. RosterReceived
                let roster_event = make_xmpp_event(
                    "xmpp.roster.received",
                    EventPayload::RosterReceived {
                        items: vec![RosterItem {
                            jid: "bob@example.com".to_string(),
                            name: Some("Bob".to_string()),
                            subscription: Subscription::Both,
                            groups: vec![],
                        }],
                    },
                );
                roster.handle_event(&roster_event).await;
                presence.handle_event(&roster_event).await;

                // Verify roster stored
                let stored = roster.get_roster().await.unwrap();
                assert_eq!(stored.len(), 1);

                // 3. Presence sends initial Available
                let pres_event = timeout(TIMEOUT, ui_sub.recv())
                    .await
                    .expect("timed out")
                    .unwrap();
                assert!(matches!(
                    pres_event.payload,
                    EventPayload::PresenceSetRequested {
                        show: PresenceShow::Available,
                        ..
                    }
                ));

                // 4. OwnPresenceChanged triggers MAM sync
                let own_presence = make_xmpp_event(
                    "xmpp.presence.own_changed",
                    EventPayload::OwnPresenceChanged {
                        show: PresenceShow::Available,
                        status: None,
                    },
                );
                presence.handle_event(&own_presence).await;

                let mam_clone = mam.clone();
                let mam_handle = tokio::task::spawn_local(async move {
                    mam_clone.handle_event(&own_presence).await;
                });

                // 5. MAM queries for catch-up
                let query_event = timeout(TIMEOUT, ui_sub.recv())
                    .await
                    .expect("timed out waiting for MAM query")
                    .unwrap();

                let query_id = match &query_event.payload {
                    EventPayload::MamQueryRequested { query_id, .. } => query_id.clone(),
                    other => panic!("expected MamQueryRequested, got {other:?}"),
                };

                // Respond with empty archive
                bus.publish(Event::new(
                    Channel::new("xmpp.mam.fin.received").unwrap(),
                    EventSource::Xmpp,
                    EventPayload::MamFinReceived {
                        iq_id: query_id,
                        complete: true,
                        last_id: None,
                    },
                ))
                .unwrap();

                timeout(Duration::from_secs(5), mam_handle)
                    .await
                    .expect("MAM handle timed out")
                    .expect("MAM handle should not panic");

                // 6. SyncStarted and SyncCompleted events
                let started = timeout(TIMEOUT, sys_sub.recv())
                    .await
                    .expect("timed out waiting for SyncStarted")
                    .unwrap();
                assert!(matches!(started.payload, EventPayload::SyncStarted));

                let completed = timeout(TIMEOUT, sys_sub.recv())
                    .await
                    .expect("timed out waiting for SyncCompleted")
                    .unwrap();
                assert!(matches!(
                    completed.payload,
                    EventPayload::SyncCompleted { messages_synced: 0 }
                ));
                assert_eq!(started.correlation_id, completed.correlation_id);
            })
            .await;
    }
}
