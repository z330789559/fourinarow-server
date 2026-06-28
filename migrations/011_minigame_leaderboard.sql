-- 小游戏排行榜（OpenSpec: minigame-leaderboard-service）
-- 仅新增表，不修改旧 migration。以 (user_id, game_key, level_id) 存每关最佳分/星；
-- 排行榜按 SUM(best_score) 实时聚合，与 RPG /api/leaderboard 完全独立。

CREATE TABLE minigame_level_score (
    user_id    TEXT        NOT NULL,                 -- 复用 users.id（含匿名 anon 用户）
    game_key   TEXT        NOT NULL,                 -- 'yingge'，预留多小游戏
    level_id   INTEGER     NOT NULL,
    best_score INTEGER     NOT NULL DEFAULT 0,
    best_stars SMALLINT    NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, game_key, level_id)
);

-- 支撑按 game_key 聚合 + 按 user 命中
CREATE INDEX idx_mls_board ON minigame_level_score (game_key, user_id);
