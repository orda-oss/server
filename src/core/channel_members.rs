pub mod dto;
pub mod handlers;
pub mod service;

use std::sync::Arc;

use axum::{
    Router,
    routing::{delete, get, post},
};

use crate::Orbit;

pub fn router() -> Router<Arc<Orbit>> {
    Router::new()
        // Join / Leave
        .route("/channels/{channel_id}/join", post(handlers::join))
        .route("/channels/{channel_id}/leave", post(handlers::leave))
        .route(
            "/channels/{channel_id}/mark_read",
            post(handlers::mark_read),
        )
        // Members
        .route(
            "/channels/{channel_id}/members",
            get(handlers::list_members).post(handlers::add_member),
        )
        .route(
            "/channels/{channel_id}/members/{user_id}",
            delete(handlers::remove_member),
        )
}
