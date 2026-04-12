pub mod dto;
pub mod handlers;
pub mod service;

use std::sync::Arc;

use axum::{Router, routing::get};

use crate::Orbit;

pub fn router() -> Router<Arc<Orbit>> {
    Router::new()
        .route("/users", get(handlers::list))
        .route("/users/presence", get(handlers::presence))
        .route("/users/{user_id}/channels", get(handlers::joined_channels))
}
