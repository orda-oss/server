use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum ResponseContent<T> {
    Success {
        data: T,
    },
    Error {
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<Value>,
    },
}

#[derive(Debug, Serialize)]
pub(crate) struct InnerResponse<T: Serialize> {
    #[serde(flatten)]
    content: ResponseContent<T>,
}

#[derive(Debug)]
pub struct ApiResponse<T: Serialize = ()> {
    pub(crate) status_code: StatusCode,
    pub(crate) response: InnerResponse<T>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(status_code: StatusCode, data: T) -> Self {
        Self {
            status_code,
            response: InnerResponse {
                content: ResponseContent::Success { data },
            },
        }
    }

    #[allow(dead_code)]
    pub fn error(status_code: StatusCode, error: impl Into<String>) -> Self {
        Self::error_with_details(status_code, error, None)
    }

    pub fn error_with_details(
        status_code: StatusCode,
        error: impl Into<String>,
        details: Option<Value>,
    ) -> Self {
        Self {
            status_code,
            response: InnerResponse {
                content: ResponseContent::Error {
                    error: error.into(),
                    details,
                },
            },
        }
    }

    pub fn ok(data: T) -> Self {
        Self::success(StatusCode::OK, data)
    }

    pub fn created(data: T) -> Self {
        Self::success(StatusCode::CREATED, data)
    }
}

impl ApiResponse<()> {
    pub fn empty() -> ApiResponse<()> {
        ApiResponse::success(StatusCode::NO_CONTENT, ())
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        if self.status_code == StatusCode::NO_CONTENT {
            return self.status_code.into_response();
        }

        (self.status_code, Json(self.response)).into_response()
    }
}
