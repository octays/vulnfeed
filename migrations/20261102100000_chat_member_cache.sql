-- Cache of Feishu group member lists, used only to display response rates and
-- non-respondent names; never used to authorize card submissions.
CREATE TABLE feishu_chat_member (
    chat_id INTEGER NOT NULL REFERENCES feishu_chat(id),
    open_id TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    PRIMARY KEY(chat_id, open_id)
);
ALTER TABLE feishu_chat ADD COLUMN last_member_sync INTEGER NOT NULL DEFAULT 0;
ALTER TABLE feishu_chat ADD COLUMN last_member_error TEXT NOT NULL DEFAULT '';
