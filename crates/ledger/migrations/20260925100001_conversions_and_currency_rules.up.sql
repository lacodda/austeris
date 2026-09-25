-- Money that changes currency inside one entry.
--
-- Until now an entry balanced in each currency on its own, but nothing said
-- what the other side of a currency's lines was when the money left in one
-- currency and arrived in another. The only way to write an exchange was to put
-- a dollar line on a guarani account - and a balance is the sum of an account's
-- lines, so the guarani wallet then held -75010 of something.
--
-- The fix is the textbook one for multi-currency double entry: a third kind of
-- side, `conversion`, which is where each currency's half of an exchange lands.
-- 75000 PYG leave the wallet for the conversion, and 10 USD leave the
-- conversion for the dollar account. Every currency still sums to zero on its
-- own, every account line is in its account's currency, and the rate is a fact
-- of the entry: the two conversion lines are the two amounts that changed hands.

-- What a line is attributed to. Stated rather than inferred from which
-- reference is set: "both references empty means conversion" is a rule every
-- reader would have to know, and a line that lost its reference by mistake
-- would read as a conversion instead of failing.
CREATE TYPE line_side AS ENUM ('account', 'category', 'conversion');

-- The balance trigger is off while the column is filled in. Every update would
-- otherwise queue a deferred balance check - of amounts this does not touch -
-- and PostgreSQL refuses to alter a table with checks still queued, so the
-- next statement would fail on any installation that has entries.
ALTER TABLE entry_lines ADD COLUMN side line_side;
ALTER TABLE entry_lines DISABLE TRIGGER entry_lines_balance;
UPDATE entry_lines SET side = CASE WHEN account_id IS NOT NULL THEN 'account'::line_side ELSE 'category'::line_side END;
ALTER TABLE entry_lines ENABLE TRIGGER entry_lines_balance;
ALTER TABLE entry_lines ALTER COLUMN side SET NOT NULL;

-- The side and the references agree: an account line names an account and
-- nothing else, a category line a category, and a conversion line neither.
ALTER TABLE entry_lines DROP CONSTRAINT entry_lines_one_side;
ALTER TABLE entry_lines ADD CONSTRAINT entry_lines_side_matches CHECK (
    (side = 'account') = (account_id IS NOT NULL)
    AND (side = 'category') = (category_id IS NOT NULL)
);

-- An account holds one currency, so a line moving it is in that currency.
--
-- Checked against what is already stored before the rule starts to hold: a line
-- that breaks it cannot be repaired by guessing which of the two currencies was
-- meant, so the migration stops and says so rather than converting anything.
DO $$
DECLARE
    mismatched bigint;
BEGIN
    SELECT count(*) INTO mismatched
      FROM entry_lines l JOIN accounts a ON a.id = l.account_id
     WHERE l.currency <> a.currency;
    IF mismatched > 0 THEN
        RAISE EXCEPTION '% line(s) move an account in a currency other than its own; fix or delete those entries, then migrate again', mismatched;
    END IF;
END;
$$;

CREATE FUNCTION line_in_account_currency() RETURNS trigger AS $$
DECLARE
    held text;
BEGIN
    SELECT currency INTO held FROM accounts WHERE id = NEW.account_id;
    IF held IS DISTINCT FROM NEW.currency THEN
        RAISE EXCEPTION 'a line on account % is in %, but the account is in %; money in another currency reaches it through a conversion',
            NEW.account_id, NEW.currency, held
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER entry_lines_account_currency
    BEFORE INSERT OR UPDATE OF account_id, currency ON entry_lines
    FOR EACH ROW WHEN (NEW.account_id IS NOT NULL)
    EXECUTE FUNCTION line_in_account_currency();

-- The other half of the same rule: an account's currency does not change under
-- the lines already written in it.
CREATE FUNCTION account_currency_is_fixed() RETURNS trigger AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM entry_lines WHERE account_id = NEW.id) THEN
        RAISE EXCEPTION 'account % has entries in %; its currency cannot change', NEW.id, OLD.currency
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER accounts_currency_fixed
    BEFORE UPDATE OF currency ON accounts
    FOR EACH ROW WHEN (NEW.currency IS DISTINCT FROM OLD.currency)
    EXECUTE FUNCTION account_currency_is_fixed();

-- The balance rule, now also the conversion rule.
--
-- A conversion is money turning from one currency into another, so its lines
-- have to say that: in every currency they touch they add up to something other
-- than zero, at least one currency goes in and at least one comes out. Without
-- this, a conversion line of +100 PYG against an account line of -100 PYG
-- balances and makes money vanish into a side nobody reports on.
CREATE OR REPLACE FUNCTION entry_is_balanced() RETURNS trigger AS $$
DECLARE
    subject uuid := COALESCE(NEW.entry_id, OLD.entry_id);
    offending record;
    conversion record;
BEGIN
    SELECT l.currency, SUM(l.amount) AS total
      INTO offending
      FROM entry_lines l
     WHERE l.entry_id = subject
     GROUP BY l.currency
    HAVING SUM(l.amount) <> 0
     LIMIT 1;

    IF FOUND THEN
        RAISE EXCEPTION 'entry % does not balance in %: the lines sum to %',
            subject, offending.currency, offending.total
            USING ERRCODE = 'check_violation';
    END IF;

    SELECT count(*) AS currencies,
           count(*) FILTER (WHERE total > 0) AS arriving,
           count(*) FILTER (WHERE total < 0) AS leaving,
           count(*) FILTER (WHERE total = 0) AS cancelled
      INTO conversion
      FROM (SELECT l.currency, SUM(l.amount) AS total
              FROM entry_lines l
             WHERE l.entry_id = subject AND l.side = 'conversion'
             GROUP BY l.currency) AS per_currency;

    IF conversion.currencies > 0
       AND (conversion.cancelled > 0 OR conversion.arriving = 0 OR conversion.leaving = 0) THEN
        RAISE EXCEPTION 'entry % has a conversion that does not convert: its conversion lines must take one currency in and give another out',
            subject
            USING ERRCODE = 'check_violation';
    END IF;

    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Categories the ledger itself files money under.
--
-- An exchange that cost more than the day's rate says has a third line: what
-- the bank or the exchange office kept. That line needs a category, and asking
-- a person to create one before their first exchange is a form in the way of a
-- sentence. The ledger creates it on first use and finds it again by purpose,
-- not by name - the person may rename or move it, and it stays the one.
CREATE TYPE category_purpose AS ENUM ('exchange_fees');

ALTER TABLE categories ADD COLUMN purpose category_purpose;
CREATE UNIQUE INDEX categories_owner_purpose_key ON categories (owner_id, purpose) WHERE purpose IS NOT NULL;
