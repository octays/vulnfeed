-- New installs start with an empty identity and authorization model.
CREATE TABLE feishu_app (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    app_id TEXT NOT NULL UNIQUE,
    app_secret TEXT NOT NULL,
    tenant_key TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    revision INTEGER NOT NULL DEFAULT 1,
    UNIQUE(id, tenant_key)
);
CREATE TABLE feishu_chat (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    app_id INTEGER NOT NULL REFERENCES feishu_app(id),
    tenant_key TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    name TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE(app_id, tenant_key, chat_id),
    FOREIGN KEY(app_id, tenant_key) REFERENCES feishu_app(id, tenant_key) ON UPDATE CASCADE
);
CREATE TABLE receipt_person (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE
);
CREATE TABLE feishu_identity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    person_id INTEGER NOT NULL REFERENCES receipt_person(id),
    app_id INTEGER NOT NULL REFERENCES feishu_app(id),
    tenant_key TEXT NOT NULL,
    open_id TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE(app_id, tenant_key, open_id),
    UNIQUE(person_id, app_id, tenant_key),
    FOREIGN KEY(app_id, tenant_key) REFERENCES feishu_app(id, tenant_key) ON UPDATE CASCADE
);
CREATE TABLE receipt_person_team (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id INTEGER NOT NULL REFERENCES receipt_team(id),
    person_id INTEGER NOT NULL REFERENCES receipt_person(id),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE(team_id, person_id),
    UNIQUE(id, team_id)
);
CREATE TABLE receipt_team_group (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id INTEGER NOT NULL REFERENCES receipt_team(id),
    chat_id INTEGER NOT NULL REFERENCES feishu_chat(id),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE(team_id, chat_id),
    UNIQUE(id, team_id)
);
CREATE TABLE receipt_confirm_grant (
    member_id INTEGER NOT NULL,
    group_id INTEGER NOT NULL,
    team_id INTEGER NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY(member_id, group_id),
    FOREIGN KEY(member_id, team_id) REFERENCES receipt_person_team(id, team_id),
    FOREIGN KEY(group_id, team_id) REFERENCES receipt_team_group(id, team_id)
);
-- New assignments reference people in a team instead of app-specific identities.
CREATE TABLE receipt_person_assignee (
    task_id INTEGER NOT NULL REFERENCES receipt_task(id),
    member_id INTEGER NOT NULL REFERENCES receipt_person_team(id),
    PRIMARY KEY(task_id, member_id)
);

-- A grant is valid only for a person with an identity in the group's app.
CREATE TRIGGER receipt_confirm_grant_identity_insert
BEFORE INSERT ON receipt_confirm_grant
WHEN NOT EXISTS (
    SELECT 1 FROM receipt_person_team m
    JOIN receipt_team_group g ON g.id=NEW.group_id AND g.team_id=m.team_id
    JOIN feishu_chat c ON c.id=g.chat_id
    JOIN feishu_identity i ON i.person_id=m.person_id AND i.app_id=c.app_id AND i.tenant_key=c.tenant_key
    WHERE m.id=NEW.member_id AND m.team_id=NEW.team_id
)
BEGIN SELECT RAISE(ABORT, 'grant requires an identity for the group application'); END;
CREATE TRIGGER receipt_confirm_grant_identity_update
BEFORE UPDATE OF member_id,group_id,team_id ON receipt_confirm_grant
WHEN NOT EXISTS (
    SELECT 1 FROM receipt_person_team m
    JOIN receipt_team_group g ON g.id=NEW.group_id AND g.team_id=m.team_id
    JOIN feishu_chat c ON c.id=g.chat_id
    JOIN feishu_identity i ON i.person_id=m.person_id AND i.app_id=c.app_id AND i.tenant_key=c.tenant_key
    WHERE m.id=NEW.member_id AND m.team_id=NEW.team_id
)
BEGIN SELECT RAISE(ABORT, 'grant requires an identity for the group application'); END;
