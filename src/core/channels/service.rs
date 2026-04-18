use std::sync::Arc;

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use super::dto::{CreateChannelDto, UpdateChannelDto};
use crate::{
    Station,
    core::{
        channel_members::service::MembershipService,
        channels::dto::{ChannelFilterDto, UpdateChannelChangeset},
        models::{Channel, ChannelKind, Server},
        satellite::ServerEvent,
        types::SqliteJson,
    },
    schema::channels::dsl::*,
    utils::{
        helpers::{generate_slug, now_rfc3339},
        response::{ApiError, ApiResponse, ApiResult, codes},
    },
};

pub struct ChannelService;

impl ChannelService {
    pub async fn create(
        station: Arc<Station>,
        payload: CreateChannelDto,
        created_by_user: String,
    ) -> ApiResult<Channel> {
        let station_c = station.clone();
        let new_channel = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            // Slugs must be unique within the server - check before inserting.
            let new_slug = generate_slug(&payload.name);

            let collision = channels
                .filter(server_id.eq(Server::SINGLETON_ID))
                .filter(slug.eq(&new_slug))
                .count()
                .get_result::<i64>(&mut conn)
                .map_err(ApiError::internal)?;

            if collision > 0 {
                tracing::warn!(channel_name = %payload.name, "Channel creation failed: name already exists");
                return Err(
                    ApiError::conflict(codes::ERR_CHANNEL_ALREADY_EXISTS)
                        .with_details(json!(format!(
                            "Channel with 'name: {}' already exists",
                            &payload.name
                        ))),
                );
            }

            let new_id = Uuid::new_v4().to_string();
            let now = now_rfc3339();

            let new_channel = Channel {
                id: new_id,
                server_id: Server::SINGLETON_ID.to_string(),
                name: payload.name,
                slug: new_slug,
                kind: payload.kind.unwrap_or(ChannelKind::Voice),
                metadata: SqliteJson(payload.metadata.unwrap_or_default()),
                is_private: payload.is_private,
                is_default: payload.is_default,
                is_archived: Some(false),
                is_nsfw: Some(false),
                pin_limit: Some(1),
                created_by: created_by_user,
                created_at: Some(now.clone()),
                updated_at: Some(now),
            };

            diesel::insert_into(channels)
                .values(&new_channel)
                .get_result::<Channel>(&mut conn)
                .map_err(ApiError::internal)
        })
        .await
        .map_err(ApiError::internal)??;

        tracing::debug!(channel_id = %new_channel.id, channel_name = %new_channel.name, "Channel created");

        // Auto-join the creator to the new channel.
        MembershipService::join(
            station.clone(),
            new_channel.id.clone(),
            new_channel.created_by.clone(),
        )
        .await
        .ok();

        station
            .satellite
            .broadcast_server(&ServerEvent::ChannelCreated {
                channel: new_channel.clone(),
            });

        Ok(ApiResponse::created(new_channel))
    }

    pub async fn list(station: Arc<Station>, filter: ChannelFilterDto) -> ApiResult<Vec<Channel>> {
        // All channels on the server are discoverable by any authenticated
        // member, including private ones. Listing only returns metadata; joining,
        // sending messages, or reading history still require membership and are
        // enforced at the relevant endpoints.
        let channel_list = tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().map_err(ApiError::internal)?;

            let mut query = channels.into_boxed();

            query = query.filter(server_id.eq(Server::SINGLETON_ID));

            if let Some(f_name) = filter.name {
                query = query.filter(name.like(format!("%{}%", f_name)));
            }
            if let Some(f_kind) = filter.kind {
                query = query.filter(kind.eq(f_kind));
            }
            if let Some(f_archived) = filter.is_archived {
                query = query.filter(is_archived.eq(f_archived));
            }
            if let Some(f_private) = filter.is_private {
                query = query.filter(is_private.eq(f_private));
            }
            if let Some(f_nsfw) = filter.is_nsfw {
                query = query.filter(is_nsfw.eq(f_nsfw));
            }
            if let Some(f_default) = filter.is_default {
                query = query.filter(is_default.eq(f_default));
            }

            match filter.sort.as_deref() {
                Some("name:desc") => query = query.order(name.desc()),
                Some("created_at:asc") => query = query.order(created_at.asc()),
                Some("created_at:desc") => query = query.order(created_at.desc()),
                _ => query = query.order(name.asc()),
            }

            query.load::<Channel>(&mut conn).map_err(ApiError::internal)
        })
        .await
        .map_err(ApiError::internal)??;

        tracing::debug!(count = channel_list.len(), "Channels listed");
        Ok(ApiResponse::ok(channel_list))
    }

    pub async fn update(
        station: Arc<Station>,
        channel_id: String,
        payload: UpdateChannelDto,
        user_id: String,
        is_owner: bool,
    ) -> ApiResult<Channel> {
        let station_c = station.clone();
        let updated_channel = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            let target = channels
                .find(&channel_id)
                .get_result::<Channel>(&mut conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => {
                        tracing::warn!(channel_id = %channel_id, "Channel update failed: not found");
                        ApiError::not_found(codes::ERR_CHANNEL_NOT_FOUND)
                    }
                    _ => ApiError::internal(e),
                })?;

            // Creator can always edit their own channel. Otherwise need channel manager
            // role or server MANAGE_CHANNELS permission.
            let is_creator = target.created_by == user_id;
            if !is_creator {
                crate::core::permissions::require_channel_management(
                    &mut conn, &user_id, &channel_id, is_owner, false,
                )?;
            }

            let mut new_slug = None;

            if let Some(new_name) = &payload.name {
                let generated_slug = generate_slug(new_name);

                let collision = channels
                    .filter(server_id.eq(Server::SINGLETON_ID))
                    .filter(slug.eq(&generated_slug))
                    // Exclude self so renaming to the same name is a no-op, not a conflict.
                    .filter(id.ne(&channel_id))
                    .count()
                    .get_result::<i64>(&mut conn)
                    .map_err(ApiError::internal)?;

                if collision > 0 {
                    tracing::warn!(channel_name = %new_name, "Channel update failed: name already exists");
                    return Err(ApiError::conflict(
                        codes::ERR_CHANNEL_ALREADY_EXISTS,
                    )
                    .with_details(json!(format!(
                        "Channel with 'name: {}' already exists",
                        new_name
                    ))));
                }

                new_slug = Some(generated_slug);
            }

            // Slug mangling on archive/unarchive transition
            if let Some(archiving) = payload.is_archived {
                let was_archived = target.is_archived.unwrap_or(false);
                if archiving && !was_archived {
                    // Archiving: mangle slug to free the name for reuse
                    let ts = chrono::Utc::now().timestamp();
                    new_slug = Some(format!("{}--archived-{}", target.slug, ts));
                } else if !archiving && was_archived {
                    // Unarchiving: try to restore original slug
                    let original = target
                        .slug
                        .split("--archived-")
                        .next()
                        .unwrap_or(&target.slug)
                        .to_string();

                    let collision = channels
                        .filter(server_id.eq(Server::SINGLETON_ID))
                        .filter(slug.eq(&original))
                        .filter(id.ne(&channel_id))
                        .count()
                        .get_result::<i64>(&mut conn)
                        .map_err(ApiError::internal)?;

                    let ts = chrono::Utc::now().timestamp();
                    new_slug = Some(if collision == 0 {
                        original
                    } else {
                        format!("{}--unarchived-{}", original, ts)
                    });
                }
            }

            // Merge incoming metadata fields onto the existing record rather than
            // replacing it wholesale - preserves fields the caller didn't send.
            let merged_metadata = if let Some(incoming_meta) = payload.metadata {
                let mut final_meta = target.clone().metadata.0;

                if let Some(t) = incoming_meta.topic {
                    final_meta.topic = Some(t);
                }
                if let Some(l) = incoming_meta.user_limit {
                    final_meta.user_limit = Some(l);
                }

                Some(SqliteJson(final_meta))
            } else {
                None
            };

            let changeset = UpdateChannelChangeset {
                name: payload.name,
                slug: new_slug,
                is_default: payload.is_default,
                is_private: payload.is_private,
                is_archived: payload.is_archived,
                is_nsfw: payload.is_nsfw,
                pin_limit: payload.pin_limit,
                metadata: merged_metadata,
            };

            let updated_channel = diesel::update(&target)
                .set(&changeset)
                .get_result::<Channel>(&mut conn)
                .map_err(ApiError::internal)?;

            Ok(updated_channel)
        })
        .await
        .map_err(ApiError::internal)??;

        tracing::debug!(channel_id = %updated_channel.id, "Channel updated");

        station
            .satellite
            .broadcast_server(&ServerEvent::ChannelUpdated {
                channel: updated_channel.clone(),
            });

        Ok(ApiResponse::ok(updated_channel))
    }

    pub async fn delete(
        station: Arc<Station>,
        target_id: String,
        user_id: String,
        is_owner: bool,
    ) -> ApiResult<()> {
        let station_c = station.clone();
        let deleted_id = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            let target = channels
                .find(&target_id)
                .get_result::<Channel>(&mut conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => {
                        ApiError::not_found(codes::ERR_CHANNEL_NOT_FOUND)
                    }
                    _ => ApiError::internal(e),
                })?;

            // Channel must be archived before deletion
            if target.is_archived != Some(true) {
                return Err(ApiError::forbidden(codes::ERR_CHANNEL_NOT_ARCHIVED));
            }

            // Creator can always delete their own channel. Otherwise need channel manager
            // role or server MANAGE_CHANNELS permission.
            let is_creator = target.created_by == user_id;
            if !is_creator {
                crate::core::permissions::require_channel_management(
                    &mut conn, &user_id, &target_id, is_owner, false,
                )?;
            }

            diesel::delete(channels.find(&target_id))
                .execute(&mut conn)
                .map_err(ApiError::internal)?;

            tracing::debug!(channel_id = %target_id, "Channel deleted by {}", user_id);
            Ok(target_id)
        })
        .await
        .map_err(ApiError::internal)??;

        // Clean up any active voice/screenshare in this channel before removing it
        let voice_users = station.satellite.voice_participants(&deleted_id);
        for uid in &voice_users {
            station.satellite.voice_leave(&deleted_id, uid);
        }
        // Clear screenshare if active (stop with empty user forces removal)
        if let Some(sharer) = station.satellite.screenshare_get(&deleted_id) {
            station.satellite.screenshare_stop(&deleted_id, &sharer);
        }

        station
            .satellite
            .broadcast_server(&ServerEvent::ChannelDeleted {
                channel_id: deleted_id.clone(),
            });
        station.satellite.remove_channel_sender(&deleted_id);

        Ok(ApiResponse::empty())
    }
}
