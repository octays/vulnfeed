-- Audit trail removed: only final response state is kept (receipt_event
-- remains purely for callback dedup).
DROP TABLE IF EXISTS receipt_response_audit;
