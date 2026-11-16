CREATE TABLE receipt_card_refresh (
    task_id INTEGER PRIMARY KEY REFERENCES receipt_task(id),
    revision INTEGER NOT NULL DEFAULT 1,
    completed_revision INTEGER NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT '',
    last_attempt INTEGER NOT NULL DEFAULT 0
);
