CREATE TABLE receipt_team (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE receipt_case (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_type TEXT NOT NULL CHECK(source_type IN ('vuln', 'notice')),
    source_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open' CHECK(status IN ('open', 'closed', 'cancelled')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    closed_at TEXT
);
CREATE UNIQUE INDEX receipt_active_source ON receipt_case(source_type, source_id) WHERE status = 'open';

CREATE TABLE receipt_task (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    case_id INTEGER NOT NULL REFERENCES receipt_case(id),
    team_id INTEGER NOT NULL REFERENCES receipt_team(id),
    team_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'investigating', 'unaffected', 'affected', 'in_progress', 'awaiting_verification', 'verified')),
    version INTEGER NOT NULL DEFAULT 0,
    UNIQUE(case_id, team_id)
);

CREATE TABLE receipt_event (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES receipt_task(id),
    app_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    operator_open_id TEXT NOT NULL,
    old_status TEXT NOT NULL,
    new_status TEXT NOT NULL,
    details TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(app_id, event_id)
);

CREATE TABLE receipt_delivery (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    case_id INTEGER NOT NULL REFERENCES receipt_case(id),
    task_id INTEGER NOT NULL REFERENCES receipt_task(id),
    app_id TEXT NOT NULL,
    tenant_key TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    message_id TEXT,
    request_id TEXT NOT NULL UNIQUE,
    task_version INTEGER NOT NULL,
    UNIQUE(task_id, app_id, tenant_key, chat_id, task_version),
    UNIQUE(app_id, tenant_key, message_id)
);

CREATE TABLE receipt_admin_event (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    case_id INTEGER NOT NULL REFERENCES receipt_case(id),
    task_id INTEGER REFERENCES receipt_task(id),
    admin_user_id INTEGER NOT NULL REFERENCES admin_user(id),
    old_status TEXT NOT NULL,
    new_status TEXT NOT NULL,
    details TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
