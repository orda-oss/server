mod common;

use axum::{body::Body, http::Request};

#[tokio::test]
async fn missing_token_returns_401() {
    let app = common::test_app(common::test_orbit());
    let req = Request::builder()
        .uri("/users")
        .body(Body::empty())
        .unwrap();
    let res = common::send(app, req).await;
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn garbage_token_returns_401() {
    let app = common::test_app(common::test_orbit());
    let req = Request::builder()
        .uri("/users")
        .header("authorization", "Bearer not-a-jwt")
        .body(Body::empty())
        .unwrap();
    let res = common::send(app, req).await;
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn expired_token_returns_401() {
    let app = common::test_app(common::test_orbit());
    let token = common::expired_token("user-1");
    let req = Request::builder()
        .uri("/users")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = common::send(app, req).await;
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn wrong_server_id_returns_401() {
    let app = common::test_app(common::test_orbit());
    let token = common::wrong_server_token("user-1");
    let req = Request::builder()
        .uri("/users")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = common::send(app, req).await;
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn valid_token_passes_auth() {
    let app = common::test_app(common::test_orbit());
    let req = common::authed(axum::http::Method::GET, "/users", "user-1");
    let res = common::send(app, req).await;
    assert_ne!(res.status().as_u16(), 401, "valid token should pass auth");
}
