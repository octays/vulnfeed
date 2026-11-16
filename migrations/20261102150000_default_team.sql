-- One implicit team: 团队 comes from each submitter's Feishu department and
-- names are resolved automatically, so admin-managed teams are retired.
INSERT INTO receipt_team(name) VALUES ('全部团队');
-- Every chat is delivered to via the implicit team.
-- (WHERE true disambiguates the upsert after INSERT ... SELECT on SQLite.)
INSERT INTO receipt_team_group(team_id, chat_id)
SELECT (SELECT MAX(id) FROM receipt_team), id FROM feishu_chat WHERE true
ON CONFLICT(team_id, chat_id) DO NOTHING;
-- Single-task open cases move to the implicit team; older multi-team cases
-- keep their tasks as history (close and re-trigger them if still needed).
UPDATE receipt_task SET team_id=(SELECT MAX(id) FROM receipt_team), team_name='全部团队'
WHERE case_id IN (SELECT id FROM receipt_case WHERE status='open')
  AND NOT EXISTS(SELECT 1 FROM receipt_task t2 WHERE t2.case_id=receipt_task.case_id AND t2.id<>receipt_task.id);
UPDATE receipt_team SET enabled=0 WHERE id<>(SELECT MAX(id) FROM receipt_team);
