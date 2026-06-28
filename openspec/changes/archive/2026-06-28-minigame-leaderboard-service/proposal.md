## Why

英歌小游戏需要一套按"累计总分"排名的排行榜：玩家每局结束上报 `{level_id, score, stars}`，服务端取每关历史最佳分求和作为排名分，玩家可拉取 Top 榜与自己的名次。

现有 `/api/leaderboard` 是 RPG 四子棋的 `skill_rating/wins` 榜，语义、表结构都不匹配，**不可复用**。需要一套以 `game_key` 维度、独立存储的小游戏排行榜，与配置下发 `/api/minigame-config/{game_key}` 共用 game_key 约定。

## What Changes

- 新增 migration `011_minigame_leaderboard.sql`：表 `minigame_level_score`（每用户每关最佳分/星）。
- 新增 `database::minigame_leaderboard` 模块：upsert 每关最佳、聚合 Top 榜、查我的名次。
- 新增 `api::minigame_leaderboard` 模块并挂载 `/api/minigame` scope：
  - `POST /api/minigame/{game_key}/score`（SessionToken）：上报并 upsert 每关最佳，返回最新总分与名次。
  - `GET /api/minigame/{game_key}/leaderboard?page=`：Top 榜（分页）。
  - `GET /api/minigame/{game_key}/leaderboard/me`（SessionToken）：我的名次与总分。

## Capabilities

### New Capabilities

- `minigame-leaderboard`: 以 game_key 维度的小游戏排行榜——每关最佳分存储、累计总分（SUM(best_score)）排名、分数上报与名次查询。

## Out of Scope

- 周榜/赛季榜/分段榜（只做全服 all-time）。
- 排名缓存表/物化视图（先实时聚合 SQL，量大再优化）。
- 反作弊/分数签名校验（仅做基础范围钳制）。
- 与 RPG `/api/leaderboard` 的任何合并或改动。
- 客户端接入（属客户端 change `minigame-leaderboard-ui`）。
