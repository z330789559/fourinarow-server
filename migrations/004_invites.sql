CREATE TABLE IF NOT EXISTS invite_codes (
    code            VARCHAR(20)  PRIMARY KEY,
    creator_id      VARCHAR(12)  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    max_uses        INTEGER      NOT NULL DEFAULT 1,
    uses            INTEGER      NOT NULL DEFAULT 0,
    reward_item_id  VARCHAR(50)  REFERENCES items(id),
    reward_quantity INTEGER      NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    expires_at      TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_invite_codes_creator ON invite_codes (creator_id);

CREATE TABLE IF NOT EXISTS invite_code_uses (
    id       BIGSERIAL   PRIMARY KEY,
    code     VARCHAR(20) NOT NULL REFERENCES invite_codes(code) ON DELETE CASCADE,
    used_by  VARCHAR(12) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    used_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (code, used_by)
);
