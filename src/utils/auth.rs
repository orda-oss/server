use std::sync::Arc;

use axum::{extract::FromRequestParts, http::request::Parts};
use serde::Deserialize;

use crate::{
    Orbit, Station,
    utils::{
        jwt_verify::{extract_bearer, verify_token},
        response::{ApiError, codes},
    },
};

#[derive(Debug, Deserialize)]
pub struct AccessClaims {
    pub sub: String,
    pub sid: String,
    #[serde(rename = "type")]
    pub token_type: String,
    pub name: String,
    #[allow(dead_code)]
    pub owner: bool,
    #[serde(default = "default_discriminator")]
    pub discriminator: i32,
    #[serde(default)]
    pub staff: bool,
}

fn default_discriminator() -> i32 {
    9999
}

// Resolves both the user_id and the per-server Station from the JWT sid claim.
pub struct AuthContext {
    pub user_id: String,
    pub station: Arc<Station>,
}

impl FromRequestParts<Arc<Orbit>> for AuthContext {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<Orbit>,
    ) -> Result<Self, Self::Rejection> {
        let jwks = match &state.jwks {
            Some(jwks) => jwks,
            None => {
                if cfg!(debug_assertions) {
                    let user_id = parts
                        .headers
                        .get("X-User-Id")
                        .and_then(|v| v.to_str().ok())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .ok_or_else(|| ApiError::unauthorized(codes::ERR_MISSING_USER_ID))?;
                    let station = state
                        .default_station()
                        .ok_or_else(|| ApiError::unauthorized(codes::ERR_SERVER_MISMATCH))?;
                    return Ok(AuthContext { user_id, station });
                } else {
                    return Err(ApiError::unauthorized(codes::ERR_MISSING_TOKEN));
                }
            }
        };

        let token = extract_bearer(parts)?;
        let token_data =
            verify_token::<AccessClaims>(jwks, token, &["sub", "sid", "exp", "iat"]).await?;
        let claims = token_data.claims;

        if claims.token_type != "access" {
            return Err(ApiError::unauthorized(codes::ERR_INVALID_TOKEN));
        }

        let station = state
            .get_station(&claims.sid)
            .ok_or_else(|| ApiError::unauthorized(codes::ERR_SERVER_MISMATCH))?;

        // Per-station per-user rate limit (keyed by user_id, not IP, so
        // multiple users behind the same NAT don't interfere with each other)
        if station.user_rate_limiter.check_key(&claims.sub).is_err() {
            return Err(ApiError::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                codes::ERR_RATE_LIMITED,
            ));
        }

        // Maintenance check (moved from middleware, needs the station)
        let method = &parts.method;
        let path = parts.uri.path();
        crate::check_maintenance(&station, method, path)?;

        let user_id = claims.sub.clone();

        // First auth since boot: upsert user row + auto-join default channels
        if !station.is_user_synced(&user_id) {
            let _ = crate::core::users::service::UserService::sync_identity(
                station.clone(),
                user_id.clone(),
                claims.name.clone(),
                claims.discriminator,
                claims.staff,
            )
            .await;
            let _ = crate::core::users::service::UserService::sync_channels(
                station.clone(),
                user_id.clone(),
            )
            .await;
            station.mark_user_synced(&user_id);
        }

        Ok(AuthContext {
            user_id: claims.sub,
            station,
        })
    }
}
