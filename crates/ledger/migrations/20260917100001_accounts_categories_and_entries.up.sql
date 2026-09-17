-- The bookkeeping core: where money sits, what it is spent on, and every
-- movement between the two.
--
-- The shape is double entry, from the first day rather than as a later repair.
-- An entry is a header and its lines, and the lines of an entry sum to zero in
-- each currency they touch. That one rule is what makes a plain expense, a
-- transfer between accounts and a receipt split across three categories the
-- same mechanism instead of three - and it is why no balance is ever stored:
-- an account's balance is the sum of its lines, so there is nothing to repair
-- when the two disagree.

-- Where money sits. The kinds differ in what they mean to a person, not in how
-- they are posted to: a card and a loan are both a balance that lines move.
CREATE TYPE account_kind AS ENUM ('cash', 'bank', 'card', 'brokerage', 'crypto_wallet', 'deposit', 'loan');

CREATE TABLE accounts (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Whose account. Every row in this schema belongs to one person: the
    -- gateway vouches for the caller and the service never reads across.
    owner_id    uuid        NOT NULL,
    kind        account_kind NOT NULL,
    name        text        NOT NULL,
    -- The currency this account is denominated in. An account holds one, and a
    -- second currency means a second account - a single account "in EUR and
    -- USD" has no balance anyone can state.
    currency    text        NOT NULL,
    -- What was in it before austeris knew about it. Stored on the account
    -- rather than posted as an entry with no counterpart: an opening balance is
    -- not a movement, and inventing a category for it would put it in reports
    -- of money that was never earned or spent.
    opening_balance numeric(38, 18) NOT NULL DEFAULT 0,
    -- Closed rather than deleted: entries reference it, and its history is
    -- still true after the account is shut.
    closed_at   timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- One name per person. Two accounts called "Cash" is a mistake at entry time,
-- not something to resolve when reading a report.
CREATE UNIQUE INDEX accounts_owner_name_key ON accounts (owner_id, lower(name));
CREATE INDEX accounts_owner_idx ON accounts (owner_id);

-- What money is earned from or spent on. In bookkeeping terms these are the
-- income and expense accounts: the other side of an entry whose first side is
-- an account, which is why a line references one or the other and never both.
CREATE TYPE category_flow AS ENUM ('income', 'expense');

CREATE TABLE categories (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id    uuid        NOT NULL,
    -- A tree: groceries under food, food under living. `NULL` is a root.
    parent_id   uuid        REFERENCES categories (id) ON DELETE RESTRICT,
    flow        category_flow NOT NULL,
    name        text        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- Siblings are distinct; the same leaf name under two parents is fine, because
-- "food/taxi" and "travel/taxi" are two different things a person spends on.
-- Two partial indexes rather than one: `NULL` parents do not compare equal, so
-- a single index would let a person create "food" at the root twice.
CREATE UNIQUE INDEX categories_root_name_key ON categories (owner_id, lower(name)) WHERE parent_id IS NULL;
CREATE UNIQUE INDEX categories_child_name_key ON categories (owner_id, parent_id, lower(name)) WHERE parent_id IS NOT NULL;
CREATE INDEX categories_owner_idx ON categories (owner_id);

-- One movement, whatever its shape: a purchase, a transfer, a currency
-- exchange, a salary split across two accounts.
CREATE TABLE entries (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id    uuid        NOT NULL,
    -- The day it happened, not the instant it was recorded. A person enters
    -- yesterday's receipt today, and the date the money moved is the one every
    -- report is built on. A date rather than a timestamp: nobody knows the
    -- minute they bought bread, and a fabricated one sorts wrongly.
    occurred_on date        NOT NULL,
    -- What the person would call it: the shop, the payee, "salary".
    description text        NOT NULL DEFAULT '',
    -- Set by a module posting on a person's behalf, so the same salary run
    -- twice does not book twice. Absent for entries a person made by hand.
    idempotency_key text,
    -- Which module produced it, or NULL when a person did. Kept so an entry
    -- can be traced back to what generated it.
    source      text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX entries_owner_date_idx ON entries (owner_id, occurred_on DESC);
-- The idempotency guarantee, as a constraint rather than a check-then-insert:
-- two concurrent posts of the same key race past any lookup, and only a unique
-- index settles it. Partial, so the many hand-made entries with no key do not
-- collide with each other.
CREATE UNIQUE INDEX entries_idempotency_key ON entries (owner_id, idempotency_key) WHERE idempotency_key IS NOT NULL;

-- One side of a movement. Signed: money leaving an account is negative there
-- and positive on the expense category it went to, and the two cancel.
CREATE TABLE entry_lines (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    entry_id    uuid        NOT NULL REFERENCES entries (id) ON DELETE CASCADE,
    -- Exactly one of these. A line is either a movement of an account's balance
    -- or an attribution to a category; something that is both is not a line
    -- anyone can read, and something that is neither is money from nowhere.
    account_id  uuid        REFERENCES accounts (id) ON DELETE RESTRICT,
    category_id uuid        REFERENCES categories (id) ON DELETE RESTRICT,
    -- NUMERIC, never FLOAT (ADR 0004). Eighteen places because a crypto wallet
    -- is an account like any other and its balance is not a rounded one.
    amount      numeric(38, 18) NOT NULL,
    -- The currency this line is in. Lines balance within a currency, not
    -- across: an exchange is one entry in which each currency sums to zero on
    -- its own, so the rate is a fact of the entry rather than a conversion
    -- applied while reading.
    currency    text        NOT NULL,
    -- What this side of the movement was for, when the entry's own description
    -- is not enough: the line of the receipt, the note on the transfer.
    note        text        NOT NULL DEFAULT '',
    CONSTRAINT entry_lines_one_side CHECK ((account_id IS NULL) <> (category_id IS NULL)),
    -- A line of nothing is noise in every report that sums them.
    CONSTRAINT entry_lines_amount_not_zero CHECK (amount <> 0)
);

CREATE INDEX entry_lines_entry_idx ON entry_lines (entry_id);
-- What a balance is read by: every line of an account, in currency order.
CREATE INDEX entry_lines_account_idx ON entry_lines (account_id, currency) WHERE account_id IS NOT NULL;
CREATE INDEX entry_lines_category_idx ON entry_lines (category_id) WHERE category_id IS NOT NULL;

-- The rule that makes this a ledger.
--
-- Deferred, and it has to be: the constraint is about the whole entry, and the
-- first line of a two-line entry is unbalanced by construction. Checked at
-- COMMIT, when the entry is whole.
--
-- In the database rather than only in Rust, because the check must hold for
-- every writer - a future module posting entries, a person in psql during a
-- migration, a repair script. A rule enforced only by the code that usually
-- writes is a rule that lasts until something else writes.
CREATE FUNCTION entry_is_balanced() RETURNS trigger AS $$
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

CREATE CONSTRAINT TRIGGER entry_lines_balance
    AFTER INSERT OR UPDATE OR DELETE ON entry_lines
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION entry_is_balanced();

-- What one currency was worth in another on a day.
--
-- Owned here rather than read from `market` on every conversion: a rate used to
-- value a past entry must not change when a source revises its history, and a
-- report of last March must read the same next year. `market` is where a rate
-- is fetched from; this is where the one that was used is kept.
CREATE TABLE exchange_rates (
    -- Rates are an installation's, not a person's: PYG to USD on a day is the
    -- same fact for everyone using it.
    base_currency  text NOT NULL,
    quote_currency text NOT NULL,
    -- The day the rate applies to.
    on_date        date NOT NULL,
    -- How many of `quote` one `base` buys.
    rate           numeric(38, 18) NOT NULL CHECK (rate > 0),
    -- Where it came from: a source's name, or `manual` when a person typed it.
    source         text NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (base_currency, quote_currency, on_date)
);

-- A rate is looked up as "on or before this day": the answer for a Sunday is
-- Friday's rate, because that is the last thing anyone observed.
CREATE INDEX exchange_rates_lookup_idx ON exchange_rates (base_currency, quote_currency, on_date DESC);
