-- Create asset_prices table in assets schema
CREATE TABLE assets.asset_prices (
    id SERIAL PRIMARY KEY,
    asset_id INT REFERENCES assets.assets(id),
    price_usd FLOAT NOT NULL,
    timestamp TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT unique_asset_timestamp UNIQUE (asset_id, timestamp)
);

CREATE INDEX idx_asset_prices_asset_id_timestamp ON assets.asset_prices (asset_id, timestamp DESC);