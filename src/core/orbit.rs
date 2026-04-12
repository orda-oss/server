use std::sync::Arc;

use dashmap::DashMap;
use diesel::{
    SqliteConnection,
    r2d2::{ConnectionManager, Pool},
};
use tokio::sync::watch;

use crate::{
    Server,
    core::satellite::Satellite,
    utils::{jwks::JwksCache, rate_limit::IpRateLimiter},
};

pub type DbPool = Pool<ConnectionManager<SqliteConnection>>;

pub struct LiveKitConfig {
    pub url: String,
    pub api_key: String,
    pub api_secret: String,
    /// URL that LiveKit should POST webhook events to (e.g. "http://localhost:3000/voice/webhook")
    pub webhook_url: String,
    /// Client-facing LiveKit URL (e.g. "wss://example.com/lk").
    /// If set, returned to clients as-is. If empty, derived from Host header (dev fallback).
    pub client_url: String,
}

impl LiveKitConfig {
    pub fn from_env(port: u16, domain: Option<&str>) -> Self {
        let defaults = Self::default();
        Self {
            url: std::env::var("LIVEKIT_URL").unwrap_or(defaults.url),
            api_key: std::env::var("LIVEKIT_API_KEY").unwrap_or(defaults.api_key),
            api_secret: std::env::var("LIVEKIT_API_SECRET").unwrap_or(defaults.api_secret),
            webhook_url: std::env::var("LIVEKIT_WEBHOOK_URL")
                .unwrap_or_else(|_| format!("http://localhost:{port}/voice/webhook")),
            client_url: std::env::var("LIVEKIT_CLIENT_URL")
                .unwrap_or_else(|_| domain.map(|d| format!("wss://{d}/lk")).unwrap_or_default()),
        }
    }
}

impl Default for LiveKitConfig {
    fn default() -> Self {
        Self {
            url: "localhost:7880".to_string(),
            api_key: "devkey".to_string(),
            api_secret: "secret".to_string(),
            webhook_url: "http://localhost:3000/voice/webhook".to_string(),
            client_url: String::new(),
        }
    }
}

// Per-server state. Each server has its own DB, satellite, and identity.
pub struct Station {
    /// r2d2 connection pool. Each blocking DB task checks out one connection.
    /// Pool size is intentionally small (5) - SQLite serialises writes anyway.
    pub pool: DbPool,
    /// Cached server record loaded at startup. Kept here for cheap identity
    /// checks that don't need fresh DB data (e.g. attaching server_id to new rows).
    /// Re-query the DB for anything that an admin might change at runtime.
    pub server: Server,
    /// In-memory pub/sub hub. Owns all broadcast senders and active WS sessions.
    pub satellite: Satellite,
    /// Per-station per-user rate limiter (checked after auth resolves the station).
    pub user_rate_limiter: Arc<crate::utils::rate_limit::UserRateLimiter>,
    /// Signals all active WebSocket connections to close gracefully on shutdown.
    pub shutdown: watch::Sender<bool>,
}

impl Station {
    /// Create a new Station: init DB, create/load server record, set up satellite.
    pub fn new(server_id: &str, encryption_key: &str, name: String) -> Result<Arc<Self>, String> {
        let pool = crate::init_db(server_id, encryption_key);
        let mut conn = pool.get().map_err(|e| format!("DB pool error: {e}"))?;
        let server = Server::get_or_create(&mut conn, name, Some(server_id.to_string()))
            .map_err(|e| format!("Server record error: {e}"))?;

        let (shutdown_tx, _) = watch::channel(false);

        Ok(Arc::new(Self {
            pool,
            server,
            satellite: Satellite::new(),
            user_rate_limiter: crate::utils::rate_limit::new_user_limiter(50, 100),
            shutdown: shutdown_tx,
        }))
    }

    pub fn is_user_synced(&self, user_id: &str) -> bool {
        self.satellite.is_user_synced(user_id)
    }

    pub fn mark_user_synced(&self, user_id: &str) {
        self.satellite.mark_user_synced(user_id);
    }

    pub fn set_maintenance(&self, enabled: bool) {
        self.satellite.set_maintenance(enabled);
    }

    pub fn is_maintenance(&self) -> bool {
        self.satellite.is_maintenance()
    }
}

// Central app state. Shared across all servers.
pub struct Orbit {
    /// Per-server contexts, keyed by server remote_id (sid from JWT).
    pub stations: DashMap<String, Arc<Station>>,
    /// LiveKit server connection config.
    pub livekit: LiveKitConfig,
    /// The VM's license key from semerkant (shared by all servers in the org).
    pub license_key: Option<String>,
    /// Base URL for semerkant (e.g., "http://localhost:3001/hub/v1").
    pub semerkant_url: Option<String>,
    /// Cached JWKS keys for JWT verification. None in debug builds when SEMERKANT_URL is unset.
    pub jwks: Option<Arc<JwksCache>>,
    /// Global per-IP HTTP rate limiter (5 req/sec steady, burst 20).
    pub http_rate_limiter: Arc<IpRateLimiter>,
}

impl Orbit {
    pub fn get_station(&self, remote_id: &str) -> Option<Arc<Station>> {
        self.stations
            .get(remote_id)
            .map(|entry| entry.value().clone())
    }

    // Phase 1 convenience: returns the first station, if any.
    pub fn default_station(&self) -> Option<Arc<Station>> {
        self.stations.iter().next().map(|e| e.value().clone())
    }
}
