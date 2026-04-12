use alacahoyuk::build_health_router;
use axum::{body::Body, http::Request};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

#[tokio::test]
async fn health_without_token_returns_401() {
    let app = build_health_router(Some("my-key".to_string()));
    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn health_with_wrong_token_returns_401() {
    let app = build_health_router(Some("my-key".to_string()));
    let req = Request::builder()
        .uri("/health")
        .header("authorization", "Bearer wrong")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn health_with_valid_token_returns_200() {
    let key = "my-key";
    let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
    let app = build_health_router(Some(key.to_string()));
    let req = Request::builder()
        .uri("/health")
        .header("authorization", format!("Bearer {hash}"))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
}

#[tokio::test]
async fn health_no_license_key_returns_200_in_debug() {
    let app = build_health_router(None);
    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
}
