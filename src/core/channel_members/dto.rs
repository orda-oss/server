use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::core::models::{ChannelMember, User};

// Used when fetching members of a channel
#[derive(Debug, Deserialize, Validate)]
pub struct MemberFilterDto {
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<i64>,

    #[validate(range(min = 0))]
    pub offset: Option<i64>,
}

// Add a user to a channel (used for private channel member invites).
#[derive(Debug, Deserialize, Validate)]
pub struct AddMemberDto {
    pub user_id: String,
}

// Set channel-level role for a member
#[derive(Debug, Deserialize, Validate)]
pub struct ChannelRoleDto {
    /// "manager", "moderator", or null to clear
    pub role: Option<String>,
}

/// Response shape for a single channel member. `#[serde(flatten)]` inlines all
/// `ChannelMember` fields at the top level of the JSON object so callers don't
/// need to unwrap a nested `member` key; `details` is kept as a named sub-object
/// to distinguish it from the membership fields.
#[derive(Debug, Serialize)]
pub struct ListMemberDto {
    // Flatten member fields (joined_at, role_id, channel_role)
    #[serde(flatten)]
    pub member: ChannelMember,

    // Server-level role id from server_members (joined at query time). Distinct
    // from the unused channel_members.role_id and from channel_members.channel_role.
    pub server_role_id: Option<String>,

    // Embed user info (username, avatar)
    pub details: UserSummary,
}

#[derive(Debug, Serialize)]
pub struct UserSummary {
    pub username: String,
    // Add avatar here later if you have it in metadata
}

impl ListMemberDto {
    pub fn new(member: ChannelMember, user: User, server_role_id: Option<String>) -> Self {
        Self {
            member,
            server_role_id,
            details: UserSummary {
                username: user.username,
            },
        }
    }
}
