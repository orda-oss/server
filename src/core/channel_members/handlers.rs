use axum::extract::Path;

use super::service::MembershipService;
use crate::{
    core::channel_members::dto::{AddMemberDto, ChannelRoleDto, ListMemberDto, MemberFilterDto},
    utils::{
        response::ApiResult,
        validation::{AuthContext, ValidatedJson, ValidatedQuery},
    },
};

// POST /channels/:id/join
pub async fn join(
    AuthContext { user_id, station, .. }: AuthContext,
    Path(channel_id): Path<String>,
) -> ApiResult<()> {
    tracing::debug!(channel_id = %channel_id, user_id = %user_id, "User joining channel");
    MembershipService::join(station, channel_id, user_id).await
}

// POST /channels/:id/leave
pub async fn leave(
    AuthContext { user_id, station, .. }: AuthContext,
    Path(channel_id): Path<String>,
) -> ApiResult<()> {
    tracing::debug!(channel_id = %channel_id, user_id = %user_id, "User leaving channel");
    MembershipService::leave(station, channel_id, user_id).await
}

// POST /channels/:id/mark_read
pub async fn mark_read(
    AuthContext { user_id, station, .. }: AuthContext,
    Path(channel_id): Path<String>,
) -> ApiResult<()> {
    MembershipService::mark_read(station, channel_id, user_id).await
}

// POST /channels/:id/members
pub async fn add_member(
    AuthContext { user_id, is_owner, station }: AuthContext,
    Path(channel_id): Path<String>,
    ValidatedJson(payload): ValidatedJson<AddMemberDto>,
) -> ApiResult<()> {
    tracing::debug!(channel_id = %channel_id, adder = %user_id, target = %payload.user_id, "Adding member to channel");
    MembershipService::add_member(station, channel_id, user_id, payload.user_id, is_owner).await
}

// DELETE /channels/:channel_id/members/:user_id
pub async fn remove_member(
    AuthContext { user_id, is_owner, station }: AuthContext,
    Path((channel_id, target_user_id)): Path<(String, String)>,
) -> ApiResult<()> {
    tracing::debug!(channel_id = %channel_id, remover = %user_id, target = %target_user_id, "Removing member from channel");
    MembershipService::remove_member(station, channel_id, user_id, target_user_id, is_owner).await
}

// PUT /channels/:channel_id/members/:user_id/role
pub async fn set_channel_role(
    AuthContext { user_id, is_owner, station }: AuthContext,
    Path((channel_id, target_user_id)): Path<(String, String)>,
    ValidatedJson(payload): ValidatedJson<ChannelRoleDto>,
) -> ApiResult<()> {
    tracing::debug!(channel_id = %channel_id, actor = %user_id, target = %target_user_id, "Setting channel role");
    MembershipService::set_channel_role(station, channel_id, user_id, target_user_id, payload.role, is_owner).await
}

// GET /channels/:id/members
pub async fn list_members(
    AuthContext { station, .. }: AuthContext,
    Path(channel_id): Path<String>,
    ValidatedQuery(filter): ValidatedQuery<MemberFilterDto>,
) -> ApiResult<Vec<ListMemberDto>> {
    tracing::debug!(channel_id = %channel_id, "Listing channel members");
    MembershipService::list(station, channel_id, filter).await
}
