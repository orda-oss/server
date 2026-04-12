use axum::extract::Path;

use super::{dto::UserFilterDto, service::UserService};
use crate::{
    core::{models::User, users::dto::ChannelWithUnread},
    utils::{
        response::ApiResult,
        validation::{AuthContext, ValidatedQuery},
    },
};

pub async fn list(
    AuthContext { station, .. }: AuthContext,
    ValidatedQuery(filter): ValidatedQuery<UserFilterDto>,
) -> ApiResult<Vec<User>> {
    tracing::debug!("Listing users with filter: {:?}", filter);
    UserService::list(station, filter).await
}

pub async fn presence(
    AuthContext { station, .. }: AuthContext,
) -> ApiResult<std::collections::HashMap<String, crate::core::models::UserStatus>> {
    UserService::presence(station).await
}

pub async fn joined_channels(
    AuthContext { station, .. }: AuthContext,
    Path(target_user_id): Path<String>,
) -> ApiResult<Vec<ChannelWithUnread>> {
    tracing::debug!(user_id = %target_user_id, "Listing user channels");
    UserService::user_channels(station, target_user_id).await
}
