use axum::extract::Path;

use super::{
    dto::{CreateMessageDto, EditMessageDto, MessageFilterDto},
    service::MessageService,
};
use crate::{
    core::models::Message,
    utils::{
        response::ApiResult,
        validation::{AuthContext, ValidatedJson, ValidatedQuery},
    },
};

// POST /channels/:id/messages
pub async fn create(
    AuthContext { user_id, station, .. }: AuthContext,
    Path(channel_id): Path<String>,
    ValidatedJson(payload): ValidatedJson<CreateMessageDto>,
) -> ApiResult<Message> {
    tracing::debug!(channel_id = %channel_id, sender_id = %user_id, "Creating message");
    MessageService::create(station, channel_id, payload).await
}

pub async fn list(
    AuthContext { user_id, station, .. }: AuthContext,
    Path(channel_id): Path<String>,
    ValidatedQuery(filter): ValidatedQuery<MessageFilterDto>,
) -> ApiResult<Vec<Message>> {
    tracing::debug!(channel_id = %channel_id, "Listing messages");
    MessageService::list(station, channel_id, filter, user_id).await
}

// DELETE /channels/:channel_id/messages/:message_id
pub async fn delete(
    AuthContext { user_id, is_owner, station }: AuthContext,
    Path((channel_id, message_id)): Path<(String, String)>,
) -> ApiResult<()> {
    tracing::debug!(channel_id = %channel_id, message_id = %message_id, "Deleting message");
    MessageService::delete(station, channel_id, message_id, user_id, is_owner).await
}

// PUT /channels/:channel_id/messages/:message_id
pub async fn edit(
    AuthContext { user_id, station, .. }: AuthContext,
    Path((channel_id, message_id)): Path<(String, String)>,
    ValidatedJson(payload): ValidatedJson<EditMessageDto>,
) -> ApiResult<Message> {
    tracing::debug!(channel_id = %channel_id, message_id = %message_id, "Editing message");
    MessageService::edit(station, channel_id, message_id, user_id, payload).await
}

// PUT /channels/:channel_id/messages/:message_id/restore
pub async fn restore(
    AuthContext { user_id, is_owner, station }: AuthContext,
    Path((channel_id, message_id)): Path<(String, String)>,
) -> ApiResult<Message> {
    tracing::debug!(channel_id = %channel_id, message_id = %message_id, "Restoring message");
    MessageService::restore(station, channel_id, message_id, user_id, is_owner).await
}

// GET /messages?content=hello
pub async fn search(
    AuthContext { station, .. }: AuthContext,
    ValidatedQuery(filter): ValidatedQuery<MessageFilterDto>,
) -> ApiResult<Vec<Message>> {
    tracing::debug!("Searching messages with filter: {:?}", filter);
    MessageService::search(station, filter).await
}
