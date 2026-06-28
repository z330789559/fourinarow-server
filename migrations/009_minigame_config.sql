-- 小游戏配置版本化下发（OpenSpec: minigame-config-version-service）
-- 仅新增表，不修改任何旧 migration。存储为"版本化 JSON 包"：一版一行，三份配置整包存 JSONB。

-- 配置版本：以 (game_key, channel, version) 唯一标识；历史版本保留不删。
CREATE TABLE minigame_config_version (
    id            BIGSERIAL   PRIMARY KEY,
    game_key      TEXT        NOT NULL,                          -- 'yingge'，预留多小游戏
    channel       TEXT        NOT NULL DEFAULT 'production',      -- 'production' | 'staging'
    version       INTEGER     NOT NULL,                          -- (game_key, channel) 内单调递增
    status        TEXT        NOT NULL DEFAULT 'draft',          -- 'draft' | 'published' | 'archived'
    level_config  JSONB       NOT NULL,                          -- = MinigameYinggeLevelConfigData
    scene_config  JSONB       NOT NULL,                          -- = MinigameYinggeSceneConfigData
    mode_config   JSONB       NOT NULL,                          -- = MinigameYinggeModeConfigData
    checksum      TEXT        NOT NULL,                          -- sha256(规范化三份拼接)，ETag + 完整性
    note          TEXT,
    created_by    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at  TIMESTAMPTZ,
    CONSTRAINT uq_minigame_config_version UNIQUE (game_key, channel, version)
);

CREATE INDEX idx_minigame_config_version_lookup
    ON minigame_config_version (game_key, channel, status);

-- 现役版本指针：每个 (game_key, channel) 唯一一条，指向某个已存在版本。
CREATE TABLE minigame_config_active (
    game_key   TEXT        NOT NULL,
    channel    TEXT        NOT NULL,
    version_id BIGINT      NOT NULL REFERENCES minigame_config_version(id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (game_key, channel)
);
