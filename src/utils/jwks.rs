use std::sync::Arc;

use jsonwebtoken::{DecodingKey, jwk::JwkSet};
use tokio::sync::RwLock;

pub struct JwksCache {
    keys: RwLock<Vec<(String, DecodingKey)>>,
    jwks_url: String,
}

impl JwksCache {
    pub fn from_keys(keys: Vec<(String, DecodingKey)>) -> Self {
        Self {
            keys: RwLock::new(keys),
            jwks_url: String::new(),
        }
    }

    pub fn new(semerkant_url: &str) -> Self {
        // Derive JWKS URL: strip /hub/v1 suffix, append /.well-known/jwks.json
        let base = semerkant_url
            .trim_end_matches('/')
            .trim_end_matches("/hub/v1");
        let jwks_url = format!("{}/.well-known/jwks.json", base);

        Self {
            keys: RwLock::new(Vec::new()),
            jwks_url,
        }
    }

    pub async fn fetch(&self) -> Result<(), String> {
        let res = reqwest::get(&self.jwks_url)
            .await
            .map_err(|e| format!("JWKS fetch failed: {e}"))?;

        if !res.status().is_success() {
            return Err(format!("JWKS fetch returned {}", res.status()));
        }

        let jwk_set: JwkSet = res
            .json()
            .await
            .map_err(|e| format!("JWKS parse failed: {e}"))?;

        let mut parsed_keys = Vec::new();
        for jwk in &jwk_set.keys {
            let kid = jwk.common.key_id.clone().unwrap_or_default();
            match DecodingKey::from_jwk(jwk) {
                Ok(key) => {
                    parsed_keys.push((kid, key));
                }
                Err(e) => {
                    tracing::warn!(kid = %kid, error = %e, "Failed to parse JWK, skipping");
                }
            }
        }

        if parsed_keys.is_empty() {
            return Err("No valid keys found in JWKS".to_string());
        }

        tracing::info!(count = parsed_keys.len(), "JWKS keys loaded");
        *self.keys.write().await = parsed_keys;
        Ok(())
    }

    pub async fn get_key(&self, kid: &str) -> Option<DecodingKey> {
        let keys = self.keys.read().await;

        // Try exact kid match first
        if let Some((_, key)) = keys.iter().find(|(k, _)| k == kid) {
            return Some(key.clone());
        }

        // Fall back to first key if kid doesn't match
        keys.first().map(|(_, key)| key.clone())
    }
}

pub fn spawn_refresh_task(jwks: Arc<JwksCache>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        interval.tick().await; // skip immediate first tick

        loop {
            interval.tick().await;
            if let Err(e) = jwks.fetch().await {
                tracing::warn!("JWKS refresh failed: {e}");
            }
        }
    });
}
