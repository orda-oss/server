pub mod handlers;

use std::sync::Arc;

use axum::{Router, routing::get};

use crate::Orbit;

pub fn router() -> Router<Arc<Orbit>> {
    Router::new().route("/meta", get(handlers::meta))
}
