-- Funding and liquidations migration

-- Funding rate history
CREATE TABLE funding_rates (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    market_id SMALLINT NOT NULL REFERENCES markets(id),
    funding_rate NUMERIC(20, 18) NOT NULL,
    mark_price NUMERIC(38, 18) NOT NULL,
    index_price NUMERIC(38, 18) NOT NULL,
    premium_index NUMERIC(20, 18) NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_funding_rates_market ON funding_rates(market_id);
CREATE INDEX idx_funding_rates_timestamp ON funding_rates(timestamp);
CREATE INDEX idx_funding_rates_market_time ON funding_rates(market_id, timestamp DESC);

-- Funding payments (per user)
CREATE TABLE funding_payments (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    account BYTEA NOT NULL,
    market_id SMALLINT NOT NULL REFERENCES markets(id),
    position_size NUMERIC(38, 18) NOT NULL,
    funding_rate NUMERIC(20, 18) NOT NULL,
    payment NUMERIC(38, 18) NOT NULL, -- Positive = paid, negative = received
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_funding_payments_account ON funding_payments(account);
CREATE INDEX idx_funding_payments_market ON funding_payments(market_id);
CREATE INDEX idx_funding_payments_timestamp ON funding_payments(timestamp);

-- Liquidations
CREATE TABLE liquidations (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    block_height BIGINT NOT NULL REFERENCES blocks(height),
    account BYTEA NOT NULL,
    market_id SMALLINT NOT NULL REFERENCES markets(id),
    side VARCHAR(4) NOT NULL, -- original position side
    liquidated_size NUMERIC(38, 18) NOT NULL,
    liquidation_price NUMERIC(38, 18) NOT NULL,
    bankruptcy_price NUMERIC(38, 18),
    pnl NUMERIC(38, 18) NOT NULL,
    fee_to_insurance NUMERIC(38, 18) NOT NULL,
    is_bankruptcy BOOLEAN NOT NULL DEFAULT FALSE,
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_liquidations_account ON liquidations(account);
CREATE INDEX idx_liquidations_market ON liquidations(market_id);
CREATE INDEX idx_liquidations_timestamp ON liquidations(timestamp);

-- ADL events
CREATE TABLE adl_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    block_height BIGINT NOT NULL REFERENCES blocks(height),
    bankrupt_account BYTEA NOT NULL,
    counter_account BYTEA NOT NULL,
    market_id SMALLINT NOT NULL REFERENCES markets(id),
    size NUMERIC(38, 18) NOT NULL,
    price NUMERIC(38, 18) NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_adl_bankrupt ON adl_events(bankrupt_account);
CREATE INDEX idx_adl_counter ON adl_events(counter_account);
CREATE INDEX idx_adl_timestamp ON adl_events(timestamp);

-- Insurance fund history
CREATE TABLE insurance_fund_history (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    balance NUMERIC(38, 18) NOT NULL,
    delta NUMERIC(38, 18) NOT NULL,
    reason VARCHAR(50) NOT NULL, -- 'liquidation_fee', 'bankruptcy_coverage', 'deposit', etc.
    reference_id UUID, -- Link to liquidation or other event
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_insurance_fund_timestamp ON insurance_fund_history(timestamp);

-- Leaderboard materialized view (refresh periodically)
CREATE MATERIALIZED VIEW leaderboard AS
SELECT
    p.account,
    SUM(p.realized_pnl) as total_realized_pnl,
    SUM(
        CASE
            WHEN p.size > 0 THEN p.size * (ms.mark_price - p.entry_notional / p.size)
            WHEN p.size < 0 THEN ABS(p.size) * (p.entry_notional / ABS(p.size) - ms.mark_price)
            ELSE 0
        END
    ) as total_unrealized_pnl,
    COUNT(DISTINCT p.market_id) as markets_traded,
    MAX(p.updated_at) as last_trade_time
FROM positions p
JOIN market_states ms ON p.market_id = ms.market_id
WHERE p.size != 0 OR p.realized_pnl != 0
GROUP BY p.account
ORDER BY (SUM(p.realized_pnl) + SUM(
    CASE
        WHEN p.size > 0 THEN p.size * (ms.mark_price - p.entry_notional / p.size)
        WHEN p.size < 0 THEN ABS(p.size) * (p.entry_notional / ABS(p.size) - ms.mark_price)
        ELSE 0
    END
)) DESC;

CREATE UNIQUE INDEX idx_leaderboard_account ON leaderboard(account);
