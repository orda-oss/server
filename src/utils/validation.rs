use std::collections::HashMap;

use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Query, Request},
    http::request::Parts,
};
use serde::de::DeserializeOwned;
use validator::Validate;

// Re-export auth extractors so existing imports still work
pub use crate::utils::auth::AuthContext;
use crate::utils::response::{ApiError, codes};

fn validation_error(errors: validator::ValidationErrors) -> ApiError {
    let error_map: HashMap<String, String> = errors
        .field_errors()
        .iter()
        .map(|(field, errs)| {
            let msg = errs
                .first()
                .map(|e| e.message.as_ref().unwrap_or(&e.code))
                .unwrap_or(&std::borrow::Cow::Borrowed("Invalid"));
            (field.to_string(), msg.to_string())
        })
        .collect();

    ApiError::unprocessable(codes::ERR_VALIDATION_FAILED)
        .with_details(serde_json::to_value(error_map).unwrap())
}

pub struct ValidatedJson<T>(pub T);

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Reject oversized bodies before buffering. Content-Length is set by
        // real clients; tower's oneshot tests also set it via Body::from.
        if let Some(len) = req.headers().get(axum::http::header::CONTENT_LENGTH)
            && let Ok(n) = len.to_str().unwrap_or("0").parse::<usize>()
            && n > crate::MAX_BODY_SIZE
        {
            return Err(ApiError::new(
                axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                codes::ERR_BODY_TOO_LARGE,
            ));
        }

        let Json(value) = Json::<T>::from_request(req, state).await.map_err(|e| {
            if e.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
                return ApiError::new(e.status(), codes::ERR_BODY_TOO_LARGE);
            }
            tracing::warn!("JSON Parse Error: {}", e);
            ApiError::bad_request(codes::ERR_JSON_PARSE)
        })?;

        if let Err(errors) = value.validate() {
            tracing::debug!("Validation failed: {:?}", errors);
            return Err(validation_error(errors));
        }

        Ok(ValidatedJson(value))
    }
}

pub struct ValidatedQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for ValidatedQuery<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 1. Parse Query
        let Query(value) = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::bad_request(codes::ERR_INVALID_QUERY))?;

        if let Err(errors) = value.validate() {
            return Err(validation_error(errors));
        }

        Ok(ValidatedQuery(value))
    }
}
