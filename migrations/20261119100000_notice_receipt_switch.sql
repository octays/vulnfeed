-- Separate auto-receipt switch for security notices. Vendor patch notices
-- duplicated what stack-subscription filtering already encodes ("do we run
-- this product") and piled up unanswered -- 62% of open notice cases had
-- zero responses -- so notice auto-receipts now default off; vulnerability
-- auto-receipts keep the existing auto_receipts switch.
ALTER TABLE sync_task ADD COLUMN auto_receipts_notice BOOLEAN NOT NULL DEFAULT FALSE;
