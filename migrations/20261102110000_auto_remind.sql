-- Automatic reminder cadence per task: nudge open tasks with pending
-- respondents at most once every 24h, at most 3 times per task.
ALTER TABLE receipt_task ADD COLUMN last_reminded_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE receipt_task ADD COLUMN remind_count INTEGER NOT NULL DEFAULT 0;
-- Master switch alongside auto_receipts on the sync task row.
ALTER TABLE sync_task ADD COLUMN auto_remind BOOLEAN NOT NULL DEFAULT FALSE;
