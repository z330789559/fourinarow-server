# Tasks: minigame-leaderboard-service

## 1. 数据库

- [x] 1.1 新增 `migrations/011_minigame_leaderboard.sql`：建 `minigame_level_score` 表 + `idx_mls_board`（不改旧 migration）。
- [x] 1.2 新建 `src/database/minigame_leaderboard.rs`：
  - `submit_score(...)` → upsert（GREATEST），RETURNING 当关最佳。
  - `get_user_rank(game_key, user_id)` → `Option<MinigameMyRank>`（含 totals）。
  - `get_top(game_key, limit, offset)` → `Vec<MinigameBoardEntry>`。
- [x] 1.3 `src/database/mod.rs`：`pub mod minigame_leaderboard;` + `DatabaseManager` 增字段并在 `new` 初始化（用同一 `PgPool`）。

## 2. API

- [x] 2.1 新建 `src/api/minigame_leaderboard.rs`：`config(cfg)` 注册三路由。
- [x] 2.2 `POST /{game_key}/score`：取 session→user；校验/钳制 `level_id>0,score>=0,stars∈[0,3]`；submit→回读 totals+rank→200。
- [x] 2.3 `GET /{game_key}/leaderboard`：`page>=1`，limit=50，offset=(page-1)*50；返回数组。
- [x] 2.4 `GET /{game_key}/leaderboard/me`：取 session→user→get_user_rank；无成绩 404。
- [x] 2.5 `game_key` 校验（本地 `validate_game_key`，与 minigame-config 同约定）。
- [x] 2.6 `src/api/mod.rs`：`pub mod minigame_leaderboard;` + 挂载 `/minigame` scope。

## 3. 验证（手动）

- [x] 3.1 `cargo build` 通过；migrate 011 已应用（minigame_level_score 已建）。
- [x] 3.2 匿名 session POST 多关；L1 补提 s80 未覆盖 best 100/3（GREATEST 生效）。
- [x] 3.3 GET leaderboard：dev-B(200) rank1 / dev-A(150) rank2，按 total_score 降序连续。
- [x] 3.4 GET leaderboard/me：dev-A rank2/total150/stars5；无成绩 dev-C → 404。
- [x] 3.5 缺 session → 401；CORS 预检 `/score` → 200（允许 SessionToken 头、preview 源）。

## 4. 收尾

- [x] 4.1 验证记录见本文件「验收记录」。
- [x] 4.2 OpenSpec 归档。

## 验收记录 (2026-06-28)

curl + 客户端预览双验证（migration 011 已应用）：
- 每关最佳：L1 补提低分 s80 未覆盖 best 100/3（GREATEST 生效）；总分=每关最佳之和。
- 排行榜：按 total_score 降序，rank 从 1 连续；并列以 total_stars/first_at 决胜。
- `/me`：本人 rank/total 正确；无成绩 → 404；缺 SessionToken 提交 → 401。
- CORS 预检 `/score` → 200（允许 SessionToken 头与 preview 源 localhost:7456）。
- 客户端预览实测：真实玩家打一关 → `POST /score` 200 → `/leaderboard/me` 200（英歌侠2946 上榜 #1，总分1600）。
