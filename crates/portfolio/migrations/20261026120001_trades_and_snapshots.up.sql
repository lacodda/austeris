-- What a person bought, sold and moved, and pictures of what it was worth.
--
-- The 2025 schema is the donor, not the model: it kept quantities and prices as
-- FLOAT, a trade's time as a timestamp without a zone, a wallet as a row of its
-- own, and a snapshot as a JSON list of symbols. Here amounts are NUMERIC
-- (ADR 0004), a trade happens at an instant, the place it happens is one of the
-- person's accounts in the ledger, and a snapshot is rows that can be compared.
--
-- The money a trade moves lives in the ledger, as an entry this module keeps
-- (ADR 0015); the trade is the truth it is derived from. The coins and shares
-- themselves live here: a position is what the trades add up to, worked out
-- whenever it is asked for and never stored beside them.

-- What a trade is.
--
-- `buy`: money on the account became units of the instrument. `sell`: units
-- became money. `transfer`: units left one account for another, and whatever
-- the network took on the way.
CREATE TYPE trade_kind AS ENUM ('buy', 'sell', 'transfer');

CREATE TABLE trades (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id      uuid           NOT NULL,
    kind          trade_kind     NOT NULL,
    -- The account it happened on, in the ledger: where a buy or a sell was
    -- made, and where a transfer left. A broker's or an exchange's, whose
    -- money - dollars, USDT - is the account's balance there.
    account_id    uuid           NOT NULL,
    -- Where a transfer arrived; nothing else has one.
    to_account_id uuid,
    -- What changed hands, in the market's catalogue.
    instrument_id uuid           NOT NULL,
    -- How many units: bought, sold, or sent.
    quantity      numeric(38, 18) NOT NULL CHECK (quantity > 0),
    -- What one unit cost, in the account's currency - a buy's and a sell's.
    price         numeric(38, 18) CHECK (price > 0),
    -- The account's currency when the trade was made: what its price and its
    -- fee are in, and what its money moved in.
    currency      text CHECK (currency ~ '^[A-Z]{3,5}$'),
    -- What it cost to make: in `currency` for a buy or a sell; in units of the
    -- instrument for a transfer, which is what a network takes.
    fee           numeric(38, 18) NOT NULL DEFAULT 0 CHECK (fee >= 0),
    -- The instant it happened, as the exchange's history has it.
    occurred_at   timestamptz    NOT NULL,
    -- The day it happened where the person was: the day its money is in the
    -- books under. Within a day of the instant, whatever the time zone.
    occurred_on   date           NOT NULL,
    note          text           NOT NULL DEFAULT '',
    -- The ledger's entry for its money: a buy's or a sell's. Set in the same
    -- transaction the trade is written in, once the ledger has it.
    entry_id      uuid,
    created_at    timestamptz    NOT NULL DEFAULT now(),
    updated_at    timestamptz    NOT NULL DEFAULT now(),
    -- A buy and a sell have a price and the currency it is in; a transfer has
    -- neither, and has where it went.
    CONSTRAINT trades_priced_unless_moved CHECK (
        (kind = 'transfer') = (price IS NULL)
        AND (kind = 'transfer') = (currency IS NULL)
        AND (kind = 'transfer') = (to_account_id IS NOT NULL)
    ),
    CONSTRAINT trades_transfer_goes_elsewhere CHECK (to_account_id IS DISTINCT FROM account_id),
    -- Something arrives: a network does not take everything that was sent...
    CONSTRAINT trades_transfer_arrives CHECK (kind <> 'transfer' OR fee < quantity),
    -- ...and a sale brings in more than it cost to make.
    CONSTRAINT trades_sale_brings_money CHECK (kind <> 'sell' OR quantity * price > fee),
    CONSTRAINT trades_day_is_the_instants CHECK (
        occurred_on BETWEEN (occurred_at AT TIME ZONE 'UTC')::date - 1 AND (occurred_at AT TIME ZONE 'UTC')::date + 1
    ),
    CONSTRAINT trades_money_only_when_some_moved CHECK (kind <> 'transfer' OR entry_id IS NULL)
);

CREATE INDEX trades_owner_occurred_idx ON trades (owner_id, occurred_at DESC, id);
CREATE INDEX trades_position_idx ON trades (owner_id, account_id, instrument_id, occurred_at);
CREATE INDEX trades_arrivals_idx ON trades (owner_id, to_account_id, instrument_id, occurred_at) WHERE to_account_id IS NOT NULL;

-- A buy or a sell has its money in the books by the time it is written: the
-- ledger's entry is asked for inside the transaction that writes the trade,
-- and this is where one that never got it fails.
CREATE FUNCTION trade_has_its_money() RETURNS trigger AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM trades WHERE id = NEW.id AND kind <> 'transfer' AND entry_id IS NULL) THEN
        RAISE EXCEPTION 'trade % moved money, and the books have no entry for it', NEW.id
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE CONSTRAINT TRIGGER trades_have_their_money
    AFTER INSERT OR UPDATE ON trades
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION trade_has_its_money();

-- Nobody sells, or sends, what they did not have at the time.
--
-- A position is what its trades add up to, in the order they happened; at the
-- same instant what arrives is counted before what leaves. Checked for every
-- position a write touches - a buy deleted can leave a later sale with nothing
-- to sell - and as the statement ends, not at commit: the module writes a
-- trade before it asks the ledger to book its money, and a trade that cannot
-- be is refused before anything is booked.
--
-- Positions are written one at a time: two sales racing each other would each
-- see the units the other is about to sell. A lock per position, taken before
-- adding it up, makes the second one add up what the first left.
CREATE FUNCTION position_never_below_zero(whose uuid, account uuid, instrument uuid) RETURNS void AS $$
DECLARE
    lowest numeric;
    lowest_at timestamptz;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtext('portfolio.position'), hashtext(account::text || instrument::text));
    SELECT running, at INTO lowest, lowest_at
      FROM (SELECT at, sum(delta) OVER (ORDER BY at, arrival DESC, id ROWS UNBOUNDED PRECEDING) AS running
              FROM (SELECT occurred_at AS at, id, CASE WHEN kind = 'buy' THEN quantity ELSE -quantity END AS delta,
                           kind = 'buy' AS arrival
                      FROM trades
                     WHERE owner_id = whose AND account_id = account AND instrument_id = instrument
                    UNION ALL
                    SELECT occurred_at, id, quantity - fee, true
                      FROM trades
                     WHERE owner_id = whose AND to_account_id = account AND instrument_id = instrument) AS moves
           ) AS positions
     ORDER BY running, at
     LIMIT 1;
    IF lowest < 0 THEN
        RAISE EXCEPTION 'that leaves the position below nothing at %: % short', lowest_at, -lowest
            USING ERRCODE = 'AUP01',
                  DETAIL = jsonb_build_object('account_id', account, 'instrument_id', instrument, 'at', lowest_at, 'short', trim_scale(-lowest)::text)::text;
    END IF;
END;
$$ LANGUAGE plpgsql;

CREATE FUNCTION positions_hold() RETURNS trigger AS $$
DECLARE
    touched record;
BEGIN
    -- Each position once, in one order, so two writes lock them alike.
    FOR touched IN
        SELECT DISTINCT whose, account, instrument FROM (
            SELECT OLD.owner_id AS whose, OLD.account_id AS account, OLD.instrument_id AS instrument WHERE TG_OP <> 'INSERT'
            UNION ALL SELECT OLD.owner_id, OLD.to_account_id, OLD.instrument_id WHERE TG_OP <> 'INSERT' AND OLD.to_account_id IS NOT NULL
            UNION ALL SELECT NEW.owner_id, NEW.account_id, NEW.instrument_id WHERE TG_OP <> 'DELETE'
            UNION ALL SELECT NEW.owner_id, NEW.to_account_id, NEW.instrument_id WHERE TG_OP <> 'DELETE' AND NEW.to_account_id IS NOT NULL
        ) AS positions
        ORDER BY account, instrument
    LOOP
        PERFORM position_never_below_zero(touched.whose, touched.account, touched.instrument);
    END LOOP;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Not when only the entry it is booked under is written down: no position
-- moved.
CREATE TRIGGER trades_positions_hold
    AFTER INSERT OR DELETE OR UPDATE OF kind, account_id, to_account_id, instrument_id, quantity, fee, occurred_at ON trades
    FOR EACH ROW EXECUTE FUNCTION positions_hold();

-- What happened to a trade: who made, changed or deleted it, and what it was
-- before - as the ledger remembers an entry (ADR 0013), by the same rules.
CREATE TYPE revision_action AS ENUM ('created', 'changed', 'deleted');
CREATE TYPE revision_actor AS ENUM ('person', 'module', 'outside');

-- A trade as it was, with its amounts as text: a JSON number is a float to
-- most readers (ADR 0004).
CREATE FUNCTION trade_snapshot(subject uuid) RETURNS jsonb AS $$
    SELECT jsonb_build_object(
        'kind', t.kind,
        'account_id', t.account_id,
        'to_account_id', t.to_account_id,
        'instrument_id', t.instrument_id,
        'quantity', trim_scale(t.quantity)::text,
        'price', trim_scale(t.price)::text,
        'currency', t.currency,
        'fee', trim_scale(t.fee)::text,
        'occurred_at', t.occurred_at,
        'occurred_on', t.occurred_on,
        'note', t.note,
        'entry_id', t.entry_id
    )
      FROM trades t
     WHERE t.id = subject
$$ LANGUAGE sql STABLE;

CREATE TABLE trade_revisions (
    -- The order they happened in.
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    owner_id     uuid            NOT NULL,
    -- Not a foreign key: a deleted trade's history is what is left of it.
    trade_id     uuid            NOT NULL,
    action       revision_action NOT NULL,
    actor        revision_actor  NOT NULL,
    actor_person uuid,
    actor_module text,
    at           timestamptz     NOT NULL DEFAULT now(),
    txid         xid8            NOT NULL DEFAULT pg_current_xact_id(),
    before       jsonb,
    CONSTRAINT trade_revisions_once_per_transaction UNIQUE (trade_id, txid),
    CONSTRAINT trade_revisions_person_when_one CHECK ((actor = 'person') = (actor_person IS NOT NULL)),
    CONSTRAINT trade_revisions_module_when_one CHECK (actor_module IS NULL OR actor = 'module'),
    CONSTRAINT trade_revisions_before_when_there_was_one CHECK ((before IS NULL) = (action = 'created'))
);

CREATE INDEX trade_revisions_trade_idx ON trade_revisions (trade_id, id);

-- Records that the current transaction touches a trade, once, before it
-- changes. Who is read from `austeris.actor`; nothing set is a write from
-- outside austeris - unless the connection is austeris's own, which is a path
-- that forgot to say on whose behalf it writes, and is refused.
CREATE FUNCTION trade_touched(subject uuid, act revision_action, owner uuid) RETURNS void AS $$
DECLARE
    said text := current_setting('austeris.actor', true);
    present trade_revisions%ROWTYPE;
    who revision_actor;
    person uuid;
    module text;
BEGIN
    SELECT * INTO present FROM trade_revisions WHERE trade_id = subject AND txid = pg_current_xact_id();
    IF FOUND THEN
        IF act = 'deleted' AND present.action = 'created' THEN
            -- Made and unmade in one transaction: nothing anyone could see.
            DELETE FROM trade_revisions WHERE id = present.id;
        ELSIF act = 'deleted' THEN
            UPDATE trade_revisions SET action = 'deleted' WHERE id = present.id;
        END IF;
        RETURN;
    END IF;

    IF said IS NULL OR said = '' THEN
        IF current_setting('application_name', true) = 'austeris' THEN
            RAISE EXCEPTION 'austeris changed trade % without saying on whose behalf', subject;
        END IF;
        who := 'outside';
    ELSIF said LIKE 'person:%' THEN
        who := 'person';
        person := substr(said, 8)::uuid;
    ELSIF said LIKE 'module:%' THEN
        who := 'module';
        module := NULLIF(substr(said, 8), '');
    ELSE
        RAISE EXCEPTION 'austeris.actor is `%`, which names nobody', said;
    END IF;

    INSERT INTO trade_revisions (owner_id, trade_id, action, actor, actor_person, actor_module, before)
    VALUES (owner, subject, act, who, person, module,
            CASE WHEN act = 'created' THEN NULL ELSE trade_snapshot(subject) END);
END;
$$ LANGUAGE plpgsql;

CREATE FUNCTION trades_remembered() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        PERFORM trade_touched(NEW.id, 'created', NEW.owner_id);
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        PERFORM trade_touched(OLD.id, 'changed', OLD.owner_id);
        RETURN NEW;
    END IF;
    PERFORM trade_touched(OLD.id, 'deleted', OLD.owner_id);
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trades_remembered
    BEFORE INSERT OR UPDATE OR DELETE ON trades
    FOR EACH ROW EXECUTE FUNCTION trades_remembered();

-- A change that changed nothing is not one: a form saved untouched writes
-- the same trade back. Settled at commit, when the transaction's last word on
-- the trade is known.
CREATE FUNCTION trade_revision_says_something() RETURNS trigger AS $$
DECLARE
    kept trade_revisions%ROWTYPE;
BEGIN
    SELECT * INTO kept FROM trade_revisions WHERE id = NEW.id;
    IF FOUND AND kept.action = 'changed' AND kept.before = trade_snapshot(kept.trade_id) THEN
        DELETE FROM trade_revisions WHERE id = kept.id;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE CONSTRAINT TRIGGER trade_revisions_say_something
    AFTER INSERT ON trade_revisions
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW WHEN (NEW.action = 'changed')
    EXECUTE FUNCTION trade_revision_says_something();

-- A picture of what the portfolio held and what it was worth on a day, kept as
-- it was then - what a later edit to a trade or a price would otherwise
-- rewrite. In one currency, the one it was taken in.
--
-- Of a day rather than an instant, as the books are: an entry has a day and
-- no time, so the money beside the positions is what the accounts held at
-- the day's end, and the positions are the trades of that day and before.
CREATE TABLE snapshots (
    id        uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id  uuid        NOT NULL,
    -- The day it shows.
    on_date   date        NOT NULL,
    -- The moment its prices are of: when it was taken, for one of today; the
    -- day's end, for a day past.
    priced_at timestamptz NOT NULL,
    -- What it is valued in.
    currency  text        NOT NULL CHECK (currency ~ '^[A-Z]{3,5}$'),
    note      text        NOT NULL DEFAULT '',
    -- When it was taken, which may be long after its day.
    taken_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX snapshots_owner_on_idx ON snapshots (owner_id, on_date DESC, taken_at DESC);

-- A row of a snapshot: a position, or the money on one of the accounts
-- positions are held on.
CREATE TYPE snapshot_line_kind AS ENUM ('position', 'cash');

CREATE TABLE snapshot_lines (
    snapshot_id   uuid               NOT NULL REFERENCES snapshots (id) ON DELETE CASCADE,
    kind          snapshot_line_kind NOT NULL,
    account_id    uuid               NOT NULL,
    -- The account's name then.
    account_name  text               NOT NULL,
    -- The instrument of a position; nothing for money.
    instrument_id uuid,
    -- The instrument's ticker, or the money's currency.
    symbol        text               NOT NULL,
    -- The instrument's name then; empty for money.
    name          text               NOT NULL DEFAULT '',
    -- Units held, or the amount of money.
    quantity      numeric(38, 18)    NOT NULL,
    -- What a unit was worth in the snapshot's currency - a price, or the rate
    -- of the money's currency into it. Nothing when it was not known.
    price         numeric(38, 18),
    -- When that price was observed, or the day the rate was set for.
    priced_at     timestamptz,
    -- The two multiplied; nothing when the price was not known.
    value         numeric(38, 18),
    -- The price was older than its kind stays current for.
    stale         boolean            NOT NULL DEFAULT false,
    CONSTRAINT snapshot_lines_position_names_its_instrument CHECK ((kind = 'position') = (instrument_id IS NOT NULL)),
    CONSTRAINT snapshot_lines_valued_when_priced CHECK ((price IS NULL) = (value IS NULL)),
    CONSTRAINT snapshot_lines_once UNIQUE NULLS NOT DISTINCT (snapshot_id, kind, account_id, instrument_id, symbol)
);
