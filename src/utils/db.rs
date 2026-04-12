use diesel::{connection::SimpleConnection, r2d2::CustomizeConnection, sqlite::SqliteConnection};

#[derive(Debug, Clone)]
pub(crate) struct SqliteConnectionCustomizer {
    pub db_key: String,
}

impl CustomizeConnection<SqliteConnection, diesel::r2d2::Error> for SqliteConnectionCustomizer {
    fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), diesel::r2d2::Error> {
        // Set the encryption key
        conn.batch_execute(&format!(
            "{} = '{}';",
            obfstr::obfstr!("PRAGMA key"),
            self.db_key
        ))
        .map_err(diesel::r2d2::Error::QueryError)?;

        // Set other pragmas
        conn.batch_execute(
            r#"
                PRAGMA busy_timeout = 5000;
                PRAGMA foreign_keys = ON;
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = NORMAL;
            "#,
        )
        .map_err(diesel::r2d2::Error::QueryError)?;

        Ok(())
    }
}
