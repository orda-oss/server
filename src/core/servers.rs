pub mod dto;
pub mod handlers;
pub mod service;

use std::sync::Arc;

use axum::{Router, routing::get};

use crate::Orbit;

pub fn router() -> Router<Arc<Orbit>> {
    Router::new().route("/servers", get(handlers::list))
}
