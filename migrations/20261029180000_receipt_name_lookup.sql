-- Permit system-generated audit entries without losing the existing audit history.
CREATE TABLE receipt_response_audit_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    response_id INTEGER NOT NULL REFERENCES receipt_response(id),
    action TEXT NOT NULL CHECK(action IN ('submit','update','delete','restore')),
    actor_type TEXT NOT NULL CHECK(actor_type IN ('feishu','admin','system')),
    actor_id TEXT NOT NULL,
    old_name TEXT NOT NULL DEFAULT '', new_name TEXT NOT NULL DEFAULT '',
    old_status TEXT NOT NULL DEFAULT '', new_status TEXT NOT NULL DEFAULT '',
    old_details TEXT NOT NULL DEFAULT '', new_details TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO receipt_response_audit_new SELECT * FROM receipt_response_audit;
DROP TABLE receipt_response_audit;
ALTER TABLE receipt_response_audit_new RENAME TO receipt_response_audit;
CREATE TABLE receipt_name_lookup (
    response_id INTEGER PRIMARY KEY REFERENCES receipt_response(id),
    last_attempt INTEGER NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT ''
);
INSERT INTO receipt_name_lookup(response_id)
SELECT id FROM receipt_response WHERE display_name='' AND deleted=0;
-- Update existing cards to remove the manual name field.
INSERT INTO receipt_card_refresh(task_id)
SELECT t.id FROM receipt_task t JOIN receipt_case c ON c.id=t.case_id
WHERE c.status='open' AND EXISTS(SELECT 1 FROM receipt_delivery d WHERE d.task_id=t.id AND d.message_id IS NOT NULL AND d.withdrawn=0)
ON CONFLICT(task_id) DO UPDATE SET revision=revision+1,last_error='';
