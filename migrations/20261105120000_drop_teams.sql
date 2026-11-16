-- Teams are removed: every case has exactly one task and delivers to all
-- enabled chats. Name mapping (person/identity) is unrelated and stays.
-- receipt_task carries UNIQUE(id, team_id)/UNIQUE(case_id, team_id), so
-- DROP COLUMN cannot work; rebuild the table instead.
CREATE TABLE receipt_task_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    case_id INTEGER NOT NULL REFERENCES receipt_case(id),
    status TEXT NOT NULL DEFAULT 'pending',
    version INTEGER NOT NULL DEFAULT 0,
    last_reminded_at INTEGER NOT NULL DEFAULT 0,
    remind_count INTEGER NOT NULL DEFAULT 0
);
INSERT INTO receipt_task_new(id,case_id,status,version,last_reminded_at,remind_count)
SELECT id,case_id,status,version,last_reminded_at,remind_count FROM receipt_task;
DROP TABLE receipt_task;
ALTER TABLE receipt_task_new RENAME TO receipt_task;
DROP TABLE IF EXISTS receipt_team_group;
DROP TABLE IF EXISTS receipt_person_team;
DROP TABLE IF EXISTS receipt_confirm_grant;
DROP TABLE IF EXISTS receipt_team;
