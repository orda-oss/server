use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    core::models::Role,
    schema::{channel_members, roles, server_members},
    utils::response::{ApiError, codes},
};

// Server-level permission flags stored in roles.permissions (i32 bitmask).
// Open-by-default: most actions need no flag. These only gate actions on
// *other people's* resources.
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Permissions: i32 {
        const ADMINISTRATOR     = 1 << 0;
        const MANAGE_CHANNELS   = 1 << 1;
        const MANAGE_MEMBERS    = 1 << 2;
        const MANAGE_MESSAGES   = 1 << 3;
        const MANAGE_SERVER     = 1 << 4;
    }
}

impl Permissions {
    pub fn from_role(role: &Role) -> Self {
        Self::from_bits_truncate(role.permissions)
    }
}

// Channel-level role stored as TEXT in channel_members.channel_role.
// NULL = regular member (can send, join voice, pin).
// Hierarchy: Manager > Moderator > Member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelRole {
    Moderator = 1,
    Manager = 2,
}

impl ChannelRole {
    pub fn from_str_opt(s: Option<&str>) -> Option<Self> {
        match s {
            Some("manager") => Some(Self::Manager),
            Some("moderator") => Some(Self::Moderator),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manager => "manager",
            Self::Moderator => "moderator",
        }
    }

    pub fn is_at_least(&self, required: ChannelRole) -> bool {
        *self >= required
    }
}

// Default role IDs (deterministic, seeded on server init)
pub const ROLE_ADMIN_ID: &str = "role-admin";
pub const ROLE_MOD_ID: &str = "role-mod";
pub const ROLE_MEMBER_ID: &str = "role-member";

// Check if a server-level permission is granted
pub fn has_server_permission(
    is_owner: bool,
    server_role: Option<&Role>,
    perm: Permissions,
) -> bool {
    if is_owner {
        return true;
    }
    let Some(role) = server_role else {
        return false;
    };
    let perms = Permissions::from_role(role);
    perms.contains(Permissions::ADMINISTRATOR) || perms.contains(perm)
}

// Check if user can manage a channel (edit, archive, delete)
// Allowed: server owner, ADMINISTRATOR, MANAGE_CHANNELS, channel creator, channel manager
pub fn can_manage_channel(
    is_owner: bool,
    server_role: Option<&Role>,
    channel_role: Option<ChannelRole>,
    is_creator: bool,
) -> bool {
    if is_owner || is_creator {
        return true;
    }
    if has_server_permission(false, server_role, Permissions::MANAGE_CHANNELS) {
        return true;
    }
    matches!(channel_role, Some(ChannelRole::Manager))
}

// Check if user can moderate a channel (delete others' messages, post in broadcast)
// Allowed: everything can_manage_channel allows + channel moderator + MANAGE_MESSAGES
pub fn can_moderate_channel(
    is_owner: bool,
    server_role: Option<&Role>,
    channel_role: Option<ChannelRole>,
    is_creator: bool,
) -> bool {
    if can_manage_channel(is_owner, server_role, channel_role, is_creator) {
        return true;
    }
    if has_server_permission(false, server_role, Permissions::MANAGE_MESSAGES) {
        return true;
    }
    matches!(channel_role, Some(r) if r.is_at_least(ChannelRole::Moderator))
}

// Check if user can manage members in a channel (add to private, remove from any)
pub fn can_manage_members(
    is_owner: bool,
    server_role: Option<&Role>,
    channel_role: Option<ChannelRole>,
    is_creator: bool,
) -> bool {
    if is_owner || is_creator {
        return true;
    }
    if has_server_permission(false, server_role, Permissions::MANAGE_MEMBERS) {
        return true;
    }
    matches!(channel_role, Some(ChannelRole::Manager))
}

// Sync helpers that load role data from DB

// Load the server-level Role for a user (if assigned)
pub fn load_server_role(conn: &mut SqliteConnection, user_id: &str) -> Option<Role> {
    use crate::core::models::Server;

    server_members::table
        .filter(server_members::user_id.eq(user_id))
        .filter(server_members::server_id.eq(Server::SINGLETON_ID))
        .select(server_members::role_id)
        .first::<Option<String>>(conn)
        .ok()
        .flatten()
        .and_then(|rid| roles::table.find(&rid).first::<Role>(conn).ok())
}

// Load the channel-level role for a user in a specific channel
pub fn load_channel_role(
    conn: &mut SqliteConnection,
    channel_id: &str,
    user_id: &str,
) -> Option<ChannelRole> {
    channel_members::table
        .filter(channel_members::channel_id.eq(channel_id))
        .filter(channel_members::user_id.eq(user_id))
        .select(channel_members::channel_role)
        .first::<Option<String>>(conn)
        .ok()
        .flatten()
        .and_then(|s| ChannelRole::from_str_opt(Some(&s)))
}

// Load the priority of the actor's assigned server role. Returns 0 when the
// user has no role (baseline). Owners don't have a role entry, so callers that
// check owner status should bypass this lookup entirely.
pub fn load_actor_priority(conn: &mut SqliteConnection, user_id: &str) -> i32 {
    load_server_role(conn, user_id)
        .and_then(|r| r.priority)
        .unwrap_or(0)
}

// Enforce the role hierarchy: the actor's own priority must be strictly
// greater than `target_priority`. Owner short-circuits to OK.
//
// Used by role CRUD and role assignment to prevent an administrator from
// editing or handing out a role whose priority matches or exceeds theirs.
// Strict inequality means a role can never modify itself via priority parity.
pub fn require_priority_above(
    conn: &mut SqliteConnection,
    actor_user_id: &str,
    is_owner: bool,
    target_priority: i32,
) -> Result<(), ApiError> {
    if is_owner {
        return Ok(());
    }
    let actor_priority = load_actor_priority(conn, actor_user_id);
    if actor_priority > target_priority {
        Ok(())
    } else {
        Err(ApiError::forbidden(codes::ERR_PRIORITY_BLOCKED))
    }
}

// Require a server-level permission or return ERR_MISSING_PERMISSION
pub fn require_server_permission(
    conn: &mut SqliteConnection,
    user_id: &str,
    is_owner: bool,
    perm: Permissions,
) -> Result<(), ApiError> {
    if is_owner {
        return Ok(());
    }
    let role = load_server_role(conn, user_id);
    if has_server_permission(false, role.as_ref(), perm) {
        Ok(())
    } else {
        Err(ApiError::forbidden(codes::ERR_MISSING_PERMISSION))
    }
}

// Require channel management capability or return ERR_MISSING_PERMISSION
pub fn require_channel_management(
    conn: &mut SqliteConnection,
    user_id: &str,
    channel_id: &str,
    is_owner: bool,
    is_creator: bool,
) -> Result<(), ApiError> {
    if is_owner || is_creator {
        return Ok(());
    }
    let server_role = load_server_role(conn, user_id);
    let channel_role = load_channel_role(conn, channel_id, user_id);
    if can_manage_channel(false, server_role.as_ref(), channel_role, false) {
        Ok(())
    } else {
        Err(ApiError::forbidden(codes::ERR_MISSING_PERMISSION))
    }
}

// Require channel moderation capability or return ERR_MISSING_PERMISSION
pub fn require_channel_moderation(
    conn: &mut SqliteConnection,
    user_id: &str,
    channel_id: &str,
    is_owner: bool,
    is_creator: bool,
) -> Result<(), ApiError> {
    if is_owner || is_creator {
        return Ok(());
    }
    let server_role = load_server_role(conn, user_id);
    let channel_role = load_channel_role(conn, channel_id, user_id);
    if can_moderate_channel(false, server_role.as_ref(), channel_role, false) {
        Ok(())
    } else {
        Err(ApiError::forbidden(codes::ERR_MISSING_PERMISSION))
    }
}

// Require channel member management capability or return ERR_MISSING_PERMISSION
pub fn require_member_management(
    conn: &mut SqliteConnection,
    user_id: &str,
    channel_id: &str,
    is_owner: bool,
    is_creator: bool,
) -> Result<(), ApiError> {
    if is_owner || is_creator {
        return Ok(());
    }
    let server_role = load_server_role(conn, user_id);
    let channel_role = load_channel_role(conn, channel_id, user_id);
    if can_manage_members(false, server_role.as_ref(), channel_role, false) {
        Ok(())
    } else {
        Err(ApiError::forbidden(codes::ERR_MISSING_PERMISSION))
    }
}
