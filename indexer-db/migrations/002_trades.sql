-- Trades and fills migration

-- Fills/Trades table
CREATE TABLE fills (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    block_height BIGINT NOT NULL REFERENCES blocks(height),
    tx_hash BYTEA NOT NULL,
    market_id SMALLINT NOT NULL REFERENCES markets(id),
    maker BYTEA NOT NULL,
    taker BYTEA NOT NULL,
    maker_order_id BIGINT NOT NULL,
    taker_order_id BIGINT NOT NULL,
    price NUMERIC(38, 18) NOT NULL,
    size NUMERIC(38, 18) NOT NULL,
    side VARCHAR(4) NOT NULL, -- 'buy', 'sell' (taker side)
    maker_fee NUMERIC(38, 18) NOT NULL,
    taker_fee NUMERIC(38, 18) NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_fills_market ON fills(market_id);
CREATE INDEX idx_fills_maker ON fills(maker);
CREATE INDEX idx_fills_taker ON fills(taker);
CREATE INDEX idx_fills_timestamp ON fills(timestamp);
CREATE INDEX idx_fills_block ON fills(block_height);

-- Aggregated trades (for public trade feed)
CREATE TABLE trades (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    market_id SMALLINT NOT NULL REFERENCES markets(id),
    price NUMERIC(38, 18) NOT NULL,
    size NUMERIC(38, 18) NOT NULL,
    side VARCHAR(4) NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_trades_market ON trades(market_id);
CREATE INDEX idx_trades_timestamp ON trades(timestamp);
CREATE INDEX idx_trades_market_time ON trades(market_id, timestamp DESC);

-- Candles (OHLCV)
CREATE TABLE candles (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    market_id SMALLINT NOT NULL REFERENCES markets(id),
    interval VARCHAR(5) NOT NULL, -- '1m', '5m', '15m', '1h', '4h', '1d'
    open_time TIMESTAMPTZ NOT NULL,
    close_time TIMESTAMPTZ NOT NULL,
    open NUMERIC(38, 18) NOT NULL,
    high NUMERIC(38, 18) NOT NULL,
    low NUMERIC(38, 18) NOT NULL,
    close NUMERIC(38, 18) NOT NULL,
    volume NUMERIC(38, 18) NOT NULL DEFAULT 0,
    trade_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(market_id, interval, open_time)
);

CREATE INDEX idx_candles_market_interval ON candles(market_id, interval);
CREATE INDEX idx_candles_time ON candles(open_time);

-- User trade history view
CREATE VIEW user_trades AS
SELECT
    f.id,
    f.market_id,
    m.symbol,
    f.price,
    f.size,
    CASE
        WHEN f.maker = fills_account THEN 'maker'
        ELSE 'taker'
    END as role,
    CASE
        WHEN f.maker = fills_account THEN f.maker_fee
        ELSE f.taker_fee
    END as fee,
    f.timestamp,
    fills_account as account
FROM fills f
JOIN markets m ON f.market_id = m.id
CROSS JOIN LATERAL (
    SELECT f.maker as fills_account
    UNION ALL
    SELECT f.taker as fills_account
) accounts;
