mod common;

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request},
};

#[tokio::test]
async fn meta_without_token_returns_401() {
    let app = common::test_app(common::test_orbit());
    let req = Request::builder()
        .method(Method::GET)
        .uri("/meta")
        .body(Body::empty())
        .unwrap();
    let res = common::send(app, req).await;
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn meta_returns_version_and_features() {
    let app = common::test_app(common::test_orbit());
    let token = common::access_token("user-1", "user1");
    let req = Request::builder()
        .method(Method::GET)
        .uri("/meta")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = common::send(app, req).await;
    assert_eq!(res.status().as_u16(), 200);

    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["version"].as_str(), Some(alacahoyuk::VERSION));
    assert_eq!(json["api_version"].as_u64(), Some(1));
    assert!(json["features"].is_array());
}
