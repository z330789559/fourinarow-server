## 1. Baseline And Schema Preparation

- [x] 1.1 盘点当前玩家写路径：`UserCollection::update`、`ItemCollection::{add_item,consume_item,purchase}`、`QuestCollection::on_event`、`InviteCollection::redeem`、`PlayedGame` 结算，并记录迁移顺序
- [x] 1.2 设计新增 migration：资产流水、幂等记录、玩家统计/版本字段；确认不修改既有 `001..004` migration
- [x] 1.3 新增 migration 并用 PostgreSQL/sqlx 校验 schema 可迁移
- [x] 1.4 为新 schema 写最小 SQL 查询验证步骤，覆盖资产流水、幂等唯一约束和统计/版本字段

## 2. Player Aggregate And Repository

- [x] 2.1 新增 `src/player` 模块结构，并在 `main.rs` 中注册模块
- [x] 2.2 定义 `PlayerAggregate`、资产子域、任务子域、成就子域、统计子域和 `DirtyBucket`
- [x] 2.3 实现 `PlayerRepository` 只读加载流程，从现有 PostgreSQL 表聚合玩家状态
- [x] 2.4 实现新玩家默认子域加载，确保无背包/任务/成就记录时返回默认聚合
- [x] 2.5 将 `DatabaseManager` 集成 `PlayerRepository`，不改变现有 REST/WS 行为
- [x] 2.6 运行 `cargo check`，并手动验证 `/api/users/me`、`/api/inventory`、`/api/quests/*` 仍可读取

## 3. Dirty Cache Infrastructure

- [x] 3.1 实现玩家缓存 entry，包含 `PlayerAggregate`、dirty 分桶、修改时间、最后落库时间和失败次数
- [x] 3.2 实现只读访问，确保不会标记 dirty 分桶
- [x] 3.3 实现闭包式可变访问，确保修改后只标记受影响 dirty 分桶
- [x] 3.4 实现 `flush_player`，按 dirty 分桶写回 PostgreSQL，成功后清除对应 dirty 状态
- [x] 3.5 实现失败保留 dirty 状态和错误日志，日志包含 user id、bucket、错误原因
- [x] 3.6 增加 focused tests 或可执行验证代码，覆盖只读不标脏、修改标脏、失败保留 dirty

## 4. Flush Lifecycle

- [x] 4.1 实现后台 flush tick，默认 500ms 检查，5s 冷却写回
- [x] 4.2 实现 10min force flush，防止持续修改的玩家长期不落库
- [x] 4.3 实现 `flush_all`，用于服务关闭和手动维护
- [x] 4.4 接入玩家登出或连接结束时的 `flush_player`
- [x] 4.5 接入服务关闭刷盘路径；若无法一次接入信号处理，先提供明确的 `flush_all` 调用点和日志
- [x] 4.6 添加开关或常量支持 `flush_immediately`，用于上线初期禁用延迟 write-behind

## 5. Migrate Player Mutation Paths

- [x] 5.1 将对局结算中的 SR、统计、任务进度和奖励更新迁入 `PlayerRepository`
- [x] 5.2 将任务奖励发放迁入 `PlayerRepository`，避免任务已 rewarded 但奖励未入账
- [x] 5.3 将邀请兑换奖励迁入 `PlayerRepository`，保证同一兑换不会重复发奖
- [x] 5.4 将背包增减迁入 `PlayerRepository`，限制业务层直接调用 `ItemCollection::add_item/consume_item`
- [x] 5.5 将商城购买迁入 `PlayerRepository`，确保扣货币、加商品、库存/记录更新处于一致性边界内
- [x] 5.6 运行 `rg` 检查玩家写路径是否仍绕过 repository，并记录保留原因

## 6. Transaction Integrity And Ledger

- [x] 6.1 实现资产流水写入，记录 user id、item id、delta、source、idempotency key、业务关联 id、发生时间
- [x] 6.2 实现任务奖励幂等键，重复 flush 或重复请求不得重复发奖
- [x] 6.3 实现邀请兑换幂等键，重复兑换和并发兑换不得重复发奖
- [x] 6.4 实现商城购买幂等或事务边界，失败时不得出现货币扣除但商品未发
- [x] 6.5 实现对局结算失败日志和可恢复上下文，包含 game id、winner、loser、失败阶段
- [x] 6.6 为资产流水和幂等记录提供 SQL 手动核验步骤

## 7. Compatibility And Manual Verification

- [x] 7.1 运行 `cargo check`，确认 sqlx 查询与 migration schema 对齐
- [x] 7.2 手动验证 REST 兼容：注册/登录、`/api/users/me`、`/api/inventory`、`/api/quests/story`、`/api/quests/daily`、`/api/quests/achievements`
- [x] 7.3 手动验证 WebSocket 对局流程：两名玩家登录、匹配/进入房间、落子、结算、`GameOver` 消息兼容
- [x] 7.4 手动验证对局结算后数据库状态：`users.skill_rating`、`games`、任务进度、资产流水、背包余额一致
- [x] 7.5 手动验证重复任务奖励、重复邀请兑换、重复购买请求不会重复发放资产
- [x] 7.6 手动验证 flush 失败重试：模拟数据库错误或使用可控错误路径，确认 dirty 状态保留且日志可定位
- [x] 7.7 手动验证关闭刷盘：制造 dirty 玩家，触发 `flush_all` 或服务关闭，确认 PostgreSQL 最终状态一致

## 8. Rollback And Documentation

- [x] 8.1 文档化 `flush_immediately` 或同步写回回退方式
- [x] 8.2 文档化新增 migration 的恢复策略，禁止删除已产生的资产流水/幂等记录
- [x] 8.3 更新 `docs/implement_sum.md`，说明当前实现是否启用 write-behind、哪些路径已迁入 repository
- [x] 8.4 总结剩余未迁移路径和风险，作为 archive 前验收输入

## 9. Write-behind 接入（范围=所有非资产域）

- [x] 9.1 重构 cache 一致性模型：资产域写后改为 reload 覆盖（替代 `remove`），废弃 `invalidate_clean` 的 skip 分支
- [x] 9.2 settle_game 拆分：SR 保持同步事务，仅 stats 移出同步事务；提交后 reload + `with_player_mut([Stats])` 标脏
- [x] 9.3 purchase/redeem 收尾改为 reload 覆盖，确认操作前 `flush_player` 保留
- [x] 9.4 接入 stats 的 `with_player_mut` 生产写入口；Profile 保留机制无生产写入口，SR 明确保留同步写路径
- [x] 9.5 文档化 SR 不异步的原因和未来异步丢分风险，并提供 stats 对账思路或脚本（从 `games` 表重算）
- [x] 9.6 验证：异步域延迟落库、settle 后内存即时一致 + 5s 后 DB 一致、关闭刷盘、DB 错误保留 dirty
- [x] 9.7 `cargo check` + 手动 REST/WS 回归，确认对外合同不变
