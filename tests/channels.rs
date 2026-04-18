mod common;

use axum::http::Method;
use serde_json::json;

#[tokio::test]
async fn create_channel() {
    let orbit = common::test_orbit();
    let res = common::request(
        orbit,
        common::authed_json(
            Method::POST,
            "/channels",
            "user-1",
            json!({"name": "general"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 201);

    let body = common::body_json(res).await;
    assert_eq!(body["data"]["name"], "general");
    assert_eq!(body["data"]["slug"], "general");
}

#[tokio::test]
async fn create_channel_without_auth_returns_401() {
    let res = common::request(
        common::test_orbit(),
        axum::http::Request::builder()
            .method(Method::POST)
            .uri("/channels")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"name":"test"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn create_channel_slug_collision_returns_409() {
    let orbit = common::test_orbit();
    let name = format!("dup-{}", &uuid::Uuid::new_v4().to_string()[..8]);

    let res = common::request(
        orbit.clone(),
        common::authed_json(Method::POST, "/channels", "user-1", json!({"name": name})),
    )
    .await;
    assert_eq!(res.status().as_u16(), 201);

    let res = common::request(
        orbit,
        common::authed_json(Method::POST, "/channels", "user-1", json!({"name": name})),
    )
    .await;
    assert_eq!(res.status().as_u16(), 409);
}

#[tokio::test]
async fn create_channel_name_too_short_returns_422() {
    let res = common::request(
        common::test_orbit(),
        common::authed_json(Method::POST, "/channels", "user-1", json!({"name": "a"})),
    )
    .await;
    assert_eq!(res.status().as_u16(), 422);
}

#[tokio::test]
async fn list_channels() {
    let orbit = common::test_orbit();
    common::request(
        orbit.clone(),
        common::authed_json(
            Method::POST,
            "/channels",
            "user-1",
            json!({"name": "listme"}),
        ),
    )
    .await;

    let res = common::request(orbit, common::authed(Method::GET, "/channels", "user-1")).await;
    assert_eq!(res.status().as_u16(), 200);

    let body = common::body_json(res).await;
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "listme")
    );
}

#[tokio::test]
async fn delete_channel_requires_archive_first() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "user-1", json!({})).await;

    // Deleting without archiving first should fail
    let res = common::request(
        orbit.clone(),
        common::authed(Method::DELETE, &format!("/channels/{id}"), "user-1"),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);

    // Archive first
    common::request(
        orbit.clone(),
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "user-1",
            json!({"is_archived": true}),
        ),
    )
    .await;

    // Now delete should succeed
    let res = common::request(
        orbit,
        common::authed(Method::DELETE, &format!("/channels/{id}"), "user-1"),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);
}

// Channel update

#[tokio::test]
async fn update_public_channel_by_non_creator_returns_403() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "owner-up", json!({})).await;
    common::join_channel(orbit.clone(), &id, "member-up").await;

    // Regular members can no longer edit channels they didn't create
    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "member-up",
            json!({"name": "renamed"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn update_private_channel_by_non_creator_returns_403() {
    let orbit = common::test_orbit();
    let (id, _) =
        common::create_channel(orbit.clone(), "owner-priv-up", json!({"is_private": true})).await;

    common::ensure_user(orbit.clone(), "other-priv-up").await;
    common::request(
        orbit.clone(),
        common::authed_json(
            Method::POST,
            &format!("/channels/{id}/members"),
            "owner-priv-up",
            json!({"user_id": "other-priv-up"}),
        ),
    )
    .await;

    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "other-priv-up",
            json!({"name": "nope"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn update_private_channel_by_creator_succeeds() {
    let orbit = common::test_orbit();
    let (id, _) =
        common::create_channel(orbit.clone(), "owner-priv-ok", json!({"is_private": true})).await;

    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "owner-priv-ok",
            json!({"name": "updated-priv"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(common::body_json(res).await["data"]["name"], "updated-priv");
}

#[tokio::test]
async fn archive_and_unarchive_channel() {
    let orbit = common::test_orbit();
    let (id, body) = common::create_channel(orbit.clone(), "owner-arch", json!({})).await;
    let original_slug = body["data"]["slug"].as_str().unwrap().to_string();

    let res = common::request(
        orbit.clone(),
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "owner-arch",
            json!({"is_archived": true}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    let body = common::body_json(res).await;
    assert_eq!(body["data"]["is_archived"], true);
    assert_ne!(body["data"]["slug"].as_str().unwrap(), original_slug);

    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "owner-arch",
            json!({"is_archived": false}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(common::body_json(res).await["data"]["is_archived"], false);
}
