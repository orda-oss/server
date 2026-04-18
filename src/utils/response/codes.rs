// Generic Errors
pub const ERR_JSON_PARSE: &str = "ERR_JSON_PARSE";
pub const ERR_INVALID_QUERY: &str = "ERR_INVALID_QUERY";
pub const ERR_VALIDATION_FAILED: &str = "ERR_VALIDATION_FAILED";
pub const ERR_INTERNAL_SERVER_ERROR: &str = "ERR_INTERNAL_SERVER_ERROR";

// Auth Errors
pub const ERR_MISSING_USER_ID: &str = "ERR_MISSING_USER_ID";
pub const ERR_MISSING_TOKEN: &str = "ERR_MISSING_TOKEN";
pub const ERR_INVALID_TOKEN: &str = "ERR_INVALID_TOKEN";
pub const ERR_EXPIRED_TOKEN: &str = "ERR_EXPIRED_TOKEN";
pub const ERR_SERVER_MISMATCH: &str = "ERR_SERVER_MISMATCH";

// Generic Auth
pub const ERR_FORBIDDEN: &str = "ERR_FORBIDDEN";

// Channel Errors
pub const ERR_CHANNEL_ALREADY_EXISTS: &str = "ERR_CHANNEL_ALREADY_EXISTS";
pub const ERR_CHANNEL_NOT_FOUND: &str = "ERR_CHANNEL_NOT_FOUND";
pub const ERR_CHANNEL_NOT_A_MEMBER: &str = "ERR_CHANNEL_NOT_A_MEMBER";
pub const ERR_CHANNEL_OWNER_CANNOT_LEAVE: &str = "ERR_CHANNEL_OWNER_CANNOT_LEAVE";

// Message Errors
pub const ERR_MESSAGE_NOT_FOUND: &str = "ERR_MESSAGE_NOT_FOUND";

// Voice Errors
pub const ERR_SCREENSHARE_IN_USE: &str = "ERR_SCREENSHARE_IN_USE";

// Permission Errors
pub const ERR_MISSING_PERMISSION: &str = "ERR_MISSING_PERMISSION";
pub const ERR_ROLE_NOT_FOUND: &str = "ERR_ROLE_NOT_FOUND";
pub const ERR_ROLE_PROTECTED: &str = "ERR_ROLE_PROTECTED";
/// Built-in Admin role: only the server owner can edit it. Non-owner
/// administrators are blocked to prevent them from stripping ADMINISTRATOR
/// from the foundational role and locking everyone out.
pub const ERR_ROLE_ADMIN_PROTECTED: &str = "ERR_ROLE_ADMIN_PROTECTED";
/// Role priority hierarchy violation: actor attempted to edit, delete, or
/// assign a role at or above their own role's priority.
pub const ERR_PRIORITY_BLOCKED: &str = "ERR_PRIORITY_BLOCKED";
pub const ERR_CHANNEL_NOT_ARCHIVED: &str = "ERR_CHANNEL_NOT_ARCHIVED";

// Rate Limiting
pub const ERR_RATE_LIMITED: &str = "ERR_RATE_LIMITED";
pub const ERR_BODY_TOO_LARGE: &str = "ERR_BODY_TOO_LARGE";
