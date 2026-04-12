PRAGMA foreign_keys = OFF;

DROP INDEX IF EXISTS idx_reactions_collapse;
DROP INDEX IF EXISTS idx_messages_thread;
DROP INDEX IF EXISTS idx_messages_channel_time;
DROP INDEX IF EXISTS idx_notifications_unread;
DROP INDEX IF EXISTS idx_channels_slug;
DROP INDEX IF EXISTS idx_users_username;
DROP INDEX IF EXISTS idx_users_remote_id;

DROP TABLE IF EXISTS notifications;
DROP TABLE IF EXISTS channel_pins;
DROP TABLE IF EXISTS reactions;
DROP TABLE IF EXISTS messages;
DROP TABLE IF EXISTS channel_members;
DROP TABLE IF EXISTS channels;
DROP TABLE IF EXISTS group_members;
DROP TABLE IF EXISTS groups;
DROP TABLE IF EXISTS server_members;
DROP TABLE IF EXISTS roles;
DROP TABLE IF EXISTS servers;
DROP TABLE IF EXISTS users;

PRAGMA foreign_keys = ON;
