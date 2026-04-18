use std::sync::Arc;

use diesel::prelude::*;
use uuid::Uuid;

use super::dto::{AssignRoleDto, CreateRoleDto, ServerMemberDto, UpdateRoleDto};
use crate::{
    Station,
    core::{
        models::{metadata::RoleMetadata, Role, Server, User},
        permissions::{self, Permissions, ROLE_ADMIN_ID, ROLE_MEMBER_ID, ROLE_MOD_ID},
        satellite::ServerEvent,
        types::SqliteJson,
    },
    schema::{roles, server_members, users},
    utils::{
        helpers::now_rfc3339,
        response::{ApiError, ApiResponse, ApiResult, codes},
    },
};

const BUILT_IN_ROLE_IDS: &[&str] = &[ROLE_ADMIN_ID, ROLE_MOD_ID, ROLE_MEMBER_ID];

/// Changeset used by `RoleService::update` to apply all optional fields of
/// an UpdateRoleDto in a single SQL UPDATE. Diesel's `AsChangeset` skips
/// `None` fields automatically, so the bitmask of "what to change" is the
/// shape of the struct itself.
#[derive(AsChangeset)]
#[diesel(table_name = roles)]
struct RoleUpdateChangeset {
    name: Option<String>,
    permissions: Option<i32>,
    priority: Option<i32>,
    color: Option<i32>,
    is_mentionable: Option<bool>,
}

pub struct RoleService;

impl RoleService {
    // Seed default roles on server init. Idempotent via INSERT OR IGNORE.
    pub fn seed_defaults(conn: &mut SqliteConnection) {
        let defaults = [
            Role {
                id: ROLE_ADMIN_ID.to_string(),
                server_id: Server::SINGLETON_ID.to_string(),
                name: "Admin".to_string(),
                permissions: Permissions::ADMINISTRATOR.bits(),
                priority: Some(100),
                color: Some(0xE74C3C_u32 as i32),
                is_mentionable: Some(true),
                metadata: SqliteJson(RoleMetadata::default()),
                created_by: "system".to_string(),
                created_at: Some(now_rfc3339()),
            },
            Role {
                id: ROLE_MOD_ID.to_string(),
                server_id: Server::SINGLETON_ID.to_string(),
                name: "Moderator".to_string(),
                permissions: (Permissions::MANAGE_MEMBERS | Permissions::MANAGE_MESSAGES).bits(),
                priority: Some(50),
                color: Some(0x3498DB_u32 as i32),
                is_mentionable: Some(true),
                metadata: SqliteJson(RoleMetadata::default()),
                created_by: "system".to_string(),
                created_at: Some(now_rfc3339()),
            },
            Role {
                id: ROLE_MEMBER_ID.to_string(),
                server_id: Server::SINGLETON_ID.to_string(),
                name: "Member".to_string(),
                permissions: 0,
                priority: Some(0),
                color: Some(0x95A5A6_u32 as i32),
                is_mentionable: Some(false),
                metadata: SqliteJson(RoleMetadata::default()),
                created_by: "system".to_string(),
                created_at: Some(now_rfc3339()),
            },
        ];

        diesel::insert_or_ignore_into(roles::table)
            .values(&defaults[..])
            .execute(conn)
            .ok();

        // Backfill Member for any pre-existing server_members rows with a
        // null role_id. Auto-assign on join only covers new joiners; this
        // catches anyone who joined before auto-assign shipped. Safe here
        // because role-member is guaranteed to exist after the insert above.
        diesel::update(server_members::table.filter(server_members::role_id.is_null()))
            .set(server_members::role_id.eq(ROLE_MEMBER_ID))
            .execute(conn)
            .ok();
    }

    /// List every server member with identity + role assignment. Used by the
    /// Members settings view to render a roster with role pickers. Any authed
    /// member of the server can call this; visibility of the list isn't a
    /// secret; the role _picker_ is gated on the client by ADMINISTRATOR.
    pub async fn list_members(station: Arc<Station>) -> ApiResult<Vec<ServerMemberDto>> {
        let rows = tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().map_err(ApiError::internal)?;

            server_members::table
                .inner_join(users::table.on(users::id.eq(server_members::user_id)))
                .filter(server_members::server_id.eq(Server::SINGLETON_ID))
                .order(users::username.asc())
                .select((
                    server_members::user_id,
                    server_members::role_id,
                    server_members::nickname,
                    server_members::joined_at,
                    User::as_select(),
                ))
                .load::<(
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    User,
                )>(&mut conn)
                .map_err(ApiError::internal)
        })
        .await
        .map_err(ApiError::internal)??;

        let response = rows
            .into_iter()
            .map(|(user_id, role_id, nickname, joined_at, user)| ServerMemberDto {
                user_id,
                username: user.username,
                discriminator: user.discriminator,
                staff: user.staff,
                role_id,
                nickname,
                joined_at,
            })
            .collect();

        Ok(ApiResponse::ok(response))
    }

    pub async fn list(station: Arc<Station>) -> ApiResult<Vec<Role>> {
        let result = tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().map_err(ApiError::internal)?;
            roles::table
                .filter(roles::server_id.eq(Server::SINGLETON_ID))
                .order(roles::priority.desc())
                .load::<Role>(&mut conn)
                .map_err(ApiError::internal)
        })
        .await
        .map_err(ApiError::internal)??;

        Ok(ApiResponse::ok(result))
    }

    pub async fn create(
        station: Arc<Station>,
        payload: CreateRoleDto,
        user_id: String,
        is_owner: bool,
    ) -> ApiResult<Role> {
        let station_c = station.clone();
        let role = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            // Only owner or ADMINISTRATOR can manage roles
            permissions::require_server_permission(
                &mut conn,
                &user_id,
                is_owner,
                Permissions::ADMINISTRATOR,
            )?;

            // Hierarchy: new role's priority must be strictly below the actor's.
            let new_priority = payload.priority.unwrap_or(0);
            permissions::require_priority_above(&mut conn, &user_id, is_owner, new_priority)?;

            let new_role = Role {
                id: Uuid::new_v4().to_string(),
                server_id: Server::SINGLETON_ID.to_string(),
                name: payload.name,
                permissions: payload.permissions,
                priority: payload.priority,
                color: payload.color,
                is_mentionable: payload.is_mentionable,
                metadata: SqliteJson(payload.metadata.unwrap_or_default()),
                created_by: user_id,
                created_at: Some(now_rfc3339()),
            };

            diesel::insert_into(roles::table)
                .values(&new_role)
                .execute(&mut conn)
                .map_err(ApiError::internal)?;

            Ok(new_role)
        })
        .await
        .map_err(ApiError::internal)??;

        station
            .satellite
            .broadcast_server(&ServerEvent::RoleCreated { role: role.clone() });

        Ok(ApiResponse::ok(role))
    }

    pub async fn update(
        station: Arc<Station>,
        role_id: String,
        payload: UpdateRoleDto,
        user_id: String,
        is_owner: bool,
    ) -> ApiResult<Role> {
        let station_c = station.clone();
        let role = tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;

            permissions::require_server_permission(
                &mut conn,
                &user_id,
                is_owner,
                Permissions::ADMINISTRATOR,
            )?;

            // Member is the implicit "everyone" role auto-assigned on join.
            // Its shape is load-bearing (baseline priority 0, no permissions),
            // so it's fully immutable; not even the owner edits it.
            if role_id == ROLE_MEMBER_ID {
                return Err(ApiError::forbidden(codes::ERR_ROLE_PROTECTED));
            }

            // The built-in Admin role is the foundation of every permission
            // check. Locking non-owners out prevents an administrator from
            // stripping ADMINISTRATOR off it and taking down every admin.
            if role_id == ROLE_ADMIN_ID && !is_owner {
                return Err(ApiError::forbidden(codes::ERR_ROLE_ADMIN_PROTECTED));
            }

            // Verify role exists and load its current priority for the
            // hierarchy check below.
            let existing = roles::table
                .find(&role_id)
                .first::<Role>(&mut conn)
                .map_err(|_| ApiError::not_found(codes::ERR_ROLE_NOT_FOUND))?;

            // Actor must outrank the role they're editing.
            permissions::require_priority_above(
                &mut conn,
                &user_id,
                is_owner,
                existing.priority.unwrap_or(0),
            )?;

            // If priority is being changed, the new value must also be below
            // the actor's own priority, otherwise an admin could promote a
            // role up to their own level.
            if let Some(new_priority) = payload.priority {
                permissions::require_priority_above(&mut conn, &user_id, is_owner, new_priority)?;
            }

            let changeset = RoleUpdateChangeset {
                name: payload.name,
                permissions: payload.permissions,
                priority: payload.priority,
                color: payload.color,
                is_mentionable: payload.is_mentionable,
            };

            diesel::update(roles::table.find(&role_id))
                .set(&changeset)
                .execute(&mut conn)
                .map_err(ApiError::internal)?;

            roles::table
                .find(&role_id)
                .first::<Role>(&mut conn)
                .map_err(ApiError::internal)
        })
        .await
        .map_err(ApiError::internal)??;

        station
            .satellite
            .broadcast_server(&ServerEvent::RoleUpdated { role: role.clone() });

        Ok(ApiResponse::ok(role))
    }

    pub async fn delete(
        station: Arc<Station>,
        role_id: String,
        user_id: String,
        is_owner: bool,
    ) -> ApiResult<()> {
        let station_c = station.clone();
        let role_id_c = role_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;
            let role_id = role_id_c;

            permissions::require_server_permission(
                &mut conn,
                &user_id,
                is_owner,
                Permissions::ADMINISTRATOR,
            )?;

            if BUILT_IN_ROLE_IDS.contains(&role_id.as_str()) {
                return Err(ApiError::forbidden(codes::ERR_ROLE_PROTECTED));
            }

            // Verify role exists and load priority for the hierarchy check.
            let existing = roles::table
                .find(&role_id)
                .first::<Role>(&mut conn)
                .map_err(|_| ApiError::not_found(codes::ERR_ROLE_NOT_FOUND))?;

            permissions::require_priority_above(
                &mut conn,
                &user_id,
                is_owner,
                existing.priority.unwrap_or(0),
            )?;

            // Unassign from all server members (ON DELETE SET NULL would also do this,
            // but we're explicit)
            diesel::update(
                server_members::table.filter(server_members::role_id.eq(&role_id)),
            )
            .set(server_members::role_id.eq::<Option<String>>(None))
            .execute(&mut conn)
            .map_err(ApiError::internal)?;

            diesel::delete(roles::table.find(&role_id))
                .execute(&mut conn)
                .map_err(ApiError::internal)?;

            Ok(())
        })
        .await
        .map_err(ApiError::internal)??;

        station
            .satellite
            .broadcast_server(&ServerEvent::RoleDeleted { role_id });

        Ok(ApiResponse::empty())
    }

    pub async fn assign_server_role(
        station: Arc<Station>,
        target_user_id: String,
        payload: AssignRoleDto,
        actor_user_id: String,
        is_owner: bool,
    ) -> ApiResult<()> {
        let station_c = station.clone();
        let target_c = target_user_id.clone();
        let role_id_c = payload.role_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = station_c.pool.get().map_err(ApiError::internal)?;
            let target_user_id = target_c;

            permissions::require_server_permission(
                &mut conn,
                &actor_user_id,
                is_owner,
                Permissions::ADMINISTRATOR,
            )?;

            // Hierarchy check on the role being assigned: actor can't hand out
            // a role that matches or exceeds their own priority.
            if let Some(ref rid) = payload.role_id {
                let assigned = roles::table
                    .find(rid)
                    .first::<Role>(&mut conn)
                    .map_err(|_| ApiError::not_found(codes::ERR_ROLE_NOT_FOUND))?;
                permissions::require_priority_above(
                    &mut conn,
                    &actor_user_id,
                    is_owner,
                    assigned.priority.unwrap_or(0),
                )?;
            }

            // Hierarchy check on the target's *current* role: if the target
            // already outranks the actor, the actor can't unseat them.
            // Without this, an admin could strip a peer's role down to null.
            let current_role_id: Option<String> = server_members::table
                .filter(server_members::user_id.eq(&target_user_id))
                .filter(server_members::server_id.eq(Server::SINGLETON_ID))
                .select(server_members::role_id)
                .first::<Option<String>>(&mut conn)
                .ok()
                .flatten();
            if let Some(current_rid) = current_role_id {
                if let Ok(current) = roles::table.find(&current_rid).first::<Role>(&mut conn) {
                    permissions::require_priority_above(
                        &mut conn,
                        &actor_user_id,
                        is_owner,
                        current.priority.unwrap_or(0),
                    )?;
                }
            }

            diesel::update(
                server_members::table
                    .filter(server_members::user_id.eq(&target_user_id))
                    .filter(server_members::server_id.eq(Server::SINGLETON_ID)),
            )
            .set(server_members::role_id.eq(&payload.role_id))
            .execute(&mut conn)
            .map_err(ApiError::internal)?;

            Ok(())
        })
        .await
        .map_err(ApiError::internal)??;

        station
            .satellite
            .broadcast_server(&ServerEvent::MemberRoleUpdated {
                user_id: target_user_id,
                role_id: role_id_c,
            });

        Ok(ApiResponse::empty())
    }

    pub async fn my_permissions(
        station: Arc<Station>,
        user_id: String,
        is_owner: bool,
    ) -> ApiResult<serde_json::Value> {
        let result = tokio::task::spawn_blocking(move || {
            let mut conn = station.pool.get().map_err(ApiError::internal)?;

            if is_owner {
                // Owner has no role row; the client treats is_owner as infinite
                // priority and bypasses hierarchy checks accordingly.
                return Ok(serde_json::json!({
                    "permissions": Permissions::all().bits(),
                    "is_owner": true,
                    "role_id": null,
                    "priority": null,
                }));
            }

            let role = permissions::load_server_role(&mut conn, &user_id);
            let perms = role
                .as_ref()
                .map(|r| Permissions::from_role(r))
                .unwrap_or(Permissions::empty());

            let effective = if perms.contains(Permissions::ADMINISTRATOR) {
                Permissions::all().bits()
            } else {
                perms.bits()
            };

            let priority = role.as_ref().and_then(|r| r.priority).unwrap_or(0);

            Ok(serde_json::json!({
                "permissions": effective,
                "is_owner": false,
                "role_id": role.map(|r| r.id),
                "priority": priority,
            }))
        })
        .await
        .map_err(ApiError::internal)??;

        Ok(ApiResponse::ok(result))
    }
}
