use diesel::prelude::*;
use serde::Deserialize;
use validator::Validate;

use crate::{
    core::{
        models::{ChannelKind, ChannelMetadata},
        types::SqliteJson,
    },
    schema::channels,
};

#[derive(Deserialize, Validate)]
pub struct CreateChannelDto {
    #[validate(length(min = 2, max = 32))]
    pub name: String,
    pub kind: Option<ChannelKind>,
    pub is_private: Option<bool>,
    pub is_default: Option<bool>,
    pub metadata: Option<ChannelMetadata>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ChannelFilterDto {
    // Filters
    #[validate(length(min = 1, message = "Search term cannot be empty"))]
    pub name: Option<String>,
    pub kind: Option<ChannelKind>,
    pub is_archived: Option<bool>,
    pub is_private: Option<bool>,
    pub is_nsfw: Option<bool>,
    pub is_default: Option<bool>,

    // Sorting
    // Format: "name:asc", "created_at:desc"
    #[validate(custom(function = "validate_sort_format"))]
    pub sort: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct UpdateChannelDto {
    #[validate(length(min = 2, max = 32))]
    pub name: Option<String>,
    pub is_default: Option<bool>,
    pub is_private: Option<bool>,
    pub is_archived: Option<bool>,
    pub is_nsfw: Option<bool>,
    #[validate(range(min = 0, max = 10))]
    pub pin_limit: Option<i32>,
    pub metadata: Option<ChannelMetadata>,
}

/// Diesel changeset built from `UpdateChannelDto` after server-side processing
/// (slug generation, metadata merging). Kept separate from the DTO so the API
/// surface stays clean while Diesel gets exactly the columns it needs.
///
/// `treat_none_as_null = false` is critical: it makes Diesel skip `None` fields
/// in the UPDATE statement rather than setting them to NULL, giving us a true
/// partial-update / PATCH semantic.
#[derive(AsChangeset)]
#[diesel(table_name = channels)]
#[diesel(treat_none_as_null = false)]
pub struct UpdateChannelChangeset {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub is_default: Option<bool>,
    pub is_private: Option<bool>,
    pub is_archived: Option<bool>,
    pub is_nsfw: Option<bool>,
    pub pin_limit: Option<i32>,
    pub metadata: Option<SqliteJson<ChannelMetadata>>,
}

// Methods
fn validate_sort_format(sort: &str) -> Result<(), validator::ValidationError> {
    if sort.is_empty() {
        return Ok(());
    }

    let allowed_cols = ["name", "created_at"];
    let allowed_dirs = ["asc", "desc"];

    let parts: Vec<&str> = sort.split(':').collect();
    if parts.len() != 2 || !allowed_cols.contains(&parts[0]) || !allowed_dirs.contains(&parts[1]) {
        return Err(validator::ValidationError::new(
            "Invalid sort format. Use 'field:asc' or 'field:desc'",
        ));
    }
    Ok(())
}
