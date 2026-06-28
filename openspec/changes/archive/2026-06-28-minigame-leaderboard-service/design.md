# Design: minigame-leaderboard-service

## 数据模型（migration 011_minigame_leaderboard.sql）

```sql
CREATE TABLE minigame_level_score (
    user_id    TEXT        NOT NULL,
    game_key   TEXT        NOT NULL,
    level_id   INTEGER     NOT NULL,
    best_score INTEGER     NOT NULL DEFAULT 0,
    best_stars SMALLINT    NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, game_key, level_id)
);
CREATE INDEX idx_mls_board ON minigame_level_score (game_key, user_id);
```

- 排名分 = `SUM(best_score)`（每关只留最佳，重复刷简单关不累加）。
- 不加旧 migration 改动；新增文件即可（sqlx migrate）。

## API 契约（scope `/api/minigame`）

### POST /api/minigame/{game_key}/score
**Auth**: SessionToken required
**Body**: `{ "level_id": int>0, "score": int>=0, "stars": int 0..3 }`
**行为**: 钳制 `score>=0`、`stars∈[0,3]`、`level_id>0`（非法 400）；
`INSERT ... ON CONFLICT (user_id,game_key,level_id) DO UPDATE SET best_score=GREATEST(best_score,EXCLUDED.best_score), best_stars=GREATEST(best_stars,EXCLUDED.best_stars), updated_at=now()`。
**Response 200**:
```json
{ "level_id": 3, "best_score": 1200, "best_stars": 3,
  "total_score": 5400, "total_stars": 9, "rank": 12 }
```

### GET /api/minigame/{game_key}/leaderboard?page=1
**Auth**: 无
**行为**: 每页 50，`page>=1`；Top 100 即前 2 页。
聚合：
```sql
SELECT ROW_NUMBER() OVER (ORDER BY s.total_score DESC, s.total_stars DESC, s.first_at ASC) AS rank,
       s.user_id, u.username, s.total_score, s.total_stars
FROM (
  SELECT user_id, SUM(best_score) total_score, SUM(best_stars) total_stars, MIN(updated_at) first_at
  FROM minigame_level_score WHERE game_key=$1 GROUP BY user_id
) s JOIN users u ON u.id = s.user_id AND u.deleted_at IS NULL
ORDER BY rank LIMIT 50 OFFSET $2;
```
**Response 200**: `[{ rank, user_id, username, total_score, total_stars }]`

### GET /api/minigame/{game_key}/leaderboard/me
**Auth**: SessionToken required
**行为**: 同聚合套一层取本人行；无成绩 404。
**Response 200**: `{ rank, username, total_score, total_stars }`

## 实现分层

- `database::minigame_leaderboard`：`struct MinigameLeaderboardCollection { pool }`，方法 `submit_score`、`get_top`、`get_user_rank`、`get_user_totals`。沿用既有 `database::leaderboard` 的写法风格（`sqlx::query_as`）。
- `api::minigame_leaderboard`：`config(cfg)` 注册三路由；`game_key` 校验沿用 minigame-config 的 `validate_game_key`（抽公用或复制一份小函数）；鉴权用既有 `get_session_token` + `db.users.get_session_token`。
- `database::mod.rs`：`DatabaseManager` 增 `pub minigame_leaderboard: MinigameLeaderboardCollection` 字段并在 `new` 初始化。
- `api::mod.rs`：`pub mod minigame_leaderboard;` + `.service(web::scope("/minigame").configure(minigame_leaderboard::config))`。

## 兼容 / 安全 / 性能 / 回滚

- 兼容：纯新增表+端点，RPG `/api/leaderboard` 不动。
- 安全：提交需 session；分数客户端可伪造，本期只做范围钳制（与匿名身份的低保障一致）。
- 性能：实时聚合，量级小可接受；`idx_mls_board` 支撑按 game_key 聚合。后续可加缓存表。
- 回滚：移除 `/minigame` scope；表可保留（无害）或单独 drop。

## 验证

`cargo build` + migrate 后对 `127.0.0.1:40146`，先用匿名 session（或任意 session）：
1. POST score 多关多次（含同关更低分，验证 GREATEST 不回退）。
2. GET leaderboard 看排序与 rank 连续。
3. GET leaderboard/me 看本人 total 与 rank；无成绩用户 404。
