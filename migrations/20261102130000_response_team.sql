-- Respondents declare their own team in the card form instead of the card
-- being stamped with the assigned confirmation team.
ALTER TABLE receipt_response ADD COLUMN team TEXT NOT NULL DEFAULT '';
