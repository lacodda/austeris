CREATE TABLE portfolio_snapshots (
    id SERIAL PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    assets JSONB NOT NULL
);
