mod common;

use axum::http::Method;
use serde_json::json;

#[tokio::test]
async fn join_and_leave_channel() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "owner-1", json!({})).await;

    let res = common::request(
        orbit.clone(),
        common::authed(Method::POST, &format!("/channels/{id}/join"), "user-2"),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);

    let res = common::request(
        orbit,
        common::authed(Method::POST, &format!("/channels/{id}/leave"), "user-2"),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);
}

#[tokio::test]
async fn private_channel_creator_cannot_leave() {
    let orbit = common::test_orbit();
    let (id, _) =
        common::create_channel(orbit.clone(), "owner-priv", json!({"is_private": true})).await;

    let res = common::request(
        orbit,
        common::authed(Method::POST, &format!("/channels/{id}/leave"), "owner-priv"),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn add_member_requires_membership() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "owner-add", json!({})).await;
    common::ensure_user(orbit.clone(), "user-outsider").await;
    common::ensure_user(orbit.clone(), "user-target").await;

    let res = common::request(
        orbit,
        common::authed_json(
            Method::POST,
            &format!("/channels/{id}/members"),
            "user-outsider",
            json!({"user_id": "user-target"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn add_member_by_existing_member() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "owner-addok", json!({})).await;
    common::ensure_user(orbit.clone(), "user-invited").await;

    let res = common::request(
        orbit.clone(),
        common::authed_json(
            Method::POST,
            &format!("/channels/{id}/members"),
            "owner-addok",
            json!({"user_id": "user-invited"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);

    let res = common::request(
        orbit,
        common::authed(
            Method::GET,
            &format!("/channels/{id}/members"),
            "owner-addok",
        ),
    )
    .await;
    let body = common::body_json(res).await;
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["user_id"] == "user-invited")
    );
}

#[tokio::test]
async fn remove_member_requires_creator() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "owner-rm", json!({})).await;
    common::join_channel(orbit.clone(), &id, "user-rm-2").await;
    common::join_channel(orbit.clone(), &id, "user-rm-3").await;

    let res = common::request(
        orbit.clone(),
        common::authed(
            Method::DELETE,
            &format!("/channels/{id}/members/user-rm-3"),
            "user-rm-2",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);

    let res = common::request(
        orbit,
        common::authed(
            Method::DELETE,
            &format!("/channels/{id}/members/user-rm-3"),
            "owner-rm",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);
}

#[tokio::test]
async fn join_without_auth_returns_401() {
    let res = common::request(
        common::test_orbit(),
        axum::http::Request::builder()
            .method(Method::POST)
            .uri("/channels/fake-id/join")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn idempotent_join() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "owner-idem", json!({})).await;

    for _ in 0..2 {
        let res = common::request(
            orbit.clone(),
            common::authed(Method::POST, &format!("/channels/{id}/join"), "user-idem"),
        )
        .await;
        assert_eq!(res.status().as_u16(), 204);
    }
}

#[tokio::test]
async fn non_member_cannot_join_private_channel() {
    let orbit = common::test_orbit();
    let (id, _) =
        common::create_channel(orbit.clone(), "owner-gate", json!({"is_private": true})).await;

    let res = common::request(
        orbit,
        common::authed(
            Method::POST,
            &format!("/channels/{id}/join"),
            "outsider-gate",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}
