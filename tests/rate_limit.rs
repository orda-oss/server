mod common;

use axum::{
    body::Body,
    http::{Method, Request},
};

#[tokio::test]
async fn body_too_large_returns_413() {
    let app = common::test_app(common::test_orbit());
    let token = common::access_token("user-1", "user1");
    let size = 65 * 1024;
    let req = Request::builder()
        .method(Method::POST)
        .uri("/channels")
        .header("content-type", "application/json")
        .header("content-length", size.to_string())
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(vec![0u8; size]))
        .unwrap();
    let res = common::send(app, req).await;
    assert_eq!(res.status().as_u16(), 413);
}

#[tokio::test]
async fn spoofed_content_length_still_returns_413() {
    let app = common::test_app(common::test_orbit());
    let token = common::access_token("user-1", "user1");
    let req = Request::builder()
        .method(Method::POST)
        .uri("/channels")
        .header("content-type", "application/json")
        .header("content-length", "1")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(vec![0u8; 65 * 1024]))
        .unwrap();
    let res = common::send(app, req).await;
    assert_eq!(res.status().as_u16(), 413);
}

#[tokio::test]
async fn rate_limit_blocks_excess_requests() {
    let orbit = common::orbit_with_limits(1, 1);

    let first = common::send(
        common::test_app(orbit.clone()),
        common::authed(Method::GET, "/users", "user-1"),
    )
    .await;
    assert_ne!(first.status().as_u16(), 429, "first request should pass");

    let second = common::send(
        common::test_app(orbit.clone()),
        common::authed(Method::GET, "/users", "user-1"),
    )
    .await;
    assert_eq!(
        second.status().as_u16(),
        429,
        "second request should be rate-limited",
    );
}
