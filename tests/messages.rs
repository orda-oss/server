mod common;

use axum::http::Method;
use serde_json::json;

#[tokio::test]
async fn create_message() {
    let orbit = common::test_orbit();
    let (channel_id, _) = common::create_channel(orbit.clone(), "user-1", json!({})).await;

    let (_, body) = common::create_message(orbit, &channel_id, "user-1", "hello world").await;
    assert_eq!(body["data"]["content"], "hello world");
}

#[tokio::test]
async fn create_message_without_auth_returns_401() {
    let res = common::request(
        common::test_orbit(),
        axum::http::Request::builder()
            .method(Method::POST)
            .uri("/channels/fake-id/messages")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                r#"{"content":"hi","sender_id":"x"}"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn edit_own_message() {
    let orbit = common::test_orbit();
    let (channel_id, _) = common::create_channel(orbit.clone(), "user-1", json!({})).await;
    let (msg_id, _) =
        common::create_message(orbit.clone(), &channel_id, "user-1", "original").await;

    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{channel_id}/messages/{msg_id}"),
            "user-1",
            json!({"content": "edited"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(common::body_json(res).await["data"]["content"], "edited");
}

#[tokio::test]
async fn edit_other_users_message_returns_403() {
    let orbit = common::test_orbit();
    let (channel_id, _) = common::create_channel(orbit.clone(), "user-1", json!({})).await;
    let (msg_id, _) = common::create_message(orbit.clone(), &channel_id, "user-1", "mine").await;
    common::join_channel(orbit.clone(), &channel_id, "user-2").await;

    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{channel_id}/messages/{msg_id}"),
            "user-2",
            json!({"content": "hijacked"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn delete_other_users_message_returns_403() {
    let orbit = common::test_orbit();
    let (channel_id, _) = common::create_channel(orbit.clone(), "user-1", json!({})).await;
    let (msg_id, _) =
        common::create_message(orbit.clone(), &channel_id, "user-1", "protected").await;
    common::join_channel(orbit.clone(), &channel_id, "user-2").await;

    let res = common::request(
        orbit,
        common::authed(
            Method::DELETE,
            &format!("/channels/{channel_id}/messages/{msg_id}"),
            "user-2",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn delete_and_restore_message() {
    let orbit = common::test_orbit();
    let (channel_id, _) = common::create_channel(orbit.clone(), "user-1", json!({})).await;
    let (msg_id, _) =
        common::create_message(orbit.clone(), &channel_id, "user-1", "ephemeral").await;

    let res = common::request(
        orbit.clone(),
        common::authed(
            Method::DELETE,
            &format!("/channels/{channel_id}/messages/{msg_id}"),
            "user-1",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);

    let res = common::request(
        orbit,
        common::authed(
            Method::PUT,
            &format!("/channels/{channel_id}/messages/{msg_id}/restore"),
            "user-1",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    assert!(common::body_json(res).await["data"]["deleted_at"].is_null());
}

#[tokio::test]
async fn list_messages_with_pagination() {
    let orbit = common::test_orbit();
    let (channel_id, _) = common::create_channel(orbit.clone(), "user-1", json!({})).await;

    for i in 0..3 {
        common::create_message(orbit.clone(), &channel_id, "user-1", &format!("msg-{i}")).await;
    }

    let res = common::request(
        orbit,
        common::authed(
            Method::GET,
            &format!("/channels/{channel_id}/messages?limit=2"),
            "user-1",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(
        common::body_json(res).await["data"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

// Broadcast channels

#[tokio::test]
async fn broadcast_creator_can_send() {
    let orbit = common::test_orbit();
    let (channel_id, _) =
        common::create_channel(orbit.clone(), "bc-owner", json!({"kind": "broadcast"})).await;

    let (_, body) = common::create_message(orbit, &channel_id, "bc-owner", "announcement").await;
    assert_eq!(body["data"]["content"], "announcement");
}

#[tokio::test]
async fn broadcast_non_creator_cannot_send() {
    let orbit = common::test_orbit();
    let (channel_id, _) =
        common::create_channel(orbit.clone(), "bc-owner2", json!({"kind": "broadcast"})).await;
    common::join_channel(orbit.clone(), &channel_id, "bc-reader").await;

    let res = common::request(
        orbit,
        common::authed_json(
            Method::POST,
            &format!("/channels/{channel_id}/messages"),
            "bc-reader",
            json!({"content": "not allowed", "sender_id": "bc-reader"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

// Archived channels

#[tokio::test]
async fn archived_channel_rejects_messages() {
    let orbit = common::test_orbit();
    let (channel_id, _) = common::create_channel(orbit.clone(), "arch-owner", json!({})).await;
    common::request(
        orbit.clone(),
        common::authed_json(
            Method::PUT,
            &format!("/channels/{channel_id}"),
            "arch-owner",
            json!({"is_archived": true}),
        ),
    )
    .await;

    let res = common::request(
        orbit,
        common::authed_json(
            Method::POST,
            &format!("/channels/{channel_id}/messages"),
            "arch-owner",
            json!({"content": "nope", "sender_id": "arch-owner"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

// Private channel message access

#[tokio::test]
async fn non_member_cannot_list_private_channel_messages() {
    let orbit = common::test_orbit();
    let (channel_id, _) =
        common::create_channel(orbit.clone(), "priv-owner", json!({"is_private": true})).await;
    common::ensure_user(orbit.clone(), "priv-outsider").await;

    let res = common::request(
        orbit,
        common::authed(
            Method::GET,
            &format!("/channels/{channel_id}/messages"),
            "priv-outsider",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}
