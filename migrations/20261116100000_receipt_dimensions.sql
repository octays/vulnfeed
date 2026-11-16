-- Dimension-based receipts (安全漏洞处理流程 SOP v24): each case is confirmed
-- by three fixed dimensions (P 产线 / NovoOne 产线 / 运维). A dimension submits
-- one of five receipt options plus owners picked from the Feishu directory;
-- a case closes only after every enabled dimension has completed its receipt.
CREATE TABLE receipt_dimension(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  slug TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  owner_name TEXT NOT NULL DEFAULT '',
  owner_open_id TEXT NOT NULL DEFAULT '',
  enabled BOOLEAN NOT NULL DEFAULT TRUE,
  sort INTEGER NOT NULL DEFAULT 0
);
INSERT INTO receipt_dimension(slug,name,owner_name,sort) VALUES
 ('p','P 产线','乔虎虎',1),
 ('novo','NovoOne 产线','钟佳添',2),
 ('ops','运维','郑荣超',3);
-- Rebuild receipt_response: the status CHECK gains the five receipt options,
-- the per-operator UNIQUE makes room for one row per dimension, and the new
-- dimension/owners columns land inline (SQLite cannot ALTER constraints).
CREATE TABLE receipt_response_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES receipt_task(id),
    app_id TEXT NOT NULL,
    tenant_key TEXT NOT NULL,
    operator_open_id TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK(status IN ('none','ops','iter','both','special','investigating','unaffected','affected')),
    details TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    deleted BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    team TEXT NOT NULL DEFAULT '',
    dimension TEXT NOT NULL DEFAULT '',
    owners TEXT NOT NULL DEFAULT ''
);
INSERT INTO receipt_response_new(id,task_id,app_id,tenant_key,operator_open_id,display_name,status,details,version,deleted,updated_at,team,dimension,owners)
SELECT id,task_id,app_id,tenant_key,operator_open_id,display_name,status,details,version,deleted,updated_at,team,'','' FROM receipt_response;
DROP TABLE receipt_response;
ALTER TABLE receipt_response_new RENAME TO receipt_response;
CREATE INDEX idx_receipt_response_task_dimension ON receipt_response(task_id, dimension);
-- Reminder escalation target (@-ed from the 3rd reminder on).
ALTER TABLE sync_task ADD COLUMN remind_escalate_open_id TEXT NOT NULL DEFAULT '';
