-- Global switch: may a respondent delete their own submitted receipt from
-- the card? Default off; respondents can always overwrite their answer.
ALTER TABLE sync_task ADD COLUMN allow_response_deletion BOOLEAN NOT NULL DEFAULT FALSE;
