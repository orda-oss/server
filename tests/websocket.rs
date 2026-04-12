mod common;

use axum::http::Method;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

async fn next_event(
    ws: &mut (
             impl futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
             + Unpin
         ),
) -> serde_json::Value {
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
            .await
            .expect("timed out waiting for WS message")
            .expect("stream ended")
            .expect("ws error");
        if let Message::Text(text) = msg {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

#[tokio::test]
async fn ws_connect_and_disconnect() {
    let addr = common::spawn_server(common::test_orbit()).await;
    let mut ws = common::ws_connect(addr, "ws-user-1").await;
    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn ws_receives_channel_message() {
    let orbit = common::test_orbit();
    let addr = common::spawn_server(orbit.clone()).await;
    let (channel_id, _) = common::create_channel(orbit.clone(), "ws-sender", json!({})).await;

    let mut ws = common::ws_connect(addr, "ws-sender").await;

    common::create_message(orbit, &channel_id, "ws-sender", "hello from rest").await;

    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "send_message");
    assert_eq!(event["message"]["content"], "hello from rest");
    assert_eq!(event["message"]["channel_id"], channel_id);

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_receives_server_event_on_channel_create() {
    let orbit = common::test_orbit();
    let addr = common::spawn_server(orbit.clone()).await;
    common::ensure_user(orbit.clone(), "ws-observer").await;

    let mut ws = common::ws_connect(addr, "ws-observer").await;

    common::create_channel(orbit, "ws-observer", json!({})).await;

    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "channel_created");
    assert!(event["channel"]["id"].is_string());

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_send_message_via_socket() {
    let orbit = common::test_orbit();
    let addr = common::spawn_server(orbit.clone()).await;
    let (channel_id, _) = common::create_channel(orbit, "ws-direct", json!({})).await;

    let mut ws = common::ws_connect(addr, "ws-direct").await;

    ws.send(Message::Text(
        json!({"action": "send_message", "channel_id": channel_id, "content": "hello via ws"})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();

    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "send_message");
    assert_eq!(event["message"]["content"], "hello via ws");

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_typing_event() {
    let orbit = common::test_orbit();
    let addr = common::spawn_server(orbit.clone()).await;
    let (channel_id, _) = common::create_channel(orbit, "ws-typer", json!({})).await;

    let mut ws = common::ws_connect(addr, "ws-typer").await;

    ws.send(Message::Text(
        json!({"action": "typing_event", "channel_id": channel_id})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "typing_event");
    assert_eq!(event["channel_id"], channel_id);
    assert_eq!(event["user_id"], "ws-typer");

    ws.send(Message::Text(
        json!({"action": "typing_stop", "channel_id": channel_id})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "typing_stop");

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_member_joined_event() {
    let orbit = common::test_orbit();
    let addr = common::spawn_server(orbit.clone()).await;
    let (channel_id, _) = common::create_channel(orbit.clone(), "ws-ch-owner", json!({})).await;

    let mut ws = common::ws_connect(addr, "ws-ch-owner").await;

    common::join_channel(orbit, &channel_id, "ws-joiner").await;

    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "member_joined");
    assert_eq!(event["channel_id"], channel_id);
    assert_eq!(event["user_id"], "ws-joiner");

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_member_left_event() {
    let orbit = common::test_orbit();
    let addr = common::spawn_server(orbit.clone()).await;
    let (channel_id, _) = common::create_channel(orbit.clone(), "ws-left-owner", json!({})).await;
    common::join_channel(orbit.clone(), &channel_id, "ws-leaver").await;

    let mut ws = common::ws_connect(addr, "ws-left-owner").await;

    common::request(
        orbit,
        common::authed(
            Method::POST,
            &format!("/channels/{channel_id}/leave"),
            "ws-leaver",
        ),
    )
    .await;

    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "member_left");
    assert_eq!(event["channel_id"], channel_id);
    assert_eq!(event["user_id"], "ws-leaver");

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_channel_updated_event() {
    let orbit = common::test_orbit();
    let addr = common::spawn_server(orbit.clone()).await;
    common::ensure_user(orbit.clone(), "ws-upd-observer").await;

    let mut ws = common::ws_connect(addr, "ws-upd-observer").await;

    let (channel_id, _) = common::create_channel(orbit.clone(), "ws-upd-observer", json!({})).await;
    let _ = next_event(&mut ws).await; // channel_created

    common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{channel_id}"),
            "ws-upd-observer",
            json!({"name": "renamed-ws"}),
        ),
    )
    .await;

    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "channel_updated");
    assert_eq!(event["channel"]["name"], "renamed-ws");

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_channel_deleted_event() {
    let orbit = common::test_orbit();
    let addr = common::spawn_server(orbit.clone()).await;
    common::ensure_user(orbit.clone(), "ws-del-observer").await;

    let mut ws = common::ws_connect(addr, "ws-del-observer").await;

    let (channel_id, _) = common::create_channel(orbit.clone(), "ws-del-observer", json!({})).await;
    let _ = next_event(&mut ws).await; // channel_created

    common::request(
        orbit,
        common::authed(
            Method::DELETE,
            &format!("/channels/{channel_id}"),
            "ws-del-observer",
        ),
    )
    .await;

    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "channel_deleted");
    assert_eq!(event["channel_id"], channel_id);

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_user_online_offline_events() {
    let orbit = common::test_orbit();
    let addr = common::spawn_server(orbit.clone()).await;
    common::ensure_user(orbit, "ws-watcher").await;

    let mut watcher = common::ws_connect(addr, "ws-watcher").await;

    let mut newcomer = common::ws_connect(addr, "ws-newcomer").await;

    let event = next_event(&mut watcher).await;
    assert_eq!(event["action"], "user_online");
    assert_eq!(event["user_id"], "ws-newcomer");

    newcomer.close(None).await.ok();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let event = next_event(&mut watcher).await;
    assert_eq!(event["action"], "user_offline");
    assert_eq!(event["user_id"], "ws-newcomer");

    watcher.close(None).await.ok();
}

#[tokio::test]
async fn ws_maintenance_events() {
    let orbit = common::test_orbit();
    let station = common::station(&orbit);
    let addr = common::spawn_server(orbit.clone()).await;
    common::ensure_user(orbit.clone(), "ws-maint").await;

    let mut ws = common::ws_connect(addr, "ws-maint").await;

    station.set_maintenance(true);
    station
        .satellite
        .broadcast_server(&alacahoyuk::core::satellite::types::ServerEvent::MaintenanceStarted);
    assert_eq!(next_event(&mut ws).await["action"], "maintenance_started");

    station.set_maintenance(false);
    station
        .satellite
        .broadcast_server(&alacahoyuk::core::satellite::types::ServerEvent::MaintenanceEnded);
    assert_eq!(next_event(&mut ws).await["action"], "maintenance_ended");

    ws.close(None).await.ok();
}

// Voice (satellite-side, no LiveKit)

#[tokio::test]
async fn ws_voice_join_leave_events() {
    let orbit = common::test_orbit();
    let station = common::station(&orbit);
    let addr = common::spawn_server(orbit.clone()).await;
    let (channel_id, _) = common::create_channel(orbit.clone(), "ws-voice-owner", json!({})).await;
    common::join_channel(orbit.clone(), &channel_id, "ws-voice-peer").await;

    let mut ws = common::ws_connect(addr, "ws-voice-owner").await;

    alacahoyuk::core::voice::service::VoiceService::handle_participant_joined(
        &station,
        &channel_id,
        "ws-voice-peer",
    );
    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "voice_joined");
    assert_eq!(event["user_id"], "ws-voice-peer");

    alacahoyuk::core::voice::service::VoiceService::handle_participant_left(
        &station,
        &channel_id,
        "ws-voice-peer",
    );
    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "voice_left");
    assert_eq!(event["user_id"], "ws-voice-peer");

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_screenshare_events() {
    let orbit = common::test_orbit();
    let station = common::station(&orbit);
    let addr = common::spawn_server(orbit.clone()).await;
    let (channel_id, _) = common::create_channel(orbit.clone(), "ws-ss-owner", json!({})).await;

    let mut ws = common::ws_connect(addr, "ws-ss-owner").await;

    assert!(
        alacahoyuk::core::voice::service::VoiceService::handle_screenshare_start(
            &station,
            &channel_id,
            "ws-ss-owner"
        )
        .is_ok()
    );
    let event = next_event(&mut ws).await;
    assert_eq!(event["action"], "screenshare_started");
    assert_eq!(event["user_id"], "ws-ss-owner");

    // Second user conflicts
    assert!(
        alacahoyuk::core::voice::service::VoiceService::handle_screenshare_start(
            &station,
            &channel_id,
            "someone-else"
        )
        .is_err()
    );

    alacahoyuk::core::voice::service::VoiceService::handle_screenshare_stop(
        &station,
        &channel_id,
        "ws-ss-owner",
    );
    assert_eq!(next_event(&mut ws).await["action"], "screenshare_stopped");

    ws.close(None).await.ok();
}

#[tokio::test]
async fn voice_leave_clears_screenshare() {
    let orbit = common::test_orbit();
    let station = common::station(&orbit);
    let addr = common::spawn_server(orbit.clone()).await;
    let (channel_id, _) = common::create_channel(orbit.clone(), "ws-vl-owner", json!({})).await;

    let mut ws = common::ws_connect(addr, "ws-vl-owner").await;

    alacahoyuk::core::voice::service::VoiceService::handle_participant_joined(
        &station,
        &channel_id,
        "ws-vl-owner",
    );
    let _ = next_event(&mut ws).await; // voice_joined

    alacahoyuk::core::voice::service::VoiceService::handle_screenshare_start(
        &station,
        &channel_id,
        "ws-vl-owner",
    )
    .ok();
    let _ = next_event(&mut ws).await; // screenshare_started

    // Leave voice -> should auto-clear screenshare
    alacahoyuk::core::voice::service::VoiceService::handle_participant_left(
        &station,
        &channel_id,
        "ws-vl-owner",
    );

    let e1 = next_event(&mut ws).await;
    let e2 = next_event(&mut ws).await;
    let actions: Vec<&str> = vec![
        e1["action"].as_str().unwrap(),
        e2["action"].as_str().unwrap(),
    ];
    assert!(actions.contains(&"voice_left"));
    assert!(actions.contains(&"screenshare_stopped"));

    ws.close(None).await.ok();
}

#[tokio::test]
async fn ws_without_auth_rejected() {
    let addr = common::spawn_server(common::test_orbit()).await;
    let url = format!("ws://{addr}/ws");
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(
        result.is_err() || {
            let (_, response) = result.unwrap();
            response.status() == 401
        }
    );
}
