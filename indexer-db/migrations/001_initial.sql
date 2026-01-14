-- HyperCore Indexer Schema
-- Initial migration: core tables

-- Enable extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Blocks table
CREATE TABLE blocks (
    height BIGINT PRIMARY KEY,
    hash BYTEA NOT NULL UNIQUE,
    timestamp TIMESTAMPTZ NOT NULL,
    proposer BYTEA,
    tx_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_blocks_timestamp ON blocks(timestamp);
CREATE INDEX idx_blocks_hash ON blocks(hash);

-- Transactions table
CREATE TABLE transactions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    hash BYTEA NOT NULL UNIQUE,
    block_height BIGINT NOT NULL REFERENCES blocks(height),
    tx_index INTEGER NOT NULL,
    sender BYTEA NOT NULL,
    tx_type VARCHAR(50) NOT NULL,
    data JSONB NOT NULL,
    status VARCHAR(20) NOT NULL, -- 'success', 'failed'
    error_message TEXT,
    gas_used BIGINT,
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_transactions_block ON transactions(block_height);
CREATE INDEX idx_transactions_sender ON transactions(sender);
CREATE INDEX idx_transactions_type ON transactions(tx_type);
CREATE INDEX idx_transactions_timestamp ON transactions(timestamp);

-- Accounts table
CREATE TABLE accounts (
    address BYTEA PRIMARY KEY,
    balance NUMERIC(38, 0) NOT NULL DEFAULT 0,
    nonce BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Markets table
CREATE TABLE markets (
    id SMALLINT PRIMARY KEY,
    symbol VARCHAR(20) NOT NULL UNIQUE,
    base_asset VARCHAR(10) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'active',
    tick_size NUMERIC(38, 18) NOT NULL,
    lot_size NUMERIC(38, 18) NOT NULL,
    max_leverage SMALLINT NOT NULL,
    maker_fee NUMERIC(10, 8) NOT NULL,
    taker_fee NUMERIC(10, 8) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Market state (latest snapshot)
CREATE TABLE market_states (
    market_id SMALLINT PRIMARY KEY REFERENCES markets(id),
    mark_price NUMERIC(38, 18) NOT NULL,
    index_price NUMERIC(38, 18) NOT NULL,
    funding_rate NUMERIC(20, 18) NOT NULL DEFAULT 0,
    funding_accumulator NUMERIC(38, 18) NOT NULL DEFAULT 0,
    next_funding_time TIMESTAMPTZ NOT NULL,
    open_interest_long NUMERIC(38, 18) NOT NULL DEFAULT 0,
    open_interest_short NUMERIC(38, 18) NOT NULL DEFAULT 0,
    volume_24h NUMERIC(38, 18) NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Positions table
CREATE TABLE positions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    account BYTEA NOT NULL,
    market_id SMALLINT NOT NULL REFERENCES markets(id),
    size NUMERIC(38, 18) NOT NULL,
    entry_notional NUMERIC(38, 18) NOT NULL,
    realized_pnl NUMERIC(38, 18) NOT NULL DEFAULT 0,
    last_funding_index NUMERIC(38, 18) NOT NULL DEFAULT 0,
    leverage SMALLINT NOT NULL DEFAULT 10,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(account, market_id)
);

CREATE INDEX idx_positions_account ON positions(account);
CREATE INDEX idx_positions_market ON positions(market_id);

-- Orders table (open orders)
CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    account BYTEA NOT NULL,
    market_id SMALLINT NOT NULL REFERENCES markets(id),
    side VARCHAR(4) NOT NULL, -- 'buy', 'sell'
    price NUMERIC(38, 18) NOT NULL,
    original_size NUMERIC(38, 18) NOT NULL,
    remaining_size NUMERIC(38, 18) NOT NULL,
    order_type VARCHAR(20) NOT NULL,
    reduce_only BOOLEAN NOT NULL DEFAULT FALSE,
    post_only BOOLEAN NOT NULL DEFAULT FALSE,
    client_order_id VARCHAR(100),
    status VARCHAR(20) NOT NULL, -- 'open', 'partial', 'filled', 'canceled'
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_orders_account ON orders(account);
CREATE INDEX idx_orders_market ON orders(market_id);
CREATE INDEX idx_orders_status ON orders(status);
CREATE INDEX idx_orders_client_id ON orders(account, client_order_id) WHERE client_order_id IS NOT NULL;

-- Insert default markets
INSERT INTO markets (id, symbol, base_asset, tick_size, lot_size, max_leverage, maker_fee, taker_fee)
VALUES
    (0, 'BTC-PERP', 'BTC', 0.1, 0.001, 50, 0.0002, 0.0005),
    (1, 'ETH-PERP', 'ETH', 0.01, 0.01, 50, 0.0002, 0.0005);

-- Insert initial market states
INSERT INTO market_states (market_id, mark_price, index_price, next_funding_time)
VALUES
    (0, 65000, 65000, NOW() + INTERVAL '8 hours'),
    (1, 3500, 3500, NOW() + INTERVAL '8 hours');
