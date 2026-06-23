-- ── users ────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS users (
    id              VARCHAR(12)  PRIMARY KEY,
    username        VARCHAR(50)  UNIQUE NOT NULL,
    -- hex-encoded Keccak-256 hash; empty string for passwordless platform users
    password_hash   TEXT         NOT NULL DEFAULT '',
    email           VARCHAR(255),
    skill_rating    INTEGER      NOT NULL DEFAULT 1000,
    -- originating platform: 'app', 'wechat', 'douyin'
    source_platform VARCHAR(20)  NOT NULL DEFAULT 'app',
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    deleted_at      TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_users_username_lower ON users (LOWER(username));

-- ── auth_identities ───────────────────────────────────────────────────────────
-- Stores per-platform credentials (WeChat openid, Douyin open_id, …)
CREATE TABLE IF NOT EXISTS auth_identities (
    id               BIGSERIAL    PRIMARY KEY,
    user_id          VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- 'wechat' | 'douyin'
    provider         VARCHAR(20)  NOT NULL,
    -- openid returned by the platform
    provider_user_id TEXT         NOT NULL,
    -- WeChat unionid when bound to Open Platform (optional)
    union_id         TEXT,
    -- platform session_key for token refresh (refreshed on every login)
    session_key      TEXT,
    extra            JSONB,
    created_at       TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE (provider, provider_user_id)
);

CREATE INDEX IF NOT EXISTS idx_auth_identities_user ON auth_identities (user_id);

-- ── sessions ──────────────────────────────────────────────────────────────────
-- Replaces the in-document session_tokens array from the old MongoDB schema.
CREATE TABLE IF NOT EXISTS sessions (
    token       VARCHAR(100) PRIMARY KEY,
    user_id     VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions (user_id);

-- ── friendships ───────────────────────────────────────────────────────────────
-- user_id_1 is always LEAST(from, to) so the pair (1,2) is canonical.
-- requester_id records who originally sent the request.
-- status: 'pending' | 'friends'
CREATE TABLE IF NOT EXISTS friendships (
    id             BIGSERIAL    PRIMARY KEY,
    user_id_1      VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_id_2      VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    requester_id   VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status         VARCHAR(20)  NOT NULL DEFAULT 'pending',
    chat_thread_id VARCHAR(20),
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE (user_id_1, user_id_2),
    -- Enforces normalised pair: the smaller ID always goes in user_id_1.
    -- See src/database/friendships.rs normalize() for the Rust-side enforcement.
    CHECK  (user_id_1 < user_id_2)
);

CREATE INDEX IF NOT EXISTS idx_friendships_uid1 ON friendships (user_id_1);
CREATE INDEX IF NOT EXISTS idx_friendships_uid2 ON friendships (user_id_2);

-- ── games ─────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS games (
    id         BIGSERIAL    PRIMARY KEY,
    winner_id  VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    loser_id   VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    played_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_games_winner ON games (winner_id);
CREATE INDEX IF NOT EXISTS idx_games_loser  ON games (loser_id);

-- ── chat_messages ─────────────────────────────────────────────────────────────
-- id (BIGSERIAL) is globally monotonic; within a thread ordering by id is safe.
-- from_id is NULL for system messages.
CREATE TABLE IF NOT EXISTS chat_messages (
    id          BIGSERIAL    PRIMARY KEY,
    thread_id   VARCHAR(30)  NOT NULL,
    from_id     VARCHAR(12)  REFERENCES users(id) ON DELETE SET NULL,
    content     TEXT         NOT NULL,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_chat_messages_thread ON chat_messages (thread_id, id DESC);
