use std::time::Duration;

#[cfg(feature = "native")]
use std::sync::Arc;

pub use crate::transport::ConnectionConfig;
use crate::{error::ConnectionError, transport::XmppTransport};

#[cfg(feature = "native")]
use waddle_core::event::{Channel, Event, EventBus, EventPayload, EventSource};

#[cfg(not(any(feature = "native", feature = "web")))]
compile_error!("waddle-xmpp requires either the `native` or `web` feature.");

#[cfg(feature = "native")]
type DefaultTransport = crate::transport::NativeTcpTransport;

#[cfg(all(feature = "web", not(feature = "native")))]
type DefaultTransport = crate::transport::WebSocketTransport;

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
}

pub struct ConnectionManager<T = DefaultTransport>
where
    T: XmppTransport,
{
    state: ConnectionState,
    config: ConnectionConfig,
    transport: Option<T>,
    #[cfg(feature = "native")]
    event_bus: Option<Arc<dyn EventBus>>,
}

impl<T> ConnectionManager<T>
where
    T: XmppTransport,
{
    const INITIAL_RECONNECT_DELAY_SECONDS: u64 = 1;
    const MAX_RECONNECT_DELAY_SECONDS: u64 = 60;

    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            state: ConnectionState::Disconnected,
            config,
            transport: None,
            #[cfg(feature = "native")]
            event_bus: None,
        }
    }

    #[cfg(feature = "native")]
    pub fn with_event_bus(config: ConnectionConfig, event_bus: Arc<dyn EventBus>) -> Self {
        Self {
            state: ConnectionState::Disconnected,
            config,
            transport: None,
            event_bus: Some(event_bus),
        }
    }

    pub async fn connect(&mut self) -> Result<(), ConnectionError> {
        if matches!(self.state, ConnectionState::Connected) {
            return Ok(());
        }

        self.state = ConnectionState::Connecting;
        let mut reconnect_attempt = 0_u32;

        loop {
            match T::connect(&self.config).await {
                Ok(transport) => {
                    self.transport = Some(transport);
                    self.state = ConnectionState::Connected;
                    #[cfg(feature = "native")]
                    self.emit_connection_established();
                    return Ok(());
                }
                Err(error) => {
                    self.transport = None;
                    let next_attempt = reconnect_attempt.saturating_add(1);
                    let will_retry = error.is_retryable() && self.should_retry(next_attempt);

                    #[cfg(feature = "native")]
                    {
                        self.emit_connection_lost(error.to_string(), will_retry);
                        self.emit_connection_error(&error);
                    }

                    if !will_retry {
                        self.state = ConnectionState::Disconnected;
                        return Err(error);
                    }

                    reconnect_attempt = next_attempt;
                    self.state = ConnectionState::Reconnecting {
                        attempt: reconnect_attempt,
                    };
                    #[cfg(feature = "native")]
                    self.emit_connection_reconnecting(reconnect_attempt);

                    tokio::time::sleep(Self::reconnect_delay(reconnect_attempt)).await;
                    self.state = ConnectionState::Connecting;
                }
            }
        }
    }

    pub async fn disconnect(&mut self) -> Result<(), ConnectionError> {
        if let Some(mut transport) = self.transport.take() {
            if let Err(error) = transport.close().await {
                self.state = ConnectionState::Disconnected;
                #[cfg(feature = "native")]
                {
                    self.emit_connection_lost(error.to_string(), false);
                    self.emit_connection_error(&error);
                }
                return Err(error);
            }
        }

        if !matches!(self.state, ConnectionState::Disconnected) {
            #[cfg(feature = "native")]
            self.emit_connection_lost("user requested disconnect".to_string(), false);
        }

        self.state = ConnectionState::Disconnected;
        Ok(())
    }

    pub fn state(&self) -> ConnectionState {
        self.state.clone()
    }

    fn should_retry(&self, attempt: u32) -> bool {
        self.config.max_reconnect_attempts == 0 || attempt <= self.config.max_reconnect_attempts
    }

    fn reconnect_delay(attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1);
        let seconds = 1_u64.checked_shl(shift).unwrap_or(u64::MAX).clamp(
            Self::INITIAL_RECONNECT_DELAY_SECONDS,
            Self::MAX_RECONNECT_DELAY_SECONDS,
        );
        Duration::from_secs(seconds)
    }

    #[cfg(feature = "native")]
    fn emit_connection_established(&self) {
        self.emit_event(
            "system.connection.established",
            EventPayload::ConnectionEstablished {
                jid: self.config.jid.clone(),
            },
        );
    }

    #[cfg(feature = "native")]
    fn emit_connection_lost(&self, reason: String, will_retry: bool) {
        self.emit_event(
            "system.connection.lost",
            EventPayload::ConnectionLost { reason, will_retry },
        );
    }

    #[cfg(feature = "native")]
    fn emit_connection_reconnecting(&self, attempt: u32) {
        self.emit_event(
            "system.connection.reconnecting",
            EventPayload::ConnectionReconnecting { attempt },
        );
    }

    #[cfg(feature = "native")]
    fn emit_connection_error(&self, error: &ConnectionError) {
        self.emit_event(
            "system.error.occurred",
            EventPayload::ErrorOccurred {
                component: "connection".to_string(),
                message: error.to_string(),
                recoverable: error.is_retryable(),
            },
        );
    }

    #[cfg(feature = "native")]
    fn emit_event(&self, channel_name: &str, payload: EventPayload) {
        let Some(event_bus) = &self.event_bus else {
            return;
        };

        let Ok(channel) = Channel::new(channel_name) else {
            return;
        };

        let event = Event::new(channel, EventSource::Xmpp, payload);
        let _ = event_bus.publish(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyTransport;

    impl XmppTransport for DummyTransport {
        async fn connect(_config: &ConnectionConfig) -> Result<Self, ConnectionError> {
            Ok(Self)
        }

        async fn send(&mut self, _data: &[u8]) -> Result<(), ConnectionError> {
            Ok(())
        }

        async fn recv(&mut self) -> Result<Vec<u8>, ConnectionError> {
            Ok(Vec::new())
        }

        async fn close(&mut self) -> Result<(), ConnectionError> {
            Ok(())
        }
    }

    #[test]
    fn reconnect_delay_is_exponential_and_capped_at_sixty_seconds() {
        assert_eq!(
            ConnectionManager::<DummyTransport>::reconnect_delay(1),
            Duration::from_secs(1)
        );
        assert_eq!(
            ConnectionManager::<DummyTransport>::reconnect_delay(2),
            Duration::from_secs(2)
        );
        assert_eq!(
            ConnectionManager::<DummyTransport>::reconnect_delay(3),
            Duration::from_secs(4)
        );
        assert_eq!(
            ConnectionManager::<DummyTransport>::reconnect_delay(4),
            Duration::from_secs(8)
        );
        assert_eq!(
            ConnectionManager::<DummyTransport>::reconnect_delay(6),
            Duration::from_secs(32)
        );
        assert_eq!(
            ConnectionManager::<DummyTransport>::reconnect_delay(7),
            Duration::from_secs(60)
        );
        assert_eq!(
            ConnectionManager::<DummyTransport>::reconnect_delay(99),
            Duration::from_secs(60)
        );
    }
}

#[cfg(all(test, feature = "native"))]
mod native_tests {
    use std::{
        collections::VecDeque,
        sync::{Mutex, OnceLock},
    };

    use tokio::{sync::Mutex as AsyncMutex, time};
    use waddle_core::event::{BroadcastEventBus, EventPayload};

    use super::*;

    #[derive(Default)]
    struct TestTransportState {
        connect_outcomes: VecDeque<Result<(), ConnectionError>>,
        connect_calls: u32,
        close_calls: u32,
    }

    fn transport_state() -> &'static Mutex<TestTransportState> {
        static STATE: OnceLock<Mutex<TestTransportState>> = OnceLock::new();
        STATE.get_or_init(|| Mutex::new(TestTransportState::default()))
    }

    fn test_lock() -> &'static AsyncMutex<()> {
        static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| AsyncMutex::new(()))
    }

    fn configure_transport(outcomes: Vec<Result<(), ConnectionError>>) {
        let mut state = transport_state()
            .lock()
            .expect("failed to lock transport state");
        state.connect_outcomes = outcomes.into_iter().collect();
        state.connect_calls = 0;
        state.close_calls = 0;
    }

    fn connect_calls() -> u32 {
        transport_state()
            .lock()
            .expect("failed to lock transport state")
            .connect_calls
    }

    fn close_calls() -> u32 {
        transport_state()
            .lock()
            .expect("failed to lock transport state")
            .close_calls
    }

    fn config(max_reconnect_attempts: u32) -> ConnectionConfig {
        ConnectionConfig {
            jid: "alice@example.com".to_string(),
            password: "password".to_string(),
            server: Some("xmpp.example.com".to_string()),
            port: Some(5222),
            timeout_seconds: 30,
            max_reconnect_attempts,
        }
    }

    struct TestTransport;

    impl XmppTransport for TestTransport {
        async fn connect(_config: &ConnectionConfig) -> Result<Self, ConnectionError> {
            let mut state = transport_state()
                .lock()
                .expect("failed to lock transport state");
            state.connect_calls += 1;
            match state.connect_outcomes.pop_front().unwrap_or(Ok(())) {
                Ok(()) => Ok(Self),
                Err(error) => Err(error),
            }
        }

        async fn send(&mut self, _data: &[u8]) -> Result<(), ConnectionError> {
            Ok(())
        }

        async fn recv(&mut self) -> Result<Vec<u8>, ConnectionError> {
            Ok(Vec::new())
        }

        async fn close(&mut self) -> Result<(), ConnectionError> {
            let mut state = transport_state()
                .lock()
                .expect("failed to lock transport state");
            state.close_calls += 1;
            Ok(())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn connect_emits_established_and_transitions_to_connected() {
        let _guard = test_lock().lock().await;
        configure_transport(vec![Ok(())]);

        let event_bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::new(16));
        let mut established = event_bus
            .subscribe("system.connection.established")
            .expect("failed to subscribe established events");

        let mut manager =
            ConnectionManager::<TestTransport>::with_event_bus(config(0), event_bus.clone());
        manager.connect().await.expect("connect should succeed");

        assert_eq!(manager.state(), ConnectionState::Connected);
        assert_eq!(connect_calls(), 1);

        let event = time::timeout(Duration::from_millis(100), established.recv())
            .await
            .expect("timed out waiting for established event")
            .expect("failed to receive established event");
        assert_eq!(event.channel.as_str(), "system.connection.established");
        assert!(matches!(
            event.payload,
            EventPayload::ConnectionEstablished { jid } if jid == "alice@example.com"
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn authentication_failure_is_non_retryable() {
        let _guard = test_lock().lock().await;
        configure_transport(vec![Err(ConnectionError::AuthenticationFailed(
            "invalid credentials".to_string(),
        ))]);

        let event_bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::new(16));
        let mut lost = event_bus
            .subscribe("system.connection.lost")
            .expect("failed to subscribe lost events");
        let mut errors = event_bus
            .subscribe("system.error.occurred")
            .expect("failed to subscribe error events");

        let mut manager =
            ConnectionManager::<TestTransport>::with_event_bus(config(10), event_bus.clone());
        let result = manager.connect().await;

        assert!(matches!(
            result,
            Err(ConnectionError::AuthenticationFailed(_))
        ));
        assert_eq!(manager.state(), ConnectionState::Disconnected);
        assert_eq!(connect_calls(), 1);

        let lost_event = time::timeout(Duration::from_millis(100), lost.recv())
            .await
            .expect("timed out waiting for lost event")
            .expect("failed to receive lost event");
        assert!(matches!(
            lost_event.payload,
            EventPayload::ConnectionLost {
                will_retry: false,
                ..
            }
        ));

        let error_event = time::timeout(Duration::from_millis(100), errors.recv())
            .await
            .expect("timed out waiting for error event")
            .expect("failed to receive error event");
        assert!(matches!(
            error_event.payload,
            EventPayload::ErrorOccurred {
                recoverable: false,
                ..
            }
        ));
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn retryable_errors_emit_reconnecting_and_retry() {
        let _guard = test_lock().lock().await;
        configure_transport(vec![Err(ConnectionError::Timeout), Ok(())]);

        let event_bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::new(16));
        let mut reconnecting = event_bus
            .subscribe("system.connection.reconnecting")
            .expect("failed to subscribe reconnecting events");
        let mut lost = event_bus
            .subscribe("system.connection.lost")
            .expect("failed to subscribe lost events");
        let mut established = event_bus
            .subscribe("system.connection.established")
            .expect("failed to subscribe established events");

        let manager =
            ConnectionManager::<TestTransport>::with_event_bus(config(3), event_bus.clone());
        let connect_task = tokio::spawn(async move {
            let mut manager = manager;
            let result = manager.connect().await;
            (manager, result)
        });

        let reconnecting_event = reconnecting
            .recv()
            .await
            .expect("failed to receive reconnecting event");
        assert!(matches!(
            reconnecting_event.payload,
            EventPayload::ConnectionReconnecting { attempt: 1 }
        ));

        let lost_event = lost.recv().await.expect("failed to receive lost event");
        assert!(matches!(
            lost_event.payload,
            EventPayload::ConnectionLost {
                will_retry: true,
                ..
            }
        ));

        time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;

        let (manager, result) = connect_task.await.expect("connect task failed");
        result.expect("connect should succeed after retry");
        assert_eq!(manager.state(), ConnectionState::Connected);
        assert_eq!(connect_calls(), 2);

        let established_event = established
            .recv()
            .await
            .expect("failed to receive established event");
        assert!(matches!(
            established_event.payload,
            EventPayload::ConnectionEstablished { jid } if jid == "alice@example.com"
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disconnect_closes_transport_and_emits_lost_without_retry() {
        let _guard = test_lock().lock().await;
        configure_transport(vec![Ok(())]);

        let event_bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::new(16));
        let mut lost = event_bus
            .subscribe("system.connection.lost")
            .expect("failed to subscribe lost events");

        let mut manager =
            ConnectionManager::<TestTransport>::with_event_bus(config(0), event_bus.clone());
        manager.connect().await.expect("connect should succeed");
        manager
            .disconnect()
            .await
            .expect("disconnect should succeed");

        assert_eq!(manager.state(), ConnectionState::Disconnected);
        assert_eq!(close_calls(), 1);

        let first_lost_event = time::timeout(Duration::from_millis(100), lost.recv())
            .await
            .expect("timed out waiting for lost event")
            .expect("failed to receive lost event");
        assert!(matches!(
            first_lost_event.payload,
            EventPayload::ConnectionLost {
                will_retry: false,
                ..
            }
        ));
    }
}
