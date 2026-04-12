use diesel::{prelude::*, sqlite::Sqlite};
use serde::Serialize;

use super::{enums::*, metadata::*};
use crate::{core::types::SqliteJson, schema::*};

// --- Database models ---
//
// Each struct maps 1:1 to a table via Diesel's derive macros.
// `Queryable` = can be loaded from a SELECT result.
// `Insertable` = can be passed to INSERT INTO.
// `Selectable` = enables `.select(Model::as_select())` for type-safe column sets.
// `Identifiable` = provides `.find(pk)` and `belongs_to` join support.

#[derive(Queryable, Insertable, Selectable, Identifiable, Debug, Serialize)]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = users)]
pub struct User {
    pub id: String,
    pub remote_id: String,
    pub username: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub discriminator: i32,
    pub staff: bool,
}

#[derive(Queryable, Insertable, Selectable, Identifiable, Debug, Clone, Serialize)]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = servers)]
pub struct Server {
    pub id: String,
    pub remote_id: Option<String>,
    pub name: String,
    pub metadata: SqliteJson<ServerMetadata>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub last_event_cursor: Option<i32>,
    pub cert_version: Option<i32>,
}

#[derive(Queryable, Insertable, Selectable, Identifiable, Associations, Debug, Serialize)]
#[diesel(belongs_to(Server))]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = roles)]
pub struct Role {
    pub id: String,
    pub server_id: String,
    pub name: String,
    /// Bitmask of permissions. Use `bitflags` to decode individual flags.
    pub permissions: i32,
    pub priority: Option<i32>,
    pub color: Option<i32>,
    pub is_mentionable: Option<bool>,
    pub metadata: SqliteJson<RoleMetadata>,
    pub created_by: String,
    pub created_at: Option<String>,
}

#[derive(Queryable, Insertable, Selectable, Identifiable, Associations, Debug, Serialize)]
#[diesel(belongs_to(Server))]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = groups)]
pub struct Group {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub description: Option<String>,
    pub is_mentionable: Option<bool>,
    pub created_by: String,
    pub created_at: Option<String>,
}

#[derive(Queryable, Insertable, Selectable, Identifiable, Associations, Debug, Serialize)]
#[diesel(belongs_to(Server))]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = server_members)]
#[diesel(primary_key(server_id, user_id))]
pub struct ServerMember {
    pub server_id: String,
    pub user_id: String,
    pub role_id: Option<String>,
    pub nickname: Option<String>,
    pub metadata: SqliteJson<serde_json::Value>,
    pub joined_at: Option<String>,
}

#[derive(Queryable, Insertable, Selectable, Identifiable, Associations, Debug, Serialize)]
#[diesel(belongs_to(Group))]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = group_members)]
#[diesel(primary_key(group_id, user_id))]
pub struct GroupMember {
    pub group_id: String,
    pub user_id: String,
    pub added_by: String,
    pub added_at: Option<String>,
}

#[derive(
    Queryable, Insertable, Selectable, Identifiable, Associations, Debug, Serialize, Clone,
)]
#[diesel(belongs_to(Server))]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = channels)]
pub struct Channel {
    pub id: String,
    pub server_id: String,
    pub name: String,
    /// URL-safe version of `name`, used for routing and display.
    pub slug: String,
    pub kind: ChannelKind,
    pub is_default: Option<bool>,
    pub is_private: Option<bool>,
    pub is_archived: Option<bool>,
    pub is_nsfw: Option<bool>,
    pub pin_limit: Option<i32>,
    pub metadata: SqliteJson<ChannelMetadata>,
    pub created_by: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Queryable, Insertable, Selectable, Identifiable, Associations, Debug, Serialize)]
#[diesel(belongs_to(Channel))]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = channel_members)]
#[diesel(primary_key(channel_id, user_id))]
pub struct ChannelMember {
    pub channel_id: String,
    pub user_id: String,
    pub role_id: Option<String>,
    pub added_by: Option<String>,
    /// Per-user channel preferences (mute, notifications, etc.) stored as freeform JSON.
    pub settings: SqliteJson<serde_json::Value>,
    pub joined_at: Option<String>,
    pub last_read_message_id: Option<String>,
}

#[derive(
    Queryable, Insertable, Selectable, Identifiable, Associations, Debug, Clone, Serialize,
)]
#[diesel(belongs_to(Channel))]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = messages)]
pub struct Message {
    pub id: String,
    pub channel_id: String,
    pub sender_id: String,
    pub content: String,
    pub kind: MessageKind,
    pub is_repliable: Option<bool>,
    pub is_reactable: Option<bool>,
    pub is_pinned: Option<bool>,
    /// Top-level message that started a thread this message belongs to.
    pub root_thread_id: Option<String>,
    /// Direct parent in a nested reply chain.
    pub parent_id: Option<String>,
    /// If this is a cross-post, the ID of the original message.
    pub origin_message_id: Option<String>,
    pub deleted_at: Option<String>,
    pub updated_at: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Queryable, Insertable, Selectable, Identifiable, Associations, Debug, Serialize)]
#[diesel(belongs_to(Message))]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = reactions)]
#[diesel(primary_key(message_id, emoji, user_id))]
pub struct Reaction {
    pub message_id: String,
    pub user_id: String,
    pub emoji: String,
    pub created_at: Option<String>,
}

#[derive(Queryable, Insertable, Selectable, Identifiable, Associations, Debug, Serialize)]
#[diesel(belongs_to(Channel))]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = channel_pins)]
#[diesel(primary_key(channel_id, message_id))]
pub struct ChannelPin {
    pub channel_id: String,
    pub message_id: String,
    pub pinned_by: String,
    pub pinned_at: Option<String>,
}

#[derive(Queryable, Insertable, Selectable, Identifiable, Debug, Serialize)]
#[diesel(check_for_backend(Sqlite))]
#[diesel(table_name = notifications)]
pub struct Notification {
    pub id: String,
    pub user_id: String,
    pub sender_id: Option<String>,
    pub kind: NotificationKind,
    /// Generic foreign key - points to the relevant message, channel, etc.
    /// depending on `kind`.
    pub reference_id: Option<String>,
    pub is_read: Option<bool>,
    pub created_at: Option<String>,
}
