use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::core::models::metadata::RoleMetadata;

#[derive(Debug, Deserialize, Validate)]
pub struct CreateRoleDto {
    #[validate(length(min = 2, max = 32))]
    pub name: String,
    pub permissions: i32,
    pub priority: Option<i32>,
    pub color: Option<i32>,
    pub is_mentionable: Option<bool>,
    pub metadata: Option<RoleMetadata>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateRoleDto {
    #[validate(length(min = 2, max = 32))]
    pub name: Option<String>,
    pub permissions: Option<i32>,
    pub priority: Option<i32>,
    pub color: Option<i32>,
    pub is_mentionable: Option<bool>,
    pub metadata: Option<RoleMetadata>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct AssignRoleDto {
    pub role_id: Option<String>,
}

/// Response shape for `GET /members`: one row per server member with enough
/// user detail for a roster view plus role_id for the role picker.
#[derive(Debug, Serialize)]
pub struct ServerMemberDto {
    pub user_id: String,
    pub username: String,
    pub discriminator: i32,
    pub staff: bool,
    pub role_id: Option<String>,
    pub nickname: Option<String>,
    pub joined_at: Option<String>,
}
