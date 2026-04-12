pub mod dto;
pub mod handlers;
pub mod service;
pub mod ws;

use std::sync::Arc;

use axum::{
    Router,
    routing::{delete, get, post, put},
};

use crate::Orbit;

pub fn router() -> Router<Arc<Orbit>> {
    Router::new()
        // REST
        .route("/channels/{channel_id}/messages", post(handlers::create))
        .route("/channels/{channel_id}/messages", get(handlers::list))
        .route(
            "/channels/{channel_id}/messages/{message_id}",
            put(handlers::edit),
        )
        .route(
            "/channels/{channel_id}/messages/{message_id}",
            delete(handlers::delete),
        )
        .route(
            "/channels/{channel_id}/messages/{message_id}/restore",
            put(handlers::restore),
        )
        .route("/messages", get(handlers::search))
        // WebSocket - single multiplexed connection per user; channel subscriptions
        // are managed server-side via join/leave endpoints and satellite session registry.
        .route("/ws", get(ws::ws_handler))
}
