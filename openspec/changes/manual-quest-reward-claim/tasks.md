## 1. Baseline And Contract

- [x] 1.1 确认当前任务奖励配置字段、任务查询响应和自动发奖路径，记录无需新增 migration 的依据
- [x] 1.2 定义 claim API 响应结构和错误映射，确认不修改 WebSocket 协议

## 2. Repository Claim Logic

- [x] 2.1 新增 `QuestClaimKind`、`QuestClaimReward`、`QuestClaimError` 等类型，表达 story/daily 领奖结果与错误
- [x] 2.2 修改 story / daily 任务完成逻辑：完成时设置 `completed_at`，但保持 `rewarded=false`，不再自动发任务奖励
- [x] 2.3 实现 `PlayerRepository::claim_quest_reward`，在事务内校验完成状态、幂等发奖、写资产流水并更新 `rewarded=true`
- [x] 2.4 claim 前保留 `flush_player`，提交后 reload 覆盖玩家 cache；失败时不得留下部分发奖状态

## 3. REST Integration

- [x] 3.1 新增 `POST /api/quests/story/{quest_id}/claim`，调用 repository 并返回领取结果
- [x] 3.2 新增 `POST /api/quests/daily/{quest_id}/claim`，只领取当天 daily 任务奖励
- [x] 3.3 将 claim 错误映射为 400/401/404/409/500，并保持现有任务查询接口字段兼容

## 4. Verification And Documentation

- [x] 4.1 增加 focused tests 或 ignored DB tests，覆盖任务完成后可领取、首次领取发奖、重复领取不重复发奖、未完成不可领取
- [x] 4.2 更新前端对接文档，说明 `completed_at` / `rewarded` 状态和 claim 接口
- [x] 4.3 运行 `cargo test player::repository -- --nocapture` 和相关 ignored DB tests
- [x] 4.4 运行 `cargo check`，确认 sqlx 查询与 schema 对齐
- [x] 4.5 运行 `openspec validate "manual-quest-reward-claim" --strict` 和 apply 状态检查
