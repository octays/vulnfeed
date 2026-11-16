-- Cache mapping open_id -> department, built from the department tree plus
-- find_by_department (both work without the user-object organization field
-- scope that gates department_ids).
CREATE TABLE feishu_department (
    department_id TEXT PRIMARY KEY,
    name TEXT NOT NULL DEFAULT ''
);
CREATE TABLE feishu_department_member (
    department_id TEXT NOT NULL,
    open_id TEXT NOT NULL,
    PRIMARY KEY(department_id, open_id)
);
CREATE TABLE feishu_department_scan (
    id INTEGER PRIMARY KEY CHECK(id=1),
    scanned_at INTEGER NOT NULL DEFAULT 0,
    lease_until INTEGER NOT NULL DEFAULT 0
);
