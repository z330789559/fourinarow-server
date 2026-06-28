-- Migration 008: notification badge + inbox

-- ─── 红点系统 ─────────────────────────────────────────────────────────────────
CREATE TABLE user_notification_badges (
    user_id    VARCHAR(12) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    module     VARCHAR(32) NOT NULL,  -- quests | achievements | friends | inbox
    has_new    BOOLEAN     NOT NULL DEFAULT false,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, module)
);

-- ─── 站内信 Inbox ─────────────────────────────────────────────────────────────
CREATE TABLE user_inbox (
    id             BIGSERIAL    PRIMARY KEY,
    user_id        VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    type           VARCHAR(40)  NOT NULL,
    title          VARCHAR(200) NOT NULL,
    body           TEXT         NOT NULL DEFAULT '',
    reward_item_id VARCHAR(50)  REFERENCES items(id),
    reward_qty     INT          NOT NULL DEFAULT 0,
    claimed        BOOLEAN      NOT NULL DEFAULT false,
    read           BOOLEAN      NOT NULL DEFAULT false,
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    expires_at     TIMESTAMPTZ
);

CREATE INDEX idx_user_inbox_user ON user_inbox(user_id, created_at DESC);
