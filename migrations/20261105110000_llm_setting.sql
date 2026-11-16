-- LLM classifier settings, edited from the admin UI (single row).
-- api_key is write-only via the API, never returned in plaintext.
CREATE TABLE llm_setting (
    id INTEGER PRIMARY KEY CHECK(id=1),
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    base_url TEXT NOT NULL DEFAULT '',
    api_key TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT 'deepseek-chat',
    timeout_secs INTEGER NOT NULL DEFAULT 20
);
INSERT INTO llm_setting(id) VALUES(1);
