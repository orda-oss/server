pub mod handlers;

use std::sync::Arc;

use axum::{Router, routing::post};

use crate::Orbit;

pub fn router() -> Router<Arc<Orbit>> {
    Router::new()
        .route("/internal/revoke", post(handlers::revoke))
        .route("/internal/sync_user", post(handlers::sync_user))
        .route("/internal/maintenance", post(handlers::maintenance))
        .route("/internal/provision", post(handlers::provision))
        .route(
            "/internal/deprovision/{server_id}",
            post(handlers::deprovision),
        )
}
