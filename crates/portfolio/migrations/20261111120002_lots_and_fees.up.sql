-- Lots and what they cost, and fees as money spent.
--
-- Every purchase - and every reward received - is a lot: so many units, at a
-- moment, for so much. A sale takes units out of the lots of its account, and
-- which ones it takes is how a country's tax rules count a gain: the oldest
-- first, the newest first, or a share of each at their average cost. The lots
-- are worked out from the trades whenever they are asked for, as positions
-- are, and never stored beside them (ADR 0017); what is kept is how each
-- account counts.
CREATE TYPE cost_method AS ENUM ('fifo', 'lifo', 'average');

-- How an account's sales and transfers out pick the lots they take. An account
-- with no row counts first in, first out.
CREATE TABLE cost_methods (
    owner_id   uuid        NOT NULL,
    account_id uuid        NOT NULL,
    method     cost_method NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_id, account_id)
);

-- A snapshot keeps what each position cost as well as what it was worth, in
-- the snapshot's currency: the gain it showed then reads the same after a
-- trade of its time is corrected. Nothing for snapshots taken before, and for
-- money.
ALTER TABLE snapshot_lines ADD COLUMN cost numeric(38, 18);

-- Which way a trade's money is booked. Until now a fee was part of what a
-- coin cost, and its money went into the holding with the price. Now the fee
-- is spending - what a broker or an exchange took - and goes to a category the
-- portfolio keeps for it in the books, while the holding is the price alone.
-- What the books say about a trade recorded before is restated by the
-- portfolio once it is running (1 is the old way, 2 the fee apart), and every
-- trade written from now on is booked the new way.
ALTER TABLE trades ADD COLUMN booked_as smallint NOT NULL DEFAULT 1 CHECK (booked_as IN (1, 2));
ALTER TABLE trades ALTER COLUMN booked_as SET DEFAULT 2;
CREATE INDEX trades_booked_before_idx ON trades (owner_id, id) WHERE booked_as < 2;
