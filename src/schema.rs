// @generated automatically by Diesel CLI.

diesel::table! {
    channel_members (channel_id, user_id) {
        channel_id -> Text,
        user_id -> Text,
        role_id -> Nullable<Text>,
        added_by -> Nullable<Text>,
        settings -> Text,
        joined_at -> Nullable<Text>,
        last_read_message_id -> Nullable<Text>,
        channel_role -> Nullable<Text>,
    }
}

diesel::table! {
    channel_pins (channel_id, message_id) {
        channel_id -> Text,
        message_id -> Text,
        pinned_by -> Text,
        pinned_at -> Nullable<Text>,
    }
}

diesel::table! {
    channels (id) {
        id -> Text,
        server_id -> Text,
        name -> Text,
        slug -> Text,
        kind -> Text,
        is_default -> Nullable<Bool>,
        is_private -> Nullable<Bool>,
        is_archived -> Nullable<Bool>,
        is_nsfw -> Nullable<Bool>,
        pin_limit -> Nullable<Integer>,
        metadata -> Text,
        created_by -> Text,
        created_at -> Nullable<Text>,
        updated_at -> Nullable<Text>,
    }
}

diesel::table! {
    group_members (group_id, user_id) {
        group_id -> Text,
        user_id -> Text,
        added_by -> Text,
        added_at -> Nullable<Text>,
    }
}

diesel::table! {
    groups (id) {
        id -> Text,
        server_id -> Text,
        name -> Text,
        description -> Nullable<Text>,
        is_mentionable -> Nullable<Bool>,
        created_by -> Text,
        created_at -> Nullable<Text>,
    }
}

diesel::table! {
    messages (id) {
        id -> Text,
        channel_id -> Text,
        sender_id -> Text,
        content -> Text,
        kind -> Text,
        is_repliable -> Nullable<Bool>,
        is_reactable -> Nullable<Bool>,
        is_pinned -> Nullable<Bool>,
        root_thread_id -> Nullable<Text>,
        parent_id -> Nullable<Text>,
        origin_message_id -> Nullable<Text>,
        deleted_at -> Nullable<Text>,
        updated_at -> Nullable<Text>,
        created_at -> Nullable<Text>,
    }
}

diesel::table! {
    notifications (id) {
        id -> Text,
        user_id -> Text,
        sender_id -> Nullable<Text>,
        kind -> Text,
        reference_id -> Nullable<Text>,
        is_read -> Nullable<Bool>,
        created_at -> Nullable<Text>,
    }
}

diesel::table! {
    reactions (message_id, user_id, emoji) {
        message_id -> Text,
        user_id -> Text,
        emoji -> Text,
        created_at -> Nullable<Text>,
    }
}

diesel::table! {
    roles (id) {
        id -> Text,
        server_id -> Text,
        name -> Text,
        permissions -> Integer,
        priority -> Nullable<Integer>,
        color -> Nullable<Integer>,
        is_mentionable -> Nullable<Bool>,
        metadata -> Text,
        created_by -> Text,
        created_at -> Nullable<Text>,
    }
}

diesel::table! {
    server_members (server_id, user_id) {
        server_id -> Text,
        user_id -> Text,
        role_id -> Nullable<Text>,
        nickname -> Nullable<Text>,
        metadata -> Text,
        joined_at -> Nullable<Text>,
    }
}

diesel::table! {
    servers (id) {
        id -> Text,
        remote_id -> Nullable<Text>,
        name -> Text,
        metadata -> Text,
        created_at -> Nullable<Text>,
        updated_at -> Nullable<Text>,
        last_event_cursor -> Nullable<Integer>,
        cert_version -> Nullable<Integer>,
    }
}

diesel::table! {
    users (id) {
        id -> Text,
        remote_id -> Text,
        username -> Text,
        created_at -> Nullable<Text>,
        updated_at -> Nullable<Text>,
        discriminator -> Integer,
        staff -> Bool,
    }
}

diesel::joinable!(channel_members -> channels (channel_id));
diesel::joinable!(channel_members -> roles (role_id));
diesel::joinable!(channel_pins -> channels (channel_id));
diesel::joinable!(channel_pins -> messages (message_id));
diesel::joinable!(channels -> servers (server_id));
diesel::joinable!(group_members -> groups (group_id));
diesel::joinable!(groups -> servers (server_id));
diesel::joinable!(messages -> channels (channel_id));
diesel::joinable!(reactions -> messages (message_id));
diesel::joinable!(roles -> servers (server_id));
diesel::joinable!(server_members -> roles (role_id));
diesel::joinable!(server_members -> servers (server_id));

diesel::allow_tables_to_appear_in_same_query!(
    channel_members,
    channel_pins,
    channels,
    group_members,
    groups,
    messages,
    notifications,
    reactions,
    roles,
    server_members,
    servers,
    users,
);
