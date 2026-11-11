-- Units that arrived with nothing paid for them: a staking reward, an airdrop,
-- a gift.
--
-- A buy needs a price above nothing and moves money; a transfer needs an
-- account the units left. Neither is what happened, so the 2025 tracker's
-- "buy at 0" rows had nowhere to go, and the sale that followed one was refused
-- as selling more than was held. A `receive` is units arriving on one account
-- and no money moving: no entry in the books. Its price is what a unit was
-- worth when it arrived, in the account's currency - what its lot cost, as a
-- reward is counted where it is income - and may be nothing, for a gift whose
-- worth is not counted; it has no fee.
ALTER TYPE trade_kind ADD VALUE IF NOT EXISTS 'receive';

-- The new value cannot be named as itself in the transaction that adds it, so
-- the rules below compare it as text.
ALTER TABLE trades DROP CONSTRAINT trades_price_check;
ALTER TABLE trades ADD CONSTRAINT trades_price_check CHECK (price > 0 OR (kind::text = 'receive' AND price = 0));
ALTER TABLE trades ADD CONSTRAINT trades_receive_costs_nothing CHECK (kind::text <> 'receive' OR fee = 0);

-- Only a buy and a sale move money, and have an entry for it.
ALTER TABLE trades DROP CONSTRAINT trades_money_only_when_some_moved;
ALTER TABLE trades ADD CONSTRAINT trades_money_only_when_some_moved CHECK (kind IN ('buy', 'sell') OR entry_id IS NULL);

CREATE OR REPLACE FUNCTION trade_has_its_money() RETURNS trigger AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM trades WHERE id = NEW.id AND kind IN ('buy', 'sell') AND entry_id IS NULL) THEN
        RAISE EXCEPTION 'trade % moved money, and the books have no entry for it', NEW.id
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- What arrives - bought or received - counts before what leaves at the same
-- instant, as before.
CREATE OR REPLACE FUNCTION position_never_below_zero(whose uuid, account uuid, instrument uuid) RETURNS void AS $$
DECLARE
    lowest numeric;
    lowest_at timestamptz;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtext('portfolio.position'), hashtext(account::text || instrument::text));
    SELECT running, at INTO lowest, lowest_at
      FROM (SELECT at, sum(delta) OVER (ORDER BY at, arrival DESC, id ROWS UNBOUNDED PRECEDING) AS running
              FROM (SELECT occurred_at AS at, id,
                           CASE WHEN kind::text IN ('buy', 'receive') THEN quantity ELSE -quantity END AS delta,
                           kind::text IN ('buy', 'receive') AS arrival
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
