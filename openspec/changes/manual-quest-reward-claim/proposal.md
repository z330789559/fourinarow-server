## Why

当前任务系统已经有奖励配置，但任务完成时会在对局结算事务内自动发奖，前端无法展示“可领取”状态，也无法由玩家主动点击领取。小游戏任务体验通常需要完成任务后展示红点/按钮，由玩家点击领取奖励，并确保重复点击或重试不会重复发放资产。

## What Changes

- 将 story / daily 任务奖励语义从“完成即自动发放”改为“完成后进入可领取状态”。
- 新增任务领奖 REST 接口，前端可对已完成且未领取的任务执行 claim。
- 领奖事务内完成奖励发放、`user_quest_progress.rewarded=true`、资产流水和幂等记录，避免任务已领取但奖励未到账，或重复领取重复发奖。
- 任务读取接口继续返回 `completed_at` 和 `rewarded`，前端据此区分未完成、可领取、已领取。
- 保持 WebSocket 对局消息协议不变；对局结算只推进任务进度，不再直接发放 story / daily 任务奖励。
- 成就奖励本 change 暂不改造，仍保持当前 tier 达成时自动发放，避免引入 pending tier 状态。

## Capabilities

### New Capabilities

- `manual-quest-reward-claim`: 定义任务完成后前端点击领取奖励的 REST 合同、状态语义、事务一致性和幂等行为。

### Modified Capabilities

- 无。当前仓库尚无归档主规格；本 change 新增能力约束并在实现中调整现有任务行为。

## Impact

- 影响 API：`src/api/quests.rs` 新增 story / daily 任务领奖路由；现有查询路由字段保持兼容。
- 影响玩家域：`src/player/repository.rs` 中任务事件处理不再自动发 story / daily 奖励，新增 claim 方法处理发奖。
- 影响数据：复用现有 `quests.reward_item_id`、`quests.reward_quantity`、`user_quest_progress.rewarded`、`asset_ledger`、`player_operation_idempotency`；预计不需要新增 migration。
- 影响前端：前端需要在 `completed_at != null && rewarded == false` 时展示领取按钮，并在 claim 成功后刷新任务和背包。
- 兼容风险：旧客户端若只依赖自动发奖，将不再自动收到任务奖励；这是本 change 的预期行为，需要前端同步接入 claim。
- 回滚策略：可恢复自动发奖逻辑并保留 claim 接口为 no-op 或隐藏；不得删除已产生的资产流水和幂等记录。
