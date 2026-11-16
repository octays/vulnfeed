-- Per-chat @ mention list (comma-separated open_ids): the confirmation card
-- and reminder cards mention these people when delivered to this chat.
ALTER TABLE feishu_chat ADD COLUMN mention_open_ids TEXT NOT NULL DEFAULT '';
