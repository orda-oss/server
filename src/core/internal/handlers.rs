use std::sync::Arc;

use axum::{Json, http::StatusCode, response::IntoResponse};
use diesel::prelude::*;
use serde::Deserialize;

use crate::{
    Orbit, Station,
    core::satellite::{UserCommand, types::ServerEvent},
    utils::service_auth::{ServiceAuth, ServiceAuthContext},
};

#[derive(Deserialize)]
pub struct RevokePayload {
    pub user_id: String,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct MaintenancePayload {
    pub enabled: bool,
}

pub async fn revoke(
    ServiceAuthContext { station }: ServiceAuthContext,
    Json(payload): Json<RevokePayload>,
) -> impl IntoResponse {
    let user_id = payload.user_id;
    let reason = payload.reason.unwrap_or_else(|| "kicked".to_string());
    let uid = user_id.clone();

    let station_c = station.clone();
    let _ = tokio::task::spawn_blocking(move || {
        use crate::schema::{channel_members, server_members};

        let mut conn = match station_c.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to get DB connection for revoke");
                return;
            }
        };

        let cm_deleted =
            diesel::delete(channel_members::table.filter(channel_members::user_id.eq(&uid)))
                .execute(&mut conn)
                .unwrap_or(0);

        let sm_deleted =
            diesel::delete(server_members::table.filter(server_members::user_id.eq(&uid)))
                .execute(&mut conn)
                .unwrap_or(0);

        tracing::info!(
            user_id = %uid,
            channel_members_removed = cm_deleted,
            server_members_removed = sm_deleted,
            "User revoked"
        );
    })
    .await;

    // Clear sync cache so re-joining triggers sync_channels again (auto-join default channels)
    station.satellite.clear_user_synced(&user_id);

    station
        .satellite
        .send_user_command(&user_id, UserCommand::Disconnect(reason));

    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
pub struct SyncUserPayload {
    pub user_id: String,
    pub username: String,
    pub discriminator: i32,
    pub staff: bool,
}

pub async fn sync_user(
    ServiceAuthContext { station }: ServiceAuthContext,
    Json(payload): Json<SyncUserPayload>,
) -> impl IntoResponse {
    let uid = payload.user_id.clone();
    let uname = payload.username.clone();
    let disc = payload.discriminator;
    let is_staff = payload.staff;

    let station_c = station.clone();
    let rows = tokio::task::spawn_blocking(move || {
        use crate::{schema::users::dsl::*, utils::helpers::now_rfc3339};

        let mut conn = match station_c.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to get DB connection for sync_user");
                return 0usize;
            }
        };

        let rows = diesel::update(users.filter(remote_id.eq(&uid)))
            .set((
                username.eq(&uname),
                discriminator.eq(disc),
                staff.eq(is_staff),
                updated_at.eq(now_rfc3339()),
            ))
            .execute(&mut conn)
            .unwrap_or(0);

        tracing::info!(user_id = %uid, username = %uname, rows, "User synced from semerkant");
        rows
    })
    .await
    .unwrap_or(0);

    if rows > 0 {
        station
            .satellite
            .broadcast_server(&ServerEvent::UserUpdated {
                user_id: payload.user_id,
                username: payload.username,
                discriminator: payload.discriminator,
                staff: payload.staff,
            });
    }

    StatusCode::NO_CONTENT
}

// Dynamic provisioning: create a new Station at runtime
#[derive(Deserialize)]
pub struct ProvisionPayload {
    pub server_id: String,
    pub encryption_key: String,
    pub name: String,
}

pub async fn provision(
    _auth: ServiceAuth,
    axum::extract::State(orbit): axum::extract::State<Arc<Orbit>>,
    Json(payload): Json<ProvisionPayload>,
) -> impl IntoResponse {
    // Validate server_id is a UUID (used in DB file path)
    if uuid::Uuid::parse_str(&payload.server_id).is_err() {
        return StatusCode::BAD_REQUEST;
    }

    // Already provisioned
    if orbit.stations.contains_key(&payload.server_id) {
        tracing::info!(server_id = %payload.server_id, "Station already exists, skipping provision");
        return StatusCode::OK;
    }

    tracing::info!(
        server_id = %payload.server_id,
        name = %payload.name,
        "Provisioning new station..."
    );

    let station = match Station::new(
        &payload.server_id,
        &payload.encryption_key,
        payload.name.clone(),
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create station for provision");
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    };

    orbit.stations.insert(payload.server_id.clone(), station);

    // No need to spawn heartbeat -- the batch task picks up new stations automatically
    tracing::info!(server_id = %payload.server_id, name = %payload.name, "Station provisioned");
    StatusCode::CREATED
}

// Dynamic deprovisioning: remove a Station at runtime
pub async fn deprovision(
    _auth: ServiceAuth,
    axum::extract::State(orbit): axum::extract::State<Arc<Orbit>>,
    axum::extract::Path(server_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let Some((_, station)) = orbit.stations.remove(&server_id) else {
        return StatusCode::NOT_FOUND;
    };

    tracing::info!(server_id = %server_id, "Deprovisioning station...");

    // Signal WS connections to close, then wait briefly for them to drain
    let _ = station.shutdown.send(true);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Flush WAL before dropping the pool
    if let Ok(mut conn) = station.pool.get() {
        let _ = diesel::sql_query("PRAGMA wal_checkpoint(TRUNCATE);").execute(&mut conn);
    }

    tracing::info!(server_id = %server_id, "Station deprovisioned");
    StatusCode::NO_CONTENT
}

pub async fn maintenance(
    ServiceAuthContext { station }: ServiceAuthContext,
    Json(payload): Json<MaintenancePayload>,
) -> impl IntoResponse {
    station.set_maintenance(payload.enabled);

    let event = if payload.enabled {
        ServerEvent::MaintenanceStarted
    } else {
        ServerEvent::MaintenanceEnded
    };
    station.satellite.broadcast_server(&event);

    tracing::info!(enabled = payload.enabled, "Maintenance mode changed");
    StatusCode::NO_CONTENT
}
