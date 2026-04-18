-- Fix notifications FK: was referencing server_members(user_id) which is not
-- a unique column. Should reference users(id) instead.
CREATE TABLE notifications_new (
    id          TEXT PRIMARY KEY NOT NULL,
    user_id     TEXT NOT NULL,
    sender_id   TEXT,

    kind            TEXT NOT NULL,
    reference_id    TEXT,

    is_read     BOOLEAN DEFAULT 0,
    created_at  TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),

    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (sender_id) REFERENCES users(id) ON DELETE SET NULL
);

INSERT INTO notifications_new SELECT * FROM notifications;
DROP TABLE notifications;
ALTER TABLE notifications_new RENAME TO notifications;

CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications (user_id, is_read, created_at DESC);
