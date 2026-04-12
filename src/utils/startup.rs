use std::{env, fs, path::Path, sync::Arc};

use diesel::{
    SqliteConnection,
    connection::SimpleConnection,
    r2d2::{ConnectionManager, Pool},
};
use diesel_migrations::MigrationHarness;

use crate::{
    DbPool, MIGRATIONS,
    utils::{
        db::SqliteConnectionCustomizer,
        helpers::{ActivateServerInfo, activate_with_semerkant},
        jwks::{self, JwksCache},
    },
};

pub fn parse_port() -> u16 {
    let parse = |s: &str, source: &str| -> Option<u16> {
        match s.parse::<u16>() {
            Ok(p) if p > 1023 => Some(p),
            Ok(p) => {
                tracing::warn!(
                    port = p,
                    source,
                    "Port rejected (must be 1024-65535), ignoring."
                );
                None
            }
            Err(_) => {
                tracing::warn!(value = s, source, "Port is not a valid number, ignoring.");
                None
            }
        }
    };
    std::env::args()
        .skip_while(|a| a != "--port")
        .nth(1)
        .and_then(|s| parse(&s, "--port"))
        .or_else(|| env::var("PORT").ok().and_then(|s| parse(&s, "PORT env")))
        .unwrap_or_else(|| {
            tracing::info!("No valid port specified, defaulting to 3000.");
            3000
        })
}

pub struct ResolvedKeys {
    pub servers: Vec<ActivateServerInfo>,
    pub domain: Option<String>,
    pub semerkant_url: Option<String>,
}

pub async fn resolve_keys(port: u16) -> ResolvedKeys {
    let license_key = env::var("LICENSE_KEY").ok();
    let semerkant_url = if cfg!(feature = "hardcoded-semerkant-url") {
        Some(obfstr::obfstr!("https://api.orda.chat/hub/v1").to_string())
    } else {
        env::var("SEMERKANT_URL").ok()
    };

    match (&license_key, &semerkant_url) {
        (Some(lk), Some(url)) => match activate_with_semerkant(url, lk, port).await {
            Ok(result) => {
                tracing::info!(
                    count = result.servers.len(),
                    "Activated {} server(s) with semerkant.",
                    result.servers.len()
                );
                ResolvedKeys {
                    servers: result.servers,
                    domain: result.domain,
                    semerkant_url: Some(url.clone()),
                }
            }
            Err(e) => {
                if cfg!(debug_assertions) {
                    let key =
                        env::var("ENCRYPTION_KEY").unwrap_or_else(|_| "my-secret".to_string());
                    tracing::warn!(
                        "Failed to activate with semerkant: {e}. Using ENCRYPTION_KEY fallback (debug only)."
                    );
                    ResolvedKeys {
                        servers: vec![ActivateServerInfo {
                            server_id: env::var("SERVER_REMOTE_ID")
                                .unwrap_or_else(|_| "debug_server".to_string()),
                            encryption_key: key,
                            name: "Debug Server".to_string(),
                        }],
                        domain: None,
                        semerkant_url: Some(url.clone()),
                    }
                } else {
                    tracing::error!(
                        "Failed to activate with semerkant: {e}. Cannot start without semerkant."
                    );
                    std::process::exit(1);
                }
            }
        },
        _ => {
            if cfg!(debug_assertions) {
                tracing::info!(
                    "LICENSE_KEY or SEMERKANT_URL not set. Using env var fallbacks (debug only)."
                );
                let key = env::var("ENCRYPTION_KEY").unwrap_or_else(|_| "my-secret".to_string());
                ResolvedKeys {
                    servers: vec![ActivateServerInfo {
                        server_id: env::var("SERVER_REMOTE_ID")
                            .unwrap_or_else(|_| "debug_server".to_string()),
                        encryption_key: key,
                        name: "Debug Server".to_string(),
                    }],
                    domain: None,
                    semerkant_url: semerkant_url.clone(),
                }
            } else {
                tracing::error!("LICENSE_KEY and SEMERKANT_URL are required.");
                std::process::exit(1);
            }
        }
    }
}

/// DB URL for a server, keyed by remote_id.
pub fn db_url_for(remote_id: &str) -> String {
    let base = env::var("DATA_DIR").unwrap_or_else(|_| "/opt/alacahoyuk/data".to_string());
    format!("sqlite://{base}/tumulus_{remote_id}.db")
}

pub fn init_db(remote_id: &str, db_key: &str) -> DbPool {
    let db_url = db_url_for(remote_id);
    let db_path = db_url.strip_prefix("sqlite://").unwrap_or(&db_url);

    if let Some(parent) = Path::new(db_path).parent()
        && !parent.exists()
    {
        tracing::info!("Creating missing directory: {:?}", parent);
        fs::create_dir_all(parent).expect("Could not create DB directory");
    }

    {
        use diesel::Connection;
        tracing::info!(remote_id, "Initializing database...");
        let mut conn = SqliteConnection::establish(&db_url).expect("Failed to connect to database");

        conn.batch_execute(&format!(
            "{} = '{}';",
            obfstr::obfstr!("PRAGMA key"),
            db_key
        ))
        .expect("Failed to set encryption key");
        conn.batch_execute("PRAGMA journal_mode = WAL;")
            .expect("Failed to set WAL mode");
        conn.run_pending_migrations(MIGRATIONS)
            .map_err(|e| format!("Migration error: {}", e))
            .unwrap();

        match conn.batch_execute("SELECT count(*) FROM sqlite_master") {
            Ok(_) => tracing::info!(remote_id, "Database decrypted successfully."),
            Err(e) => {
                tracing::error!(remote_id, error = %e, "Failed to decrypt database. Wrong encryption key?");
                std::process::exit(1);
            }
        }
    }

    let manager = ConnectionManager::<SqliteConnection>::new(db_url);
    Pool::builder()
        .max_size(5)
        .connection_customizer(Box::new(SqliteConnectionCustomizer {
            db_key: db_key.to_string(),
        }))
        .build(manager)
        .unwrap()
}

pub async fn init_jwks(semerkant_url: &Option<String>) -> Option<Arc<JwksCache>> {
    let url = semerkant_url.as_deref()?;
    let cache = Arc::new(JwksCache::new(url));

    match cache.fetch().await {
        Ok(()) => {
            jwks::spawn_refresh_task(cache.clone());
            Some(cache)
        }
        Err(e) => {
            if cfg!(debug_assertions) {
                tracing::warn!("JWKS fetch failed: {e}. Running without JWT auth (debug only).");
                None
            } else {
                tracing::error!("JWKS fetch failed: {e}. Cannot start without JWKS.");
                std::process::exit(1);
            }
        }
    }
}
