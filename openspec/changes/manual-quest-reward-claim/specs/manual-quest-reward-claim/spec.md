## ADDED Requirements

### Requirement: Quest completion creates claimable rewards

系统 MUST 在 story / daily 任务完成后保留可领取状态，而不是立即发放任务奖励。可领取状态 MUST 由现有任务进度字段表达：`completed_at IS NOT NULL` 且 `rewarded = false`。

#### Scenario: Story quest becomes claimable

- **GIVEN** 玩家触发满足某个 story 任务条件的游戏事件
- **WHEN** 服务端处理该事件并更新任务进度
- **THEN** 对应 `user_quest_progress.completed_at` MUST 被设置
- **AND** 对应 `user_quest_progress.rewarded` MUST 保持 `false`
- **AND** 系统 MUST NOT 因完成该 story 任务立即增加背包奖励

#### Scenario: Daily quest becomes claimable for today

- **GIVEN** 玩家触发满足某个 daily 任务条件的游戏事件
- **WHEN** 服务端处理该事件并更新当天任务进度
- **THEN** 当天对应 `user_quest_progress.completed_at` MUST 被设置
- **AND** 当天对应 `user_quest_progress.rewarded` MUST 保持 `false`
- **AND** 系统 MUST NOT 因完成该 daily 任务立即增加背包奖励

### Requirement: Client can claim completed quest reward

系统 MUST 提供 REST 接口让前端领取已完成且未领取的 story / daily 任务奖励。领奖成功后，系统 MUST 返回领取结果并使后续任务查询显示已领取。

#### Scenario: Claim story quest reward

- **GIVEN** 玩家已登录，且某个 story 任务 `completed_at IS NOT NULL`、`rewarded = false`
- **WHEN** 前端调用 `POST /api/quests/story/{quest_id}/claim`
- **THEN** 服务端 MUST 在同一事务内发放该任务配置的奖励
- **AND** 服务端 MUST 将该任务进度更新为 `rewarded = true`
- **AND** 响应 MUST 包含 `quest_id`、`reward_item_id` 和 `reward_quantity`

#### Scenario: Claim daily quest reward

- **GIVEN** 玩家已登录，且当天某个 daily 任务 `completed_at IS NOT NULL`、`rewarded = false`
- **WHEN** 前端调用 `POST /api/quests/daily/{quest_id}/claim`
- **THEN** 服务端 MUST 只领取当天该 daily 任务的奖励
- **AND** 服务端 MUST 将当天任务进度更新为 `rewarded = true`
- **AND** 响应 MUST 包含 `quest_id`、`reward_item_id` 和 `reward_quantity`

#### Scenario: Claim quest without reward item

- **GIVEN** 某个已完成任务没有配置 `reward_item_id` 或 `reward_quantity <= 0`
- **WHEN** 前端调用对应 claim 接口
- **THEN** 服务端 MUST 将该任务进度更新为 `rewarded = true`
- **AND** 服务端 MUST NOT 增加背包资产
- **AND** 响应 MUST 表示无奖励物品

### Requirement: Claim reward is idempotent and atomic

系统 MUST 保证任务领奖在业务操作层面原子、一致且幂等。重复点击、网络重试或并发请求 MUST NOT 导致重复发奖。

#### Scenario: Repeated claim does not duplicate reward

- **GIVEN** 玩家已经成功领取某个任务奖励
- **WHEN** 前端再次调用相同任务的 claim 接口
- **THEN** 服务端 MUST NOT 再次增加背包资产
- **AND** 服务端 MUST NOT 再次写入等价资产流水
- **AND** 服务端 SHOULD 返回 `409 Conflict`

#### Scenario: Claim failure leaves no partial reward

- **GIVEN** 玩家领取任务奖励时数据库写入失败
- **WHEN** 服务端返回失败
- **THEN** 系统 MUST NOT 留下“背包已增加但 `rewarded=false`”或“`rewarded=true` 但奖励未到账”的状态
- **AND** 后续重试 MUST 能重新尝试领取

#### Scenario: Claim writes audit trail

- **GIVEN** 玩家成功领取带奖励物品的任务
- **WHEN** 领奖事务提交
- **THEN** 系统 MUST 写入 `player_operation_idempotency`
- **AND** 系统 MUST 写入 `asset_ledger`
- **AND** 幂等键 MUST 对同一用户、任务类型、任务 id 和 daily 日期保持稳定

### Requirement: Quest read APIs expose claim state

系统 MUST 通过现有任务查询接口暴露任务领取状态，且不破坏原有字段。

#### Scenario: Query shows claimable quest

- **GIVEN** 玩家有已完成但未领取的任务
- **WHEN** 前端调用 `GET /api/quests/story` 或 `GET /api/quests/daily`
- **THEN** 响应中的该任务 MUST 包含非空 `completed_at`
- **AND** 响应中的该任务 MUST 包含 `rewarded = false`

#### Scenario: Query shows claimed quest

- **GIVEN** 玩家已经成功领取某个任务奖励
- **WHEN** 前端调用对应任务查询接口
- **THEN** 响应中的该任务 MUST 包含 `rewarded = true`

### Requirement: WebSocket protocol remains unchanged

任务手动领奖 MUST NOT 修改现有 WebSocket 对战消息协议。对局完成仍通过现有 `GAME_OVER` 等消息通知客户端，任务奖励领取由 REST 完成。

#### Scenario: Game over message remains compatible

- **GIVEN** 两名玩家完成一局对战
- **WHEN** 服务端处理对局结束和任务进度更新
- **THEN** 客户端收到的 WebSocket `GAME_OVER` 消息语义 MUST 保持不变
- **AND** 客户端 MUST 通过 REST 查询任务状态并调用 claim 接口领取任务奖励
