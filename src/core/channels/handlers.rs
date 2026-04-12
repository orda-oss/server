use axum::extract::Path;

use crate::{
    core::{
        channels::{
            dto::{ChannelFilterDto, CreateChannelDto, UpdateChannelDto},
            service::ChannelService,
        },
        models::Channel,
    },
    utils::{
        response::ApiResult,
        validation::{AuthContext, ValidatedJson, ValidatedQuery},
    },
};

pub async fn create(
    AuthContext { user_id, station }: AuthContext,
    ValidatedJson(payload): ValidatedJson<CreateChannelDto>,
) -> ApiResult<Channel> {
    tracing::debug!(user_id = %user_id, "Creating channel: {}", payload.name);
    ChannelService::create(station, payload, user_id).await
}

pub async fn list(
    AuthContext { user_id, station }: AuthContext,
    ValidatedQuery(filter): ValidatedQuery<ChannelFilterDto>,
) -> ApiResult<Vec<Channel>> {
    tracing::debug!("Listing channels with filter: {:?}", filter);
    ChannelService::list(station, filter, user_id).await
}

pub async fn update(
    AuthContext { user_id, station }: AuthContext,
    Path(id): Path<String>,
    ValidatedJson(payload): ValidatedJson<UpdateChannelDto>,
) -> ApiResult<Channel> {
    tracing::debug!(channel_id = %id, user_id = %user_id, "Updating channel");
    ChannelService::update(station, id, payload, user_id).await
}

pub async fn delete(
    AuthContext { user_id, station }: AuthContext,
    Path(id): Path<String>,
) -> ApiResult<()> {
    tracing::debug!(channel_id = %id, user_id = %user_id, "Deleting channel");
    ChannelService::delete(station, id, user_id).await
}
