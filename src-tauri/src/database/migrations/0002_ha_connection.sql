CREATE TABLE ha_connections (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    credential_ref TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 1,
    tls_verify INTEGER NOT NULL DEFAULT 1 CHECK (tls_verify = 1),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
