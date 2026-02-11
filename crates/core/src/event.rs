use chrono::{DateTime, Utc};
#[cfg(feature = "native")]
use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};
#[cfg(feature = "native")]
use tokio::sync::broadcast;
use uuid::Uuid;

/// Hierarchical channel name validation and parsing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Channel(String);

impl Channel {
    /// Create a new channel, validating its format.
    pub fn new(name: impl Into<String>) -> crate::Result<Self> {
        let name = name.into();
        if Self::is_valid(&name) {
            Ok(Self(name))
        } else {
            Err(crate::WaddleError::Internal(format!(
                "Invalid channel name: {}",
                name
            )))
        }
    }

    /// Check if a channel name is valid.
    pub fn is_valid(name: &str) -> bool {
        if name.is_empty() || name.starts_with('.') || name.ends_with('.') || name.contains("..") {
            return false;
        }

        // Must be lowercase and only contain a-z, 0-9, and dots
        if name
            .chars()
            .any(|c| !matches!(c, 'a'..='z' | '0'..='9' | '.'))
        {
            return false;
        }

        let parts: Vec<&str> = name.split('.').collect();
        if parts.is_empty() {
            return false;
        }

        // Check domain
        match parts[0] {
            "system" | "xmpp" | "ui" | "plugin" => {}
            _ => return false,
        }

        true
    }

    /// Get the domain of the channel.
    pub fn domain(&self) -> &str {
        self.0.split('.').next().unwrap_or("")
    }

    /// Get the full channel name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Channel> for String {
    fn from(channel: Channel) -> Self {
        channel.0
    }
}

/// The standard event envelope wrapping all events in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// Hierarchical channel name (e.g., "xmpp.message.received")
    pub channel: Channel,

    /// When the event was created (UTC)
    pub timestamp: DateTime<Utc>,

    /// Unique identifier for this event
    pub id: Uuid,

    /// Optional correlation ID linking related events (e.g., request-response)
    pub correlation_id: Option<Uuid>,

    /// Source component that emitted this event
    pub source: EventSource,

    /// The typed event payload
    pub payload: EventPayload,
}

impl Event {
    /// Create a new event with a given channel and payload.
    pub fn new(channel: Channel, source: EventSource, payload: EventPayload) -> Self {
        Self {
            channel,
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            correlation_id: None,
            source,
            payload,
        }
    }

    /// Create a new event with a correlation ID.
    pub fn with_correlation(
        channel: Channel,
        source: EventSource,
        payload: EventPayload,
        correlation_id: Uuid,
    ) -> Self {
        Self {
            channel,
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            correlation_id: Some(correlation_id),
            source,
            payload,
        }
    }
}

/// Identifies the source of an event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "camelCase")]
pub enum EventSource {
    /// Core system component
    System(String),
    /// XMPP subsystem
    Xmpp,
    /// User interface
    Ui(UiTarget),
    /// Plugin with its ID
    Plugin(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiTarget {
    Tui,
    Gui,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum EventPayload {
    // ── System events ──────────────────────────────────────────────
    StartupComplete,
    ShutdownRequested {
        reason: String,
    },
    ConnectionEstablished {
        jid: String,
    },
    ConnectionLost {
        reason: String,
        will_retry: bool,
    },
    ConnectionReconnecting {
        attempt: u32,
    },
    GoingOffline,
    ComingOnline,
    SyncStarted,
    SyncCompleted {
        messages_synced: u64,
    },
    ConfigReloaded,
    ErrorOccurred {
        component: String,
        message: String,
        recoverable: bool,
    },

    // ── XMPP Roster events ────────────────────────────────────────
    RosterReceived {
        items: Vec<RosterItem>,
    },
    RosterUpdated {
        item: RosterItem,
    },
    RosterRemoved {
        jid: String,
    },
    SubscriptionRequest {
        from: String,
    },
    SubscriptionApproved {
        jid: String,
    },
    SubscriptionRevoked {
        jid: String,
    },

    // ── XMPP Presence events ──────────────────────────────────────
    PresenceChanged {
        jid: String,
        show: PresenceShow,
        status: Option<String>,
    },
    OwnPresenceChanged {
        show: PresenceShow,
        status: Option<String>,
    },

    // ── XMPP Message events ──────────────────────────────────────
    MessageReceived {
        message: ChatMessage,
    },
    MessageSent {
        message: ChatMessage,
    },
    MessageDelivered {
        id: String,
        to: String,
    },
    ChatStateReceived {
        from: String,
        state: ChatState,
    },
    MucMessageReceived {
        room: String,
        message: ChatMessage,
    },
    MucJoined {
        room: String,
        nick: String,
    },
    MucLeft {
        room: String,
    },
    MucSubjectChanged {
        room: String,
        subject: String,
    },
    MucOccupantChanged {
        room: String,
        occupant: MucOccupant,
    },

    // ── XMPP MAM events ──────────────────────────────────────────
    MamResultReceived {
        query_id: String,
        messages: Vec<ChatMessage>,
        complete: bool,
    },

    // ── XMPP Debug events ────────────────────────────────────────
    RawStanzaReceived {
        stanza: String,
    },
    RawStanzaSent {
        stanza: String,
    },

    // ── UI events ────────────────────────────────────────────────
    ConversationOpened {
        jid: String,
    },
    ConversationClosed {
        jid: String,
    },
    ScrollRequested {
        jid: String,
        direction: ScrollDirection,
    },
    ComposeStarted {
        jid: String,
    },
    SearchRequested {
        query: String,
    },
    ThemeChanged {
        theme_id: String,
    },
    NotificationClicked {
        event_id: String,
    },

    // ── Plugin events ────────────────────────────────────────────
    PluginLoaded {
        plugin_id: String,
        version: String,
    },
    PluginUnloaded {
        plugin_id: String,
    },
    PluginError {
        plugin_id: String,
        error: String,
    },
    PluginCustomEvent {
        plugin_id: String,
        event_type: String,
        data: serde_json::Value,
    },
    PluginInstallStarted {
        plugin_id: String,
    },
    PluginInstallCompleted {
        plugin_id: String,
    },
}

/// A single entry in the XMPP roster.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterItem {
    /// The contact's bare JID (e.g., "alice@example.com")
    pub jid: String,

    /// Display name set by the user, if any
    pub name: Option<String>,

    /// Roster subscription state
    pub subscription: Subscription,

    /// User-defined groups this contact belongs to
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Subscription {
    None,
    To,
    From,
    Both,
    Remove,
}

/// A chat message (1:1 or MUC).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// Server-assigned or client-generated unique message ID
    pub id: String,

    /// Bare JID of the sender
    pub from: String,

    /// Bare JID of the recipient (or room JID for MUC)
    pub to: String,

    /// Plain-text message body
    pub body: String,

    /// When the message was sent (UTC)
    pub timestamp: DateTime<Utc>,

    /// Message type
    pub message_type: MessageType,

    /// Thread ID for conversation threading, if present
    pub thread: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageType {
    Chat,
    Groupchat,
    Normal,
    Headline,
    Error,
}

/// XMPP presence "show" values (RFC 6121 section 4.7.2.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PresenceShow {
    /// Available (no <show/> element -- the default)
    Available,
    /// Free for chat
    Chat,
    /// Away
    Away,
    /// Extended away
    Xa,
    /// Do not disturb
    Dnd,
    /// Unavailable (offline)
    Unavailable,
}

/// XEP-0085 Chat State Notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatState {
    Active,
    Composing,
    Paused,
    Inactive,
    Gone,
}

/// An occupant in a MUC room.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MucOccupant {
    /// The occupant's room nick
    pub nick: String,

    /// The occupant's real JID, if visible
    pub jid: Option<String>,

    /// MUC affiliation
    pub affiliation: MucAffiliation,

    /// MUC role
    pub role: MucRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MucAffiliation {
    Owner,
    Admin,
    Member,
    Outcast,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MucRole {
    Moderator,
    Participant,
    Visitor,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScrollDirection {
    Up,
    Down,
    Top,
    Bottom,
}

#[cfg(feature = "native")]
pub trait EventBus: Send + Sync + 'static {
    fn publish(&self, event: Event) -> std::result::Result<(), crate::error::EventBusError>;
    fn subscribe(
        &self,
        pattern: &str,
    ) -> std::result::Result<EventSubscription, crate::error::EventBusError>;
}

#[cfg(feature = "native")]
#[derive(Clone)]
pub struct BroadcastEventBus {
    system_sender: broadcast::Sender<Event>,
    xmpp_sender: broadcast::Sender<Event>,
    ui_sender: broadcast::Sender<Event>,
    plugin_sender: broadcast::Sender<Event>,
}

#[cfg(feature = "native")]
impl BroadcastEventBus {
    pub const DEFAULT_CHANNEL_CAPACITY: usize = 1024;

    pub fn new(channel_capacity: usize) -> Self {
        let capacity = channel_capacity.max(1);
        let (system_sender, _) = broadcast::channel(capacity);
        let (xmpp_sender, _) = broadcast::channel(capacity);
        let (ui_sender, _) = broadcast::channel(capacity);
        let (plugin_sender, _) = broadcast::channel(capacity);

        Self {
            system_sender,
            xmpp_sender,
            ui_sender,
            plugin_sender,
        }
    }

    fn sender_for_domain(&self, domain: &str) -> Option<&broadcast::Sender<Event>> {
        match domain {
            "system" => Some(&self.system_sender),
            "xmpp" => Some(&self.xmpp_sender),
            "ui" => Some(&self.ui_sender),
            "plugin" => Some(&self.plugin_sender),
            _ => None,
        }
    }

    fn receivers_for_pattern(
        &self,
        pattern: &str,
    ) -> std::result::Result<DomainReceivers, crate::error::EventBusError> {
        let first_segment = pattern.split('.').next().unwrap_or_default();

        if first_segment.is_empty() {
            return Err(crate::error::EventBusError::InvalidPattern(
                pattern.to_string(),
            ));
        }

        if has_glob_meta(first_segment) {
            return Ok(DomainReceivers {
                system: Some(self.system_sender.subscribe()),
                xmpp: Some(self.xmpp_sender.subscribe()),
                ui: Some(self.ui_sender.subscribe()),
                plugin: Some(self.plugin_sender.subscribe()),
            });
        }

        match first_segment {
            "system" => Ok(DomainReceivers {
                system: Some(self.system_sender.subscribe()),
                xmpp: None,
                ui: None,
                plugin: None,
            }),
            "xmpp" => Ok(DomainReceivers {
                system: None,
                xmpp: Some(self.xmpp_sender.subscribe()),
                ui: None,
                plugin: None,
            }),
            "ui" => Ok(DomainReceivers {
                system: None,
                xmpp: None,
                ui: Some(self.ui_sender.subscribe()),
                plugin: None,
            }),
            "plugin" => Ok(DomainReceivers {
                system: None,
                xmpp: None,
                ui: None,
                plugin: Some(self.plugin_sender.subscribe()),
            }),
            _ => Err(crate::error::EventBusError::InvalidPattern(
                pattern.to_string(),
            )),
        }
    }
}

#[cfg(feature = "native")]
impl Default for BroadcastEventBus {
    fn default() -> Self {
        Self::new(Self::DEFAULT_CHANNEL_CAPACITY)
    }
}

#[cfg(feature = "native")]
impl EventBus for BroadcastEventBus {
    fn publish(&self, event: Event) -> std::result::Result<(), crate::error::EventBusError> {
        let sender = self
            .sender_for_domain(event.channel.domain())
            .ok_or_else(|| {
                crate::error::EventBusError::InvalidChannel(event.channel.to_string())
            })?;

        let _ = sender.send(event);
        Ok(())
    }

    fn subscribe(
        &self,
        pattern: &str,
    ) -> std::result::Result<EventSubscription, crate::error::EventBusError> {
        let matcher = Glob::new(pattern)
            .map_err(|_| crate::error::EventBusError::InvalidPattern(pattern.to_string()))?
            .compile_matcher();
        let receivers = self.receivers_for_pattern(pattern)?;

        Ok(EventSubscription { matcher, receivers })
    }
}

#[cfg(feature = "native")]
struct DomainReceivers {
    system: Option<broadcast::Receiver<Event>>,
    xmpp: Option<broadcast::Receiver<Event>>,
    ui: Option<broadcast::Receiver<Event>>,
    plugin: Option<broadcast::Receiver<Event>>,
}

#[cfg(feature = "native")]
pub struct EventSubscription {
    matcher: GlobMatcher,
    receivers: DomainReceivers,
}

#[cfg(feature = "native")]
impl EventSubscription {
    pub async fn recv(&mut self) -> std::result::Result<Event, crate::error::EventBusError> {
        loop {
            let system_receiver = self.receivers.system.as_mut();
            let xmpp_receiver = self.receivers.xmpp.as_mut();
            let ui_receiver = self.receivers.ui.as_mut();
            let plugin_receiver = self.receivers.plugin.as_mut();

            let received = tokio::select! {
                result = recv_from_domain(system_receiver) => result,
                result = recv_from_domain(xmpp_receiver) => result,
                result = recv_from_domain(ui_receiver) => result,
                result = recv_from_domain(plugin_receiver) => result,
            };

            match received {
                Ok(event) if self.matcher.is_match(event.channel.as_str()) => return Ok(event),
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(crate::error::EventBusError::ChannelClosed);
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    return Err(crate::error::EventBusError::Lagged(count));
                }
            }
        }
    }
}

#[cfg(feature = "native")]
async fn recv_from_domain(
    receiver: Option<&mut broadcast::Receiver<Event>>,
) -> std::result::Result<Event, broadcast::error::RecvError> {
    match receiver {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

#[cfg(feature = "native")]
fn has_glob_meta(segment: &str) -> bool {
    segment.contains('*')
        || segment.contains('?')
        || segment.contains('[')
        || segment.contains(']')
        || segment.contains('{')
        || segment.contains('}')
        || segment.contains('!')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_validation() {
        assert!(Channel::is_valid("system.startup.complete"));
        assert!(Channel::is_valid("xmpp.message.received"));
        assert!(Channel::is_valid("ui.theme.changed"));
        assert!(Channel::is_valid("plugin.test.event"));

        assert!(!Channel::is_valid("invalid.domain.event"));
        assert!(!Channel::is_valid("system..double.dot"));
        assert!(!Channel::is_valid(".starts.with.dot"));
        assert!(!Channel::is_valid("ends.with.dot."));
        assert!(!Channel::is_valid("UpperCase"));
        assert!(!Channel::is_valid("with-hyphen"));
        assert!(!Channel::is_valid(""));
    }

    #[test]
    fn test_channel_domain() {
        let c = Channel::new("xmpp.message.received").unwrap();
        assert_eq!(c.domain(), "xmpp");
    }
}
