use axum::Json;
use serde::Serialize;

use crate::{VERSION, utils::validation::AuthContext};

/// Integer API version. Bump on breaking changes that require client awareness.
const API_VERSION: u32 = 1;

/// Capability flags. Additive, never renamed or removed once shipped.
/// Clients branch on these to handle version skew between efes and alacahoyuk.
const FEATURES: &[&str] = &["roles"];

#[derive(Serialize)]
pub struct Meta {
    pub version: &'static str,
    pub api_version: u32,
    pub features: &'static [&'static str],
}

pub async fn meta(_auth: AuthContext) -> Json<Meta> {
    Json(Meta {
        version: VERSION,
        api_version: API_VERSION,
        features: FEATURES,
    })
}
