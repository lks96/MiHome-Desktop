CREATE TABLE scene_favorites (
    scene_entity_id TEXT PRIMARY KEY NOT NULL,
    is_favorite INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0
);
