CREATE TABLE entity_overrides (
    entity_id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT,
    area_id_override TEXT,
    is_favorite INTEGER NOT NULL DEFAULT 0,
    is_hidden INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX entity_overrides_favorite ON entity_overrides(is_favorite);
CREATE INDEX entity_overrides_sort_order ON entity_overrides(sort_order);
