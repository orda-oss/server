use std::sync::Arc;

use alacahoyuk::{
    utils::rate_limit::{new_ip_limiter, spawn_gc_task},
    *,
};
use dashmap::DashMap;
use diesel::RunQueryDsl;
use dotenv::dotenv;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    dotenv().ok();

    // Suppress source paths in release panic messages
    if !cfg!(debug_assertions) {
        std::panic::set_hook(Box::new(|info| {
            let payload = info
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
                .unwrap_or("unknown error");
            tracing::error!("fatal panic: {payload}");
        }));
    }

    let fmt_layer = if cfg!(debug_assertions) {
        fmt::layer().with_file(true).with_line_number(true)
    } else {
        fmt::layer()
            .with_file(false)
            .with_line_number(false)
            .with_target(false)
    };

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            if cfg!(debug_assertions) {
                EnvFilter::new("info")
            } else {
                EnvFilter::new("info,diesel=warn,r2d2=error")
            }
        }))
        .with(fmt_layer)
        .init();

    let port = parse_port();
    tracing::info!(port, "Listening port set.");

    let license_key = std::env::var("LICENSE_KEY").ok();

    // Phase 1: Bind listener and serve health-only router so semerkant can verify
    // reachability during activation.
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap();
    tracing::info!("Health listener bound on 0.0.0.0:{port}");

    let health_router = build_health_router(license_key.clone());
    let (health_shutdown_tx, health_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let health_handle = tokio::spawn(async move {
        axum::serve(listener, health_router)
            .with_graceful_shutdown(async {
                health_shutdown_rx.await.ok();
            })
            .await
            .ok();
    });

    // Phase 2: Init DB and build Station(s)
    // Gets keys from semerkant but doesn't mark server(s) online.
    // The first heartbeat (fired immediately after full startup) marks them online.
    let keys = resolve_keys(port).await;

    let livekit = LiveKitConfig::from_env(port, keys.domain.as_deref());
    tracing::info!(
        url = %livekit.url,
        client_url = %livekit.client_url,
        webhook_url = %livekit.webhook_url,
        "LiveKit config loaded."
    );

    let jwks = init_jwks(&keys.semerkant_url).await;

    let stations: DashMap<String, Arc<Station>> = DashMap::new();

    for server_info in &keys.servers {
        let station = Station::new(
            &server_info.server_id,
            &server_info.encryption_key,
            server_info.name.clone(),
        )
        .unwrap_or_else(|e| {
            tracing::error!(server_id = %server_info.server_id, error = %e, "Failed to create station");
            std::process::exit(1);
        });

        tracing::info!(
            remote_id = %server_info.server_id,
            name = %server_info.name,
            "Station initialized."
        );

        stations.insert(server_info.server_id.clone(), station);
    }

    let orbit = Arc::new(Orbit {
        stations,
        livekit,
        license_key: license_key.clone(),
        semerkant_url: keys.semerkant_url.clone(),
        jwks,
        // Global per-IP rate limiter: scales with station count.
        // Per-station per-user limiting (50/100) is in AuthContext after auth.
        http_rate_limiter: {
            let n = keys.servers.len().max(1) as u32;
            new_ip_limiter(100 * n, 200 * n)
        },
    });

    spawn_gc_task(orbit.clone());

    // Phase 3: Stop health-only server, start full server
    let _ = health_shutdown_tx.send(());
    health_handle.await.ok();

    let app = build_router(orbit.clone());

    core::voice::handlers::spawn_health_monitor(orbit.clone());

    // Single heartbeat task iterates all stations
    if let (Some(url), Some(key)) = (&keys.semerkant_url, &license_key) {
        utils::helpers::spawn_heartbeat_task(orbit.clone(), url.clone(), key.clone());
        tracing::info!(count = orbit.stations.len(), "Heartbeat task started.");
    }

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap();
    tracing::info!("Listening on 0.0.0.0:{port}");

    let orbit_c = orbit.clone();

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            utils::helpers::shutdown_signal().await;
            tracing::info!("Shutdown signal received, closing WebSocket connections...");
            for entry in orbit_c.stations.iter() {
                let _ = entry.value().shutdown.send(true);
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        })
        .await
        .unwrap();

    tracing::info!("Server stopped. Flushing WAL...");

    for entry in orbit.stations.iter() {
        if let Ok(mut conn) = entry.value().pool.get() {
            let result = diesel::sql_query("PRAGMA wal_checkpoint(TRUNCATE);").execute(&mut conn);
            match result {
                Ok(_) => tracing::info!("WAL flushed for station {}", entry.key()),
                Err(e) => {
                    tracing::error!("Failed to flush WAL for station {}: {}", entry.key(), e)
                }
            }
        }
    }

    tracing::info!("Goodbye!");
}
