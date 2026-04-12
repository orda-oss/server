pub mod types;

use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
};

use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc};
pub use types::*;

/// How many unread messages can queue up for a single slow receiver before
/// it starts receiving `Lagged` errors and missing messages. Active sockets
/// drain this immediately - the buffer only matters during bursts or transient
/// hangs. 512 x ~500 bytes = 256KB worst-case per channel, negligible at idle.
const CHANNEL_CAPACITY: usize = 512;

/// In-memory pub/sub hub for real-time messaging.
///
/// Uses DashMap (sharded concurrent HashMap) instead of RwLock<HashMap> so
/// unrelated keys never contend - important when many users connect/disconnect
/// or join/leave channels concurrently.
#[derive(Debug)]
pub struct Satellite {
    /// Server-wide broadcast for events that all connected clients should receive
    /// (e.g., channel_created, channel_updated, channel_deleted).
    server_broadcast: broadcast::Sender<String>,
    /// One broadcast sender per channel. Cloning a sender is a cheap Arc bump.
    /// Created lazily on first access; intentionally never removed.
    channel_broadcast: DashMap<String, broadcast::Sender<String>>,
    /// One mpsc sender per connected user, pointing into their WS send-task.
    /// Inserted on WS connect, removed on disconnect.
    user_sessions: DashMap<String, mpsc::UnboundedSender<UserCommand>>,
    /// In-memory tracker: channel_id -> set of user_ids currently in voice.
    /// Ephemeral - does not survive server restarts.
    voice_participants: DashMap<String, HashSet<String>>,
    /// Sticky voice flags: "channel_id:user_id" -> u8 bitfield.
    /// Bit 0 (0x01) = muted, Bit 1 (0x02) = deafened.
    /// 00=neither, 01=muted, 10=deafened, 11=muted+deafened.
    /// Only set on explicit mute/deafen events, cleared on leave.
    voice_sticky_status_flags: DashMap<String, u8>,
    /// In-memory tracker: channel_id -> user_id currently sharing screen.
    /// Ephemeral - does not survive server restarts.
    active_screenshares: DashMap<String, String>,
    /// User presence status: user_id -> UserStatus (Online, Away, Busy, Offline).
    /// Ephemeral - cleared on disconnect.
    user_statuses: DashMap<String, crate::core::models::UserStatus>,
    /// Recently synced user IDs (from JWT upsert). Avoids DB upsert on every request.
    synced_users: DashMap<String, ()>,
    /// Maintenance mode. When true, write endpoints return 503.
    maintenance: AtomicBool,
}

impl Satellite {
    pub fn new() -> Self {
        let (server_tx, _) = broadcast::channel(CHANNEL_CAPACITY);

        Self {
            server_broadcast: server_tx,
            channel_broadcast: DashMap::new(),
            user_sessions: DashMap::new(),
            voice_participants: DashMap::new(),
            voice_sticky_status_flags: DashMap::new(),
            active_screenshares: DashMap::new(),
            maintenance: AtomicBool::new(false),
            user_statuses: DashMap::new(),
            synced_users: DashMap::new(),
        }
    }

    // Broadcasting

    /// Returns a receiver for server-wide events. Each WS connection subscribes once.
    pub fn subscribe_server(&self) -> broadcast::Receiver<String> {
        self.server_broadcast.subscribe()
    }

    /// Broadcast a server-wide event to all connected clients.
    pub fn broadcast_server(&self, event: &ServerEvent) {
        match serde_json::to_string(event) {
            Ok(json) => {
                let _ = self.server_broadcast.send(json);
            }
            Err(e) => {
                tracing::error!(error = ?e, "Failed to serialize ServerEvent");
            }
        }
    }

    /// Broadcast a channel-scoped event to all subscribers of the given channel.
    pub fn broadcast_channel(&self, channel_id: &str, event: &ChannelEvent) {
        match serde_json::to_string(event) {
            Ok(json) => {
                let tx = self.get_channel_sender(channel_id);
                let _ = tx.send(json);
            }
            Err(e) => {
                tracing::error!(error = ?e, "Failed to serialize ChannelEvent");
            }
        }
    }

    /// Returns a Sender for the given channel_id, creating one if needed.
    /// `entry().or_insert_with()` is atomic at the shard level - no TOCTOU race.
    pub fn get_channel_sender(&self, channel_id: &str) -> broadcast::Sender<String> {
        self.channel_broadcast
            .entry(channel_id.to_string())
            .or_insert_with(|| {
                let (tx, _rx) = broadcast::channel(CHANNEL_CAPACITY);
                tx
            })
            .clone()
    }

    /// Removes a channel's broadcast sender (e.g., after channel deletion).
    pub fn remove_channel_sender(&self, channel_id: &str) {
        self.channel_broadcast.remove(channel_id);
    }

    // Session tracking

    /// Called when a user's WebSocket connection opens.
    pub fn register_session(&self, user_id: &str, tx: mpsc::UnboundedSender<UserCommand>) {
        self.user_sessions.insert(user_id.to_string(), tx);
    }

    /// Called when a user's WebSocket connection closes.
    pub fn unregister_session(&self, user_id: &str) {
        self.user_sessions.remove(user_id);
    }

    /// Push a command to a user's active WS send-task.
    /// Silently no-ops if the user has no active connection.
    pub fn send_user_command(&self, user_id: &str, cmd: UserCommand) {
        if let Some(tx) = self.user_sessions.get(user_id) {
            let _ = tx.send(cmd);
        }
    }

    // Voice participant tracking

    /// Adds a user to a voice channel's participant set.
    /// Returns `true` if this was a new join (user wasn't already in voice).
    pub fn voice_join(&self, channel_id: &str, user_id: &str) -> bool {
        self.voice_participants
            .entry(channel_id.to_string())
            .or_default()
            .insert(user_id.to_string())
    }

    /// Removes a user from a voice channel's participant set.
    /// Returns `true` if the user was actually present.
    pub fn voice_leave(&self, channel_id: &str, user_id: &str) -> bool {
        let key = format!("{}:{}", channel_id, user_id);
        self.voice_sticky_status_flags.remove(&key);
        if let Some(mut set) = self.voice_participants.get_mut(channel_id) {
            set.remove(user_id)
        } else {
            false
        }
    }

    /// Removes a user from ALL voice channels (used on WS disconnect).
    /// Returns a list of channel_ids the user was removed from.
    pub fn voice_leave_all(&self, user_id: &str) -> Vec<String> {
        let mut left = Vec::new();
        for mut entry in self.voice_participants.iter_mut() {
            if entry.value_mut().remove(user_id) {
                let key = format!("{}:{}", entry.key(), user_id);
                self.voice_sticky_status_flags.remove(&key);
                left.push(entry.key().clone());
            }
        }
        left
    }

    /// Returns the list of user_ids currently in voice for a channel.
    pub fn voice_participants(&self, channel_id: &str) -> Vec<String> {
        self.voice_participants
            .get(channel_id)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Update sticky voice flags for a participant.
    /// Bit 0 (0x01) = muted, Bit 1 (0x02) = deafened.
    pub fn voice_sticky_status_flags_set(&self, channel_id: &str, user_id: &str, status: &str) {
        let key = format!("{}:{}", channel_id, user_id);
        let mut flags = self.voice_sticky_status_flags.entry(key).or_insert(0);

        match status {
            "muted" => *flags |= 0x01,
            "deafened" => *flags |= 0x02,
            "undeafened" => *flags &= !0x02,
            "talking" | "idle" => *flags &= !0x01,
            _ => {}
        }
    }

    /// Returns sticky status flags for all participants in a channel.
    /// Key: user_id, Value: u8 flags (bit 0=muted, bit 1=deafened).
    pub fn voice_sticky_status_flags_get(&self, channel_id: &str) -> HashMap<String, u8> {
        let prefix = format!("{}:", channel_id);

        self.voice_sticky_status_flags
            .iter()
            .filter(|e| e.key().starts_with(&prefix))
            .filter(|e| *e.value() > 0)
            .map(|e| {
                let uid = e.key()[prefix.len()..].to_string();
                (uid, *e.value())
            })
            .collect()
    }

    /// Clears ALL voice participants (used when LiveKit goes down).
    pub fn voice_clear_all(&self) {
        self.voice_participants.clear();
        self.voice_sticky_status_flags.clear();
    }

    /// Returns a map of channel_id -> participant count for all channels with active voice.
    pub fn voice_counts(&self) -> HashMap<String, usize> {
        self.voice_participants
            .iter()
            .filter(|e| !e.value().is_empty())
            .map(|e| (e.key().clone(), e.value().len()))
            .collect()
    }

    // Screenshare tracking

    /// Claims the screenshare slot for a channel.
    /// Returns Ok(()) if claimed (or already held by same user),
    /// Err(holder_user_id) if another user is already sharing.
    pub fn screenshare_start(&self, channel_id: &str, user_id: &str) -> Result<(), String> {
        use dashmap::mapref::entry::Entry;

        match self.active_screenshares.entry(channel_id.to_string()) {
            Entry::Occupied(e) => {
                if e.get() == user_id {
                    Ok(())
                } else {
                    Err(e.get().clone())
                }
            }
            Entry::Vacant(e) => {
                e.insert(user_id.to_string());

                Ok(())
            }
        }
    }

    /// Releases the screenshare slot. Only the holder can release.
    /// Returns true if the user was actually sharing.
    pub fn screenshare_stop(&self, channel_id: &str, user_id: &str) -> bool {
        if let Some(entry) = self.active_screenshares.get(channel_id)
            && entry.value() == user_id
        {
            drop(entry);
            self.active_screenshares.remove(channel_id);

            return true;
        }

        false
    }

    /// Returns the user_id of the current screensharer for a channel, if any.
    pub fn screenshare_get(&self, channel_id: &str) -> Option<String> {
        self.active_screenshares
            .get(channel_id)
            .map(|e| e.value().clone())
    }

    /// Clears screenshare for a user across all channels (used on WS disconnect).
    /// Returns channel_ids that were cleared.
    pub fn screenshare_clear_user(&self, user_id: &str) -> Vec<String> {
        let mut cleared = Vec::new();

        self.active_screenshares.retain(|channel_id, holder| {
            if holder == user_id {
                cleared.push(channel_id.clone());

                false
            } else {
                true
            }
        });

        cleared
    }

    /// Clears ALL screenshare slots (used when LiveKit goes down).
    pub fn screenshare_clear_all(&self) {
        self.active_screenshares.clear();
    }

    // User sync tracking

    /// Returns true if the user was recently synced (avoids DB upsert on every request).
    pub fn is_user_synced(&self, user_id: &str) -> bool {
        self.synced_users.contains_key(user_id)
    }

    /// Marks a user as recently synced.
    pub fn mark_user_synced(&self, user_id: &str) {
        self.synced_users.insert(user_id.to_string(), ());
    }

    /// Clears sync cache for a user so the next request triggers a full re-sync.
    pub fn clear_user_synced(&self, user_id: &str) {
        self.synced_users.remove(user_id);
    }

    /// Clears the entire sync cache. Called periodically so stale entries from
    /// users who disconnected long ago don't accumulate forever.
    pub fn clear_all_synced(&self) {
        self.synced_users.clear();
    }

    // User presence

    pub fn set_user_status(&self, user_id: &str, status: crate::core::models::UserStatus) {
        self.user_statuses.insert(user_id.to_string(), status);
    }

    pub fn remove_user_status(&self, user_id: &str) {
        self.user_statuses.remove(user_id);
    }

    /// Returns all connected users with their current status.
    pub fn user_presence(&self) -> HashMap<String, crate::core::models::UserStatus> {
        self.user_sessions
            .iter()
            .map(|e| {
                let uid = e.key().clone();
                let status = self
                    .user_statuses
                    .get(&uid)
                    .map(|s| s.value().clone())
                    .unwrap_or(crate::core::models::UserStatus::Online);
                (uid, status)
            })
            .collect()
    }

    // Maintenance mode

    pub fn set_maintenance(&self, enabled: bool) {
        self.maintenance.store(enabled, Ordering::Relaxed);
    }

    pub fn is_maintenance(&self) -> bool {
        self.maintenance.load(Ordering::Relaxed)
    }
}

impl Default for Satellite {
    fn default() -> Self {
        Self::new()
    }
}
