# minigame-leaderboard Specification Delta

## ADDED Requirements

### Requirement: 每关最佳分存储

系统 SHALL 以 `(user_id, game_key, level_id)` 为唯一键存储每个玩家每关的最佳得分与最佳星数，重复上报只取更高值。

#### Scenario: 首次上报某关

- **WHEN** 玩家以 SessionToken 调 `POST /api/minigame/{game_key}/score`，body `{level_id, score, stars}` 合法且该关无记录
- **THEN** 系统 MUST 写入 `best_score=score, best_stars=stars`

#### Scenario: 更高分覆盖

- **WHEN** 同一玩家同一关再次上报，`score` 高于已存最佳
- **THEN** 系统 MUST 更新 `best_score` 为新值

#### Scenario: 更低分不回退

- **WHEN** 同一玩家同一关再次上报，`score` 低于或等于已存最佳
- **THEN** 系统 MUST 保留原 `best_score`（取 GREATEST），`best_stars` 同理

#### Scenario: 非法上报

- **WHEN** `level_id<=0`，或 `score<0`，或 `stars` 不在 0..3，或 `game_key` 非法
- **THEN** 系统 MUST 返回 400 且不写库
- **AND** 缺少 SessionToken 时 MUST 返回 401

### Requirement: 累计总分排行榜

系统 SHALL 提供按累计总分（各关最佳之和）降序的全服排行榜，可分页拉取。

#### Scenario: 拉取 Top 榜

- **WHEN** 任意客户端 GET `/api/minigame/{game_key}/leaderboard?page=1`
- **THEN** 系统 MUST 返回该 game_key 下玩家列表，每条含 `{rank, user_id, username, total_score, total_stars}`
- **AND** MUST 按 `total_score` 降序，并以 `total_stars` 降序、最早达成时间升序决胜
- **AND** `rank` MUST 从 1 连续递增，每页 50 条

#### Scenario: 排除已删除用户

- **WHEN** 某 user 在 `users.deleted_at` 非空
- **THEN** 该用户 MUST NOT 出现在榜单

### Requirement: 查询我的名次

系统 SHALL 提供当前玩家自己的名次与总分查询。

#### Scenario: 有成绩

- **WHEN** 玩家以 SessionToken GET `/api/minigame/{game_key}/leaderboard/me`
- **THEN** 系统 MUST 返回 `{rank, username, total_score, total_stars}`，rank 与 Top 榜口径一致

#### Scenario: 无成绩

- **WHEN** 玩家尚无任何关卡成绩
- **THEN** 系统 MUST 返回 404

### Requirement: 提交即返回最新名次

上报分数后系统 SHALL 在同一响应内返回该玩家最新的累计总分与名次，便于客户端结算页即时展示。

#### Scenario: 上报回包含名次

- **WHEN** `POST /api/minigame/{game_key}/score` 成功写库
- **THEN** 响应 MUST 含 `{level_id, best_score, best_stars, total_score, total_stars, rank}`
