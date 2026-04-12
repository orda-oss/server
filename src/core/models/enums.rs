use diesel::{
    backend::Backend,
    deserialize::{self, FromSql, FromSqlRow},
    expression::AsExpression,
    serialize::{self, Output, ToSql},
    sql_types::Text,
    sqlite::Sqlite,
};
use serde::{Deserialize, Serialize};

// --- Enums ---
//
// Each enum that maps to a DB column needs manual `ToSql`/`FromSql` impls
// because Diesel doesn't know how to convert custom Rust types to SQLite TEXT.
// `AsExpression<Text>` + `FromSqlRow<Text>` make them usable in query DSL.
// Values are stored as lowercase snake_case strings (matches `#[serde(rename_all)]`).
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    #[default]
    Offline,
    Online,
    Away,
    Busy,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, AsExpression, FromSqlRow)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    #[default]
    Text,
    Voice,
    Broadcast,
}
impl ToSql<Text, Sqlite> for ChannelKind {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
        let s = match self {
            ChannelKind::Text => "text",
            ChannelKind::Voice => "voice",
            ChannelKind::Broadcast => "broadcast",
        };
        out.set_value(s);
        Ok(serialize::IsNull::No)
    }
}
impl FromSql<Text, Sqlite> for ChannelKind {
    fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> deserialize::Result<Self> {
        let s = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?;
        match s.as_str() {
            "text" => Ok(ChannelKind::Text),
            "voice" => Ok(ChannelKind::Voice),
            "broadcast" => Ok(ChannelKind::Broadcast),
            // Unknown values fall back to the default rather than hard-erroring,
            // which keeps the app alive if the DB ever contains a future variant.
            _ => Ok(ChannelKind::Text),
        }
    }
}

/// Persisted message kinds. Ephemeral/transient events (typing indicators,
/// server error feedback) are WS-only and never stored in this column.
#[derive(Debug, Serialize, Deserialize, Default, Clone, AsExpression, FromSqlRow)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// Regular user-composed message.
    #[default]
    Text,
    /// Server-generated notice visible to all members (e.g. "User joined").
    System,
    /// Structured event payload (e.g. reactions, pins) for client-side rendering.
    Event,
}
impl ToSql<Text, Sqlite> for MessageKind {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
        let s = match self {
            MessageKind::Text => "text",
            MessageKind::System => "system",
            MessageKind::Event => "event",
        };
        out.set_value(s);
        Ok(serialize::IsNull::No)
    }
}
impl FromSql<Text, Sqlite> for MessageKind {
    fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> deserialize::Result<Self> {
        let s = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?;
        match s.as_str() {
            "text" => Ok(MessageKind::Text),
            "system" => Ok(MessageKind::System),
            "event" => Ok(MessageKind::Event),
            _ => Ok(MessageKind::Text),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, AsExpression, FromSqlRow)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    #[default]
    Other,
    Mention,
    Reply,
    Announcement,
}
impl ToSql<Text, Sqlite> for NotificationKind {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
        let s = match self {
            NotificationKind::Other => "other",
            NotificationKind::Mention => "mention",
            NotificationKind::Reply => "reply",
            NotificationKind::Announcement => "announcement",
        };
        out.set_value(s);
        Ok(serialize::IsNull::No)
    }
}
impl FromSql<Text, Sqlite> for NotificationKind {
    fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> deserialize::Result<Self> {
        let s = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?;
        match s.as_str() {
            "mention" => Ok(NotificationKind::Mention),
            "reply" => Ok(NotificationKind::Reply),
            "announcement" => Ok(NotificationKind::Announcement),
            _ => Ok(NotificationKind::Other),
        }
    }
}
