pub mod api_error;
pub mod api_response;
pub mod codes;

pub use api_error::ApiError;
pub use api_response::ApiResponse;

pub type ApiResult<T> = Result<ApiResponse<T>, ApiError>;
