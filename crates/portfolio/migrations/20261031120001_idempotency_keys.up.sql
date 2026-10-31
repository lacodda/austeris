-- The name a client gave a trade or a snapshot when it brought it, so bringing
-- the same thing again is the same record: a file read a second time, an
-- import that stopped half way and was started over, a request retried after
-- its answer was lost.
--
-- The ledger keeps an entry a module posts under such a key; this is the same
-- idea for what a person's client writes here. Unique per person, trimmed and
-- never empty; nothing for what was recorded without one. Set once, when the
-- record is made: saying what a trade is now does not rename it.
ALTER TABLE trades
    ADD COLUMN idempotency_key text CHECK (idempotency_key IS NULL OR (idempotency_key = btrim(idempotency_key) AND length(idempotency_key) BETWEEN 1 AND 200));

CREATE UNIQUE INDEX trades_owner_idempotency_key ON trades (owner_id, idempotency_key) WHERE idempotency_key IS NOT NULL;

ALTER TABLE snapshots
    ADD COLUMN idempotency_key text CHECK (idempotency_key IS NULL OR (idempotency_key = btrim(idempotency_key) AND length(idempotency_key) BETWEEN 1 AND 200));

CREATE UNIQUE INDEX snapshots_owner_idempotency_key ON snapshots (owner_id, idempotency_key) WHERE idempotency_key IS NOT NULL;
