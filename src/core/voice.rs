pub mod handlers;
pub mod service;

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};

use crate::Orbit;

pub fn router() -> Router<Arc<Orbit>> {
    Router::new()
        .route("/voice/status", get(handlers::voice_status))
        .route("/voice/counts", get(handlers::voice_counts))
        .route("/voice/webhook", post(handlers::webhook_receive))
        .route(
            "/voice/channels/{channel_id}/join",
            post(handlers::voice_join),
        )
        .route(
            "/voice/channels/{channel_id}/leave",
            post(handlers::voice_leave),
        )
        .route(
            "/voice/channels/{channel_id}/participants",
            get(handlers::voice_participants),
        )
        .route(
            "/voice/channels/{channel_id}/screenshare/start",
            post(handlers::screenshare_start),
        )
        .route(
            "/voice/channels/{channel_id}/screenshare/stop",
            post(handlers::screenshare_stop),
        )
}
