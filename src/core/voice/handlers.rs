use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use livekit_api::{access_token::TokenVerifier, webhooks::WebhookReceiver};
use serde_json::json;

use super::service::VoiceService;
use crate::{
    Orbit, Station,
    core::satellite::ServerEvent,
    utils::{
        response::{ApiError, ApiResponse, ApiResult, codes},
        validation::AuthContext,
    },
};

/// TCP-ping LiveKit and return whether it's reachable.
pub async fn check_livekit(addr: &str) -> bool {
    let addr_wo_protocol = addr
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_start_matches("ws://")
        .trim_start_matches("wss://");
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::net::TcpStream::connect(addr_wo_protocol),
    )
    .await
    .is_ok_and(|r| r.is_ok())
}

/// REST endpoint: returns current LiveKit reachability.
pub async fn voice_status(
    State(orbit): State<Arc<Orbit>>,
    AuthContext { .. }: AuthContext,
) -> ApiResult<serde_json::Value> {
    let reachable = check_livekit(&orbit.livekit.url).await;
    Ok(ApiResponse::ok(json!({ "livekit": reachable })))
}

/// POST /voice/channels/:channel_id/join
pub async fn voice_join(
    State(orbit): State<Arc<Orbit>>,
    AuthContext { user_id, station }: AuthContext,
    headers: HeaderMap,
    Path(channel_id): Path<String>,
) -> ApiResult<serde_json::Value> {
    // Use LIVEKIT_CLIENT_URL if configured (prod behind proxy),
    // otherwise derive from Host header (dev/LAN).
    let lk_ws_url = if !orbit.livekit.client_url.is_empty() {
        orbit.livekit.client_url.clone()
    } else {
        let host = headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("localhost");
        let server_host = host.split(':').next().unwrap_or("localhost");
        let lk_port = orbit.livekit.url.split(':').next_back().unwrap_or("7880");
        format!("ws://{}:{}", server_host, lk_port)
    };

    VoiceService::join(
        station,
        channel_id,
        user_id,
        lk_ws_url,
        &orbit.livekit.api_key,
        &orbit.livekit.api_secret,
    )
    .await
}

/// POST /voice/channels/:channel_id/leave
pub async fn voice_leave(
    AuthContext { user_id, station }: AuthContext,
    Path(channel_id): Path<String>,
) -> ApiResult<()> {
    VoiceService::leave(station, channel_id, user_id).await
}

/// GET /voice/channels/:channel_id/participants
pub async fn voice_participants(
    AuthContext { station, .. }: AuthContext,
    Path(channel_id): Path<String>,
) -> ApiResult<serde_json::Value> {
    VoiceService::participants(station, channel_id).await
}

/// GET /voice/counts
pub async fn voice_counts(
    AuthContext { station, .. }: AuthContext,
) -> ApiResult<HashMap<String, usize>> {
    VoiceService::counts(station).await
}

/// POST /voice/channels/:channel_id/screenshare/start
pub async fn screenshare_start(
    AuthContext { user_id, station }: AuthContext,
    Path(channel_id): Path<String>,
) -> ApiResult<()> {
    match VoiceService::handle_screenshare_start(&station, &channel_id, &user_id) {
        Ok(()) => Ok(ApiResponse::empty()),
        Err(holder) => Err(ApiError::conflict(codes::ERR_SCREENSHARE_IN_USE)
            .with_details(json!({ "user_id": holder }))),
    }
}

/// POST /voice/channels/:channel_id/screenshare/stop
pub async fn screenshare_stop(
    AuthContext { user_id, station }: AuthContext,
    Path(channel_id): Path<String>,
) -> ApiResult<()> {
    VoiceService::handle_screenshare_stop(&station, &channel_id, &user_id);
    Ok(ApiResponse::empty())
}

/// POST /voice/webhook - receives LiveKit webhook events.
/// LiveKit signs the payload with a JWT in the Authorization header.
/// We validate the signature and process participant_joined / participant_left.
pub async fn webhook_receive(
    State(orbit): State<Arc<Orbit>>,
    headers: HeaderMap,
    body: String,
) -> StatusCode {
    let auth_header = match headers.get("Authorization").and_then(|v| v.to_str().ok()) {
        Some(h) => h,
        None => {
            tracing::warn!("LiveKit webhook: missing Authorization header");
            return StatusCode::UNAUTHORIZED;
        }
    };

    let verifier = TokenVerifier::with_api_key(&orbit.livekit.api_key, &orbit.livekit.api_secret);
    let receiver = WebhookReceiver::new(verifier);

    let event = match receiver.receive(&body, auth_header) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = ?e, "LiveKit webhook: validation failed");
            return StatusCode::UNAUTHORIZED;
        }
    };

    // Room names are encoded as "server_id:channel_id" so we can route
    // webhook events to the correct station in multi-tenant setups.
    let resolve = |room_name: &str| -> Option<(Arc<Station>, String)> {
        if let Some((server_id, channel_id)) = room_name.split_once(':') {
            let station = orbit.get_station(server_id)?;
            Some((station, channel_id.to_string()))
        } else {
            // Fallback for old room names without server_id prefix
            let station = orbit.default_station()?;
            Some((station, room_name.to_string()))
        }
    };

    match event.event.as_str() {
        "participant_joined" => {
            if let (Some(room), Some(participant)) = (&event.room, &event.participant)
                && let Some((station, channel_id)) = resolve(&room.name)
            {
                VoiceService::handle_participant_joined(
                    &station,
                    &channel_id,
                    &participant.identity,
                );
            }
        }
        "participant_left" => {
            if let (Some(room), Some(participant)) = (&event.room, &event.participant)
                && let Some((station, channel_id)) = resolve(&room.name)
            {
                VoiceService::handle_participant_left(&station, &channel_id, &participant.identity);
            }
        }
        _ => {
            tracing::debug!(event = %event.event, "LiveKit webhook: unhandled event");
        }
    }

    StatusCode::OK
}

/// Background task: pings LiveKit every 10s, broadcasts a `ServerEvent`
/// over WS whenever the status changes.
pub fn spawn_health_monitor(orbit: Arc<Orbit>) {
    tokio::spawn(async move {
        let addr = &orbit.livekit.url;
        let mut was_reachable: Option<bool> = None;

        loop {
            let reachable = check_livekit(addr).await;

            if was_reachable != Some(reachable) {
                if reachable {
                    tracing::info!(addr = %addr, "LiveKit is reachable");
                } else {
                    tracing::warn!(addr = %addr, "LiveKit is unreachable");
                }

                for entry in orbit.stations.iter() {
                    let sat = &entry.value().satellite;
                    if !reachable {
                        // LiveKit is down - all voice sessions are dead.
                        // Clients will react to the livekit_status event and reset locally.
                        sat.voice_clear_all();
                        sat.screenshare_clear_all();
                    }
                    sat.broadcast_server(&ServerEvent::LivekitStatus { reachable });
                }

                was_reachable = Some(reachable);
            }

            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });
}
