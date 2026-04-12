PRAGMA foreign_keys = ON;

-- 0. USERS (Local Mirror of Remote Auth)
CREATE TABLE users (
    id          TEXT PRIMARY KEY NOT NULL,
    remote_id   TEXT NOT NULL UNIQUE,
    username    TEXT NOT NULL UNIQUE,

    created_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- 1. SERVER
CREATE TABLE servers (
    id          TEXT PRIMARY KEY NOT NULL CHECK(id = 'main'),
    remote_id   TEXT UNIQUE,
    name        TEXT NOT NULL,
    metadata    TEXT NOT NULL DEFAULT '{}',
    created_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- 2. ROLES (RBAC)
CREATE TABLE IF NOT EXISTS roles (
    id          TEXT PRIMARY KEY NOT NULL,
    server_id   TEXT NOT NULL DEFAULT 'main',
    name        TEXT NOT NULL,
    permissions INTEGER NOT NULL,
    priority    INTEGER DEFAULT 0, -- Used for sorting the "Grouped View"

    color       INTEGER DEFAULT 0,
    is_mentionable BOOLEAN DEFAULT 0,

    metadata    TEXT NOT NULL DEFAULT '{}',

    created_by  TEXT NOT NULL,
    created_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (server_id) REFERENCES servers(id) ON DELETE CASCADE
);

-- 3. USER GROUPS (GitHub-style Teams)
CREATE TABLE IF NOT EXISTS groups (
    id          TEXT PRIMARY KEY NOT NULL,
    server_id   TEXT NOT NULL DEFAULT 'main',
    name        TEXT NOT NULL,
    description TEXT,

    is_mentionable BOOLEAN DEFAULT 1,

    created_by  TEXT NOT NULL,
    created_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (server_id) REFERENCES servers(id) ON DELETE CASCADE
);

-- 4. SERVER MEMBERS
CREATE TABLE IF NOT EXISTS server_members (
    server_id   TEXT NOT NULL DEFAULT 'main',
    user_id     TEXT NOT NULL,
    role_id     TEXT,
    nickname    TEXT,
    metadata    TEXT NOT NULL DEFAULT '{}',
    joined_at   TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    PRIMARY KEY (server_id, user_id),
    FOREIGN KEY (server_id) REFERENCES servers(id) ON DELETE CASCADE,
    FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE SET NULL
);

-- 5. GROUP MEMBERSHIPS
CREATE TABLE IF NOT EXISTS group_members (
    group_id    TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    added_by    TEXT NOT NULL,
    added_at    TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    PRIMARY KEY (group_id, user_id),
    FOREIGN KEY (group_id) REFERENCES groups(id) ON DELETE CASCADE
);

-- 6. CHANNELS
CREATE TABLE IF NOT EXISTS channels (
    id          TEXT PRIMARY KEY NOT NULL,
    server_id   TEXT NOT NULL DEFAULT 'main',
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL DEFAULT 'a_channel',
    kind        TEXT NOT NULL,

    is_default  BOOLEAN DEFAULT 0,
    is_private  BOOLEAN DEFAULT 0,
    is_archived BOOLEAN DEFAULT 0,
    is_nsfw     BOOLEAN DEFAULT 0,
    pin_limit   INTEGER DEFAULT 3,

    metadata    TEXT NOT NULL DEFAULT '{}',

    created_by  TEXT NOT NULL,
    created_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (server_id) REFERENCES servers(id) ON DELETE CASCADE,
    CHECK(server_id = 'main')
);

-- 7. CHANNEL MEMBERSHIPS
CREATE TABLE IF NOT EXISTS channel_members (
    channel_id  TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    role_id     TEXT,
    added_by    TEXT,
    settings    TEXT NOT NULL DEFAULT '{}',
    joined_at   TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    PRIMARY KEY (channel_id, user_id),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON DELETE CASCADE,
    FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE SET NULL
);

-- 8. MESSAGES
CREATE TABLE IF NOT EXISTS messages (
    id          TEXT PRIMARY KEY NOT NULL,
    channel_id  TEXT NOT NULL,
    sender_id   TEXT NOT NULL,
    content     TEXT NOT NULL,
    kind        TEXT NOT NULL,

    is_repliable    BOOLEAN DEFAULT 1,
    is_reactable    BOOLEAN DEFAULT 1,
    is_pinned       BOOLEAN DEFAULT 0,

    root_thread_id      TEXT,
    parent_id           TEXT,
    origin_message_id   TEXT,

    deleted_at      TEXT,
    updated_at      TEXT,
    created_at      TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (channel_id) REFERENCES channels(id) ON DELETE CASCADE,
    FOREIGN KEY (root_thread_id) REFERENCES messages(id) ON DELETE SET NULL,
    FOREIGN KEY (parent_id) REFERENCES messages(id) ON DELETE SET NULL
);

-- 9. REACTIONS
CREATE TABLE IF NOT EXISTS reactions (
    message_id  TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    emoji       TEXT NOT NULL,
    created_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    PRIMARY KEY (message_id, emoji, user_id),
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
) WITHOUT ROWID;

-- 10. CHANNEL PINS
CREATE TABLE IF NOT EXISTS channel_pins (
    channel_id  TEXT NOT NULL,
    message_id  TEXT NOT NULL,
    pinned_by   TEXT NOT NULL,
    pinned_at   TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    PRIMARY KEY (channel_id, message_id),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON DELETE CASCADE,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);

-- 11. NOTIFICATIONS (New Table)
CREATE TABLE IF NOT EXISTS notifications (
    id          TEXT PRIMARY KEY NOT NULL,
    user_id     TEXT NOT NULL,       -- Who is this for?
    sender_id   TEXT,                -- Who caused it? (Nullable for system events)

    kind            TEXT NOT NULL,      -- 'mention', 'reply', 'announcement', 'invite'
    reference_id    TEXT,               -- The ID of the message/channel/server involved

    is_read     BOOLEAN DEFAULT 0,
    created_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (user_id) REFERENCES server_members(user_id) ON DELETE CASCADE,
    FOREIGN KEY (sender_id) REFERENCES server_members(user_id) ON DELETE SET NULL
);

-- 99. INDEXES
CREATE INDEX IF NOT EXISTS idx_messages_channel_time ON messages (channel_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages (root_thread_id) WHERE root_thread_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_reactions_collapse ON reactions (message_id, emoji);
CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications (user_id, is_read, created_at DESC);

CREATE UNIQUE INDEX idx_channels_slug ON channels(server_id, slug);
CREATE UNIQUE INDEX idx_users_username ON users(username);
CREATE UNIQUE INDEX idx_users_remote_id ON users(remote_id);
