-- Before this schema there was nowhere a trade was kept. Rolling back over one
-- would leave its money in the ledger with nothing it came from, so it is
-- refused rather than done quietly.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM trades) OR EXISTS (SELECT 1 FROM snapshots) THEN
        RAISE EXCEPTION 'the portfolio has trades or snapshots, and rolling back would drop them; delete them first';
    END IF;
END;
$$;

DROP TABLE snapshot_lines;
DROP TYPE snapshot_line_kind;
DROP TABLE snapshots;
DROP TRIGGER trades_remembered ON trades;
DROP TABLE trade_revisions;
DROP FUNCTION trade_revision_says_something();
DROP FUNCTION trades_remembered();
DROP FUNCTION trade_touched(uuid, revision_action, uuid);
DROP FUNCTION trade_snapshot(uuid);
DROP TYPE revision_actor;
DROP TYPE revision_action;
DROP TABLE trades;
DROP FUNCTION positions_hold();
DROP FUNCTION position_never_below_zero(uuid, uuid, uuid);
DROP FUNCTION trade_has_its_money();
DROP TYPE trade_kind;
