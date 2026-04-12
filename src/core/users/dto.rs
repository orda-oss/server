use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::core::models::Channel;

#[derive(Debug, Serialize)]
pub struct ChannelWithUnread {
    #[serde(flatten)]
    pub channel: Channel,
    pub unread_count: i64,
    pub last_read_message_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Validate)]
pub struct UserFilterDto {
    #[validate(length(min = 1))]
    pub username: Option<String>,
    pub status: Option<String>,
}
