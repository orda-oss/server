use std::{net::IpAddr, num::NonZeroU32, str::FromStr, sync::Arc};

use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

use crate::{
    Orbit,
    utils::response::{ApiError, codes},
};

pub type IpRateLimiter = DefaultKeyedRateLimiter<IpAddr>;
pub type UserRateLimiter = DefaultKeyedRateLimiter<String>;

/// Creates a per-IP rate limiter (global abuse protection).
pub fn new_ip_limiter(per_second: u32, burst: u32) -> Arc<IpRateLimiter> {
    Arc::new(RateLimiter::keyed(
        Quota::per_second(NonZeroU32::new(per_second).unwrap())
            .allow_burst(NonZeroU32::new(burst).unwrap()),
    ))
}

/// Creates a per-user rate limiter (per-station, after auth).
pub fn new_user_limiter(per_second: u32, burst: u32) -> Arc<UserRateLimiter> {
    Arc::new(RateLimiter::keyed(
        Quota::per_second(NonZeroU32::new(per_second).unwrap())
            .allow_burst(NonZeroU32::new(burst).unwrap()),
    ))
}

pub fn extract_ip(req: &Request<Body>) -> IpAddr {
    extract_ip_from_headers(req.headers())
}

pub fn extract_ip_from_parts(parts: &axum::http::request::Parts) -> IpAddr {
    extract_ip_from_headers(&parts.headers)
}

fn extract_ip_from_headers(headers: &axum::http::HeaderMap) -> IpAddr {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| IpAddr::from_str(s.trim()).ok())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]))
}

pub fn spawn_gc_task(orbit: Arc<Orbit>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1800));
        loop {
            interval.tick().await;
            orbit.http_rate_limiter.retain_recent();
            for entry in orbit.stations.iter() {
                entry.value().satellite.clear_all_synced();
                entry.value().user_rate_limiter.retain_recent();
            }
        }
    });
}

/// Middleware: reject requests over the global per-IP rate limit with 429.
pub async fn http_rate_limit(
    State(orbit): State<Arc<Orbit>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if orbit
        .http_rate_limiter
        .check_key(&extract_ip(&req))
        .is_err()
    {
        return ApiError::new(StatusCode::TOO_MANY_REQUESTS, codes::ERR_RATE_LIMITED)
            .into_response();
    }
    next.run(req).await
}
