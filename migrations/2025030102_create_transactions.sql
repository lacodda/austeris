CREATE TABLE transactions (
    id SERIAL PRIMARY KEY,
    asset_id INT REFERENCES assets(id),
    wallet_id INT REFERENCES wallets(id),
    amount FLOAT NOT NULL,
    price FLOAT NOT NULL,
    fee FLOAT,
    type VARCHAR(4) NOT NULL CHECK (type IN ('BUY', 'SELL')),
    notes TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
