mod common;

use axum::http::Method;
use serde_json::json;

#[tokio::test]
async fn maintenance_blocks_writes_allows_reads() {
    let orbit = common::test_orbit();
    common::station(&orbit).set_maintenance(true);

    let res = common::request(
        orbit.clone(),
        common::authed(Method::GET, "/channels", "user-1"),
    )
    .await;
    assert_ne!(res.status().as_u16(), 503);

    let res = common::request(
        orbit,
        common::authed_json(Method::POST, "/channels", "user-1", json!({"name": "nope"})),
    )
    .await;
    assert_eq!(res.status().as_u16(), 503);
}

#[tokio::test]
async fn internal_endpoints_exempt_from_maintenance() {
    let orbit = common::test_orbit();
    common::station(&orbit).set_maintenance(true);

    let res = common::request(
        orbit,
        common::authed_json(
            Method::POST,
            "/internal/maintenance",
            "user-1",
            json!({"enabled": false}),
        ),
    )
    .await;
    assert_ne!(res.status().as_u16(), 503);
}
