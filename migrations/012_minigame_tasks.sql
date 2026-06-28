-- Migration 012: 英歌小游戏任务系统（设计：darkHero docs/superpowers/specs/2026-06-28-yingge-task-system-design.md）
-- 任务定义在服务端代码静态配置（api::minigame_tasks）；本迁移只存用户侧进度/记账。
-- 主线/成就进度由 minigame_level_score 实时派生；每日由 submit_score 顺手计数。表均带 game_key 预留多小游戏。

-- 每日活跃计数（每局结束累加；每日任务读它）。
CREATE TABLE minigame_daily_activity (
    user_id  TEXT NOT NULL,
    game_key TEXT NOT NULL,
    day      DATE NOT NULL,
    plays    INT  NOT NULL DEFAULT 0,
    clears   INT  NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, game_key, day)
);

-- 签到统计（驱动「签到一次/每日签到」任务）；客户端签到成功时上报。
CREATE TABLE minigame_signin_stat (
    user_id   TEXT NOT NULL,
    game_key  TEXT NOT NULL,
    total     INT  NOT NULL DEFAULT 0,
    last_date DATE,
    PRIMARY KEY (user_id, game_key)
);

-- 一次性领取记账：每日任务 day=当天；非每日(主线) day='1970-01-01' 哨兵。
CREATE TABLE minigame_task_claim (
    user_id    TEXT        NOT NULL,
    game_key   TEXT        NOT NULL,
    task_id    TEXT        NOT NULL,
    day        DATE        NOT NULL DEFAULT DATE '1970-01-01',
    claimed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, game_key, task_id, day)
);

-- 渐进成就进度：step = 已领轮数（达成下一轮门槛后手动领取推进）。
CREATE TABLE minigame_task_achievement_progress (
    user_id        TEXT NOT NULL,
    game_key       TEXT NOT NULL,
    achievement_id TEXT NOT NULL,
    step           INT  NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, game_key, achievement_id)
);
