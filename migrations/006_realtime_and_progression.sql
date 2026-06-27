-- activity_log: unified player action event log, user_id nullable for pre-auth events
CREATE TABLE IF NOT EXISTS activity_log (
    id          BIGSERIAL    PRIMARY KEY,
    user_id     VARCHAR(12)  NULL,
    action_type VARCHAR(50)  NOT NULL,
    detail      JSONB        NULL,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_activity_log_user
    ON activity_log (user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_activity_log_type
    ON activity_log (action_type, created_at);

-- player_mode_progress: per-player per-mode level progress
-- level = highest completed level_id in this mode (0 = none completed)
CREATE TABLE IF NOT EXISTS player_mode_progress (
    user_id    VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    mode       INT          NOT NULL,
    level      INT          NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, mode)
);
