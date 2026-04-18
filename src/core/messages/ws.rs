use std::sync::Arc;

use axum::{
    extract::{
        WebSocketUpgrade,
        ws::{CloseFrame, Message as WsMessage, WebSocket},
    },
    response::IntoResponse,
};
use diesel::prelude::*;
use futures::{sink::SinkExt, stream::StreamExt};
use tokio::{
    sync::mpsc,
    time::{Duration, interval, timeout},
};
use tokio_stream::{StreamMap, wrappers::BroadcastStream};

use super::service::MessageService;
use crate::{
    Station,
    core::{
        messages::dto::CreateMessageDto,
        satellite::{ChannelEvent, ClientPayload, ServerEvent, UserCommand, VoiceStatusKind},
    },
    utils::auth::AuthContext,
};

const PING_INTERVAL: Duration = Duration::from_secs(30);

/// Axum upgrade handler - extracts `user_id` and `station` from the JWT,
/// then hands off to `handle_socket`. Returns `101 Switching Protocols`.
pub async fn ws_handler(
    AuthContext { user_id, station, .. }: AuthContext,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, station, user_id))
}

// Recv task: dispatch inbound client payloads

async fn handle_client_payload(
    payload: ClientPayload,
    station: &Arc<Station>,
    user_id: &str,
    cmd_tx: &mpsc::UnboundedSender<UserCommand>,
) {
    match payload {
        ClientPayload::SendMessage {
            channel_id,
            content,
        } => {
            let dto = CreateMessageDto {
                sender_id: user_id.to_string(),
                content: content.trim().to_string(),
            };

            match MessageService::create(station.clone(), channel_id.clone(), dto).await {
                Ok(_) => {
                    // Implicitly stop typing when a message is sent.
                    station.satellite.broadcast_channel(
                        &channel_id,
                        &ChannelEvent::TypingStop {
                            channel_id: channel_id.clone(),
                            user_id: user_id.to_string(),
                        },
                    );
                }
                Err(e) if e.status_code == axum::http::StatusCode::FORBIDDEN => {
                    let ephemeral = serde_json::json!({
                        "kind": "ephemeral",
                        "content": "You are not a member of this channel.",
                        "channel_id": channel_id,
                    })
                    .to_string();
                    let _ = cmd_tx.send(UserCommand::SendEphemeral(ephemeral));
                }
                Err(e) => {
                    tracing::warn!(error = ?e, "Failed to persist WS message");
                }
            }
        }
        ClientPayload::TypingEvent { channel_id } => {
            station.satellite.broadcast_channel(
                &channel_id,
                &ChannelEvent::TypingEvent {
                    channel_id: channel_id.clone(),
                    user_id: user_id.to_string(),
                },
            );
        }
        ClientPayload::TypingStop { channel_id } => {
            station.satellite.broadcast_channel(
                &channel_id,
                &ChannelEvent::TypingStop {
                    channel_id: channel_id.clone(),
                    user_id: user_id.to_string(),
                },
            );
        }
        ClientPayload::VoiceStatus { channel_id, status } => {
            let status_str = match &status {
                VoiceStatusKind::Muted => "muted",
                VoiceStatusKind::Deafened => "deafened",
                VoiceStatusKind::Undeafened => "undeafened",
                VoiceStatusKind::Talking => "talking",
                VoiceStatusKind::Idle => "idle",
            };

            station
                .satellite
                .voice_sticky_status_flags_set(&channel_id, user_id, status_str);
            station.satellite.broadcast_channel(
                &channel_id,
                &ChannelEvent::VoiceStatus {
                    channel_id: channel_id.clone(),
                    user_id: user_id.to_string(),
                    status,
                },
            );
        }
        ClientPayload::UserStatus { status } => {
            station.satellite.set_user_status(user_id, status.clone());
            station
                .satellite
                .broadcast_server(&ServerEvent::UserStatusChanged {
                    user_id: user_id.to_string(),
                    status,
                });
        }
    }
}

// Teardown: clean up voice and screenshare state on disconnect

fn cleanup_on_disconnect(station: &Arc<Station>, user_id: &str) {
    station
        .satellite
        .broadcast_server(&ServerEvent::UserOffline {
            user_id: user_id.to_string(),
        });
    station.satellite.remove_user_status(user_id);
    station.satellite.unregister_session(user_id);

    // Clean up voice sessions - remove user from all voice channels
    let left_channels = station.satellite.voice_leave_all(user_id);
    for cid in left_channels {
        station.satellite.broadcast_channel(
            &cid,
            &ChannelEvent::VoiceLeft {
                channel_id: cid.clone(),
                user_id: user_id.to_string(),
            },
        );
        tracing::debug!(channel_id = %cid, user_id = %user_id, "Voice cleanup on WS disconnect");
    }

    // Clean up screenshare sessions
    let cleared_screenshares = station.satellite.screenshare_clear_user(user_id);
    for cid in cleared_screenshares {
        station.satellite.broadcast_channel(
            &cid,
            &ChannelEvent::ScreenshareStopped {
                channel_id: cid.clone(),
            },
        );
        tracing::debug!(channel_id = %cid, user_id = %user_id, "Screenshare cleanup on WS disconnect");
    }

    tracing::debug!(user_id = %user_id, "WS connection closed");
}

/// Core per-connection logic. Runs two sibling tasks that share the socket halves:
///
///   recv_task  - reads frames from the client, validates, persists, broadcasts
///   send_task  - multiplexes broadcast channels + cmd_rx into outbound frames
///
/// Communication between the two tasks (and from external REST handlers) goes
/// through an `mpsc::unbounded_channel<UserCommand>` (`cmd_tx` / `cmd_rx`).
/// When either task exits (disconnect, error, or server shutdown) the other is
/// aborted via `tokio::select!` in the teardown block.
async fn handle_socket(socket: WebSocket, station: Arc<Station>, user_id: String) {
    let (mut sender, mut receiver) = socket.split();

    // Unbounded so REST handlers (join/leave) never block while delivering commands.
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<UserCommand>();

    // Register this connection in the satellite so join/leave REST calls can
    // push subscribe/unsubscribe commands directly to this socket.
    station.satellite.register_session(&user_id, cmd_tx.clone());
    station
        .satellite
        .set_user_status(&user_id, crate::core::models::UserStatus::Online);
    station
        .satellite
        .broadcast_server(&ServerEvent::UserOnline {
            user_id: user_id.to_string(),
        });

    // A. Receive task
    let station_c = station.clone();
    let uid_c = user_id.clone();
    let cmd_tx_c = cmd_tx.clone();

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let WsMessage::Text(text) = msg {
                match serde_json::from_str::<ClientPayload>(&text) {
                    Ok(payload) => {
                        handle_client_payload(payload, &station_c, &uid_c, &cmd_tx_c).await;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, raw = %text, "Failed to parse WS payload");
                    }
                }
            } else if let WsMessage::Close(_) = msg {
                break;
            }
        }
    });

    // B. Initial channel subscriptions
    // StreamMap merges multiple broadcast receivers into one stream.
    // The key is channel_id (String); the value is a BroadcastStream wrapping a
    // `broadcast::Receiver<String>`. `StreamMap::next()` polls all inner streams
    // in round-robin and yields `(key, item)` - no manual fan-in needed.

    let mut channels_map: StreamMap<String, BroadcastStream<String>> = StreamMap::new();

    // Pre-subscribe to channels the user is already a member of.
    // Necessary for reconnects - join/leave only fire for new memberships.
    {
        let uid = user_id.clone();
        let station_c = station.clone();

        let initial_channels = tokio::task::spawn_blocking(move || {
            use crate::schema::channel_members::dsl as cm;

            let mut conn = station_c.pool.get().ok()?;
            cm::channel_members
                .filter(cm::user_id.eq(uid))
                .select(cm::channel_id)
                .load::<String>(&mut conn)
                .ok()
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

        for cid in initial_channels {
            let rx = station.satellite.get_channel_sender(&cid).subscribe();
            channels_map.insert(cid, BroadcastStream::new(rx));
        }
    }

    // C. Send task
    // Multiplexes all inbound streams (commands, channel messages, server events,
    // keepalive pings, shutdown signal) into outbound WebSocket frames.

    let mut server_rx = BroadcastStream::new(station.satellite.subscribe_server());
    let mut shutdown_rx = station.shutdown.subscribe();
    let mut ping_ticker = interval(PING_INTERVAL);
    ping_ticker.tick().await; // skip the immediate first tick

    let station_c = station.clone();
    let uid_c = user_id.clone();
    let mut send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // 1. Handle subscribe/unsubscribe commands (from join/leave REST
                //    endpoints via the satellite, or from the client directly).
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        UserCommand::Subscribe(cid) => {
                            let rx = station_c.satellite.get_channel_sender(&cid).subscribe();
                            channels_map.insert(cid, BroadcastStream::new(rx));
                        }
                        UserCommand::Unsubscribe(cid) => {
                            channels_map.remove(&cid);
                        }
                        UserCommand::SendEphemeral(json) => {
                            let _ = sender.send(WsMessage::Text(json.into())).await;
                        }
                        UserCommand::Disconnect(reason) => {
                            let _ = sender.send(WsMessage::Close(Some(CloseFrame {
                                code: 4001,
                                reason: reason.into(),
                            }))).await;
                            break;
                        }
                    }
                }

                // 2. Forward messages from any subscribed channel to the client
                Some((channel_id, result)) = channels_map.next() => {
                    match result {
                        Ok(msg_json) => {
                            if sender.send(WsMessage::Text(msg_json.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                            tracing::warn!(skipped = n, %channel_id, "WS receiver lagged");
                        }
                    }
                }

                // 3. Server-wide events (channel_created, channel_updated, etc.)
                Some(result) = server_rx.next() => {
                    if let Ok(json) = result
                        && sender.send(WsMessage::Text(json.into())).await.is_err() {
                            break;
                        }
                }

                // 4. Keepalive pings
                _ = ping_ticker.tick() => {
                    if sender.send(WsMessage::Ping(vec![].into())).await.is_err() {
                        break;
                    }
                }

                // 5. Server is shutting down - send a clean Close frame
                Ok(_) = shutdown_rx.changed() => {
                    tracing::info!("WS shutdown: sending Close frame");
                    tracing::debug!(user_id = %uid_c, "WS shutdown: sending Close frame");
                    let _ = sender.send(WsMessage::Close(Some(CloseFrame {
                        code: 1001, // Going Away
                        reason: "server shutting down".into(),
                    }))).await;
                    break;
                }
            }
        }
    });

    // D. Teardown
    // Whichever task exits first wins.
    // If send_task exits (disconnect, ping timeout, or server shutdown): give
    // recv_task 2s to drain any in-flight frames, then abort it hard.
    // If recv_task exits (client close frame, read error): abort send_task immediately.

    tokio::select! {
        _ = (&mut send_task) => {
            let _ = timeout(Duration::from_secs(2), &mut recv_task).await;
            recv_task.abort();
        }
        _ = (&mut recv_task) => send_task.abort(),
    }

    cleanup_on_disconnect(&station, &user_id);
}
