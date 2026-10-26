-- 多推送渠道:钉钉/企微/飞书等 IM 机器人,可配置多个实例
CREATE TABLE IF NOT EXISTS push_channel (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL DEFAULT '',
    channel_type TEXT NOT NULL DEFAULT 'dingtalk',
    access_token TEXT NOT NULL,
    secret_token TEXT NOT NULL DEFAULT '',
    status BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
-- 迁移存量钉钉机器人配置
INSERT INTO push_channel (name, channel_type, access_token, secret_token, status)
SELECT '钉钉机器人', 'dingtalk', access_token, secret_token, status FROM ding_bot_config;
