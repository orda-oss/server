use serde::{Deserialize, Serialize};

use crate::core::models::{Channel, Message, UserStatus};

/// Voice mic status broadcast by each participant.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceStatusKind {
    Muted,
    Talking,
    Idle,
    Deafened,
    Undeafened,
}

/// Commands routed from anywhere in the system into a user's active WS send-task.
pub enum UserCommand {
    /// Start forwarding broadcast messages from this channel to the user's socket.
    Subscribe(String),
    /// Stop forwarding messages from this channel.
    Unsubscribe(String),
    /// Send a private, non-persisted message directly to this user's socket.
    SendEphemeral(String),
    /// Close the WebSocket with an application-specific close code.
    Disconnect(String),
}

/// Inbound: frames the client sends to the server over the WebSocket.
#[derive(Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ClientPayload {
    SendMessage {
        channel_id: String,
        content: String,
    },
    TypingEvent {
        channel_id: String,
    },
    TypingStop {
        channel_id: String,
    },
    VoiceStatus {
        channel_id: String,
        status: VoiceStatusKind,
    },
    UserStatus {
        status: UserStatus,
    },
}

/// Outbound (server-wide): events broadcast to every connected client.
#[allow(dead_code)]
#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ServerEvent {
    MemberJoined {
        user_id: String,
    },
    MemberLeft {
        user_id: String,
    },
    ChannelCreated {
        channel: Channel,
    },
    ChannelUpdated {
        channel: Channel,
    },
    ChannelDeleted {
        channel_id: String,
    },
    LivekitStatus {
        reachable: bool,
    },
    UserOnline {
        user_id: String,
    },
    UserOffline {
        user_id: String,
    },
    UserUpdated {
        user_id: String,
        username: String,
        discriminator: i32,
        staff: bool,
    },
    UserStatusChanged {
        user_id: String,
        status: UserStatus,
    },
    MaintenanceStarted,
    MaintenanceEnded,
}

/// Outbound (channel-scoped): events broadcast to all subscribers of a specific channel.
#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ChannelEvent {
    SendMessage {
        message: Message,
    },
    MessageDeleted {
        channel_id: String,
        message_id: String,
    },
    MessageRestored {
        message: Message,
    },
    MessageUpdated {
        message: Message,
    },
    MemberJoined {
        channel_id: String,
        user_id: String,
    },
    MemberLeft {
        channel_id: String,
        user_id: String,
    },
    TypingEvent {
        channel_id: String,
        user_id: String,
    },
    TypingStop {
        channel_id: String,
        user_id: String,
    },
    VoiceJoined {
        channel_id: String,
        user_id: String,
    },
    VoiceLeft {
        channel_id: String,
        user_id: String,
    },
    VoiceStatus {
        channel_id: String,
        user_id: String,
        status: VoiceStatusKind,
    },
    ScreenshareStarted {
        channel_id: String,
        user_id: String,
    },
    ScreenshareStopped {
        channel_id: String,
    },
}
