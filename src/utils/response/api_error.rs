use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;

// Import from sibling modules
use super::{api_response::ApiResponse, codes};

#[derive(Debug)]
pub struct ApiError {
    pub(crate) status_code: StatusCode,
    pub(crate) message: String,
    pub(crate) details: Option<Value>,
}

impl ApiError {
    pub fn new(status_code: StatusCode, error: impl Into<String>) -> Self {
        Self {
            status_code,
            message: error.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn internal(err: impl std::fmt::Display) -> Self {
        tracing::error!("Internal Server Error: {}", err);

        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            codes::ERR_INTERNAL_SERVER_ERROR,
        )
    }

    pub fn bad_request(code: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code)
    }

    pub fn unauthorized(code: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code)
    }

    pub fn forbidden(code: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code)
    }

    pub fn not_found(code: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code)
    }

    pub fn conflict(code: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code)
    }

    pub fn unprocessable(code: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code)
    }

    pub fn service_unavailable(code: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code)
    }

    pub fn maintenance() -> Self {
        Self::service_unavailable("Server is in maintenance mode")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.status_code.is_server_error() {
            tracing::error!(status = %self.status_code, code = %self.message, "API error");
        } else {
            tracing::warn!(status = %self.status_code, code = %self.message, "API error");
        }

        let body =
            ApiResponse::<()>::error_with_details(self.status_code, self.message, self.details);
        body.into_response()
    }
}
