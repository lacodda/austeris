-- A made-up household's records in the 2025 tracker's schema, for the import
-- of v0.15 to read. Every number here is invented.
--
-- Applied over the tracker's own migrations (`git show legacy-2025:migrations/...`),
-- which seed the wallets Binance and Ledger Nano; `tools/legacy-fixture.sh`
-- dumps the result with pg_dump into legacy-2025.sql beside this file.
--
-- What it holds, and why:
-- - USDT that came into Binance (#1), and USDT recorded again as the money of
--   a Bitcoin buy (#3 for #2) and of a sale (#6 for #5);
-- - an Ether buy whose money was never recorded in USDT (#4), so the books need
--   more money than the tracker saw arrive;
-- - Solana received for nothing (#7) and partly sold (#8): the books cannot say
--   the first, so the second sells what was never bought;
-- - Bitcoin "bought" on a hardware wallet with no money anywhere (#10), and an
--   amount that is a float's sum, 0.1 + 0.2 (#11);
-- - snapshots the way the tracker took them - floats added up - two of them on
--   one evening in Asuncion that is the next morning in UTC;
-- - prices, of a coin nothing traded too (DOGE);
-- - the asset service's unfinished schema, which the import does not read.

-- The sqlx bookkeeping a real database carries.
CREATE TABLE public._sqlx_migrations (
    version bigint PRIMARY KEY,
    description text NOT NULL,
    installed_on timestamptz NOT NULL DEFAULT now(),
    success boolean NOT NULL,
    checksum bytea NOT NULL,
    execution_time bigint NOT NULL
);
INSERT INTO public._sqlx_migrations VALUES
    (2025030100, 'create assets', '2025-03-01 10:00:00+00', true, '\xdeadbeef', 1203456),
    (2025030800, 'create asset prices', '2025-03-08 10:00:00+00', true, '\x0102030405', 803456);

INSERT INTO wallets (name, type, address, created_at) VALUES ('Bybit', 'exchange', NULL, '2025-03-01 10:00:00');

INSERT INTO assets (id, symbol, name, cmc_id, decimals, rank, created_at) VALUES
    (1, 'BTC', 'Bitcoin', 1, 8, 1, '2025-03-01 10:00:00'),
    (2, 'ETH', 'Ethereum', 1027, 18, 2, '2025-03-01 10:00:00'),
    (3, 'USDT', 'Tether USDt', 825, 6, 3, '2025-03-01 10:00:00'),
    (4, 'SOL', 'Solana', 5426, 9, 5, '2025-03-01 10:00:00'),
    (5, 'DOGE', 'Dogecoin', 74, 8, 8, '2025-03-01 10:00:00');
SELECT setval('assets_id_seq', 5);

INSERT INTO transactions (id, asset_id, wallet_id, amount, price, fee, type, notes, created_at) VALUES
    (1, 3, 1, 2000, 1, NULL, 'BUY', 'first deposit', '2025-03-03 13:00:00.512341'),
    (2, 1, 1, 0.02, 85000, 1.7, 'BUY', NULL, '2025-03-05 14:10:00.123456'),
    (3, 3, 1, 1701.7, 1, NULL, 'SELL', NULL, '2025-03-05 14:12:31.000001'),
    (4, 2, 1, 0.5, 2100, 1.05, 'BUY', E'dip buy ☕ café\twith a tab', '2025-03-10 09:00:00'),
    (5, 1, 1, 0.005, 84000, 0.42, 'SELL', NULL, '2025-03-20 16:00:00'),
    (6, 3, 1, 419.58, 1, NULL, 'BUY', NULL, '2025-03-20 16:01:00'),
    (7, 4, 1, 10, 0, NULL, 'BUY', 'staking reward', '2025-04-02 23:30:00'),
    (8, 4, 1, 4, 120, 0, 'SELL', NULL, '2025-04-03 10:00:00'),
    (9, 2, 1, 0.2, 1600, 0.32, 'SELL', NULL, '2025-04-10 12:00:00'),
    (10, 1, 2, 0.01, 86000, NULL, 'BUY', E'cold storage\\vault', '2025-03-06 08:00:00'),
    (11, 2, 2, 0.1::float8 + 0.2::float8, 2000, NULL, 'BUY', NULL, '2025-04-12 15:00:00');
SELECT setval('transactions_id_seq', 11);

-- A snapshot as the tracker took one: what every transaction until then added
-- up to, by ticker, in floats, leaving out what came to nothing.
CREATE FUNCTION pg_temp.held(moment timestamp) RETURNS jsonb AS $$
    SELECT coalesce(jsonb_agg(jsonb_build_object('symbol', symbol, 'amount', amount, 'cmc_id', cmc_id) ORDER BY symbol), '[]'::jsonb)
      FROM (SELECT a.symbol, a.cmc_id, sum(CASE WHEN t.type = 'BUY' THEN t.amount ELSE -t.amount END) AS amount
              FROM transactions t JOIN assets a ON a.id = t.asset_id
             WHERE t.created_at <= moment
             GROUP BY a.symbol, a.cmc_id) AS held
     WHERE amount > 0
$$ LANGUAGE sql;

INSERT INTO portfolio_snapshots (id, created_at, assets) VALUES
    (1, '2025-03-06 20:00:00', pg_temp.held('2025-03-06 20:00:00')),
    (2, '2025-03-21 01:00:00', pg_temp.held('2025-03-21 01:00:00')),
    (3, '2025-03-21 02:30:00', pg_temp.held('2025-03-21 02:30:00')),
    (4, '2025-04-15 12:00:00', pg_temp.held('2025-04-15 12:00:00'));
SELECT setval('portfolio_snapshots_id_seq', 4);

INSERT INTO asset_prices (asset_id, price_usd, timestamp) VALUES
    (1, 85000.12, '2025-03-05 14:00:00'),
    (1, 86500.5, '2025-03-06 19:59:59'),
    (1, 84000, '2025-03-20 15:00:00'),
    (1, 83000.75, '2025-04-15 11:00:00'),
    (2, 2150, '2025-03-06 19:59:59'),
    (2, 1980.25, '2025-03-20 15:00:00'),
    (2, 1590.5, '2025-04-15 11:00:00'),
    (3, 0.9998, '2025-03-06 19:59:59'),
    (3, 1.0001, '2025-03-20 15:00:00'),
    (3, 0.9999, '2025-04-15 11:00:00'),
    (4, 125.5, '2025-04-15 11:00:00'),
    (5, 0.17, '2025-04-15 11:00:00');

-- The asset service the 2025 split had begun, in its own schema.
CREATE SCHEMA assets;
CREATE TABLE assets.assets (id serial PRIMARY KEY, symbol varchar(10) NOT NULL, name varchar(50) NOT NULL, cmc_id int UNIQUE NOT NULL);
INSERT INTO assets.assets (symbol, name, cmc_id) VALUES ('BTC', 'Bitcoin', 1);
