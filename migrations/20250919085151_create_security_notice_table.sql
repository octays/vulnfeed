-- Add migration script here
CREATE TABLE IF NOT EXISTS security_notice (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL DEFAULT '',
    product_name TEXT NOT NULL DEFAULT '',
    risk_level TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT '',
    source_name TEXT NOT NULL DEFAULT '',
    is_zero_day BOOLEAN NOT NULL DEFAULT FALSE,
    publish_time TEXT NOT NULL DEFAULT '',
    detail_link TEXT NOT NULL DEFAULT '',
    pushed BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
