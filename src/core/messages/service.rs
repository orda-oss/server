use std::sync::Arc;

use diesel::prelude::*;
use uuid::Uuid;

use super::dto::{CreateMessageDto, EditMessageDto, MessageFilterDto};
use crate::{
    Station,
    core::{
        models::{Channel, ChannelKind, Message, MessageKind},
        satellite::ChannelEvent,
    },
    schema::messages::{self, channel_id},
    utils::{
        helpers::now_rfc3339,
        response::{ApiError, ApiResponse, ApiResult, codes},
    },
};

/// Loads the channel and rejects writes if archived or if broadcast and not authorized.
/// Broadcast channels allow: creator, channel moderator+, server MANAGE_CHANNELS/admin.
fn check_channel_writable(
    conn: &mut diesel::SqliteConnection,
    target_channel_id: &str,
    sender_id: Option<&str>,
    is_owner: bool,
) -> Result<(), ApiError> {
    use crate::schema::channels::dsl as ch;
    let channel = ch::channels
        .find(target_channel_id)
        .first::<Channel>(conn)
        .optional()
        .map_err(ApiError::internal)?;

    if let Some(channel) = channel {
        if channel.is_archived == Some(true) {
            return Err(ApiError::forbidden(codes::ERR_FORBIDDEN));
        }
        if let Some(uid) = sender_id
            && matches!(channel.kind, ChannelKind::Broadcast)
            && channel.created_by != uid
        {
            // Not the creator - check if they have moderation rights
            let is_creator = false;
            crate::core::permissions::require_channel_moderation(
                conn, uid, target_channel_id, is_owner, is_creator,
            )?;
        }
    }
    Ok(())
}

/// Rejects if the user is not a member of the channel.
fn check_membership(
    conn: &mut diesel::SqliteConnection,
    target_channel_id: &str,
    user_id: &str,
) -> Result<(), ApiError> {
    use crate::schema::channel_members::dsl as cm;
    let count: i64 = cm::channel_members
        .filter(cm::channel_id.eq(target_channel_id))
        .filter(cm::user_id.eq(user_id))
        .count()
        .get_result(conn)
        .map_err(ApiError::internal)?;
    if count == 0 {
        return Err(ApiError::forbidden(codes::ERR_CHANNEL_NOT_A_MEMBER));
    }
    Ok(())
}

pub struct MessageService;

impl MessageService {
    pub async fn create(
        station: Arc<Station>,
        target_channel_id: String,
        payload: CreateMessageDto,
    ) -> ApiResult<Message> {
        let station_c = station.clone();
        let cid_c = target_channel_id.clone();

        let saved_msg = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            use crate::schema::channel_members::dsl as members_dsl;

            // Defence-in-depth membership check: the WS handler already guards
            // against non-members, but the REST endpoint hits this path directly,
            // so we re-verify here rather than trusting the caller.
            let is_member: bool = diesel::select(diesel::dsl::exists(
                members_dsl::channel_members
                    .filter(members_dsl::channel_id.eq(&target_channel_id))
                    .filter(members_dsl::user_id.eq(&payload.sender_id)),
            ))
            .get_result(&mut conn)
            .unwrap_or(false);

            if !is_member {
                tracing::warn!(
                    sender_id = %payload.sender_id,
                    channel_id = %target_channel_id,
                    "Message rejected: sender is not a channel member"
                );
                return Err(ApiError::forbidden(codes::ERR_CHANNEL_NOT_A_MEMBER));
            }

            check_channel_writable(&mut conn, &target_channel_id, Some(&payload.sender_id), false)?;

            let new_message = Message {
                id: Uuid::now_v7().to_string(),
                channel_id: target_channel_id.clone(),
                sender_id: payload.sender_id,
                content: payload.content,
                kind: MessageKind::Text,
                is_repliable: Some(true),
                is_reactable: Some(true),
                is_pinned: Some(false),
                root_thread_id: None,
                parent_id: None,
                origin_message_id: None,
                deleted_at: None,
                updated_at: None,
                created_at: Some(now_rfc3339()),
            };

            let saved: Message = diesel::insert_into(messages::table)
                .values(&new_message)
                .get_result(&mut conn)
                .map_err(ApiError::internal)?;

            // Auto-advance sender's unread cursor
            {
                use crate::schema::channel_members::dsl as cm;
                diesel::update(
                    cm::channel_members
                        .filter(cm::channel_id.eq(&target_channel_id))
                        .filter(cm::user_id.eq(&saved.sender_id)),
                )
                .set(cm::last_read_message_id.eq(&saved.id))
                .execute(&mut conn)
                .map_err(ApiError::internal)?;
            }

            Ok(saved)
        })
        .await
        .map_err(ApiError::internal)??;

        // Broadcast the persisted message to all WS subscribers of this channel.
        // Fire-and-forget: broadcast_channel silently no-ops if there are no receivers.
        station.satellite.broadcast_channel(
            &cid_c,
            &ChannelEvent::SendMessage {
                message: saved_msg.clone(),
            },
        );

        tracing::debug!(message_id = %saved_msg.id, channel_id = %cid_c, "Message created");
        Ok(ApiResponse::created(saved_msg))
    }

    pub async fn list(
        station: Arc<Station>,
        target_channel_id: String,
        filter: MessageFilterDto,
        viewing_user_id: String,
    ) -> ApiResult<Vec<Message>> {
        let message_list = tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().map_err(ApiError::internal)?;

            // Private channels: only members can view messages
            {
                use crate::schema::channels::dsl as ch;
                let is_priv: Option<bool> = ch::channels
                    .find(&target_channel_id)
                    .select(ch::is_private)
                    .first(&mut conn)
                    .optional()
                    .map_err(ApiError::internal)?
                    .flatten();

                if is_priv == Some(true) {
                    use crate::schema::channel_members::dsl as cm;
                    let member_count: i64 = cm::channel_members
                        .filter(cm::channel_id.eq(&target_channel_id))
                        .filter(cm::user_id.eq(&viewing_user_id))
                        .count()
                        .get_result(&mut conn)
                        .map_err(ApiError::internal)?;

                    if member_count == 0 {
                        return Err(ApiError::forbidden(codes::ERR_FORBIDDEN));
                    }
                }
            }

            let mut query = messages::table
                .filter(channel_id.eq(target_channel_id))
                .into_boxed();

            query = query
                .order(messages::created_at.desc())
                .limit(filter.limit.unwrap_or(50))
                .offset(filter.offset.unwrap_or(0));

            let results = query
                .load::<Message>(&mut conn)
                .map_err(ApiError::internal)?;

            Ok(results)
        })
        .await
        .map_err(ApiError::internal)??;

        tracing::debug!(count = message_list.len(), "Messages listed");
        Ok(ApiResponse::ok(message_list))
    }

    pub async fn edit(
        station: Arc<Station>,
        target_channel_id: String,
        message_id: String,
        user_id: String,
        payload: EditMessageDto,
    ) -> ApiResult<Message> {
        let cid_c = target_channel_id.clone();
        let station_c = station.clone();
        let updated_msg = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            check_channel_writable(&mut conn, &target_channel_id, None, false)?;

            check_membership(&mut conn, &target_channel_id, &user_id)?;

            let msg = messages::table
                .find(&message_id)
                .filter(channel_id.eq(&target_channel_id))
                .get_result::<Message>(&mut conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => {
                        ApiError::not_found(codes::ERR_MESSAGE_NOT_FOUND)
                    }
                    _ => ApiError::internal(e),
                })?;

            if msg.sender_id != user_id {
                return Err(ApiError::forbidden(codes::ERR_FORBIDDEN));
            }

            if msg.deleted_at.is_some() {
                return Err(ApiError::forbidden(codes::ERR_FORBIDDEN));
            }

            let updated: Message = diesel::update(messages::table.find(&message_id))
                .set((
                    messages::content.eq(&payload.content),
                    messages::updated_at.eq(Some(now_rfc3339())),
                ))
                .get_result(&mut conn)
                .map_err(ApiError::internal)?;

            Ok(updated)
        })
        .await
        .map_err(ApiError::internal)??;

        station.satellite.broadcast_channel(
            &cid_c,
            &ChannelEvent::MessageUpdated {
                message: updated_msg.clone(),
            },
        );

        tracing::debug!(message_id = %updated_msg.id, "Message edited");
        Ok(ApiResponse::ok(updated_msg))
    }

    pub async fn search(
        station: Arc<Station>,
        filter: MessageFilterDto,
        // viewing_user_id: String // TODO: Will need this for "Messages I can see" logic
    ) -> ApiResult<Vec<Message>> {
        let message_list = tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().map_err(ApiError::internal)?;
            let mut query = messages::table.into_boxed();

            // Filter A: By Channel (Specific)
            if let Some(cid) = filter.channel_id {
                query = query.filter(messages::channel_id.eq(cid));
                // TODO: You should check if viewing_user_id is in this channel
            } else {
                // Filter B: Global Search (Complex)
                // TODO
                // "Show me messages matching 'X' in ANY channel I belong to"
                // This requires a JOIN with channel_members.
                // query = query.filter(
                //     messages::channel_id.eq_any(
                //         channel_members::table
                //             .select(channel_members::channel_id)
                //             .filter(channel_members::user_id.eq(viewing_user_id))
                //     )
                // );
            }

            if let Some(search_text) = filter.content {
                query = query.filter(messages::content.like(format!("%{}%", search_text)));
            }

            query = query
                .order(messages::created_at.desc())
                .limit(filter.limit.unwrap_or(50))
                .offset(filter.offset.unwrap_or(0));

            let results = query
                .load::<Message>(&mut conn)
                .map_err(ApiError::internal)?;

            Ok(results)
        })
        .await
        .map_err(ApiError::internal)??;

        tracing::debug!(count = message_list.len(), "Messages search completed");
        Ok(ApiResponse::ok(message_list))
    }

    pub async fn delete(
        station: Arc<Station>,
        target_channel_id: String,
        message_id: String,
        requesting_user_id: String,
        is_owner: bool,
    ) -> ApiResult<()> {
        let station_c = station.clone();
        let cid_c = target_channel_id.clone();
        let mid_c = message_id.clone();

        tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            check_channel_writable(&mut conn, &target_channel_id, None, false)?;

            check_membership(&mut conn, &target_channel_id, &requesting_user_id)?;

            let msg = messages::table
                .find(&message_id)
                .filter(channel_id.eq(&target_channel_id))
                .get_result::<Message>(&mut conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => {
                        ApiError::not_found(codes::ERR_MESSAGE_NOT_FOUND)
                    }
                    _ => ApiError::internal(e),
                })?;

            // Author can always delete their own. Otherwise need channel moderator+
            // or server MANAGE_MESSAGES.
            if msg.sender_id != requesting_user_id {
                let channel = crate::schema::channels::table
                    .find(&target_channel_id)
                    .first::<Channel>(&mut conn)
                    .map_err(ApiError::internal)?;
                let is_creator = channel.created_by == requesting_user_id;
                crate::core::permissions::require_channel_moderation(
                    &mut conn, &requesting_user_id, &target_channel_id, is_owner, is_creator,
                )?;
            }

            diesel::update(messages::table.find(&message_id))
                .set(messages::deleted_at.eq(Some(now_rfc3339())))
                .execute(&mut conn)
                .map_err(ApiError::internal)?;

            Ok(())
        })
        .await
        .map_err(ApiError::internal)??;

        station.satellite.broadcast_channel(
            &cid_c,
            &ChannelEvent::MessageDeleted {
                channel_id: cid_c.clone(),
                message_id: mid_c.clone(),
            },
        );

        tracing::debug!(message_id = %mid_c, channel_id = %cid_c, "Message soft-deleted");
        Ok(ApiResponse::empty())
    }

    pub async fn restore(
        station: Arc<Station>,
        target_channel_id: String,
        message_id: String,
        requesting_user_id: String,
        is_owner: bool,
    ) -> ApiResult<Message> {
        let station_c = station.clone();
        let cid_c = target_channel_id.clone();

        let restored_msg = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            check_channel_writable(&mut conn, &target_channel_id, None, false)?;
            check_membership(&mut conn, &target_channel_id, &requesting_user_id)?;

            let msg = messages::table
                .find(&message_id)
                .filter(channel_id.eq(&target_channel_id))
                .get_result::<Message>(&mut conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => {
                        ApiError::not_found(codes::ERR_MESSAGE_NOT_FOUND)
                    }
                    _ => ApiError::internal(e),
                })?;

            // Author can always restore. Otherwise need channel moderator+ or
            // server MANAGE_MESSAGES.
            if msg.sender_id != requesting_user_id {
                let channel = crate::schema::channels::table
                    .find(&target_channel_id)
                    .first::<Channel>(&mut conn)
                    .map_err(ApiError::internal)?;
                let is_creator = channel.created_by == requesting_user_id;
                crate::core::permissions::require_channel_moderation(
                    &mut conn, &requesting_user_id, &target_channel_id, is_owner, is_creator,
                )?;
            }

            let restored = diesel::update(messages::table.find(&message_id))
                .set(messages::deleted_at.eq(None::<String>))
                .get_result::<Message>(&mut conn)
                .map_err(ApiError::internal)?;

            Ok(restored)
        })
        .await
        .map_err(ApiError::internal)??;

        station.satellite.broadcast_channel(
            &cid_c,
            &ChannelEvent::MessageRestored {
                message: restored_msg.clone(),
            },
        );

        tracing::debug!(message_id = %restored_msg.id, channel_id = %cid_c, "Message restored");
        Ok(ApiResponse::ok(restored_msg))
    }
}
