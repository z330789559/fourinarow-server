## Context

当前服务端已经从四子棋实时房间服务扩展出背包、任务、排行榜、邀请码和平台登录，但玩家数据读写仍分散在多个 database collection 中。`UserCollection` 只有 60 秒 TTL 读缓存，`ItemCollection`、`QuestCollection`、`InviteCollection` 等仍直接读写 PostgreSQL。对局结算在 `UserManager` actor 中串联更新 SR、游戏记录、任务和奖励，缺少统一事务边界。

目标是引入统一玩家聚合与缓存写回机制，让玩家属性更新从“业务代码直接写多个表”过渡到“业务代码修改 `PlayerAggregate`，由 `PlayerRepository` 统一标脏、flush 和保证一致性”。该设计参考 `slg-rs` 的 cache-management 思路，但适配本项目的 Actix actor、PostgreSQL/sqlx 和现有 REST/WS 兼容约束。

## Goals / Non-Goals

**Goals:**

- 建立 `PlayerAggregate`，覆盖基础信息、SR、资产、任务/成就、统计等玩家域数据。
- 建立 `PlayerRepository`，作为玩家域业务读写入口，替代直接散落调用底层 collection 修改玩家状态。
- 建立 dirty 分桶与 flush worker，支持延迟写回、强制刷盘、失败重试、下线刷盘和服务关闭刷盘。
- 收口任务奖励、邀请奖励、商城购买、对局结算的资产一致性与幂等策略。
- 保持现有 REST 路由和 WebSocket 文本可靠协议兼容，优先重构内部数据路径。

**Non-Goals:**

- 不在本 change 中改造 WebSocket 为 JSON 协议。
- 不引入 Redis 或分布式缓存。
- 不实现支付、抽奖、世界大厅、活动调度等新业务域。
- 不修改旧 migration；如需 schema 支撑，新增 migration。

## Decisions

### Decision 1: Add `src/player` as player domain layer

新增 `src/player` 模块承载玩家域聚合与缓存逻辑，避免继续把业务规则堆入 `src/database`。建议结构：

- `src/player/aggregate.rs`: `PlayerAggregate`、子域结构、dirty bucket 枚举。
- `src/player/repository.rs`: `PlayerRepository`，对外提供 `get_readonly`、`with_player_mut`、`flush_player`、`flush_all`。
- `src/player/cache.rs`: cache entry、dirty state、flush 判断。
- `src/player/persistence.rs`: PostgreSQL 加载/保存分桶的 sqlx 实现。
- `src/player/ledger.rs`: 资产流水与幂等键封装。

备选方案是继续扩展 `DatabaseManager` 下的各 collection。这个方案短期改动小，但不能解决业务入口分散和一致性边界不清的问题，因此不作为主方案。

### Decision 2: Repository exposes closure-based mutation API

`PlayerRepository` 使用闭包式修改入口，例如 `with_player_mut(user_id, |player, ctx| { ... })`。闭包返回业务结果，repository 负责标记 dirty bucket 和提交一致性上下文。

这样可以把“谁修改了哪个分桶”收口在 repository 内部，减少调用方拿到可变引用后忘记标脏的风险。实现上需要注意不要在持有 DashMap entry guard 时执行 `.await`，避免锁跨 await。

### Decision 3: Dirty buckets are domain-scoped, not full aggregate writes

dirty 状态按分桶记录：`profile`、`game_info`、`inventory`、`quests`、`achievements`、`stats`。flush 时只保存 dirty 分桶，避免一个背包奖励导致整份玩家聚合被反复写入。

每个 dirty bucket 记录：

- `changed_at`: 最近修改时间。
- `last_flush_at`: 最近成功落库时间。
- `attempts`: 连续失败次数。
- `reason`: 可选业务来源，便于日志和排查。

### Decision 4: Flush worker is owned by `DatabaseManager` startup lifecycle

`DatabaseManager::new()` 创建 `PlayerRepository`，`main.rs` 启动服务时启动 flush worker。flush worker 使用 tokio interval，默认每 500ms 检查一次 dirty 玩家，超过 5s 冷却后写回，超过 10min 未成功落库则强制刷盘。

服务关闭时增加 graceful shutdown 路径，调用 `flush_all()`。如果 Actix shutdown 接入成本较高，第一阶段至少提供显式 `flush_all()` API 和日志，第二阶段再接 SIGINT/SIGTERM。

### Decision 5: Consistency-critical operations flush synchronously first

对资产和奖励一致性要求高的操作，例如商城购买、邀请兑换、任务领奖、对局结算，第一阶段应在 repository 内使用 PostgreSQL transaction 或同步 flush 保证一致性。write-behind 适合统计、展示缓存和低风险进度聚合，但不能牺牲资产账务正确性。

因此本 change 不要求所有玩家修改都延迟落库；要求的是所有修改先经过 repository，并按风险选择同步事务或 dirty write-behind。

### Decision 6: Add asset ledger and idempotency support

新增资产流水或等价审计表，记录玩家资产增减、来源、关联业务 id 和幂等键。任务奖励、邀请奖励、购买等必须生成稳定幂等键，例如：

- `quest:{user_id}:{quest_id}:{quest_date_or_story}`
- `invite:{code}:{redeemer_id}`
- `purchase:{user_id}:{shop_id}:{item_id}:{request_id}`
- `game:{game_id}:{user_id}:{reward_type}`

如果现有请求没有 request id，先由服务端生成操作 id 并返回或记录；需要客户端参与幂等时，另开 API 合同变更。

### Decision 7: Keep external contracts stable

REST 路由仍保持 `/api/inventory`、`/api/quests/*`、`/api/invites/*`、`/api/leaderboard*`。WebSocket 仍保持当前 `HELLO::`、`MSG::`、`PC:` 等协议。内部改为走 repository 后，接口响应字段应保持兼容。

## Database / Migration Impact

可能需要新增 migration：

- `player_versions` 或在现有玩家相关表增加 version/updated_at 支撑乐观一致性。
- `asset_ledger` 记录资产流水。
- `player_operation_idempotency` 或按业务表新增 unique key 记录幂等操作。
- 如统计计数器目前无表承载，需要新增 `player_stats`。

所有 schema 变化必须新增 migration，不修改 `001..004` 旧文件。sqlx 查询必须与 migration 保持一致。

## API / WS Impact

- REST 默认兼容；若某接口需要新增 request id 或返回操作 id，必须在任务中标出并补充手动验证。
- WS 默认兼容；对局结算内部改 repository 后，`GameOver`、ACK/重传/重排序语义不得变化。
- 排行榜可以继续查询 `users`/`games`，但如果 SR 来源迁入聚合缓存，必须确保 flush 后排行榜可见性符合现有预期。

## Risks / Trade-offs

- Write-behind 崩溃丢数据窗口 -> 对资产类操作使用同步事务或同步 flush；低风险 dirty 分桶使用强制刷盘、关闭刷盘和失败重试降低风险。
- DashMap 锁跨 await 导致死锁或性能抖动 -> mutation API 必须先 clone/取出数据，释放锁后执行 async 持久化。
- 迁移过大影响稳定性 -> 先收口 repository，再逐个迁移 `items`、`quests`、`invites`、`user_mgr` 的写路径。
- 数据源双写期间不一致 -> 每阶段只允许一个业务路径作为写入口；保留只读兼容包装，避免两个 collection 同时修改同一字段。
- 无自动测试框架 -> 增加 focused unit tests，并提供 SQL/HTTP/WS 手动验收脚本。

## Migration Plan

1. 新增 `PlayerAggregate` 与 `PlayerRepository`，先只读加载现有表，不改变 REST/WS 行为。
2. 将 `UserCollection` 的 SR 更新路径迁入 repository，保留现有 `users.skill_rating` schema。
3. 将背包增减、任务进度、成就进度迁入 repository，并加入资产流水与幂等记录。
4. 接入 dirty 分桶和 flush worker，先以可配置的 `flush_immediately=true` 或同步 flush 模式运行。
5. 启用延迟 flush，并对非资产关键路径逐步开启 write-behind。
6. 接入下线刷盘和关闭刷盘；补齐失败重试日志和手动恢复步骤。
7. 验证通过后删除或限制业务层直接写底层 collection 的路径。

## Rollback Plan

- 保留开关禁用延迟 write-behind，使 repository 在 mutation 后同步 flush。
- 若 repository 迁移出现问题，按业务路径回退：先恢复对局结算，再恢复背包/任务/邀请。
- 新增 migration 不回滚删除数据；通过禁用新写入口和保留旧表数据恢复服务。
- 回滚后运行 `cargo check`，并用 REST/WS 手动流程确认登录、对战、背包、任务和排行榜仍可用。

## Open Questions

- `flush_immediately` 是否使用环境变量配置，还是编译期常量即可？
- 资产流水是否需要向客户端暴露查询 API，还是只做内部审计？
- 对局结算是否需要引入稳定 `game_id` 幂等键来防止重复结算？
- 服务关闭刷盘第一阶段是否必须接 SIGINT/SIGTERM，还是先提供内部 `flush_all()` 并在后续部署改造中接入？

## 实施细化：Write-behind 接入（范围确认后修正）

> 背景：复核发现 write-behind / dirty bucket / flush worker 已建好但**未接入任何生产写路径**（`with_player_mut` 零生产调用者，flush worker 空转）。
> 详见 `docs/player_cache_review_fixes.md` 的「Reviewer 二次复核结论」。
>
> **决策链**：方向=接入 write-behind → 范围=所有非资产域 → **SR 回退同步（缓解 B）**。
> 净效果：**本次实际真正异步化的只有 Stats（对战计数器）**；Profile 当前无写入口，名义异步、不实际产生脏；SR 保持同步。

### 接入范围（修正后）

| 数据域 | 写策略 | 落库方式 |
|--------|--------|----------|
| Stats（games_played/won/lost） | **write-behind** | settle 提交后内存累加 + 标脏 → flush worker（或 flush_immediately 即时） |
| Profile（username/email） | 名义 write-behind | 当前无写入口，机制就绪、不实际产生脏 |
| GameInfo（skill_rating） | **同步事务** | settle 事务内 `skill_rating ± delta`（SR 回退同步，天梯零丢分） |
| Inventory / Quests / Achievements | 同步事务 | purchase/redeem/settle 内即时落库 + ledger + 幂等 |

### Cache 一致性模型

唯一异步域 Stats 的脏只由 `settle_game` 产生。关键不变量：**任何会 remove/覆盖 cache 的资产写操作，执行前必须确保异步脏域已落库**。

1. **purchase / redeem_invite / add_item_once**（不碰 stats）：开头 `flush_player` 已落任何残留 stats 脏；事务改资产；收尾 reload 覆盖 cache。若 reload 失败才 remove cache，避免旧快照继续被读取。
2. **settle_game**（产生 stats 脏）：见下。

### settle_game 拆分

1. `flush_player(winner/loser)`：落旧脏域（保留）。
2. 同步事务：幂等键 + **SR `± delta`（保留同步）** + `games` 记录 + quest/achievement 资产奖励。**仅将 `upsert_stats_in_tx` 移出事务。**
3. 提交后，对 winner/loser 各调新私有方法 `bump_stats(delta, reason)`：
   - 强制 reload aggregate 覆盖 cache（拿 commit 后最新 SR/资产 + DB 现有 stats，clean）。
   - 经 `with_player_mut([Stats])` 内存 `games_played/won/lost += delta`，标脏。
   - `flush_immediately=true`（默认）→ 立即按绝对值 upsert 落库；`=false` → 留脏由 flush worker 写回。
4. **删除收尾的 `cache.remove`**（由 `bump_stats` 的 reload 接管 cache，避免丢 stats 脏）。

`with_player_mut` 由此成为生产调用者，dirty bucket / flush worker 在生产中第一次真正生效。

### 风险与缓解

- **崩溃丢 stats（可恢复）**：commit 后、stats flush 前崩溃 → 因幂等键存在 settle 不重做 → 丢本局 stats。但 `games` 表同步记录了每局 winner/loser，**stats 可由 `games` 表完整重算**（提供对账思路/脚本）。SR 已同步，不受影响。
- **并发**：同一玩家不会同时在两局结算，stats 写无并发。
- **读一致性**：reload 覆盖保证 SR/资产读最新；stats 读内存最新值（即使未落库）。

### 验证要点

- `flush_immediately=false` 下单独触发 settle：≥5s 后 `player_stats` 由 worker 落库；其值可由 `games` 表核对。
- settle 后立即读 `/api/users/me`：SR 即时正确（同步）；stats 内存即时正确。
- 关闭刷盘：制造 dirty stats，触发 `flush_all`，确认 `player_stats` 落库。
- DB 错误：stats 保留 dirty，日志含 user/bucket/原因。
