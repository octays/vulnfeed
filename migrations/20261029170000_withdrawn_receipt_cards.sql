ALTER TABLE receipt_delivery ADD COLUMN withdrawn BOOLEAN NOT NULL DEFAULT FALSE;
CREATE TABLE receipt_withdrawn_delivery (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    delivery_id INTEGER NOT NULL REFERENCES receipt_delivery(id),
    message_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    retired_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
