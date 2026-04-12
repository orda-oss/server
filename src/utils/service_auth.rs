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
struct ServiceClaims {
    #[serde(rename = "type")]
    pub token_type: String,
    pub sid: String,
}

// Resolves the station from the service JWT's sid claim.
// Used by per-server internal endpoints (revoke, sync_user, maintenance).
pub struct ServiceAuthContext {
    pub station: Arc<Station>,
}

impl FromRequestParts<Arc<Orbit>> for ServiceAuthContext {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<Orbit>,
    ) -> Result<Self, Self::Rejection> {
        let jwks = match &state.jwks {
            Some(jwks) => jwks,
            None => {
                if cfg!(debug_assertions) {
                    let station = state
                        .default_station()
                        .ok_or_else(|| ApiError::unauthorized(codes::ERR_MISSING_TOKEN))?;
                    return Ok(ServiceAuthContext { station });
                }
                return Err(ApiError::unauthorized(codes::ERR_MISSING_TOKEN));
            }
        };

        let token = extract_bearer(parts)?;
        let token_data = verify_token::<ServiceClaims>(jwks, token, &["exp", "iat"]).await?;

        if token_data.claims.token_type != "service" {
            return Err(ApiError::unauthorized(codes::ERR_INVALID_TOKEN));
        }

        let station = state
            .get_station(&token_data.claims.sid)
            .ok_or_else(|| ApiError::unauthorized(codes::ERR_SERVER_MISMATCH))?;

        Ok(ServiceAuthContext { station })
    }
}

// VM-level service auth: verifies the JWT is a valid service token
// but does NOT require a matching station. Used by provision/deprovision
// endpoints where the target server may not exist yet (or anymore).
pub struct ServiceAuth;

impl FromRequestParts<Arc<Orbit>> for ServiceAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<Orbit>,
    ) -> Result<Self, Self::Rejection> {
        let jwks = match &state.jwks {
            Some(jwks) => jwks,
            None => {
                if cfg!(debug_assertions) {
                    return Ok(ServiceAuth);
                }
                return Err(ApiError::unauthorized(codes::ERR_MISSING_TOKEN));
            }
        };

        let token = extract_bearer(parts)?;
        let token_data = verify_token::<ServiceClaims>(jwks, token, &["exp", "iat"]).await?;

        if token_data.claims.token_type != "service" {
            return Err(ApiError::unauthorized(codes::ERR_INVALID_TOKEN));
        }

        // No station lookup -- the JWT sid may reference any server in the org.
        // The caller (semerkant) is trusted once the JWT signature is verified.
        Ok(ServiceAuth)
    }
}
