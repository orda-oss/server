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

// Used if we add "Invite" logic later, or "Update Member Role"
#[allow(dead_code)]
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateMemberDto {
    pub role_id: Option<String>,
    // settings: Option<...>
}

/// Response shape for a single channel member. `#[serde(flatten)]` inlines all
/// `ChannelMember` fields at the top level of the JSON object so callers don't
/// need to unwrap a nested `member` key; `details` is kept as a named sub-object
/// to distinguish it from the membership fields.
#[derive(Debug, Serialize)]
pub struct ListMemberDto {
    // Flatten member fields (joined_at, role_id)
    #[serde(flatten)]
    pub member: ChannelMember,

    // Embed user info (username, avatar)
    pub details: UserSummary,
}

#[derive(Debug, Serialize)]
pub struct UserSummary {
    pub username: String,
    // Add avatar here later if you have it in metadata
}

impl From<(ChannelMember, User)> for ListMemberDto {
    fn from(tuple: (ChannelMember, User)) -> Self {
        let (member, user) = tuple;
        Self {
            member,
            details: UserSummary {
                username: user.username,
            },
        }
    }
}
