use std::sync::Arc;

use diesel::prelude::*;

use crate::{
    Station,
    core::{
        models::{Server, ServerMetadata},
        servers::dto::ServerFilterDto,
        types::SqliteJson,
    },
    utils::{
        helpers::now_rfc3339,
        response::{ApiError, ApiResponse, ApiResult},
    },
};

pub struct ServerService;

impl ServerService {
    pub async fn list(station: Arc<Station>, filter: ServerFilterDto) -> ApiResult<Vec<Server>> {
        use crate::schema::servers::dsl::*;

        let server_list = tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().map_err(ApiError::internal)?;

            let mut query = servers.into_boxed();

            if let Some(filter_name) = filter.name {
                query = query.filter(name.eq(filter_name));
            }

            let raw_results = query
                .select(Server::as_select())
                .load(&mut conn)
                .map_err(ApiError::internal)?;

            let api_servers: Vec<Server> = raw_results.into_iter().collect();

            Ok(api_servers)
        })
        .await
        .map_err(ApiError::internal)??;

        Ok(ApiResponse::ok(server_list))
    }
}

impl Server {
    /// alacahoyuk is a single-server binary - there is always exactly one `Server`
    /// row in the DB with this fixed ID. Using a constant rather than auto-
    /// generating a UUID avoids needing a "get the server ID" query on every
    /// request that needs to scope data to this instance.
    pub(crate) const SINGLETON_ID: &'static str = "main";

    pub(crate) fn new(name: String, remote_id: Option<String>) -> Self {
        let id = Self::SINGLETON_ID.to_string();
        let now = now_rfc3339();

        let empty_metadata = ServerMetadata {
            icon_url: None,
            banner_url: None,
        };

        Server {
            id,
            remote_id,
            name,
            metadata: SqliteJson(empty_metadata),
            created_at: Some(now.clone()),
            updated_at: Some(now),
            last_event_cursor: None,
            cert_version: None,
        }
    }

    /// Called once at startup to bootstrap the server record.
    /// `remote_id` comes from semerkant's activate response (the server UUID in semerkant).
    /// Updated on every startup to keep it in sync.
    pub fn get_or_create(
        conn: &mut SqliteConnection,
        server_name: String,
        server_remote_id: Option<String>,
    ) -> Result<Server, diesel::result::Error> {
        use crate::schema::servers::dsl::*;

        let existing = servers
            .find(Self::SINGLETON_ID)
            .first::<Server>(conn)
            .optional()?;

        match existing {
            Some(mut server) => {
                // Update remote_id if it changed (e.g., first activate after dev mode)
                if server_remote_id.is_some() && server.remote_id != server_remote_id {
                    diesel::update(servers.find(Self::SINGLETON_ID))
                        .set(remote_id.eq(&server_remote_id))
                        .execute(conn)?;
                    server.remote_id = server_remote_id;
                    tracing::info!(remote_id = ?server.remote_id, "Updated server remote_id");
                }
                tracing::info!(remote_id = ?server.remote_id, "Found existing server: {}", server.name);
                Ok(server)
            }
            None => {
                tracing::info!(remote_id = ?server_remote_id, "Creating new server: {}", server_name);
                let new_server = Server::new(server_name, server_remote_id);

                diesel::insert_into(servers)
                    .values(&new_server)
                    .execute(conn)?;

                Ok(new_server)
            }
        }
    }
}
