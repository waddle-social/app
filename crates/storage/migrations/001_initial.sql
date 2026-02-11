CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    from_jid TEXT NOT NULL,
    to_jid TEXT NOT NULL,
    body TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    message_type TEXT NOT NULL,
    thread TEXT,
    read INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_messages_from ON messages(from_jid);
CREATE INDEX idx_messages_to ON messages(to_jid);
CREATE INDEX idx_messages_timestamp ON messages(timestamp);

CREATE TABLE roster (
    jid TEXT PRIMARY KEY,
    name TEXT,
    subscription TEXT NOT NULL,
    groups TEXT
);

CREATE TABLE muc_rooms (
    room_jid TEXT PRIMARY KEY,
    nick TEXT NOT NULL,
    joined INTEGER NOT NULL DEFAULT 0,
    subject TEXT
);

CREATE TABLE plugin_kv (
    plugin_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value BLOB NOT NULL,
    PRIMARY KEY (plugin_id, key)
);
