pub mod core;
pub mod schema;
pub mod utils;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use core::{
    models::Server,
    orbit::{DbPool, LiveKitConfig, Orbit, Station},
    satellite::Satellite,
};
use std::sync::Arc;

use diesel_migrations::{EmbeddedMigrations, embed_migrations};
pub use utils::startup::{ResolvedKeys, db_url_for, init_db, init_jwks, parse_port, resolve_keys};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations");
pub const MAX_BODY_SIZE: usize = 64 * 1024; // 64 KB

// Health check

async fn health_check(
    axum::extract::State(license_key): axum::extract::State<Option<String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse};
    use sha2::{Digest, Sha256};

    let Some(ref key) = license_key else {
        #[cfg(debug_assertions)]
        return (StatusCode::OK, "OK").into_response();
        #[cfg(not(debug_assertions))]
        return StatusCode::UNAUTHORIZED.into_response();
    };

    let hash = Sha256::digest(key.as_bytes());
    let expected = format!("{:x}", hash);

    let authorized = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|token| token == expected)
        .unwrap_or(false);

    if authorized {
        (StatusCode::OK, "OK").into_response()
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

pub fn build_health_router(license_key: Option<String>) -> axum::Router {
    use axum::routing::get;
    axum::Router::new()
        .route("/health", get(health_check))
        .with_state(license_key)
}

// Maintenance guard: checks the station resolved by the auth extractor.
// Moved from middleware to a helper called by AuthContext after resolving
// the station, since we need the JWT sid to know which station to check.
pub fn check_maintenance(
    station: &Station,
    method: &axum::http::Method,
    path: &str,
) -> Result<(), utils::response::ApiError> {
    use axum::http::Method;

    if station.is_maintenance() {
        let is_read = *method == Method::GET || *method == Method::HEAD;
        let is_exempt = path.starts_with("/internal/") || path.ends_with("/mark_read");
        if !is_read && !is_exempt {
            return Err(utils::response::ApiError::maintenance());
        }
    }
    Ok(())
}

pub fn build_router(orbit: Arc<Orbit>) -> axum::Router {
    use axum::{middleware, routing::get};
    use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};

    use crate::utils::rate_limit::http_rate_limit;

    let health = axum::Router::new()
        .route("/health", get(health_check))
        .with_state(orbit.license_key.clone());

    axum::Router::new()
        .merge(health)
        .merge(core::channels::router())
        .merge(core::channel_members::router())
        .merge(core::users::router())
        .merge(core::messages::router())
        .merge(core::servers::router())
        .merge(core::voice::router())
        .merge(core::internal::router())
        .layer(middleware::from_fn_with_state(
            orbit.clone(),
            http_rate_limit,
        ))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_SIZE))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::very_permissive())
        .with_state(orbit)
}
