-- The persistent department cache is replaced by an on-demand in-memory
-- resolver; drop its tables.
DROP TABLE IF EXISTS feishu_department_member;
DROP TABLE IF EXISTS feishu_department;
DROP TABLE IF EXISTS feishu_department_scan;
