-- The previous schema has no word for units that arrived for nothing. Rolling
-- back over one - or over the history of one - would leave a position that
-- adds up to less than was sold, so it is refused rather than done quietly.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM trades WHERE kind::text = 'receive')
       OR EXISTS (SELECT 1 FROM trade_revisions WHERE before->>'kind' = 'receive') THEN
        RAISE EXCEPTION 'trades receive units for nothing, which the previous schema cannot say; delete them before rolling back';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION position_never_below_zero(whose uuid, account uuid, instrument uuid) RETURNS void AS $$
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

CREATE OR REPLACE FUNCTION trade_has_its_money() RETURNS trigger AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM trades WHERE id = NEW.id AND kind <> 'transfer' AND entry_id IS NULL) THEN
        RAISE EXCEPTION 'trade % moved money, and the books have no entry for it', NEW.id
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

ALTER TABLE trades DROP CONSTRAINT trades_money_only_when_some_moved;
ALTER TABLE trades DROP CONSTRAINT trades_receive_costs_nothing;
ALTER TABLE trades DROP CONSTRAINT trades_price_check;
ALTER TABLE trades ADD CONSTRAINT trades_price_check CHECK (price > 0);

-- An enum keeps a value once added; the type is made again without it. The
-- rules that name its values in literals are written again for the new one.
-- So is the trigger that names the column among those it watches.
ALTER TABLE trades DROP CONSTRAINT trades_priced_unless_moved;
ALTER TABLE trades DROP CONSTRAINT trades_transfer_arrives;
ALTER TABLE trades DROP CONSTRAINT trades_sale_brings_money;
DROP TRIGGER trades_positions_hold ON trades;
ALTER TYPE trade_kind RENAME TO trade_kind_with_receive;
CREATE TYPE trade_kind AS ENUM ('buy', 'sell', 'transfer');
ALTER TABLE trades ALTER COLUMN kind TYPE trade_kind USING kind::text::trade_kind;
DROP TYPE trade_kind_with_receive;
CREATE TRIGGER trades_positions_hold
    AFTER INSERT OR DELETE OR UPDATE OF kind, account_id, to_account_id, instrument_id, quantity, fee, occurred_at ON trades
    FOR EACH ROW EXECUTE FUNCTION positions_hold();
ALTER TABLE trades ADD CONSTRAINT trades_priced_unless_moved CHECK (
    (kind = 'transfer') = (price IS NULL)
    AND (kind = 'transfer') = (currency IS NULL)
    AND (kind = 'transfer') = (to_account_id IS NOT NULL)
);
ALTER TABLE trades ADD CONSTRAINT trades_transfer_arrives CHECK (kind <> 'transfer' OR fee < quantity);
ALTER TABLE trades ADD CONSTRAINT trades_sale_brings_money CHECK (kind <> 'sell' OR quantity * price > fee);
ALTER TABLE trades ADD CONSTRAINT trades_money_only_when_some_moved CHECK (kind <> 'transfer' OR entry_id IS NULL);
