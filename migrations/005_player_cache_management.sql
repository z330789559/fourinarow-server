CREATE TABLE IF NOT EXISTS player_stats (
    user_id      VARCHAR(12) PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    games_played INTEGER     NOT NULL DEFAULT 0,
    games_won    INTEGER     NOT NULL DEFAULT 0,
    games_lost   INTEGER     NOT NULL DEFAULT 0,
    version      BIGINT      NOT NULL DEFAULT 0,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS player_operation_idempotency (
    idempotency_key VARCHAR(160) PRIMARY KEY,
    user_id         VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    operation_type  VARCHAR(40)  NOT NULL,
    business_id     VARCHAR(160) NOT NULL,
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_player_operation_idempotency_user
    ON player_operation_idempotency (user_id);

CREATE TABLE IF NOT EXISTS asset_ledger (
    id              BIGSERIAL    PRIMARY KEY,
    user_id         VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    item_id         VARCHAR(50)  NOT NULL REFERENCES items(id),
    delta           INTEGER      NOT NULL,
    source          VARCHAR(40)  NOT NULL,
    idempotency_key VARCHAR(160) NOT NULL,
    business_id     VARCHAR(160) NOT NULL,
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE (idempotency_key, item_id, delta)
);

CREATE INDEX IF NOT EXISTS idx_asset_ledger_user
    ON asset_ledger (user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_asset_ledger_idempotency
    ON asset_ledger (idempotency_key);
