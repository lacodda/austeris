-- Before v0.8.0 a conversion had no way to be written down, so an entry that
-- has one cannot be taken back to that schema. Refused rather than deleted: a
-- rollback that quietly drops someone's exchanges is worse than one that stops.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM entry_lines WHERE side = 'conversion') THEN
        RAISE EXCEPTION 'some entries convert between currencies, which the previous schema cannot express; delete them before rolling back';
    END IF;
END;
$$;

DROP INDEX categories_owner_purpose_key;
ALTER TABLE categories DROP COLUMN purpose;
DROP TYPE category_purpose;

CREATE OR REPLACE FUNCTION entry_is_balanced() RETURNS trigger AS $$
DECLARE
    offending record;
BEGIN
    SELECT l.currency, SUM(l.amount) AS total
      INTO offending
      FROM entry_lines l
     WHERE l.entry_id = COALESCE(NEW.entry_id, OLD.entry_id)
     GROUP BY l.currency
    HAVING SUM(l.amount) <> 0
     LIMIT 1;

    IF FOUND THEN
        RAISE EXCEPTION 'entry % does not balance in %: the lines sum to %',
            COALESCE(NEW.entry_id, OLD.entry_id), offending.currency, offending.total
            USING ERRCODE = 'check_violation';
    END IF;

    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER accounts_currency_fixed ON accounts;
DROP FUNCTION account_currency_is_fixed();
DROP TRIGGER entry_lines_account_currency ON entry_lines;
DROP FUNCTION line_in_account_currency();

ALTER TABLE entry_lines DROP CONSTRAINT entry_lines_side_matches;
ALTER TABLE entry_lines ADD CONSTRAINT entry_lines_one_side CHECK ((account_id IS NULL) <> (category_id IS NULL));
ALTER TABLE entry_lines DROP COLUMN side;
DROP TYPE line_side;
