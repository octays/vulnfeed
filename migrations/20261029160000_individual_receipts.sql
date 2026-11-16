CREATE TABLE receipt_response (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES receipt_task(id),
    app_id TEXT NOT NULL,
    tenant_key TEXT NOT NULL,
    operator_open_id TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK(status IN ('investigating','unaffected','affected')),
    details TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    deleted BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(task_id,app_id,tenant_key,operator_open_id)
);
CREATE TABLE receipt_response_audit (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    response_id INTEGER NOT NULL REFERENCES receipt_response(id),
    action TEXT NOT NULL CHECK(action IN ('submit','update','delete','restore')),
    actor_type TEXT NOT NULL CHECK(actor_type IN ('feishu','admin')),
    actor_id TEXT NOT NULL,
    old_name TEXT NOT NULL DEFAULT '',
    new_name TEXT NOT NULL DEFAULT '',
    old_status TEXT NOT NULL DEFAULT '',
    new_status TEXT NOT NULL DEFAULT '',
    old_details TEXT NOT NULL DEFAULT '',
    new_details TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
-- Preserve the latest known answer from the original single-response model.
INSERT INTO receipt_response(task_id,app_id,tenant_key,operator_open_id,display_name,status,details,updated_at)
SELECT e.task_id,e.app_id,COALESCE(a.tenant_key,''),e.operator_open_id,COALESCE(p.name,''),e.new_status,e.details,e.created_at
FROM receipt_event e
LEFT JOIN feishu_app a ON a.app_id=e.app_id
LEFT JOIN feishu_identity i ON i.app_id=a.id AND i.open_id=e.operator_open_id
LEFT JOIN receipt_person p ON p.id=i.person_id
WHERE e.id=(SELECT MAX(e2.id) FROM receipt_event e2 WHERE e2.task_id=e.task_id AND e2.app_id=e.app_id AND e2.operator_open_id=e.operator_open_id)
AND e.new_status IN ('investigating','unaffected','affected');
-- Refresh already-sent open cards so other people can submit their own answer.
INSERT INTO receipt_card_refresh(task_id)
SELECT t.id FROM receipt_task t JOIN receipt_case c ON c.id=t.case_id
WHERE c.status='open' AND EXISTS(SELECT 1 FROM receipt_delivery d WHERE d.task_id=t.id AND d.message_id IS NOT NULL)
ON CONFLICT(task_id) DO UPDATE SET revision=revision+1,last_error='';
