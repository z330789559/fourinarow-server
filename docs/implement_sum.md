实现总结
1. Player 中间缓存层 (src/database/users.rs)
   CachedEntry 结构体缓存用户数据（不含 playing addr）
   DashMap<UserId, CachedEntry> + 60秒 TTL（基于 chrono::Utc::now()）
   get(id, use_cache, friendships) 统一接口
   update() / insert() 写 PG 后自动回写缓存；invalidate_cache() 显式失效
2. 物品系统
   迁移 002_items_inventory_shop.sql：items、user_inventory、shop_items 表 + 种子数据（coin/gem/ticket）
   src/items/mod.rs：静态 ItemDef 注册表
   src/database/items.rs：add_item、consume_item（原子扣减）、purchase（事务购买）、get_inventory、get_shop
   API：GET /api/inventory、GET/POST /api/inventory/shop/{id}/buy/{item}
3. 通用任务系统
   迁移 003_quests.sql：quests、user_quest_progress（含函数式唯一索引处理 NULL 日期）、achievement_tiers、user_achievement_progress
   src/quests/mod.rs：GameEvent::GameWon / GamePlayed
   src/database/quests.rs：on_event() 统一处理主线/每日/成就进度，自动发放奖励并升级成就 tier
   游戏结束事件：在 user_mgr.rs 的 PlayedGame 处理中调用，自动发放物品奖励
   API：GET /api/quests/story、/daily、/achievements
4. 排行榜 (src/database/leaderboard.rs)
   get_top_by_rating、get_top_by_wins（支持分页）
   get_user_rank（单用户排名查询）
   API：GET /api/leaderboard?type=skill_rating|wins&page=N、GET /api/leaderboard/me
5. 好友邀请 (src/database/invites.rs)
   迁移 004_invites.sql：invite_codes + invite_code_uses 表
   create、redeem（检查过期/限额/重复使用）、list_by_creator
   API：POST /api/invites（创建）、POST /api/invites/redeem（兑换 + 自动发放奖励）、GET /api/invites（我的邀请码）

## player-cache-management OpenSpec change 实现摘要

1. 新增玩家聚合层
   - `src/player/aggregate.rs` 定义 `PlayerAggregate`、`PlayerProfile`、`PlayerStats`、任务/成就进度和 `DirtyBucket`。
   - `src/player/repository.rs` 定义 `PlayerRepository`，作为玩家域读写入口。
   - `DatabaseManager` 持有 `players: PlayerRepository`，`main.rs` 注册 `mod player`。

2. 新增 schema
   - `migrations/005_player_cache_management.sql` 新增 `player_stats`、`player_operation_idempotency`、`asset_ledger`。
   - 旧的 `001..004` migration 未修改。
   - `asset_ledger` 使用 `(idempotency_key, item_id, delta)` 唯一约束防止同一资产流水重复写入。

3. 读路径迁移
   - `GET /api/inventory` 改为通过 `PlayerRepository::get_readonly` 返回背包快照。
   - `GET /api/quests/story|daily|achievements` 改为通过 `PlayerRepository::get_readonly` 返回任务/成就快照。
   - 只读访问不标记 dirty 分桶，也不安排写库。

4. 写路径迁移
   - 商城购买：`POST /api/inventory/shop/{shop_id}/buy/{item_id}` 改为调用 `PlayerRepository::purchase`。
   - 购买支持可选 `Idempotency-Key` header；缺失时服务端生成一次性 operation id，保证正常多次购买不会被误判为重复请求。
   - 邀请兑换：`POST /api/invites/redeem` 改为调用 `PlayerRepository::redeem_invite`，兑换记录、邀请码 uses、奖励和资产流水在同一事务内完成。
   - 对局结算：`UserManager` 的 `PlayedGame` 改为调用 `PlayerRepository::settle_game`，并传入 lobby 内部 `GameOId` 作为 `settlement_id`；repository 在事务开头写入 `player_operation_idempotency`，重复投递同一局会 no-op。
   - 对局结算事务内保持 SR、games、任务/成就进度、奖励和资产流水同步提交；`player_stats` 从事务中移出，提交后 reload 最新 aggregate，再通过 `with_player_mut([Stats])` 标脏并由 flush 机制写回。
   - 结算尝试结束后 `UserManager` 会统一失效胜负双方 `UserCollection` 缓存；即使 post-commit stats 阶段失败，也不会继续读到旧 SR TTL 快照。

5. dirty cache 与 flush
   - `PlayerCacheEntry` 记录 `PlayerAggregate`、dirty 分桶、修改时间、最后落库时间、失败次数和原因。
   - `with_player_mut` 支持闭包式修改并标记指定 dirty bucket。
   - `flush_player` 按 bucket 写回 PostgreSQL；失败时保留 dirty 状态并递增 attempts。
   - Inventory flush 使用 aggregate 当前绝对值写回，避免并发 flush 时 delta 被重复应用；Stats flush 也按当前绝对值 upsert。
   - 单个 bucket flush 失败不会中止同一玩家其他 dirty bucket 的尝试，最终统一返回错误。
   - 成功清理 dirty bucket 前会比较本次 flush 快照的 `changed_at`，避免刷旧快照时误清除 flush 期间的新修改。
   - 后台 flush worker 默认 500ms tick、5s cooldown、10min force flush。
   - `PLAYER_CACHE_FLUSH_IMMEDIATELY` 默认启用同步刷盘；设为 `0` 或 `false` 可启用延迟 write-behind。
   - 玩家 StopPlaying 时调用 `flush_player`；HTTP server 正常退出后调用 `flush_all` 并记录错误日志。
   - 资产同步写路径（任务/奖励、购买、邀请）在操作前保留 `flush_player`，提交后 reload 覆盖 cache；reload 失败时才移除 cache，避免旧快照继续被读到。
   - 对局结算事务提交后不再 remove 玩家 cache，而是先 reload 到提交后的 SR/资产快照，再累加 stats 并标记 `DirtyBucket::Stats`；延迟模式下内存立即一致，DB 在 5s cooldown 或 `flush_all` 后一致。

6. 验证状态
   - 已通过：`openspec validate "player-cache-management" --strict`。
   - 已通过：`cargo check`。
   - 已通过：`cargo test player::aggregate -- --nocapture`，覆盖只读不标脏、修改标脏、失败保留 dirty、成功清理指定 bucket。
   - 已通过：`cargo test player::repository -- --nocapture`，覆盖 flush 失败保留 dirty、`flush_all` 尝试所有玩家、同玩家多 dirty bucket 失败仍全部尝试。
   - 已通过：`cargo test player::repository::tests::db_ -- --ignored --nocapture`，覆盖本地 PostgreSQL 下强制刷盘、并发 Inventory flush 绝对值写回、结算后 reload cache 与 dirty stats 延迟落库。
   - 全仓 `cargo fmt --all --check` 仍会报告历史文件格式差异；本次修改相关 Rust 文件已用 `rustfmt --edition 2021` 定向格式化。

7. 保留路径和风险
   - `src/database/items.rs`、`src/database/quests.rs`、`src/database/invites.rs` 的部分旧写方法仍保留，但当前 REST/WS 关键玩家资产写路径已迁到 `PlayerRepository`。
   - `UserCollection::update` 仍用于登录连接态 `playing` 地址更新，但不再写 `users.skill_rating`，避免覆盖 repository 结算结果。
   - 当前 `games` 表仍只保存 winner/loser 和自增 id；结算幂等依据记录在 `player_operation_idempotency`，不是 `games` 表唯一业务键。
   - SR 当前明确保持同步事务，不进入 write-behind；若未来要把 SR 异步化，必须先新增单独 OpenSpec，解决崩溃丢分、排行榜可见性和幂等补偿问题。
   - Stats 是当前唯一真正接入生产 write-behind 的域；若提交后、stats flush 前进程崩溃，可从 `games` 表重算 `player_stats` 做人工对账。
