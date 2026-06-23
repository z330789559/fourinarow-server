## Why

当前项目已经新增背包、任务、排行榜、邀请码等玩家域能力，但玩家数据仍分散在 `users`、`user_inventory`、`user_quest_progress` 等 collection 中直接读写数据库；`users` 里现有的 60 秒 TTL 只解决部分读缓存，不具备写意图、dirty tracking、批量落库或奖励一致性能力。

小游戏服务端后续会持续增加资产、成长、任务、活动和平台化运营能力，需要先把玩家数据更新收口到统一的 `PlayerAggregate` 与 `PlayerRepository`，让业务逻辑不再散落手动写库，并为 write-behind、强制刷盘和事务一致性建立清晰边界。

## What Changes

- 引入统一 `PlayerAggregate` 概念，聚合账号基础信息、对局属性、资产、任务/成就进度、统计计数器等玩家运行态数据。
- 引入 `PlayerRepository` 作为玩家数据唯一业务入口，提供只读访问与可变访问语义，避免任务、奖励、背包、结算等逻辑直接散落调用底层 collection。
- 引入 dirty 分桶机制，按玩家数据域标记变化，例如 profile、game_info、inventory、quests、achievements、stats。
- 引入后台 flush tick，按冷却时间批量写回 dirty 分桶，并提供连续修改情况下的强制刷盘机制。
- 引入下线/连接结束/服务关闭时的强制刷盘路径，降低进程退出或断线导致的数据丢失风险。
- 收口奖励与资产修改流程，确保任务奖励、邀请奖励、商店购买、对局结算等玩家资产变化具备幂等、事务或可恢复语义。
- 保留现有 REST 与 WebSocket 对外合同，优先重构服务端内部玩家数据读写路径。
- 不在本 change 中实现 Redis/分布式缓存、支付系统、抽奖系统、世界大厅、WebSocket JSON 协议重做。

## Capabilities

### New Capabilities

- `player-aggregate`: 定义统一玩家聚合的读取、修改、领域边界与兼容行为。
- `player-cache`: 定义玩家聚合缓存、dirty 分桶、延迟 flush、强制刷盘与失败重试行为。
- `player-transaction-integrity`: 定义玩家资产、奖励、任务进度、购买和结算的一致性、幂等与可恢复要求。

### Modified Capabilities

- 无。当前仓库尚无主规格文件，本 change 以新增能力定义后续实现合同。

## Impact

- 影响代码：`src/database/users.rs`、`src/database/items.rs`、`src/database/quests.rs`、`src/database/invites.rs`、`src/api/users/user_mgr.rs`、`src/database/mod.rs`、`src/main.rs`。
- 可能新增代码：`src/player` 或 `src/database/player`，用于 `PlayerAggregate`、`PlayerRepository`、cache entry、dirty bucket、flush worker 和一致性封装。
- 可能新增 migration：为玩家聚合版本号、资产流水、奖励事务号、幂等记录或 flush 状态增加表/字段；必须新增 migration，不修改旧 migration。
- 对外兼容：现有 REST 路由和 WebSocket 消息协议默认保持兼容；若后续必须调整响应结构或协议，需要单独在实现任务中列出并验证。
- 性能影响：读路径将更多命中内存聚合，写路径会由即时多次 SQL 转为按分桶批量写回；需要验证 flush worker 不阻塞 Actix actor 与 WebSocket 对战路径。
- 可靠性风险：write-behind 会引入短窗口数据丢失风险；必须通过强制刷盘、失败保留 dirty、下线刷盘、关闭刷盘和手动恢复步骤降低风险。
- 回滚策略：实现阶段应保留可切回即时写库的配置或小步迁移边界；若 flush 机制异常，应能禁用 write-behind 并回退到同步写库路径。
