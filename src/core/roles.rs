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
        .route("/roles", get(handlers::list))
        .route("/roles", post(handlers::create))
        .route("/roles/{role_id}", put(handlers::update))
        .route("/roles/{role_id}", delete(handlers::delete))
        .route("/members", get(handlers::list_members))
        .route("/members/{user_id}/role", put(handlers::assign_server_role))
        .route("/me/permissions", get(handlers::my_permissions))
}
