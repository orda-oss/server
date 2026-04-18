use axum::extract::Path;

use super::{
    dto::{AssignRoleDto, CreateRoleDto, ServerMemberDto, UpdateRoleDto},
    service::RoleService,
};
use crate::{
    core::models::Role,
    utils::{
        response::ApiResult,
        validation::{AuthContext, ValidatedJson},
    },
};

pub async fn list_members(
    AuthContext { station, .. }: AuthContext,
) -> ApiResult<Vec<ServerMemberDto>> {
    RoleService::list_members(station).await
}

pub async fn list(AuthContext { station, .. }: AuthContext) -> ApiResult<Vec<Role>> {
    RoleService::list(station).await
}

pub async fn create(
    AuthContext {
        user_id,
        is_owner,
        station,
    }: AuthContext,
    ValidatedJson(payload): ValidatedJson<CreateRoleDto>,
) -> ApiResult<Role> {
    RoleService::create(station, payload, user_id, is_owner).await
}

pub async fn update(
    AuthContext {
        user_id,
        is_owner,
        station,
    }: AuthContext,
    Path(role_id): Path<String>,
    ValidatedJson(payload): ValidatedJson<UpdateRoleDto>,
) -> ApiResult<Role> {
    RoleService::update(station, role_id, payload, user_id, is_owner).await
}

pub async fn delete(
    AuthContext {
        user_id,
        is_owner,
        station,
    }: AuthContext,
    Path(role_id): Path<String>,
) -> ApiResult<()> {
    RoleService::delete(station, role_id, user_id, is_owner).await
}

pub async fn assign_server_role(
    AuthContext {
        user_id,
        is_owner,
        station,
    }: AuthContext,
    Path(target_user_id): Path<String>,
    ValidatedJson(payload): ValidatedJson<AssignRoleDto>,
) -> ApiResult<()> {
    RoleService::assign_server_role(station, target_user_id, payload, user_id, is_owner).await
}

pub async fn my_permissions(
    AuthContext {
        user_id,
        is_owner,
        station,
    }: AuthContext,
) -> ApiResult<serde_json::Value> {
    RoleService::my_permissions(station, user_id, is_owner).await
}
