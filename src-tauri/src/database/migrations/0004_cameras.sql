CREATE TABLE camera_gateway (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    base_url TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE camera_profiles (
    entity_id TEXT PRIMARY KEY NOT NULL,
    source_type TEXT NOT NULL CHECK (source_type IN ('snapshot','go2rtc')),
    stream_name TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
