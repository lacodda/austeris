-- The previous schema has no word for a held payment, a counterparty, a place
-- or a tag. Rolling back over any of them would post what the bank has not, or
-- drop what the person wrote down, so it is refused rather than done quietly.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM entries WHERE status = 'pending') THEN
        RAISE EXCEPTION 'some entries are still held, which the previous schema cannot express; clear or delete them before rolling back';
    END IF;
    IF EXISTS (SELECT 1 FROM counterparties) OR EXISTS (SELECT 1 FROM places) OR EXISTS (SELECT 1 FROM tags) THEN
        RAISE EXCEPTION 'counterparties, places or tags are recorded, and the previous schema would drop them; delete them before rolling back';
    END IF;
END;
$$;

DROP INDEX entries_pending_idx;
DROP INDEX entries_place_idx;
DROP INDEX entries_counterparty_idx;
ALTER TABLE entries
    DROP CONSTRAINT entries_cleared_after_it_happened,
    DROP CONSTRAINT entries_cleared_when_said,
    DROP CONSTRAINT entries_place_fkey,
    DROP CONSTRAINT entries_counterparty_fkey,
    DROP COLUMN cleared_on,
    DROP COLUMN status,
    DROP COLUMN place_id,
    DROP COLUMN counterparty_id;
DROP TYPE entry_status;

DROP TABLE entry_tags;
DROP TABLE tags;
DROP TABLE places;
DROP TABLE counterparties;
DROP FUNCTION counterparty_key(text);
DROP TYPE counterparty_kind;

ALTER TABLE entries DROP CONSTRAINT entries_owner_id_key;
ALTER TABLE categories DROP CONSTRAINT categories_owner_id_key;
