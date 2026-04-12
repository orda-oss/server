use std::sync::Arc;

use diesel::prelude::*;
use serde::Deserialize;

use crate::{
    Station, VERSION,
    core::satellite::{UserCommand, types::ServerEvent},
    schema::{channel_members, channels, server_members, servers, users},
    utils::helpers::now_rfc3339,
};

pub struct ActivateServerInfo {
    pub server_id: String,
    pub encryption_key: String,
    pub name: String,
}

pub struct ActivateResponse {
    pub servers: Vec<ActivateServerInfo>,
    pub domain: Option<String>,
}

pub async fn activate_with_semerkant(
    base_url: &str,
    license_key: &str,
    port: u16,
) -> Result<ActivateResponse, String> {
    let url = format!("{}/provision/activate", base_url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let res = client
        .post(&url)
        .bearer_auth(license_key)
        .json(&serde_json::json!({ "port": port, "version": VERSION }))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    let body: serde_json::Value = res.json().await.map_err(|e| format!("invalid JSON: {e}"))?;

    let domain = body["data"]["domain"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let servers_json = body["data"]["servers"]
        .as_array()
        .ok_or_else(|| "missing servers array in response".to_string())?;

    let mut servers = Vec::new();
    for s in servers_json {
        let server_id = s["server_id"]
            .as_str()
            .ok_or("missing server_id")?
            .to_string();
        let encryption_key = s["encryption_key"]
            .as_str()
            .ok_or("missing encryption_key")?
            .to_string();
        let name = s["name"].as_str().unwrap_or("server").to_string();
        servers.push(ActivateServerInfo {
            server_id,
            encryption_key,
            name,
        });
    }

    if servers.is_empty() {
        return Err("no servers in activation response".to_string());
    }

    Ok(ActivateResponse { servers, domain })
}

// Heartbeat response types
#[derive(Deserialize)]
struct HeartbeatResponse {
    data: HeartbeatData,
}

#[derive(Deserialize)]
struct HeartbeatData {
    events: Vec<MembershipEventDto>,
    has_more: bool,
    maintenance: Option<bool>,
    cert_version: Option<i64>,
}

#[derive(Deserialize)]
struct MembershipEventDto {
    id: i64,
    event_type: String,
    user_id: String,
    username: String,
    #[serde(default = "default_discriminator")]
    discriminator: i32,
    #[serde(default)]
    staff: bool,
}

fn default_discriminator() -> i32 {
    9999
}

/// Single heartbeat task that iterates all stations in the DashMap.
/// New stations added via /internal/provision are picked up on the next tick.
/// Deprovisioned stations are excluded automatically (removed from DashMap).
pub fn spawn_heartbeat_task(orbit: Arc<crate::Orbit>, semerkant_url: String, license_key: String) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let heartbeat_url = format!("{}/provision/heartbeat", semerkant_url);
        let cert_url = format!("{}/provision/certificate", semerkant_url);
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5 * 60));

        loop {
            // First tick fires immediately (marks servers online after DB init).
            // Subsequent ticks every 5 min.
            interval.tick().await;
            for entry in orbit.stations.iter() {
                run_heartbeat(
                    entry.value(),
                    &client,
                    &heartbeat_url,
                    &cert_url,
                    &license_key,
                )
                .await;
            }
        }
    });
}

async fn run_heartbeat(
    station: &Arc<Station>,
    client: &reqwest::Client,
    url: &str,
    cert_url: &str,
    license_key: &str,
) {
    let mut cursor = load_cursor(station).await;

    loop {
        let server_id = station.server.remote_id.as_deref().unwrap_or("");
        let body = match cursor {
            Some(c) => {
                serde_json::json!({ "server_id": server_id, "cursor": c, "version": VERSION })
            }
            None => serde_json::json!({ "server_id": server_id, "version": VERSION }),
        };

        let res = match client
            .post(url)
            .bearer_auth(license_key)
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "Heartbeat failed");
                return;
            }
        };

        if !res.status().is_success() {
            tracing::warn!(status = %res.status(), "Heartbeat rejected");
            return;
        }

        let parsed: HeartbeatResponse = match res.json().await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse heartbeat response");
                return;
            }
        };

        let events = parsed.data.events;
        let has_more = parsed.data.has_more;

        // Sync maintenance state from semerkant (backup for push failures)
        if let Some(maintenance) = parsed.data.maintenance
            && maintenance != station.is_maintenance()
        {
            station.set_maintenance(maintenance);
            let event = if maintenance {
                crate::core::satellite::types::ServerEvent::MaintenanceStarted
            } else {
                crate::core::satellite::types::ServerEvent::MaintenanceEnded
            };
            station.satellite.broadcast_server(&event);
            tracing::info!(
                enabled = maintenance,
                "Maintenance mode synced from heartbeat"
            );
        }

        // Check cert version
        if let Some(remote_version) = parsed.data.cert_version {
            check_cert_version(station, client, cert_url, license_key, remote_version).await;
        }

        if events.is_empty() {
            tracing::debug!("Heartbeat OK, no new events");
            return;
        }

        let last_id = events.last().map(|e| e.id);
        process_events(station, &events).await;

        if let Some(id) = last_id {
            let id32 = i32::try_from(id).unwrap_or(i32::MAX);
            save_cursor(station, id32).await;
            cursor = Some(id32);
        }

        if !has_more {
            tracing::debug!(events_processed = events.len(), "Heartbeat sync complete");
            return;
        }
    }
}

async fn load_cursor(station: &Arc<Station>) -> Option<i32> {
    let pool = station.pool.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().ok()?;
        servers::table
            .find("main")
            .select(servers::last_event_cursor)
            .first::<Option<i32>>(&mut conn)
            .ok()?
    })
    .await
    .unwrap_or(None)
}

async fn save_cursor(station: &Arc<Station>, new_cursor: i32) {
    let pool = station.pool.clone();
    tokio::task::spawn_blocking(move || {
        if let Ok(mut conn) = pool.get() {
            let _ = diesel::update(servers::table.find("main"))
                .set(servers::last_event_cursor.eq(Some(new_cursor)))
                .execute(&mut conn);
        }
    })
    .await
    .ok();
}

async fn process_events(station: &Arc<Station>, events: &[MembershipEventDto]) {
    for event in events {
        match event.event_type.as_str() {
            "member_added" => handle_member_added(station, event).await,
            "member_removed" => handle_member_removed(station, event).await,
            _ => tracing::warn!(event_type = %event.event_type, "Unknown membership event"),
        }
    }
}

async fn handle_member_added(station: &Arc<Station>, event: &MembershipEventDto) {
    let uid = event.user_id.clone();
    let uname = event.username.clone();
    let disc = event.discriminator;
    let is_staff = event.staff;
    let pool = station.pool.clone();

    let newly_joined: Vec<String> = tokio::task::spawn_blocking(move || {
        let mut conn = match pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = ?e, "DB connection failed for member_added");
                return vec![];
            }
        };

        // Upsert user
        let _ = diesel::insert_into(users::table)
            .values((
                users::id.eq(&uid),
                users::remote_id.eq(&uid),
                users::username.eq(&uname),
                users::discriminator.eq(disc),
                users::staff.eq(is_staff),
                users::created_at.eq(now_rfc3339()),
            ))
            .on_conflict(users::remote_id)
            .do_update()
            .set((
                users::username.eq(&uname),
                users::discriminator.eq(disc),
                users::staff.eq(is_staff),
                users::updated_at.eq(now_rfc3339()),
            ))
            .execute(&mut conn);

        // Upsert server_member
        let _ = diesel::insert_into(server_members::table)
            .values((
                server_members::server_id.eq("main"),
                server_members::user_id.eq(&uid),
                server_members::metadata.eq("{}"),
                server_members::joined_at.eq(now_rfc3339()),
            ))
            .on_conflict((server_members::server_id, server_members::user_id))
            .do_nothing()
            .execute(&mut conn);

        // Auto-join default channels
        let default_channels: Vec<String> = channels::table
            .filter(channels::is_default.eq(true))
            .select(channels::id)
            .load(&mut conn)
            .unwrap_or_default();

        let mut newly_joined = vec![];
        let now = now_rfc3339();
        for ch_id in default_channels {
            let inserted = diesel::insert_into(channel_members::table)
                .values((
                    channel_members::channel_id.eq(&ch_id),
                    channel_members::user_id.eq(&uid),
                    channel_members::settings.eq("{}"),
                    channel_members::joined_at.eq(&now),
                ))
                .on_conflict((channel_members::channel_id, channel_members::user_id))
                .do_nothing()
                .execute(&mut conn)
                .unwrap_or(0);
            if inserted > 0 {
                newly_joined.push(ch_id);
            }
        }
        newly_joined
    })
    .await
    .unwrap_or_default();

    station
        .satellite
        .broadcast_server(&ServerEvent::MemberJoined {
            user_id: event.user_id.clone(),
        });

    tracing::info!(
        user_id = %event.user_id,
        username = %event.username,
        channels_joined = newly_joined.len(),
        "Member added via heartbeat sync"
    );
}

async fn handle_member_removed(station: &Arc<Station>, event: &MembershipEventDto) {
    let uid = event.user_id.clone();
    let pool = station.pool.clone();
    let uid_db = uid.clone();

    tokio::task::spawn_blocking(move || {
        let mut conn = match pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = ?e, "DB connection failed for member_removed");
                return;
            }
        };

        let cm =
            diesel::delete(channel_members::table.filter(channel_members::user_id.eq(&uid_db)))
                .execute(&mut conn)
                .unwrap_or(0);

        let sm = diesel::delete(server_members::table.filter(server_members::user_id.eq(&uid_db)))
            .execute(&mut conn)
            .unwrap_or(0);

        tracing::info!(
            user_id = %uid_db,
            channel_members_removed = cm,
            server_members_removed = sm,
            "Member removed via heartbeat sync"
        );
    })
    .await
    .ok();

    station.satellite.clear_user_synced(&uid);
    station.satellite.send_user_command(
        &uid,
        UserCommand::Disconnect("membership_revoked".to_string()),
    );
}

// TLS cert management

fn tls_dir() -> String {
    // Derive from DATA_DIR's sibling: /opt/alacahoyuk/data -> /opt/alacahoyuk/tls
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "/opt/alacahoyuk/data".to_string());
    let base = std::path::Path::new(&data_dir)
        .parent()
        .unwrap_or(std::path::Path::new("/opt/alacahoyuk"));
    base.join("tls").to_string_lossy().to_string()
}

async fn load_cert_version(station: &Arc<Station>) -> i32 {
    let pool = station.pool.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().ok()?;
        servers::table
            .find("main")
            .select(servers::cert_version)
            .first::<Option<i32>>(&mut conn)
            .ok()?
    })
    .await
    .unwrap_or(None)
    .unwrap_or(0)
}

async fn save_cert_version(station: &Arc<Station>, version: i32) {
    let pool = station.pool.clone();
    tokio::task::spawn_blocking(move || {
        if let Ok(mut conn) = pool.get() {
            let _ = diesel::update(servers::table.find("main"))
                .set(servers::cert_version.eq(version))
                .execute(&mut conn);
        }
    })
    .await
    .ok();
}

#[derive(Deserialize)]
struct CertResponse {
    data: CertData,
}

#[derive(Deserialize)]
struct CertData {
    cert_version: i64,
    certificate: String,
    private_key: String,
}

async fn check_cert_version(
    station: &Arc<Station>,
    client: &reqwest::Client,
    cert_url: &str,
    license_key: &str,
    remote_version: i64,
) {
    let local_version = load_cert_version(station).await;
    let remote_i32 = i32::try_from(remote_version).unwrap_or(i32::MAX);

    if remote_i32 <= local_version {
        return;
    }

    tracing::info!(
        local = local_version,
        remote = remote_i32,
        "New cert available, fetching"
    );

    let res = match client.get(cert_url).bearer_auth(license_key).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to fetch certificate");
            return;
        }
    };

    if !res.status().is_success() {
        tracing::warn!(status = %res.status(), "Certificate fetch rejected");
        return;
    }

    let parsed: CertResponse = match res.json().await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to parse certificate response");
            return;
        }
    };

    if write_cert_files(&parsed.data.certificate, &parsed.data.private_key).await {
        let version = i32::try_from(parsed.data.cert_version).unwrap_or(i32::MAX);
        save_cert_version(station, version).await;
        reload_caddy(client).await;
        tracing::info!(cert_version = version, "TLS certificate updated");
    }
}

async fn write_cert_files(cert_pem: &str, key_pem: &str) -> bool {
    let tls = tls_dir();
    let cert_path = format!("{tls}/cert.pem");
    let key_path = format!("{tls}/key.pem");

    if let Err(e) = tokio::fs::create_dir_all(&tls).await {
        tracing::error!(error = %e, "Failed to create TLS directory");
        return false;
    }

    if let Err(e) = tokio::fs::write(&cert_path, cert_pem).await {
        tracing::error!(error = %e, "Failed to write cert.pem");
        return false;
    }

    if let Err(e) = tokio::fs::write(&key_path, key_pem).await {
        tracing::error!(error = %e, "Failed to write key.pem");
        return false;
    }

    true
}

async fn reload_caddy(client: &reqwest::Client) {
    // Caddy Admin API
    // GET current config, POST it back to force a reload.
    // This makes Caddy re-read cert files from disk with zero downtime.
    // Uses CADDY_ADMIN_URL env (docker-compose: http://caddy:2019) or localhost fallback.
    let admin_url =
        std::env::var("CADDY_ADMIN_URL").unwrap_or_else(|_| "http://localhost:2019".to_string());

    let config = match client.get(format!("{}/config/", admin_url)).send().await {
        Ok(r) if r.status().is_success() => match r.text().await {
            Ok(body) => body,
            Err(e) => {
                tracing::warn!(error = %e, "Caddy reload failed (cannot read config body)");
                return;
            }
        },
        Ok(r) => {
            tracing::warn!(status = %r.status(), "Caddy reload failed (cannot get config)");
            return;
        }
        Err(e) => {
            tracing::debug!(error = %e, "Caddy reload skipped (not reachable)");
            return;
        }
    };

    match client
        .post(format!("{}/load", admin_url))
        .header("Content-Type", "application/json")
        .body(config)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            tracing::info!("Caddy reloaded with new certificate");
        }
        Ok(r) => {
            tracing::warn!(status = %r.status(), "Caddy reload returned non-success");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Caddy reload POST failed");
        }
    }
}
