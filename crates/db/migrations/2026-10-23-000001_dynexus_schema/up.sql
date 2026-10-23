-- Raw log of every DYNEXUS event. Its primary key is the exactly-once guard:
-- typed tables are only updated when the raw row is newly inserted.
CREATE TABLE dynexus_events (
    transaction_version BIGINT NOT NULL,
    event_index BIGINT NOT NULL,
    module TEXT NOT NULL,
    name TEXT NOT NULL,
    account TEXT,
    data JSONB NOT NULL,
    ts TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (transaction_version, event_index)
);
CREATE INDEX dynexus_events_kind_idx ON dynexus_events (module, name, transaction_version DESC);
CREATE INDEX dynexus_events_account_idx ON dynexus_events (account, transaction_version DESC);

CREATE TABLE tokens (
    address TEXT PRIMARY KEY,
    creator TEXT NOT NULL,
    name TEXT,
    symbol TEXT,
    decimals SMALLINT,
    icon_uri TEXT,
    project_uri TEXT,
    burned NUMERIC NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    created_version BIGINT NOT NULL
);
CREATE INDEX tokens_creator_idx ON tokens (creator);
CREATE INDEX tokens_symbol_idx ON tokens (lower(symbol));

CREATE TABLE pools (
    address TEXT PRIMARY KEY,
    token_x TEXT NOT NULL,
    token_y TEXT NOT NULL,
    creator TEXT NOT NULL,
    reserve_x NUMERIC NOT NULL,
    reserve_y NUMERIC NOT NULL,
    lp_supply NUMERIC NOT NULL,
    fee_bps INTEGER NOT NULL,
    platform_fee_bps INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    created_version BIGINT NOT NULL,
    last_version BIGINT NOT NULL
);
CREATE INDEX pools_token_x_idx ON pools (token_x);
CREATE INDEX pools_token_y_idx ON pools (token_y);

CREATE TABLE swaps (
    transaction_version BIGINT NOT NULL,
    event_index BIGINT NOT NULL,
    pool TEXT NOT NULL,
    trader TEXT NOT NULL,
    token_in TEXT NOT NULL,
    token_out TEXT NOT NULL,
    amount_in NUMERIC NOT NULL,
    amount_out NUMERIC NOT NULL,
    swap_fee NUMERIC NOT NULL,
    platform_fee NUMERIC NOT NULL,
    reserve_x NUMERIC NOT NULL,
    reserve_y NUMERIC NOT NULL,
    ts TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (transaction_version, event_index)
);
CREATE INDEX swaps_pool_ts_idx ON swaps (pool, ts DESC);
CREATE INDEX swaps_trader_ts_idx ON swaps (trader, ts DESC);

CREATE TABLE liquidity_events (
    transaction_version BIGINT NOT NULL,
    event_index BIGINT NOT NULL,
    pool TEXT NOT NULL,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('add', 'remove')),
    amount_x NUMERIC NOT NULL,
    amount_y NUMERIC NOT NULL,
    lp_amount NUMERIC NOT NULL,
    reserve_x NUMERIC NOT NULL,
    reserve_y NUMERIC NOT NULL,
    ts TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (transaction_version, event_index)
);
CREATE INDEX liquidity_events_pool_ts_idx ON liquidity_events (pool, ts DESC);
CREATE INDEX liquidity_events_provider_idx ON liquidity_events (provider, ts DESC);

CREATE TABLE ve_locks (
    user_address TEXT PRIMARY KEY,
    amount NUMERIC NOT NULL,
    unlock_time BIGINT NOT NULL,
    updated_version BIGINT NOT NULL
);

CREATE TABLE votes (
    user_address TEXT NOT NULL,
    epoch BIGINT NOT NULL,
    pool TEXT NOT NULL,
    vote_weight NUMERIC NOT NULL,
    transaction_version BIGINT NOT NULL,
    PRIMARY KEY (user_address, epoch, pool)
);
CREATE INDEX votes_pool_epoch_idx ON votes (pool, epoch);

CREATE TABLE epochs (
    epoch BIGINT PRIMARY KEY,
    emission NUMERIC NOT NULL,
    total_votes NUMERIC NOT NULL,
    distributed_version BIGINT NOT NULL
);

CREATE TABLE gauges (
    pool TEXT PRIMARY KEY,
    lp_token TEXT NOT NULL,
    total_staked NUMERIC NOT NULL DEFAULT 0,
    -- DYNEXUS per second scaled by 1e18, as emitted on chain
    reward_rate NUMERIC NOT NULL DEFAULT 0,
    period_finish BIGINT NOT NULL DEFAULT 0,
    created_version BIGINT NOT NULL
);

CREATE TABLE gauge_stakes (
    pool TEXT NOT NULL,
    user_address TEXT NOT NULL,
    staked NUMERIC NOT NULL,
    PRIMARY KEY (pool, user_address)
);
CREATE INDEX gauge_stakes_user_idx ON gauge_stakes (user_address);

CREATE TABLE bribes (
    pool TEXT NOT NULL,
    epoch BIGINT NOT NULL,
    token TEXT NOT NULL,
    amount NUMERIC NOT NULL,
    PRIMARY KEY (pool, epoch, token)
);
