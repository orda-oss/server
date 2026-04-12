use serde::Deserialize;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct ServerFilterDto {
    #[validate(length(min = 1))]
    pub name: Option<String>,
}
