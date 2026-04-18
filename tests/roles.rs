mod common;

use axum::http::Method;
use serde_json::json;

// Role CRUD

#[tokio::test]
async fn list_roles_returns_default_roles() {
    let orbit = common::test_orbit();
    common::ensure_user(orbit.clone(), "role-lister").await;

    let res = common::request(orbit, common::authed(Method::GET, "/roles", "role-lister")).await;
    assert_eq!(res.status().as_u16(), 200);

    let body = common::body_json(res).await;
    let roles = body["data"].as_array().unwrap();
    assert!(roles.len() >= 3);

    let names: Vec<&str> = roles.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Admin"));
    assert!(names.contains(&"Moderator"));
    assert!(names.contains(&"Member"));
}

#[tokio::test]
async fn create_role_requires_owner() {
    let orbit = common::test_orbit();
    common::ensure_user(orbit.clone(), "role-creator").await;

    // Regular user can't create roles
    let res = common::request(
        orbit.clone(),
        common::authed_json(
            Method::POST,
            "/roles",
            "role-creator",
            json!({"name": "Custom", "permissions": 0}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);

    // Owner can create roles
    let res = common::request(
        orbit,
        common::authed_owner_json(
            Method::POST,
            "/roles",
            "role-owner-creator",
            json!({"name": "Custom Role", "permissions": 6, "priority": 25}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    let body = common::body_json(res).await;
    assert_eq!(body["data"]["name"], "Custom Role");
    assert_eq!(body["data"]["permissions"], 6);
}

#[tokio::test]
async fn delete_builtin_role_returns_403() {
    let orbit = common::test_orbit();

    let res = common::request(
        orbit,
        common::authed_owner(Method::DELETE, "/roles/role-admin", "role-deleter"),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn assign_server_role() {
    let orbit = common::test_orbit();
    common::ensure_user(orbit.clone(), "assign-target").await;
    common::ensure_server_member(&orbit, "assign-target");

    // Owner assigns admin role
    let res = common::request(
        orbit.clone(),
        common::authed_owner_json(
            Method::PUT,
            "/members/assign-target/role",
            "role-assigner-owner",
            json!({"role_id": "role-admin"}),
        ),
    )
    .await;
    let status = res.status().as_u16();
    if status != 204 {
        let body = common::body_json(res).await;
        panic!("Expected 204, got {status}: {body}");
    }

    // Regular user can't assign roles
    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            "/members/assign-target/role",
            "role-assigner-regular",
            json!({"role_id": "role-mod"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn my_permissions_for_owner() {
    let orbit = common::test_orbit();

    let res = common::request(
        orbit,
        common::authed_owner(Method::GET, "/me/permissions", "perm-owner"),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    let body = common::body_json(res).await;
    assert_eq!(body["data"]["is_owner"], true);
    assert!(body["data"]["permissions"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn my_permissions_for_regular_user() {
    let orbit = common::test_orbit();
    common::ensure_user(orbit.clone(), "perm-regular").await;

    let res = common::request(
        orbit,
        common::authed(Method::GET, "/me/permissions", "perm-regular"),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    let body = common::body_json(res).await;
    assert_eq!(body["data"]["is_owner"], false);
    assert_eq!(body["data"]["permissions"], 0);
    assert_eq!(body["data"]["priority"], 0);
}

#[tokio::test]
async fn my_permissions_includes_priority_for_admin() {
    let orbit = common::test_orbit();
    common::ensure_user(orbit.clone(), "perm-admin-user").await;
    common::ensure_server_member(&orbit, "perm-admin-user");
    common::set_server_role(&orbit, "perm-admin-user", "role-admin");

    let res = common::request(
        orbit,
        common::authed(Method::GET, "/me/permissions", "perm-admin-user"),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    let body = common::body_json(res).await;
    assert_eq!(body["data"]["is_owner"], false);
    // Admin role has priority 100 in seed_defaults.
    assert_eq!(body["data"]["priority"], 100);
    assert_eq!(body["data"]["role_id"], "role-admin");
}

// Priority hierarchy

#[tokio::test]
async fn admin_cannot_create_role_at_or_above_own_priority() {
    let orbit = common::test_orbit();
    common::ensure_user(orbit.clone(), "prio-create-admin").await;
    common::ensure_server_member(&orbit, "prio-create-admin");
    // Moderator built-in has priority 50.
    common::set_server_role(&orbit, "prio-create-admin", "role-mod");
    // Give this role ADMINISTRATOR so the actor passes the permission gate
    // and the priority gate is what we're testing.
    common::request(
        orbit.clone(),
        common::authed_owner_json(
            Method::PUT,
            "/roles/role-mod",
            "setup-owner",
            json!({"permissions": 1}),
        ),
    )
    .await;

    // Equal priority: blocked
    let res = common::request(
        orbit.clone(),
        common::authed_json(
            Method::POST,
            "/roles",
            "prio-create-admin",
            json!({"name": "Peer", "permissions": 0, "priority": 50}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);

    // Above own priority: blocked
    let res = common::request(
        orbit.clone(),
        common::authed_json(
            Method::POST,
            "/roles",
            "prio-create-admin",
            json!({"name": "Higher", "permissions": 0, "priority": 80}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);

    // Below own priority: allowed
    let res = common::request(
        orbit,
        common::authed_json(
            Method::POST,
            "/roles",
            "prio-create-admin",
            json!({"name": "Below", "permissions": 0, "priority": 10}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
}

#[tokio::test]
async fn admin_cannot_edit_builtin_admin_role() {
    let orbit = common::test_orbit();
    common::ensure_user(orbit.clone(), "builtin-admin-editor").await;
    common::ensure_server_member(&orbit, "builtin-admin-editor");
    common::set_server_role(&orbit, "builtin-admin-editor", "role-admin");

    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            "/roles/role-admin",
            "builtin-admin-editor",
            json!({"permissions": 0}),
        ),
    )
    .await;
    let status = res.status().as_u16();
    let body = common::body_json(res).await;
    assert_eq!(status, 403);
    assert_eq!(body["error"], "ERR_ROLE_ADMIN_PROTECTED");
}

#[tokio::test]
async fn admin_cannot_assign_role_at_or_above_own_priority() {
    let orbit = common::test_orbit();
    common::ensure_user(orbit.clone(), "prio-assign-admin").await;
    common::ensure_user(orbit.clone(), "prio-assign-target").await;
    common::ensure_server_member(&orbit, "prio-assign-admin");
    common::ensure_server_member(&orbit, "prio-assign-target");
    common::set_server_role(&orbit, "prio-assign-admin", "role-mod");
    common::request(
        orbit.clone(),
        common::authed_owner_json(
            Method::PUT,
            "/roles/role-mod",
            "setup-owner-2",
            json!({"permissions": 1}),
        ),
    )
    .await;

    // Can't assign Admin (priority 100 > actor's 50).
    let res = common::request(
        orbit.clone(),
        common::authed_json(
            Method::PUT,
            "/members/prio-assign-target/role",
            "prio-assign-admin",
            json!({"role_id": "role-admin"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);

    // Can't assign own role (parity is not allowed).
    let res = common::request(
        orbit.clone(),
        common::authed_json(
            Method::PUT,
            "/members/prio-assign-target/role",
            "prio-assign-admin",
            json!({"role_id": "role-mod"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);

    // Can assign Member (priority 0).
    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            "/members/prio-assign-target/role",
            "prio-assign-admin",
            json!({"role_id": "role-member"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);
}

#[tokio::test]
async fn admin_cannot_unseat_higher_ranked_user() {
    let orbit = common::test_orbit();
    common::ensure_user(orbit.clone(), "unseat-actor").await;
    common::ensure_user(orbit.clone(), "unseat-target").await;
    common::ensure_server_member(&orbit, "unseat-actor");
    common::ensure_server_member(&orbit, "unseat-target");
    // Actor is Moderator (priority 50 with ADMINISTRATOR granted); target is Admin (priority 100).
    common::set_server_role(&orbit, "unseat-actor", "role-mod");
    common::set_server_role(&orbit, "unseat-target", "role-admin");
    common::request(
        orbit.clone(),
        common::authed_owner_json(
            Method::PUT,
            "/roles/role-mod",
            "setup-owner-3",
            json!({"permissions": 1}),
        ),
    )
    .await;

    // Clearing (null) another admin's role is still blocked by hierarchy.
    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            "/members/unseat-target/role",
            "unseat-actor",
            json!({"role_id": null}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

// Channel permissions

#[tokio::test]
async fn channel_creator_can_edit_own_channel() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "ch-creator", json!({})).await;

    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "ch-creator",
            json!({"name": "renamed-by-creator"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
}

#[tokio::test]
async fn non_creator_cannot_edit_channel() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "ch-owner-perm", json!({})).await;
    common::join_channel(orbit.clone(), &id, "ch-member-perm").await;

    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "ch-member-perm",
            json!({"name": "nope"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn server_owner_can_edit_any_channel() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "ch-some-creator", json!({})).await;

    let res = common::request(
        orbit,
        common::authed_owner_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "the-server-owner",
            json!({"name": "owner-renamed"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
}

#[tokio::test]
async fn archive_before_delete_required() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "arch-del-test", json!({})).await;

    // Can't delete non-archived channel
    let res = common::request(
        orbit.clone(),
        common::authed(Method::DELETE, &format!("/channels/{id}"), "arch-del-test"),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);

    // Archive it
    let res = common::request(
        orbit.clone(),
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "arch-del-test",
            json!({"is_archived": true}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);

    // Now delete works
    let res = common::request(
        orbit,
        common::authed(Method::DELETE, &format!("/channels/{id}"), "arch-del-test"),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);
}

// Channel roles

#[tokio::test]
async fn set_channel_role_by_creator() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "ch-role-creator", json!({})).await;
    common::join_channel(orbit.clone(), &id, "ch-role-target").await;

    // Creator can set channel role
    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}/members/ch-role-target/role"),
            "ch-role-creator",
            json!({"role": "manager"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);
}

#[tokio::test]
async fn channel_manager_can_edit_channel() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "ch-mgr-owner", json!({})).await;
    common::join_channel(orbit.clone(), &id, "ch-mgr-user").await;

    // Promote to manager
    common::request(
        orbit.clone(),
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}/members/ch-mgr-user/role"),
            "ch-mgr-owner",
            json!({"role": "manager"}),
        ),
    )
    .await;

    // Manager can edit
    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "ch-mgr-user",
            json!({"name": "manager-renamed"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
}

#[tokio::test]
async fn channel_moderator_cannot_edit_channel() {
    let orbit = common::test_orbit();
    let (id, _) = common::create_channel(orbit.clone(), "ch-mod-owner", json!({})).await;
    common::join_channel(orbit.clone(), &id, "ch-mod-user").await;

    // Promote to moderator (not manager)
    common::request(
        orbit.clone(),
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}/members/ch-mod-user/role"),
            "ch-mod-owner",
            json!({"role": "moderator"}),
        ),
    )
    .await;

    // Moderator cannot edit channel
    let res = common::request(
        orbit,
        common::authed_json(
            Method::PUT,
            &format!("/channels/{id}"),
            "ch-mod-user",
            json!({"name": "mod-nope"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);
}

// Message moderation

#[tokio::test]
async fn channel_moderator_can_delete_others_messages() {
    let orbit = common::test_orbit();
    let (ch_id, _) = common::create_channel(orbit.clone(), "mod-del-owner", json!({})).await;
    common::join_channel(orbit.clone(), &ch_id, "mod-del-sender").await;
    common::join_channel(orbit.clone(), &ch_id, "mod-del-mod").await;

    // Send a message
    let (msg_id, _) =
        common::create_message(orbit.clone(), &ch_id, "mod-del-sender", "hello").await;

    // Regular member can't delete others' messages
    let res = common::request(
        orbit.clone(),
        common::authed(
            Method::DELETE,
            &format!("/channels/{ch_id}/messages/{msg_id}"),
            "mod-del-mod",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);

    // Promote to moderator
    common::request(
        orbit.clone(),
        common::authed_json(
            Method::PUT,
            &format!("/channels/{ch_id}/members/mod-del-mod/role"),
            "mod-del-owner",
            json!({"role": "moderator"}),
        ),
    )
    .await;

    // Now moderator can delete
    let res = common::request(
        orbit,
        common::authed(
            Method::DELETE,
            &format!("/channels/{ch_id}/messages/{msg_id}"),
            "mod-del-mod",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);
}

// Member management

#[tokio::test]
async fn remove_member_requires_permission() {
    let orbit = common::test_orbit();
    let (ch_id, _) = common::create_channel(orbit.clone(), "rm-owner", json!({})).await;
    common::join_channel(orbit.clone(), &ch_id, "rm-target").await;
    common::join_channel(orbit.clone(), &ch_id, "rm-regular").await;

    // Regular member can't remove others
    let res = common::request(
        orbit.clone(),
        common::authed(
            Method::DELETE,
            &format!("/channels/{ch_id}/members/rm-target"),
            "rm-regular",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);

    // Creator can remove
    let res = common::request(
        orbit,
        common::authed(
            Method::DELETE,
            &format!("/channels/{ch_id}/members/rm-target"),
            "rm-owner",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);
}

#[tokio::test]
async fn channel_manager_can_remove_members() {
    let orbit = common::test_orbit();
    let (ch_id, _) = common::create_channel(orbit.clone(), "mgr-rm-owner", json!({})).await;
    common::join_channel(orbit.clone(), &ch_id, "mgr-rm-manager").await;
    common::join_channel(orbit.clone(), &ch_id, "mgr-rm-target").await;

    // Promote to manager
    common::request(
        orbit.clone(),
        common::authed_json(
            Method::PUT,
            &format!("/channels/{ch_id}/members/mgr-rm-manager/role"),
            "mgr-rm-owner",
            json!({"role": "manager"}),
        ),
    )
    .await;

    // Manager can remove members
    let res = common::request(
        orbit,
        common::authed(
            Method::DELETE,
            &format!("/channels/{ch_id}/members/mgr-rm-target"),
            "mgr-rm-manager",
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 204);
}

// Broadcast channel

#[tokio::test]
async fn broadcast_posting_restricted() {
    let orbit = common::test_orbit();
    let (ch_id, _) =
        common::create_channel(orbit.clone(), "bcast-owner", json!({"kind": "broadcast"})).await;
    common::join_channel(orbit.clone(), &ch_id, "bcast-member").await;
    common::join_channel(orbit.clone(), &ch_id, "bcast-mod").await;

    // Regular member can't post in broadcast
    let res = common::request(
        orbit.clone(),
        common::authed_json(
            Method::POST,
            &format!("/channels/{ch_id}/messages"),
            "bcast-member",
            json!({"content": "nope", "sender_id": "bcast-member"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 403);

    // Creator can post
    let res = common::request(
        orbit.clone(),
        common::authed_json(
            Method::POST,
            &format!("/channels/{ch_id}/messages"),
            "bcast-owner",
            json!({"content": "announcement", "sender_id": "bcast-owner"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 201);

    // Promote member to moderator
    common::request(
        orbit.clone(),
        common::authed_json(
            Method::PUT,
            &format!("/channels/{ch_id}/members/bcast-mod/role"),
            "bcast-owner",
            json!({"role": "moderator"}),
        ),
    )
    .await;

    // Moderator can post in broadcast
    let res = common::request(
        orbit,
        common::authed_json(
            Method::POST,
            &format!("/channels/{ch_id}/messages"),
            "bcast-mod",
            json!({"content": "mod post", "sender_id": "bcast-mod"}),
        ),
    )
    .await;
    assert_eq!(res.status().as_u16(), 201);
}
