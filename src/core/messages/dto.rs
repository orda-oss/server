use serde::Deserialize;
use validator::Validate;

/// `sender_id` is supplied by the caller until semerkant auth middleware injects
/// it from a verified JWT. Once auth lands, this field will be removed and the
/// ID will come from `Extension(claims.user_id)`.
#[derive(Deserialize, Validate)]
pub struct CreateMessageDto {
    #[validate(length(min = 1, max = 10000))]
    pub content: String,
    pub sender_id: String,
}

#[derive(Deserialize, Validate)]
pub struct EditMessageDto {
    #[validate(length(min = 1, max = 10000))]
    pub content: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct MessageFilterDto {
    pub channel_id: Option<String>,

    #[validate(length(min = 1))]
    pub content: Option<String>, // Search within messages

    // Pagination (Simple limit/offset for now)
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<i64>,

    #[validate(range(min = 0))]
    pub offset: Option<i64>,
}
