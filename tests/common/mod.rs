#![allow(dead_code)]
use std::sync::{Arc, OnceLock};

use alacahoyuk::{
    DbPool, LiveKitConfig, Orbit, Satellite, Server, Station, build_router, init_db,
    utils::{jwks::JwksCache, rate_limit::new_ip_limiter},
};
use axum::{
    Router,
    body::Body,
    http::{Method, Request},
    response::Response,
};
use dashmap::DashMap;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, encode};
use p384::ecdsa::SigningKey;
use serde_json::json;
use tower::ServiceExt;

const TEST_KID: &str = "test-kid";
const TEST_SERVER_REMOTE_ID: &str = "test-server-remote-id";

struct TestKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

static TEST_STATE: OnceLock<(DbPool, Server, TestKeys)> = OnceLock::new();

fn shared_state() -> &'static (DbPool, Server, TestKeys) {
    TEST_STATE.get_or_init(|| {
        let test_remote_id = "alacahoyuk_test";
        // Clean up stale files from prior runs (db + WAL + SHM)
        let db_path = format!("/tmp/tumulus_{test_remote_id}.db");
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{db_path}{suffix}"));
        }
        // SAFETY: called once via OnceLock before any threads read DATA_DIR
        unsafe { std::env::set_var("DATA_DIR", "/tmp") };
        let pool = init_db(test_remote_id, "test-key");

        let mut conn = pool.get().unwrap();
        let server = Server::get_or_create(
            &mut conn,
            "test".to_string(),
            Some(TEST_SERVER_REMOTE_ID.to_string()),
        )
        .unwrap();

        // Seed default roles
        alacahoyuk::core::roles::service::RoleService::seed_defaults(&mut conn);

        let signing_key = SigningKey::random(&mut p384::ecdsa::signature::rand_core::OsRng);
        use p384::pkcs8::{EncodePrivateKey, EncodePublicKey};
        let private_pem = signing_key.to_pkcs8_pem(Default::default()).unwrap();
        let public_pem = signing_key
            .verifying_key()
            .to_public_key_pem(Default::default())
            .unwrap();

        let keys = TestKeys {
            encoding: EncodingKey::from_ec_pem(private_pem.as_bytes()).unwrap(),
            decoding: DecodingKey::from_ec_pem(public_pem.as_bytes()).unwrap(),
        };

        (pool, server, keys)
    })
}

// JWT

#[derive(serde::Serialize)]
struct AccessClaims {
    sub: String,
    sid: String,
    #[serde(rename = "type")]
    token_type: String,
    name: String,
    owner: bool,
    discriminator: i32,
    staff: bool,
    iat: i64,
    exp: i64,
}

pub fn access_token(user_id: &str, username: &str) -> String {
    access_token_full(user_id, username, TEST_SERVER_REMOTE_ID, 3600, false)
}

pub fn access_token_owner(user_id: &str) -> String {
    access_token_full(user_id, user_id, TEST_SERVER_REMOTE_ID, 3600, true)
}

pub fn access_token_with(user_id: &str, username: &str, sid: &str, lifetime_secs: i64) -> String {
    access_token_full(user_id, username, sid, lifetime_secs, false)
}

fn access_token_full(
    user_id: &str,
    username: &str,
    sid: &str,
    lifetime_secs: i64,
    owner: bool,
) -> String {
    let state = shared_state();
    let now = chrono::Utc::now().timestamp();
    let claims = AccessClaims {
        sub: user_id.to_string(),
        sid: sid.to_string(),
        token_type: "access".to_string(),
        name: username.to_string(),
        owner,
        discriminator: 1234,
        staff: false,
        iat: now,
        exp: now + lifetime_secs,
    };
    let mut header = Header::new(jsonwebtoken::Algorithm::ES384);
    header.kid = Some(TEST_KID.to_string());
    encode(&header, &claims, &state.2.encoding).unwrap()
}

pub fn expired_token(user_id: &str) -> String {
    // -120s to clear jsonwebtoken's default 60s leeway
    access_token_with(user_id, "expired-user", TEST_SERVER_REMOTE_ID, -120)
}

pub fn wrong_server_token(user_id: &str) -> String {
    access_token_with(user_id, "wrong-server-user", "wrong-server-id", 3600)
}

// Orbit

pub fn orbit_with_limits(per_sec: u32, burst: u32) -> Arc<Orbit> {
    let state = shared_state();
    let (shutdown_tx, _) = tokio::sync::watch::channel(false);
    let jwks = JwksCache::from_keys(vec![(TEST_KID.to_string(), state.2.decoding.clone())]);

    let station = Arc::new(Station {
        pool: state.0.clone(),
        server: state.1.clone(),
        satellite: Satellite::new(),
        user_rate_limiter: alacahoyuk::utils::rate_limit::new_user_limiter(1000, 1000),
        shutdown: shutdown_tx,
    });

    let stations = DashMap::new();
    stations.insert(TEST_SERVER_REMOTE_ID.to_string(), station);

    Arc::new(Orbit {
        stations,
        livekit: LiveKitConfig::default(),
        license_key: None,
        semerkant_url: None,
        jwks: Some(Arc::new(jwks)),
        http_rate_limiter: new_ip_limiter(per_sec, burst),
    })
}

pub fn test_orbit() -> Arc<Orbit> {
    orbit_with_limits(1000, 1000)
}

/// Convenience: get the default station from an orbit (for tests that need it).
pub fn station(orbit: &Arc<Orbit>) -> Arc<Station> {
    orbit.default_station().expect("test orbit has no stations")
}

// Request helpers

pub fn authed_owner(method: Method, uri: &str, user_id: &str) -> Request<Body> {
    let token = access_token_owner(user_id);
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

pub fn authed_owner_json(
    method: Method,
    uri: &str,
    user_id: &str,
    body: serde_json::Value,
) -> Request<Body> {
    let token = access_token_owner(user_id);
    let bytes = serde_json::to_vec(&body).unwrap();
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("content-length", bytes.len().to_string())
        .body(Body::from(bytes))
        .unwrap()
}

pub fn authed(method: Method, uri: &str, user_id: &str) -> Request<Body> {
    let token = access_token(user_id, user_id);
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

pub fn authed_json(
    method: Method,
    uri: &str,
    user_id: &str,
    body: serde_json::Value,
) -> Request<Body> {
    let token = access_token(user_id, user_id);
    let bytes = serde_json::to_vec(&body).unwrap();
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("content-length", bytes.len().to_string())
        .body(Body::from(bytes))
        .unwrap()
}

// Send helpers
// Accept orbit directly so callers don't need test_app boilerplate

pub async fn send(app: Router, req: Request<Body>) -> Response {
    app.oneshot(req).await.unwrap()
}

pub fn test_app(orbit: Arc<Orbit>) -> Router {
    build_router(orbit)
}

pub async fn request(orbit: Arc<Orbit>, req: Request<Body>) -> Response {
    build_router(orbit).oneshot(req).await.unwrap()
}

pub async fn body_json(res: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// Domain helpers

pub async fn create_channel(
    orbit: Arc<Orbit>,
    user_id: &str,
    extra: serde_json::Value,
) -> (String, serde_json::Value) {
    let mut body = json!({"name": format!("ch-{}", &uuid::Uuid::new_v4().to_string()[..8])});
    if let serde_json::Value::Object(m) = extra {
        body.as_object_mut().unwrap().extend(m);
    }
    let res = request(orbit, authed_json(Method::POST, "/channels", user_id, body)).await;
    let status = res.status().as_u16();
    let body = body_json(res).await;
    assert_eq!(status, 201, "create_channel failed: {status} {body}");
    let id = body["data"]["id"].as_str().unwrap().to_string();
    (id, body)
}

pub async fn create_message(
    orbit: Arc<Orbit>,
    channel_id: &str,
    user_id: &str,
    content: &str,
) -> (String, serde_json::Value) {
    let res = request(
        orbit,
        authed_json(
            Method::POST,
            &format!("/channels/{channel_id}/messages"),
            user_id,
            json!({"content": content, "sender_id": user_id}),
        ),
    )
    .await;
    let body = body_json(res).await;
    let id = body["data"]["id"].as_str().unwrap().to_string();
    (id, body)
}

pub async fn join_channel(orbit: Arc<Orbit>, channel_id: &str, user_id: &str) {
    request(
        orbit,
        authed(
            Method::POST,
            &format!("/channels/{channel_id}/join"),
            user_id,
        ),
    )
    .await;
}

pub async fn ensure_user(orbit: Arc<Orbit>, user_id: &str) {
    request(orbit, authed(Method::GET, "/users", user_id)).await;
}

/// Ensure user exists as a server member (needed for role assignment).
/// ensure_user creates the User row via JWT sync but NOT the server_member row.
pub fn ensure_server_member(orbit: &Arc<Orbit>, user_id: &str) {
    let station = orbit.default_station().unwrap();
    let mut conn = station.pool.get().unwrap();
    use alacahoyuk::schema::server_members;
    diesel::insert_or_ignore_into(server_members::table)
        .values((
            server_members::server_id.eq("main"),
            server_members::user_id.eq(user_id),
            server_members::metadata.eq("{}"),
        ))
        .execute(&mut conn)
        .expect("ensure_server_member insert failed");
}

/// Assign a server role to a user, bypassing the API so tests can set up an
/// actor with a specific role without needing an owner token for every step.
pub fn set_server_role(orbit: &Arc<Orbit>, user_id: &str, role_id: &str) {
    let station = orbit.default_station().unwrap();
    let mut conn = station.pool.get().unwrap();
    use alacahoyuk::schema::server_members;
    diesel::update(
        server_members::table
            .filter(server_members::user_id.eq(user_id))
            .filter(server_members::server_id.eq("main")),
    )
    .set(server_members::role_id.eq(role_id))
    .execute(&mut conn)
    .expect("set_server_role update failed");
}

use diesel::prelude::*;

// WebSocket helpers

pub async fn spawn_server(orbit: Arc<Orbit>) -> std::net::SocketAddr {
    let app = build_router(orbit);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

pub async fn ws_connect(
    addr: std::net::SocketAddr,
    user_id: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let token = access_token(user_id, user_id);
    let url = format!("ws://{addr}/ws");
    let request = tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(&url)
        .header("authorization", format!("Bearer {token}"))
        .header(
            "sec-websocket-key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .header("sec-websocket-version", "13")
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("host", addr.to_string())
        .body(())
        .unwrap();
    let (ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    // Let the server complete the WS handshake and initial channel subscriptions
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    ws
}
