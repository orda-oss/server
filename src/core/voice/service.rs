use std::{collections::HashMap, sync::Arc};

use livekit_api::access_token::{AccessToken, VideoGrants};
use serde_json::json;

use crate::{
    Station,
    core::{channel_members::service::MembershipService, satellite::ChannelEvent},
    utils::response::{ApiError, ApiResponse, ApiResult, codes},
};

pub struct VoiceService;

impl VoiceService {
    /// Generates a LiveKit token for joining a voice channel.
    /// Room name = channel_id (1:1 mapping).
    /// Participant tracking is handled by LiveKit webhooks, not here.
    pub async fn join(
        station: Arc<Station>,
        channel_id: String,
        user_id: String,
        lk_ws_url: String,
        lk_api_key: &str,
        lk_api_secret: &str,
    ) -> ApiResult<serde_json::Value> {
        let is_member =
            MembershipService::is_member(station.clone(), channel_id.clone(), user_id.clone())
                .await;

        if !is_member {
            return Err(ApiError::forbidden(codes::ERR_CHANNEL_NOT_A_MEMBER));
        }

        // Room name encodes server_id so webhooks can route to the correct station
        let server_id = station.server.remote_id.as_deref().unwrap_or("default");
        let room_name = format!("{server_id}:{channel_id}");

        let token = AccessToken::with_api_key(lk_api_key, lk_api_secret)
            .with_identity(&user_id)
            .with_grants(VideoGrants {
                room_join: true,
                room: room_name,
                can_publish: true,
                can_subscribe: true,
                can_publish_data: true,
                ..Default::default()
            })
            .with_ttl(std::time::Duration::from_secs(3600))
            .to_jwt()
            .map_err(|e| {
                tracing::error!(error = ?e, "Failed to generate LiveKit token");
                ApiError::internal("Failed to generate LiveKit token")
            })?;

        Ok(ApiResponse::ok(json!({
            "token": token,
            "url": lk_ws_url,
        })))
    }

    /// Explicit leave - removes user from tracking and broadcasts departure.
    /// Also called by LiveKit webhook on participant_left, so this may be a no-op
    /// if the webhook already handled it.
    pub async fn leave(
        station: Arc<Station>,
        channel_id: String,
        user_id: String,
    ) -> ApiResult<()> {
        Self::handle_participant_left(&station, &channel_id, &user_id);
        Ok(ApiResponse::empty())
    }

    /// Called by LiveKit webhook when a participant actually connects to a room.
    pub fn handle_participant_joined(station: &Arc<Station>, channel_id: &str, user_id: &str) {
        let was_new = station.satellite.voice_join(channel_id, user_id);
        if was_new {
            station.satellite.broadcast_channel(
                channel_id,
                &ChannelEvent::VoiceJoined {
                    channel_id: channel_id.to_string(),
                    user_id: user_id.to_string(),
                },
            );
            tracing::debug!(channel_id = %channel_id, user_id = %user_id, "User joined voice");
        }
    }

    /// Called by LiveKit webhook when a participant disconnects from a room,
    /// or by the explicit /voice/leave endpoint.
    pub fn handle_participant_left(station: &Arc<Station>, channel_id: &str, user_id: &str) {
        let was_present = station.satellite.voice_leave(channel_id, user_id);
        if was_present {
            station.satellite.broadcast_channel(
                channel_id,
                &ChannelEvent::VoiceLeft {
                    channel_id: channel_id.to_string(),
                    user_id: user_id.to_string(),
                },
            );
            tracing::debug!(channel_id = %channel_id, user_id = %user_id, "User left voice");
        }

        // Also clear screenshare if this user was sharing
        let was_sharing = station.satellite.screenshare_stop(channel_id, user_id);
        if was_sharing {
            station.satellite.broadcast_channel(
                channel_id,
                &ChannelEvent::ScreenshareStopped {
                    channel_id: channel_id.to_string(),
                },
            );
            tracing::debug!(channel_id = %channel_id, user_id = %user_id, "Screenshare cleared on voice leave");
        }
    }

    /// Returns participants and active screensharer for a channel.
    pub async fn participants(
        station: Arc<Station>,
        channel_id: String,
    ) -> ApiResult<serde_json::Value> {
        let participants = station.satellite.voice_participants(&channel_id);
        let screensharer = station.satellite.screenshare_get(&channel_id);
        let flags = station.satellite.voice_sticky_status_flags_get(&channel_id);

        Ok(ApiResponse::ok(json!({
            "participants": participants,
            "screensharer": screensharer,
            "flags": flags,
        })))
    }

    /// Returns voice counts for all channels (for sidebar display).
    pub async fn counts(station: Arc<Station>) -> ApiResult<HashMap<String, usize>> {
        let counts = station.satellite.voice_counts();
        Ok(ApiResponse::ok(counts))
    }

    /// Claims the screenshare slot for a channel.
    /// Returns Ok(()) if claimed, Err(holder_user_id) if another user holds it.
    pub fn handle_screenshare_start(
        station: &Arc<Station>,
        channel_id: &str,
        user_id: &str,
    ) -> Result<(), String> {
        let result = station.satellite.screenshare_start(channel_id, user_id);
        if result.is_ok() {
            station.satellite.broadcast_channel(
                channel_id,
                &ChannelEvent::ScreenshareStarted {
                    channel_id: channel_id.to_string(),
                    user_id: user_id.to_string(),
                },
            );
            tracing::debug!(channel_id = %channel_id, user_id = %user_id, "Screenshare started");
        }
        result
    }

    /// Releases the screenshare slot for a channel.
    pub fn handle_screenshare_stop(station: &Arc<Station>, channel_id: &str, user_id: &str) {
        let was_sharing = station.satellite.screenshare_stop(channel_id, user_id);
        if was_sharing {
            station.satellite.broadcast_channel(
                channel_id,
                &ChannelEvent::ScreenshareStopped {
                    channel_id: channel_id.to_string(),
                },
            );
            tracing::debug!(channel_id = %channel_id, user_id = %user_id, "Screenshare stopped");
        }
    }
}
