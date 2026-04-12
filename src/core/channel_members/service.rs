use std::sync::Arc;

use diesel::prelude::*;
use uuid::Uuid;

use super::dto::MemberFilterDto;
use crate::{
    Station,
    core::{
        channel_members::dto::ListMemberDto,
        models::{ChannelMember, Message, MessageKind, User},
        satellite::{ChannelEvent, UserCommand},
        types::SqliteJson,
        voice::service::VoiceService,
    },
    schema::{channel_members::dsl::*, users::dsl::*},
    utils::{
        helpers::now_rfc3339,
        response::{ApiError, ApiResponse, ApiResult, codes},
    },
};

pub struct MembershipService;

impl MembershipService {
    /// Adds `user_id` to the channel. Idempotent via `ON CONFLICT DO NOTHING` -
    /// calling join on an existing member is a no-op, not an error.
    /// After the DB write succeeds, pushes `Subscribe` to the user's active WS
    /// session (if any) so they start receiving real-time messages immediately
    /// without needing to reconnect.
    // 1. JOIN (Idempotent: If already member, just return OK)
    pub async fn join(
        station: Arc<Station>,
        target_channel_id: String,
        target_user_id: String,
    ) -> ApiResult<()> {
        let uid = target_user_id.clone();
        let cid = target_channel_id.clone();

        let station_c = station.clone();
        let system_msg = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            // Reject joining archived or private channels (creator can join their own)
            {
                use crate::schema::channels::dsl as ch;
                let channel: Option<(Option<bool>, Option<bool>, String)> = ch::channels
                    .find(&target_channel_id)
                    .select((ch::is_archived, ch::is_private, ch::created_by))
                    .first(&mut conn)
                    .optional()
                    .map_err(ApiError::internal)?;

                if let Some((archived, private, creator)) = channel {
                    if archived == Some(true) {
                        return Err(ApiError::forbidden(codes::ERR_FORBIDDEN));
                    }
                    if private == Some(true) && target_user_id != creator {
                        return Err(ApiError::forbidden(codes::ERR_FORBIDDEN));
                    }
                }
            }

            let new_member = ChannelMember {
                channel_id: target_channel_id.clone(),
                user_id: target_user_id.clone(),
                role_id: None,
                added_by: None,
                settings: SqliteJson(serde_json::json!({})),
                joined_at: Some(now_rfc3339()),
                last_read_message_id: None,
            };

            let inserted = diesel::insert_into(channel_members)
                .values(&new_member)
                .on_conflict((channel_id, user_id))
                .do_nothing()
                .execute(&mut conn)
                .map_err(ApiError::internal)?;

            // Set cursor to latest message so history doesn't appear as unread
            if inserted > 0 {
                use crate::schema::messages::dsl as msg;
                let latest: Option<String> = msg::messages
                    .filter(msg::channel_id.eq(&target_channel_id))
                    .filter(msg::deleted_at.is_null())
                    .order(msg::id.desc())
                    .select(msg::id)
                    .first(&mut conn)
                    .optional()
                    .map_err(ApiError::internal)?;

                if let Some(mid) = latest {
                    diesel::update(
                        channel_members
                            .filter(channel_id.eq(&target_channel_id))
                            .filter(user_id.eq(&target_user_id)),
                    )
                    .set(last_read_message_id.eq(Some(&mid)))
                    .execute(&mut conn)
                    .map_err(ApiError::internal)?;
                }

                // System message
                let sys_msg = Message {
                    id: Uuid::now_v7().to_string(),
                    channel_id: target_channel_id,
                    sender_id: target_user_id,
                    content: "joined the channel".to_string(),
                    kind: MessageKind::System,
                    is_repliable: Some(false),
                    is_reactable: Some(false),
                    is_pinned: Some(false),
                    root_thread_id: None,
                    parent_id: None,
                    origin_message_id: None,
                    deleted_at: None,
                    updated_at: None,
                    created_at: Some(now_rfc3339()),
                };

                let saved: Message = diesel::insert_into(crate::schema::messages::table)
                    .values(&sys_msg)
                    .get_result(&mut conn)
                    .map_err(ApiError::internal)?;

                return Ok(Some(saved));
            }

            Ok(None)
        })
        .await
        .map_err(ApiError::internal)??;

        // If the user has an active WebSocket, subscribe them to the new channel immediately.
        station
            .satellite
            .send_user_command(&uid, UserCommand::Subscribe(cid.clone()));

        station.satellite.broadcast_channel(
            &cid,
            &ChannelEvent::MemberJoined {
                channel_id: cid.clone(),
                user_id: uid.clone(),
            },
        );

        if let Some(msg) = system_msg {
            station
                .satellite
                .broadcast_channel(&cid, &ChannelEvent::SendMessage { message: msg });
        }

        tracing::debug!(channel_id = %cid, user_id = %uid, "User joined channel");
        Ok(ApiResponse::empty())
    }

    /// Removes `user_id` from the channel. Returns 404 if they weren't a member.
    /// `station_c` is cloned for the `spawn_blocking` closure; the original
    /// `station` is kept in scope to call `satellite.send_user_command` after the
    /// blocking task returns (you can't hold an `Arc<Station>` across an await if it
    /// moved into the closure).
    // 2. LEAVE
    pub async fn leave(
        station: Arc<Station>,
        target_channel_id: String,
        target_user_id: String,
    ) -> ApiResult<()> {
        let uid = target_user_id.clone();
        let cid = target_channel_id.clone();

        let station_c = station.clone();
        let uid_c = uid.clone();
        let cid_c = cid.clone();
        let system_msg = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            // Prevent creator from leaving their own private channel
            {
                use crate::schema::channels::dsl as ch;
                let (creator, is_priv): (String, Option<bool>) = ch::channels
                    .find(&target_channel_id)
                    .select((ch::created_by, ch::is_private))
                    .first(&mut conn)
                    .map_err(ApiError::internal)?;
                if creator == uid_c && is_priv == Some(true) {
                    return Err(ApiError::forbidden(codes::ERR_CHANNEL_OWNER_CANNOT_LEAVE)
                        .with_details(serde_json::json!(
                            "Private channel owner cannot leave.\nDelete the channel instead."
                        )));
                }
            }

            let count = diesel::delete(
                channel_members
                    .filter(channel_id.eq(&target_channel_id))
                    .filter(user_id.eq(&target_user_id)),
            )
            .execute(&mut conn)
            .map_err(ApiError::internal)?;

            if count == 0 {
                tracing::warn!(channel_id = %cid_c, user_id = %uid_c, "Leave failed: not a member");
                return Err(ApiError::not_found(codes::ERR_CHANNEL_NOT_A_MEMBER));
            }

            let sys_msg = Message {
                id: Uuid::now_v7().to_string(),
                channel_id: target_channel_id,
                sender_id: target_user_id,
                content: "left the channel".to_string(),
                kind: MessageKind::System,
                is_repliable: Some(false),
                is_reactable: Some(false),
                is_pinned: Some(false),
                root_thread_id: None,
                parent_id: None,
                origin_message_id: None,
                deleted_at: None,
                updated_at: None,
                created_at: Some(now_rfc3339()),
            };

            let saved: Message = diesel::insert_into(crate::schema::messages::table)
                .values(&sys_msg)
                .get_result(&mut conn)
                .map_err(ApiError::internal)?;

            Ok(saved)
        })
        .await
        .map_err(ApiError::internal)??;

        // If the user is in voice for this channel, remove them.
        VoiceService::handle_participant_left(&station, &cid, &uid);

        // If the user has an active WebSocket, unsubscribe them from the channel immediately.
        station
            .satellite
            .send_user_command(&uid, UserCommand::Unsubscribe(cid.clone()));

        station.satellite.broadcast_channel(
            &cid,
            &ChannelEvent::MemberLeft {
                channel_id: cid.clone(),
                user_id: uid.clone(),
            },
        );
        station.satellite.broadcast_channel(
            &cid,
            &ChannelEvent::SendMessage {
                message: system_msg,
            },
        );

        tracing::debug!(channel_id = %cid, user_id = %uid, "User left channel");
        Ok(ApiResponse::empty())
    }

    /// Internal helper - returns `true` if the user is a member of the channel.
    /// Used by the WS recv_task before persisting a message. The double-unwrap
    /// on the `spawn_blocking` result (`unwrap_or(Some(0)).unwrap_or(0) > 0`)
    /// collapses both join-error and query-error into a safe `false`.
    // 3. CHECK MEMBERSHIP (Internal Helper)
    #[allow(dead_code)]
    pub async fn is_member(
        station: Arc<Station>,
        target_channel_id: String,
        target_user_id: String,
    ) -> bool {
        tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().ok()?;
            let count: i64 = channel_members
                .filter(channel_id.eq(target_channel_id))
                .filter(user_id.eq(target_user_id))
                .count()
                .get_result(&mut conn)
                .ok()?;
            Some(count)
        })
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0)
            > 0
    }

    // 4. MARK AS READ
    pub async fn mark_read(
        station: Arc<Station>,
        target_channel_id: String,
        target_user_id: String,
    ) -> ApiResult<()> {
        tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().map_err(ApiError::internal)?;

            use crate::schema::messages::dsl as msg;
            let latest: Option<String> = msg::messages
                .filter(msg::channel_id.eq(&target_channel_id))
                .filter(msg::deleted_at.is_null())
                .order(msg::id.desc())
                .select(msg::id)
                .first(&mut conn)
                .optional()
                .map_err(ApiError::internal)?;

            if let Some(mid) = latest {
                diesel::update(
                    channel_members
                        .filter(channel_id.eq(&target_channel_id))
                        .filter(user_id.eq(&target_user_id)),
                )
                .set(last_read_message_id.eq(Some(&mid)))
                .execute(&mut conn)
                .map_err(ApiError::internal)?;
            }

            Ok(())
        })
        .await
        .map_err(ApiError::internal)??;

        Ok(ApiResponse::empty())
    }

    // 5. ADD MEMBER (for private channels - any member can add a server member)
    pub async fn add_member(
        station: Arc<Station>,
        target_channel_id: String,
        adder_user_id: String,
        target_user_id: String,
    ) -> ApiResult<()> {
        let cid = target_channel_id.clone();
        let tid = target_user_id.clone();

        let station_c = station.clone();
        let system_msg = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            // Verify adder is a member
            let adder_count: i64 = channel_members
                .filter(channel_id.eq(&target_channel_id))
                .filter(user_id.eq(&adder_user_id))
                .count()
                .get_result(&mut conn)
                .map_err(ApiError::internal)?;
            if adder_count == 0 {
                return Err(ApiError::forbidden(codes::ERR_FORBIDDEN));
            }

            // Verify target user exists on this server
            let user_exists: i64 = users
                .find(&target_user_id)
                .count()
                .get_result(&mut conn)
                .map_err(ApiError::internal)?;
            if user_exists == 0 {
                return Err(ApiError::not_found(codes::ERR_FORBIDDEN));
            }

            // Get target username for system message
            let target_username: String = users
                .find(&target_user_id)
                .select(crate::schema::users::username)
                .first(&mut conn)
                .map_err(ApiError::internal)?;

            // Insert membership
            let now = now_rfc3339();
            let new_member = ChannelMember {
                channel_id: target_channel_id.clone(),
                user_id: target_user_id.clone(),
                role_id: None,
                added_by: Some(adder_user_id.clone()),
                settings: SqliteJson(serde_json::json!({})),
                joined_at: Some(now.clone()),
                last_read_message_id: None,
            };

            let inserted = diesel::insert_into(channel_members)
                .values(&new_member)
                .on_conflict((channel_id, user_id))
                .do_nothing()
                .execute(&mut conn)
                .map_err(ApiError::internal)?;

            if inserted == 0 {
                // Already a member
                return Ok(None);
            }

            // Set cursor to latest message
            {
                use crate::schema::messages::dsl as msg;
                let latest: Option<String> = msg::messages
                    .filter(msg::channel_id.eq(&target_channel_id))
                    .filter(msg::deleted_at.is_null())
                    .order(msg::id.desc())
                    .select(msg::id)
                    .first(&mut conn)
                    .optional()
                    .map_err(ApiError::internal)?;

                if let Some(mid) = latest {
                    diesel::update(
                        channel_members
                            .filter(channel_id.eq(&target_channel_id))
                            .filter(user_id.eq(&target_user_id)),
                    )
                    .set(last_read_message_id.eq(Some(&mid)))
                    .execute(&mut conn)
                    .map_err(ApiError::internal)?;
                }
            }

            // Insert system message
            let sys_msg = Message {
                id: Uuid::now_v7().to_string(),
                channel_id: target_channel_id,
                sender_id: adder_user_id,
                content: format!("added {} to the channel", target_username),
                kind: MessageKind::System,
                is_repliable: Some(false),
                is_reactable: Some(false),
                is_pinned: Some(false),
                root_thread_id: None,
                parent_id: None,
                origin_message_id: None,
                deleted_at: None,
                updated_at: None,
                created_at: Some(now),
            };

            let saved: Message = diesel::insert_into(crate::schema::messages::table)
                .values(&sys_msg)
                .get_result(&mut conn)
                .map_err(ApiError::internal)?;

            Ok(Some(saved))
        })
        .await
        .map_err(ApiError::internal)??;

        if let Some(msg) = system_msg {
            // Subscribe new member's WS session
            station
                .satellite
                .send_user_command(&tid, UserCommand::Subscribe(cid.clone()));

            // Broadcast member joined + system message
            station.satellite.broadcast_channel(
                &cid,
                &ChannelEvent::MemberJoined {
                    channel_id: cid.clone(),
                    user_id: tid,
                },
            );
            station
                .satellite
                .broadcast_channel(&cid, &ChannelEvent::SendMessage { message: msg });
        }

        Ok(ApiResponse::empty())
    }

    // 6. REMOVE MEMBER (channel creator only)
    pub async fn remove_member(
        station: Arc<Station>,
        target_channel_id: String,
        remover_user_id: String,
        target_user_id: String,
    ) -> ApiResult<()> {
        let cid = target_channel_id.clone();
        let tid = target_user_id.clone();

        let station_c = station.clone();
        let system_msg = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            // Verify remover is the channel creator
            use crate::schema::channels::dsl as ch;
            let creator: String = ch::channels
                .find(&target_channel_id)
                .select(ch::created_by)
                .first(&mut conn)
                .map_err(|_| ApiError::not_found(codes::ERR_CHANNEL_NOT_FOUND))?;

            if creator != remover_user_id {
                return Err(ApiError::forbidden(codes::ERR_FORBIDDEN));
            }

            // Can't remove yourself
            if target_user_id == remover_user_id {
                return Err(ApiError::forbidden(codes::ERR_FORBIDDEN));
            }

            // Get target username for system message
            let target_username: String = users
                .find(&target_user_id)
                .select(crate::schema::users::username)
                .first(&mut conn)
                .map_err(ApiError::internal)?;

            // Delete membership
            let count = diesel::delete(
                channel_members
                    .filter(channel_id.eq(&target_channel_id))
                    .filter(user_id.eq(&target_user_id)),
            )
            .execute(&mut conn)
            .map_err(ApiError::internal)?;

            if count == 0 {
                return Err(ApiError::not_found(codes::ERR_CHANNEL_NOT_A_MEMBER));
            }

            // Insert system message
            let sys_msg = Message {
                id: Uuid::now_v7().to_string(),
                channel_id: target_channel_id,
                sender_id: remover_user_id,
                content: format!("removed {} from the channel", target_username),
                kind: MessageKind::System,
                is_repliable: Some(false),
                is_reactable: Some(false),
                is_pinned: Some(false),
                root_thread_id: None,
                parent_id: None,
                origin_message_id: None,
                deleted_at: None,
                updated_at: None,
                created_at: Some(now_rfc3339()),
            };

            let saved: Message = diesel::insert_into(crate::schema::messages::table)
                .values(&sys_msg)
                .get_result(&mut conn)
                .map_err(ApiError::internal)?;

            Ok(saved)
        })
        .await
        .map_err(ApiError::internal)??;

        // Voice/screenshare cleanup
        VoiceService::handle_participant_left(&station, &cid, &tid);

        // Unsubscribe from WS channel
        station
            .satellite
            .send_user_command(&tid, UserCommand::Unsubscribe(cid.clone()));

        // Broadcast removal + system message
        station.satellite.broadcast_channel(
            &cid,
            &ChannelEvent::MemberLeft {
                channel_id: cid.clone(),
                user_id: tid.clone(),
            },
        );
        station.satellite.broadcast_channel(
            &cid,
            &ChannelEvent::SendMessage {
                message: system_msg,
            },
        );

        tracing::debug!(channel_id = %cid, user_id = %tid, "Member removed from channel");
        Ok(ApiResponse::empty())
    }

    /// Lists channel members with a denormalized `UserSummary` embedded in each
    /// row. The `inner_join(users.on(id.eq(user_id)))` lets Diesel load both
    /// `ChannelMember` and `User` in one query; `ListMemberDto::from` then
    /// projects the tuple into the API shape.
    pub async fn list(
        station: Arc<Station>,
        target_channel_id: String,
        filter: MemberFilterDto,
    ) -> ApiResult<Vec<ListMemberDto>> {
        let member_list = tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().map_err(ApiError::internal)?;

            let mut query = channel_members
                .inner_join(users.on(id.eq(user_id)))
                .filter(channel_id.eq(target_channel_id))
                .into_boxed();

            // TODO: We should .inner_join(users::table) here
            // to return { user_id, username, avatar } instead of just the link record.

            query = query
                .limit(filter.limit.unwrap_or(50))
                .offset(filter.offset.unwrap_or(0));

            let results = query
                .select((ChannelMember::as_select(), User::as_select()))
                .load::<(ChannelMember, User)>(&mut conn)
                .map_err(ApiError::internal)?;

            let response: Vec<ListMemberDto> =
                results.into_iter().map(ListMemberDto::from).collect();

            Ok(response)
        })
        .await
        .map_err(ApiError::internal)??;

        tracing::debug!(count = member_list.len(), "Channel members listed");
        Ok(ApiResponse::ok(member_list))
    }
}
