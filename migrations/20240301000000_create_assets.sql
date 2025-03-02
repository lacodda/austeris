CREATE TABLE assets (
    id SERIAL PRIMARY KEY,
    symbol VARCHAR(10) UNIQUE NOT NULL,
    name VARCHAR(50) NOT NULL,
    cmc_id VARCHAR(50) NOT NULL,
    decimals INT DEFAULT 18,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);