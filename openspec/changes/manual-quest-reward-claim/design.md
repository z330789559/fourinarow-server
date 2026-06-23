## Context

当前 `quests` / `achievement_tiers` 已配置 `reward_item_id` 与 `reward_quantity`，`user_quest_progress` 已有 `completed_at` 和 `rewarded` 字段。现有实现会在对局结算触发 `GameEvent` 时直接发放 story / daily 任务奖励，并把 `rewarded` 置为 `true`；前端只能看到“已奖励”，无法展示“可领取”按钮。

`player-cache-management` 已将对局结算、任务进度、奖励、资产流水和幂等键收口到 `PlayerRepository`。本 change 应继续沿用该边界，不恢复 `QuestCollection::on_event` 作为生产写入口。

## Goals / Non-Goals

**Goals:**

- story / daily 任务完成后保留 `rewarded=false`，让前端能展示“可领取”状态。
- 新增任务领奖 REST 接口，由前端点击触发 claim。
- claim 在单个 PostgreSQL transaction 内完成状态校验、幂等记录、资产发放、资产流水和 `rewarded=true`。
- 重复点击、重试请求或并发 claim 不得重复发放奖励。
- 保持现有任务查询响应字段兼容。

**Non-Goals:**

- 不改 WebSocket 消息协议。
- 不改成就奖励为手动领取；成就缺少 pending tier 状态，另开 change 处理。
- 不新增任务配置后台。
- 不新增 migration，除非实现时发现现有 `rewarded` 字段无法表达领取状态。

## Decisions

### Decision 1: Reuse `rewarded` as claimed state

任务进度已有 `completed_at` 与 `rewarded`：

- `completed_at IS NULL`：未完成。
- `completed_at IS NOT NULL AND rewarded = false`：已完成、可领取。
- `completed_at IS NOT NULL AND rewarded = true`：已领取。

这样不需要新增 schema，也保持现有查询字段兼容。备选方案是新增 `claimed_at`，能表达领取时间，但需要 migration；本 change 优先用现有字段交付前端领取能力。

### Decision 2: Add explicit claim endpoints

新增 REST：

- `POST /api/quests/story/{quest_id}/claim`
- `POST /api/quests/daily/{quest_id}/claim`

请求使用现有 `SessionToken` 鉴权。成功响应返回本次领取的奖励：

```json
{
  "quest_id": "daily_play_1",
  "reward_item_id": "coin",
  "reward_quantity": 20
}
```

无奖励任务也可 claim，返回 `reward_item_id=null`、`reward_quantity=0` 并将 `rewarded=true`，避免前端一直显示可领取。

错误映射：

- `401`：未登录。
- `404`：任务不存在或玩家没有对应已生成进度。
- `400`：任务尚未完成。
- `409`：任务已经领取。
- `500`：数据库、缓存或未知错误。

### Decision 3: Claim logic belongs to `PlayerRepository`

新增 `PlayerRepository::claim_quest_reward(user_id, kind, quest_id)`，而不是在 `src/api/quests.rs` 里拼 SQL。理由：

- claim 会改玩家资产、任务状态、资产流水和幂等记录，属于玩家域写操作。
- 操作前保留 `flush_player(user_id)`，避免覆盖已有 dirty。
- 提交后 reload 玩家 aggregate 覆盖 cache，确保 `/api/quests/*` 和 `/api/inventory` 读取到新状态。

### Decision 4: Completion no longer auto-grants story / daily rewards

`apply_quest_event_in_tx` 仍负责推进 story / daily 进度，但完成时只设置 `completed_at`，保持 `rewarded=false`。它不再为 story / daily 任务调用 `add_inventory_in_tx` 或 `insert_asset_ledger`。

成就奖励暂时保持原行为：`GameWon` 触发 tier 达成时自动发放。后续若需要成就点击领取，需要为 pending tier 单独建模。

### Decision 5: Idempotency keys remain stable

claim 使用稳定幂等键：

- story：`quest:{user_id}:story:{quest_id}`
- daily：`quest:{user_id}:daily:{quest_id}:{quest_date}`

`business_id` 使用 `quest_claim:story:{quest_id}` 或 `quest_claim:daily:{quest_id}:{quest_date}`。`asset_ledger.source` 继续使用 `quest`，便于沿用既有审计查询。

## Risks / Trade-offs

- **旧客户端不调用 claim 会拿不到任务奖励** → 前端文档和联调清单必须同步，展示可领取按钮并在成功后刷新任务/背包。
- **完成后到 claim 前进程崩溃** → 进度已落库，`rewarded=false` 可恢复，玩家重新进入任务页仍可领取。
- **claim 成功后响应丢失导致重试** → 幂等键和 `rewarded=true` 防止重复发放；重试返回 409 已领取。
- **daily 过期未领取** → 本 change 只支持当天 daily 领取，符合当前 daily 查询只展示当天的行为。
- **旧 `QuestCollection::on_event` 仍有自动发奖代码** → 该方法当前无生产调用者；生产写入口必须继续走 `PlayerRepository`。

## Migration Plan

1. 先部署服务端 claim 接口和“完成不自动发奖”的逻辑。
2. 前端根据 `completed_at` / `rewarded` 展示领取按钮：
   - `completed_at == null`：未完成。
   - `completed_at != null && rewarded == false`：可领取。
   - `completed_at != null && rewarded == true`：已领取。
3. 前端 claim 成功后刷新 `/api/quests/*` 与 `/api/inventory`。
4. 回滚时可恢复自动发奖逻辑，但不得删除已产生的 `asset_ledger` 和 `player_operation_idempotency` 记录。

## Open Questions

- 是否需要补一个“领取全部可领取任务”的批量接口？本 change 先不做。
- daily 任务是否允许领取昨天已完成但未领取的奖励？当前设计不允许，若产品需要可另开 change。
