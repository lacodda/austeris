-- The previous schema has no word for the key an import named a record by.
-- Rolling back over one would let that import, run again, bring everything a
-- second time, so it is refused rather than done quietly.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM trades WHERE idempotency_key IS NOT NULL)
       OR EXISTS (SELECT 1 FROM snapshots WHERE idempotency_key IS NOT NULL) THEN
        RAISE EXCEPTION 'trades or snapshots carry idempotency keys, and the previous schema would drop them; delete what was imported before rolling back';
    END IF;
END;
$$;

DROP INDEX snapshots_owner_idempotency_key;
ALTER TABLE snapshots DROP COLUMN idempotency_key;
DROP INDEX trades_owner_idempotency_key;
ALTER TABLE trades DROP COLUMN idempotency_key;
