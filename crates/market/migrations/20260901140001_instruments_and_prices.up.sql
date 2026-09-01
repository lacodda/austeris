-- What can be priced, and what it was worth when.
--
-- The 2025 schema is the donor, not the model: it stored prices as `FLOAT` and
-- stamped them with a date. Both are fixed here - amounts are NUMERIC
-- (ADR 0004), and a price carries the instant it was observed, because crypto
-- moves ten percent inside a day and a daily close is not a price history.

-- crypto today; the rest arrive with the services that need them.
CREATE TYPE instrument_kind AS ENUM ('crypto', 'stock', 'bond', 'fund', 'fx', 'manual');

CREATE TABLE instruments (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    kind        instrument_kind NOT NULL,
    -- Ticker as the world writes it: BTC, AAPL, USD. Not unique on its own -
    -- the same letters mean different things in different markets.
    symbol      text        NOT NULL,
    name        text        NOT NULL,
    -- Smallest unit the instrument trades in, when it has one.
    decimals    int,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- One instrument per symbol within a kind: BTC the crypto and a hypothetical
-- BTC the stock are two rows, and neither can be entered twice.
CREATE UNIQUE INDEX instruments_kind_symbol_key ON instruments (kind, upper(symbol));

-- Where prices come from. A row per source per instrument, so an instrument can
-- have several and fall back when one is down (idea 61: CMC went down in 2025
-- and prices simply stopped).
CREATE TABLE instrument_sources (
    instrument_id uuid    NOT NULL REFERENCES instruments (id) ON DELETE CASCADE,
    -- The source's own name for this instrument: a CoinMarketCap numeric id, a
    -- CoinGecko slug, an exchange's ticker. Opaque to everything but the source.
    source        text    NOT NULL,
    external_id   text    NOT NULL,
    -- Lower wins. Two sources at the same priority is an operator's mistake,
    -- not something to resolve silently.
    priority      int     NOT NULL DEFAULT 100,
    PRIMARY KEY (instrument_id, source)
);

CREATE INDEX instrument_sources_source_idx ON instrument_sources (source);

-- Observed prices. `NUMERIC(38, 18)`, never FLOAT: a price of 0.000000000000000001
-- is a real number in this domain, and 1e-18 in binary floating point is not
-- the number anyone typed.
CREATE TABLE prices (
    instrument_id uuid           NOT NULL REFERENCES instruments (id) ON DELETE CASCADE,
    -- What the price is quoted in - always a currency code, never an
    -- instrument: this table answers "how much money", not "how many of that".
    quote_currency text          NOT NULL,
    -- The instant the source observed it, not the instant it was stored.
    observed_at   timestamptz    NOT NULL,
    price         numeric(38, 18) NOT NULL,
    source        text           NOT NULL,
    PRIMARY KEY (instrument_id, quote_currency, observed_at, source)
);

-- The two questions asked of this table: the latest price, and the price at a
-- moment. Both walk backwards from a timestamp, so the index is descending.
CREATE INDEX prices_lookup_idx ON prices (instrument_id, quote_currency, observed_at DESC);
