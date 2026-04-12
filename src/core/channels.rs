pub mod dto;
pub mod handlers;
pub mod service;

use std::sync::Arc;

use axum::{
    Router,
    routing::{delete, get, post, put},
};

use crate::Orbit;

pub fn router() -> Router<Arc<Orbit>> {
    Router::new()
        .route("/channels", post(handlers::create))
        .route("/channels", get(handlers::list))
        .route("/channels/{channel_id}", put(handlers::update))
        .route("/channels/{channel_id}", delete(handlers::delete))
}
