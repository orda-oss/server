use serde::{Deserialize, Serialize};

// --- Metadata structs ---
//
// Pure Rust structs stored as JSON text via `SqliteJson<T>`.
// Adding optional fields here is schema-free - no migration needed.

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerMetadata {
    pub icon_url: Option<String>,
    pub banner_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelMetadata {
    pub topic: Option<String>,
    pub user_limit: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleMetadata {
    pub icon_url: Option<String>,
    pub description: Option<String>,
}
