use diesel::{
    backend::Backend,
    deserialize::{self, FromSql, FromSqlRow},
    expression::AsExpression,
    serialize::{self, Output, ToSql},
    sql_types::Text,
    sqlite::Sqlite,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Newtype wrapper that transparently serialises any `T: Serialize/Deserialize`
/// as a JSON string in a SQLite TEXT column.
///
/// Used for flexible metadata fields (e.g. `ChannelMetadata`, `RoleMetadata`)
/// where the schema can evolve without a migration - just add optional fields
/// to the Rust struct and the stored JSON round-trips cleanly.
///
/// `AsExpression<Text>` and `FromSqlRow<Text>` tell Diesel this type maps to
/// the SQL `TEXT` affinity, enabling it in `.filter()`, `.values()`, etc.
#[derive(Debug, Clone, Serialize, Deserialize, AsExpression, FromSqlRow)]
#[diesel(sql_type = Text)]
pub struct SqliteJson<T>(pub T);

impl<T> From<T> for SqliteJson<T> {
    fn from(t: T) -> Self {
        SqliteJson(t)
    }
}

impl<T> AsRef<T> for SqliteJson<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

/// Reading from DB: raw SQLite TEXT bytes -> deserialise JSON -> `SqliteJson<T>`.
/// `T: DeserializeOwned` (not `Deserialize<'_>`) because Diesel doesn't
/// guarantee the raw bytes live long enough for a borrowed deserialiser.
impl<T> FromSql<Text, Sqlite> for SqliteJson<T>
where
    T: DeserializeOwned,
{
    fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> deserialize::Result<Self> {
        let s = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?;
        let val = serde_json::from_str(&s)?;
        Ok(SqliteJson(val))
    }
}

/// Writing to DB: serialise `T` to a JSON string, hand it to Diesel as TEXT.
/// `T: Debug` is required by Diesel's `Output` infrastructure, not by us.
impl<T> ToSql<Text, Sqlite> for SqliteJson<T>
where
    T: Serialize + std::fmt::Debug,
{
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
        let s = serde_json::to_string(&self.0)?;
        out.set_value(s);
        Ok(serialize::IsNull::No)
    }
}
