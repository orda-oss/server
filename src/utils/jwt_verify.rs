use axum::http::request::Parts;
use jsonwebtoken::{Algorithm, DecodingKey, TokenData, Validation, decode_header};
use serde::de::DeserializeOwned;

use crate::utils::{
    jwks::JwksCache,
    response::{ApiError, codes},
};

pub fn extract_bearer(parts: &Parts) -> Result<&str, ApiError> {
    parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError::unauthorized(codes::ERR_MISSING_TOKEN))
}

pub async fn verify_token<T: DeserializeOwned>(
    jwks: &JwksCache,
    token: &str,
    required_claims: &[&str],
) -> Result<TokenData<T>, ApiError> {
    let header =
        decode_header(token).map_err(|_| ApiError::unauthorized(codes::ERR_INVALID_TOKEN))?;

    let kid = header.kid.unwrap_or_default();
    let key: DecodingKey = jwks
        .get_key(&kid)
        .await
        .ok_or_else(|| ApiError::unauthorized(codes::ERR_INVALID_TOKEN))?;

    let mut validation = Validation::new(Algorithm::ES384);
    validation.set_required_spec_claims(required_claims);

    jsonwebtoken::decode::<T>(token, &key, &validation).map_err(|e| {
        let code = if e.to_string().contains("ExpiredSignature") {
            codes::ERR_EXPIRED_TOKEN
        } else {
            codes::ERR_INVALID_TOKEN
        };
        ApiError::unauthorized(code)
    })
}

pub fn verify_server_id(orbit_remote_id: Option<&str>, token_sid: &str) -> Result<(), ApiError> {
    if orbit_remote_id != Some(token_sid) {
        return Err(ApiError::unauthorized(codes::ERR_SERVER_MISMATCH));
    }
    Ok(())
}
