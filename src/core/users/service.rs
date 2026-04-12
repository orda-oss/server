use std::sync::Arc;

use diesel::prelude::*;

use crate::{
    Station,
    core::{
        models::{Channel, User},
        satellite::{ChannelEvent, ServerEvent},
        users::dto::{self, UserFilterDto},
    },
    schema::{channel_members, channels, users::dsl::*},
    utils::{
        helpers::now_rfc3339,
        response::{ApiError, ApiResponse, ApiResult},
    },
};

pub struct UserService;

impl UserService {
    pub async fn list(station: Arc<Station>, filter: UserFilterDto) -> ApiResult<Vec<User>> {
        let user_list = tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().map_err(ApiError::internal)?;
            let mut query = users.into_boxed();

            if let Some(f_username) = filter.username {
                query = query.filter(username.like(format!("%{}%", f_username)));
            }

            let results = query.load::<User>(&mut conn).map_err(ApiError::internal)?;

            Ok(results)
        })
        .await
        .map_err(ApiError::internal)??;

        tracing::debug!(count = user_list.len(), "Users listed");
        Ok(ApiResponse::ok(user_list))
    }

    // Upsert identity fields from JWT. Runs once on first auth after boot.
    // Subsequent updates come via /internal/sync_user push from semerkant.
    pub async fn sync_identity(
        station: Arc<Station>,
        user_id: String,
        remote_username: String,
        remote_discriminator: i32,
        remote_staff: bool,
    ) {
        let _ = tokio::task::spawn_blocking(move || {
            let mut conn = match station.pool.get() {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(error = ?e, "Failed to get DB connection for identity sync");
                    return;
                }
            };

            let result = diesel::insert_into(users)
                .values((
                    id.eq(&user_id),
                    remote_id.eq(&user_id),
                    username.eq(&remote_username),
                    discriminator.eq(remote_discriminator),
                    staff.eq(remote_staff),
                    created_at.eq(now_rfc3339()),
                ))
                .on_conflict(remote_id)
                .do_update()
                .set((
                    username.eq(&remote_username),
                    discriminator.eq(remote_discriminator),
                    staff.eq(remote_staff),
                    updated_at.eq(now_rfc3339()),
                ))
                .execute(&mut conn);

            if let Err(e) = result {
                tracing::error!(error = ?e, "Failed to sync identity from JWT");
            }
        })
        .await;
    }

    // Auto-join default channels + broadcast. Runs once per session (guarded by synced_users).
    pub async fn sync_channels(station: Arc<Station>, remote_user_id: String) {
        let newly_joined = {
            let station = station.clone();
            let user_id = remote_user_id.clone();
            tokio::task::spawn_blocking(move || {
                let mut conn = match station.pool.get() {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!(error = ?e, "Failed to get DB connection for channel sync");
                        return vec![];
                    }
                };

                let default_channels: Vec<String> = channels::table
                    .filter(channels::is_default.eq(true))
                    .select(channels::id)
                    .load(&mut conn)
                    .unwrap_or_default();

                let mut newly_joined: Vec<String> = vec![];
                let now = now_rfc3339();
                for ch_id in default_channels {
                    let inserted = diesel::insert_into(channel_members::table)
                        .values((
                            channel_members::channel_id.eq(&ch_id),
                            channel_members::user_id.eq(&user_id),
                            channel_members::settings.eq("{}"),
                            channel_members::joined_at.eq(&now),
                        ))
                        .on_conflict((channel_members::channel_id, channel_members::user_id))
                        .do_nothing()
                        .execute(&mut conn)
                        .unwrap_or(0);

                    if inserted > 0 {
                        use crate::schema::messages;
                        let latest: Option<String> = messages::table
                            .filter(messages::channel_id.eq(&ch_id))
                            .filter(messages::deleted_at.is_null())
                            .order(messages::id.desc())
                            .select(messages::id)
                            .first(&mut conn)
                            .optional()
                            .unwrap_or(None);
                        if let Some(mid) = latest {
                            let _ = diesel::update(
                                channel_members::table
                                    .filter(channel_members::channel_id.eq(&ch_id))
                                    .filter(channel_members::user_id.eq(&user_id)),
                            )
                            .set(channel_members::last_read_message_id.eq(Some(&mid)))
                            .execute(&mut conn);
                        }
                        newly_joined.push(ch_id);
                    }
                }

                newly_joined
            })
        }
        .await
        .unwrap_or_default();

        if !newly_joined.is_empty() {
            station
                .satellite
                .broadcast_server(&ServerEvent::MemberJoined {
                    user_id: remote_user_id.clone(),
                });
            for ch_id in &newly_joined {
                station.satellite.broadcast_channel(
                    ch_id,
                    &ChannelEvent::MemberJoined {
                        channel_id: ch_id.clone(),
                        user_id: remote_user_id.clone(),
                    },
                );
            }
        }
    }

    /// Returns all channels a user is a member of, with unread message counts.
    pub async fn user_channels(
        station: Arc<Station>,
        target_user_id: String,
    ) -> ApiResult<Vec<dto::ChannelWithUnread>> {
        let result = tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().map_err(ApiError::internal)?;

            let rows: Vec<(Channel, Option<String>)> = channel_members::table
                .inner_join(channels::table.on(channels::id.eq(channel_members::channel_id)))
                .filter(channel_members::user_id.eq(&target_user_id))
                .select((
                    Channel::as_select(),
                    channel_members::last_read_message_id.nullable(),
                ))
                .order(channels::name.asc())
                .load(&mut conn)
                .map_err(ApiError::internal)?;

            use crate::schema::messages;
            let mut out = Vec::with_capacity(rows.len());
            for (channel, last_read) in rows {
                let mut q = messages::table
                    .filter(messages::channel_id.eq(&channel.id))
                    .filter(messages::deleted_at.is_null())
                    .filter(messages::kind.eq(crate::core::models::MessageKind::Text))
                    .into_boxed();
                if let Some(ref cursor) = last_read {
                    q = q.filter(messages::id.gt(cursor));
                }
                let count: i64 = q
                    .count()
                    .get_result(&mut conn)
                    .map_err(ApiError::internal)?;
                out.push(dto::ChannelWithUnread {
                    channel,
                    unread_count: count,
                    last_read_message_id: last_read,
                });
            }

            Ok(out)
        })
        .await
        .map_err(ApiError::internal)??;

        tracing::debug!(count = result.len(), "User channels listed");
        Ok(ApiResponse::ok(result))
    }

    pub async fn presence(
        station: Arc<Station>,
    ) -> ApiResult<std::collections::HashMap<String, crate::core::models::UserStatus>> {
        let map = station.satellite.user_presence();
        Ok(ApiResponse::ok(map))
    }
}
