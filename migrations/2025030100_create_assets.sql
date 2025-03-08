CREATE TABLE assets (
    id SERIAL PRIMARY KEY,
    symbol VARCHAR(10) NOT NULL,
    name VARCHAR(50) NOT NULL,
    cmc_id INT UNIQUE NOT NULL,
    decimals INT,
    rank INT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_assets_rank ON assets (rank);