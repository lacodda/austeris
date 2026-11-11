-- The previous schema counts every account first in, first out, keeps no cost
-- on a snapshot, and books a fee inside the holding. Rolling back forgets how
-- accounts were set to count, and what snapshots said a position cost; the
-- books keep the fees where they were filed, and a trade changed by the
-- previous build is booked its way again.
DROP INDEX trades_booked_before_idx;
ALTER TABLE trades DROP COLUMN booked_as;
ALTER TABLE snapshot_lines DROP COLUMN cost;
DROP TABLE cost_methods;
DROP TYPE cost_method;
